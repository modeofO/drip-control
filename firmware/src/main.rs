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

use irrigation_controller::{
    adc_to_battery_mv, classify_battery, config::*, water_tick, watering_decision, BatteryStatus,
    WaterResult, WaterTick, WateringDecision,
};

// ADC sample time — slow is fine; we're not in a hurry and the source impedance
// of the soil sensor + divider is high.
const ADC_SAMPLE: SampleTime = SampleTime::Cycles480;

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

fn apply_battery_leds(status: BatteryStatus, red: &mut Output<'_, AnyPin>) {
    match status {
        BatteryStatus::Critical | BatteryStatus::Low => red.set_high(),
        BatteryStatus::Ok => red.set_low(),
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Config::default());

    // GPIO — solenoid gate driven LOW at boot (hardware pull-down backs this up).
    let mut solenoid = Output::new(p.PA10, Level::Low, Speed::Low).degrade(); // D2
    let mut green = Output::new(p.PB3, Level::Low, Speed::Low).degrade(); // D3
    let mut red = Output::new(p.PB5, Level::Low, Speed::Low).degrade(); // D4

    // ADC.
    let mut delay = Delay;
    let mut adc = Adc::new(p.ADC1, &mut delay);
    adc.set_sample_time(ADC_SAMPLE);
    let mut soil1 = p.PA0;
    let mut soil2 = p.PA1;
    let mut vbat_pin = p.PA4;

    // Watchdog — resets MCU (and solenoid off via pull-down) if firmware hangs.
    let mut wdg = IndependentWatchdog::new(p.IWDG, WDG_TIMEOUT_US);
    wdg.unleash();

    // Boot indicator: 3 green blinks.
    blink(&mut green, 3, 100, 100).await;

    let mut daily_cycles: u8 = 0;
    let mut day_start = Instant::now();

    info!("irrigation controller booted");

    loop {
        wdg.pet();

        if Instant::now().duration_since(day_start).as_secs() >= DAY_SECS {
            daily_cycles = 0;
            day_start = Instant::now();
            info!("daily cycle counter reset");
        }

        // ── Battery ──────────────────────────────────────────────────
        let vbat_raw = adc.read(&mut vbat_pin);
        let battery_mv = adc_to_battery_mv(vbat_raw);
        let battery = classify_battery(battery_mv);
        info!("battery: {} mV (raw {}), status {}", battery_mv, vbat_raw, battery as u8);
        apply_battery_leds(battery, &mut red);

        // ── Sensors ──────────────────────────────────────────────────
        let s1 = adc.read(&mut soil1);
        let s2 = adc.read(&mut soil2);
        info!("soil: s1={} s2={}", s1, s2);

        // ── Decide ──────────────────────────────────────────────────
        match watering_decision(s1, s2, daily_cycles, battery) {
            WateringDecision::Skip => {
                // Tell the operator *why* we skipped, where it matters.
                if battery == BatteryStatus::Critical {
                    warn!("battery critical — skipping watering");
                } else if irrigation_controller::is_sensor_fault(s1)
                    || irrigation_controller::is_sensor_fault(s2)
                {
                    warn!("sensor fault — skipping watering");
                    blink(&mut red, 5, 100, 100).await;
                } else if daily_cycles >= MAX_DAILY_CYCLES {
                    warn!("max daily cycles reached ({})", MAX_DAILY_CYCLES);
                }
            }
            WateringDecision::Water => {
                info!("watering cycle {} — soil dry", daily_cycles + 1);

                green.set_high();
                solenoid.set_high();
                let start = Instant::now();

                let result = run_watering(
                    &mut adc,
                    &mut soil1,
                    &mut soil2,
                    &mut wdg,
                    start,
                )
                .await;

                solenoid.set_low();
                green.set_low();
                daily_cycles += 1;

                let elapsed = Instant::now().duration_since(start).as_secs();
                info!("watering done: {}s, {}", elapsed, result.as_str());

                if result != WaterResult::SoilWet {
                    blink(&mut red, 5, 100, 100).await;
                }
            }
        }

        // ── Heartbeat + sleep ────────────────────────────────────────
        blink(&mut green, 1, 50, 0).await;
        sleep_with_watchdog(&mut wdg, CHECK_INTERVAL_SECS).await;
    }
}

async fn run_watering(
    adc: &mut Adc<'_, embassy_stm32::peripherals::ADC1>,
    soil1: &mut embassy_stm32::peripherals::PA0,
    soil2: &mut embassy_stm32::peripherals::PA1,
    wdg: &mut IndependentWatchdog<'_, IWDG>,
    start: Instant,
) -> WaterResult {
    loop {
        wdg.pet();
        Timer::after_secs(WATERING_POLL_SECS).await;

        let elapsed = Instant::now().duration_since(start).as_secs();
        let s1 = adc.read(soil1);
        let s2 = adc.read(soil2);

        match water_tick(s1, s2, elapsed) {
            WaterTick::Continue => {}
            WaterTick::Stop(result) => {
                if result != WaterResult::SoilWet {
                    warn!("watering stopped early: {}", result.as_str());
                }
                return result;
            }
        }
    }
}
