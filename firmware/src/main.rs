#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::gpio::{AnyPin, Level, Output, Speed};
use embassy_stm32::peripherals::IWDG;
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::Config;
use embassy_time::{Delay, Instant, Timer};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

// Soil sensor ADC thresholds (12-bit, 0–4095). Calibrate with actual soil.
const SOIL_DRY: u16 = 2000;
const SOIL_WET: u16 = 2800;
const SENSOR_FAULT_LO: u16 = 50;
const SENSOR_FAULT_HI: u16 = 4045;

// Timing
const CHECK_INTERVAL_SECS: u64 = 900; // 15 min between sensor checks
const MAX_WATERING_SECS: u64 = 300; // 5-min hard cap per activation
const WATERING_POLL_SECS: u64 = 5; // re-read soil every 5s while watering
const WDG_PET_SECS: u64 = 20; // pet watchdog every 20s during sleep
const WDG_TIMEOUT_US: u32 = 30_000_000; // 30s watchdog timeout

// Battery (millivolts, measured via 100K/100K divider on ADC)
const BATTERY_LOW_MV: u16 = 3300;
const BATTERY_CRITICAL_MV: u16 = 3000;

// Limits
const MAX_DAILY_CYCLES: u8 = 6;
const DAY_SECS: u64 = 86_400;

// ADC sample time — slow is fine, we're not in a hurry
const ADC_SAMPLE: SampleTime = SampleTime::Cycles480;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_sensor_fault(reading: u16) -> bool {
    reading < SENSOR_FAULT_LO || reading > SENSOR_FAULT_HI
}

fn adc_to_battery_mv(raw: u16) -> u16 {
    // 100K/100K divider halves Vbat.
    // ADC: 0–4095 maps to 0–3300 mV at the divider midpoint.
    // Vbat = midpoint × 2.
    ((raw as u32 * 3300 * 2) / 4096) as u16
}

async fn sleep_with_watchdog(wdg: &mut IndependentWatchdog<'_, IWDG>, total_secs: u64) {
    let mut remaining = total_secs;
    while remaining > 0 {
        let chunk = remaining.min(WDG_PET_SECS);
        Timer::after_secs(chunk).await;
        wdg.pet();
        remaining = remaining.saturating_sub(chunk);
    }
}

async fn blink(led: &mut Output<'_, AnyPin>, count: u8, on_ms: u64, off_ms: u64) {
    for _ in 0..count {
        led.set_high();
        Timer::after_millis(on_ms).await;
        led.set_low();
        Timer::after_millis(off_ms).await;
    }
}

// ---------------------------------------------------------------------------
// Watering
// ---------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum WaterResult {
    SoilWet,
    MaxDuration,
    SensorFault,
}

impl WaterResult {
    fn as_str(self) -> &'static str {
        match self {
            Self::SoilWet => "soil_wet",
            Self::MaxDuration => "max_duration",
            Self::SensorFault => "sensor_fault",
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Config::default());

    // GPIO — solenoid gate driven LOW at boot (hardware pull-down backs this up)
    let mut solenoid = Output::new(p.PA10, Level::Low, Speed::Low).degrade(); // D2
    let mut green = Output::new(p.PB3, Level::Low, Speed::Low).degrade(); // D3
    let mut red = Output::new(p.PB5, Level::Low, Speed::Low).degrade(); // D4

    // ADC
    let mut delay = Delay;
    let mut adc = Adc::new(p.ADC1, &mut delay);
    adc.set_sample_time(ADC_SAMPLE);
    let mut soil1 = p.PA0;
    let mut soil2 = p.PA1;
    let mut vbat_pin = p.PA4;

    // Watchdog — resets MCU (and solenoid off via pull-down) if firmware hangs
    let mut wdg = IndependentWatchdog::new(p.IWDG, WDG_TIMEOUT_US);
    wdg.unleash();

    // Boot indicator: 3 green blinks
    blink(&mut green, 3, 100, 100).await;

    let mut daily_cycles: u8 = 0;
    let mut day_start = Instant::now();

    info!("irrigation controller booted");

    loop {
        wdg.pet();

        // ── Daily counter reset ──────────────────────────────────────
        if Instant::now().duration_since(day_start).as_secs() >= DAY_SECS {
            daily_cycles = 0;
            day_start = Instant::now();
            info!("daily cycle counter reset");
        }

        // ── Battery ──────────────────────────────────────────────────
        let vbat_raw = adc.read(&mut vbat_pin);
        let battery_mv = adc_to_battery_mv(vbat_raw);
        info!("battery: {} mV (raw {})", battery_mv, vbat_raw);

        if battery_mv < BATTERY_CRITICAL_MV {
            warn!("battery critical — skipping watering");
            red.set_high();
            green.set_low();
            sleep_with_watchdog(&mut wdg, CHECK_INTERVAL_SECS).await;
            continue;
        }

        if battery_mv < BATTERY_LOW_MV {
            red.set_high();
        } else {
            red.set_low();
        }

        // ── Sensors ──────────────────────────────────────────────────
        let s1 = adc.read(&mut soil1);
        let s2 = adc.read(&mut soil2);
        info!("soil: s1={} s2={}", s1, s2);

        if is_sensor_fault(s1) || is_sensor_fault(s2) {
            warn!("sensor fault: s1={} s2={}", s1, s2);
            blink(&mut red, 5, 100, 100).await;
            sleep_with_watchdog(&mut wdg, CHECK_INTERVAL_SECS).await;
            continue;
        }

        // ── Watering decision ────────────────────────────────────────
        let soil_dry = s1 < SOIL_DRY || s2 < SOIL_DRY;

        if soil_dry && daily_cycles < MAX_DAILY_CYCLES {
            info!("watering cycle {} — soil dry", daily_cycles + 1);

            green.set_high();
            solenoid.set_high();
            let start = Instant::now();

            let result = loop {
                wdg.pet();
                Timer::after_secs(WATERING_POLL_SECS).await;

                if Instant::now().duration_since(start).as_secs() >= MAX_WATERING_SECS {
                    warn!("max watering duration reached");
                    break WaterResult::MaxDuration;
                }

                let s1 = adc.read(&mut soil1);
                let s2 = adc.read(&mut soil2);

                if is_sensor_fault(s1) || is_sensor_fault(s2) {
                    warn!("sensor fault during watering: s1={} s2={}", s1, s2);
                    break WaterResult::SensorFault;
                }

                if s1 > SOIL_WET && s2 > SOIL_WET {
                    break WaterResult::SoilWet;
                }
            };

            solenoid.set_low();
            green.set_low();
            daily_cycles += 1;

            let elapsed = Instant::now().duration_since(start).as_secs();
            info!("watering done: {}s, {}", elapsed, result.as_str());

            if result != WaterResult::SoilWet {
                blink(&mut red, 5, 100, 100).await;
            }
        } else if daily_cycles >= MAX_DAILY_CYCLES {
            warn!("max daily cycles reached ({})", MAX_DAILY_CYCLES);
        }

        // ── Heartbeat + sleep ────────────────────────────────────────
        blink(&mut green, 1, 50, 0).await;
        sleep_with_watchdog(&mut wdg, CHECK_INTERVAL_SECS).await;
    }
}
