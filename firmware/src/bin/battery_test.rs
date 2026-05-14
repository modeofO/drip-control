#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

//! Battery monitor bench test.
//!
//! Reads the battery divider (PA4) once per second and drives the red LED
//! based on the firmware's own classification:
//!
//!   OK       — red LED off
//!   LOW      — red LED solid
//!   CRITICAL — red LED + green LED both solid (so you can tell them apart
//!              from a regular "low" without looking at the log)
//!
//! Useful for:
//!   - Verifying the 100K/100K divider math against a multimeter on the
//!     18650 cell. Compare the printed `vbat_mv` to your meter reading.
//!   - Exercising the BATTERY_LOW / BATTERY_CRITICAL thresholds by slowly
//!     discharging or by feeding the divider with a bench supply.
//!
//! Run:  cargo run --bin battery-test --release

use defmt::*;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::Config;
use embassy_time::{Delay, Timer};

use irrigation_controller::{adc_to_battery_mv, classify_battery, BatteryStatus};

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Config::default());

    let mut delay = Delay;
    let mut adc = Adc::new(p.ADC1, &mut delay);
    adc.set_sample_time(SampleTime::Cycles480);
    let mut vbat = p.PA4;

    let mut green = Output::new(p.PB3, Level::Low, Speed::Low);
    let mut red = Output::new(p.PB5, Level::Low, Speed::Low);

    info!("battery bench test — reading PA4 every 1s");

    loop {
        let raw = adc.read(&mut vbat);
        let mv = adc_to_battery_mv(raw);
        let status = classify_battery(mv);

        match status {
            BatteryStatus::Ok => {
                red.set_low();
                green.set_low();
                info!("vbat = {} mV (raw {}) — OK", mv, raw);
            }
            BatteryStatus::Low => {
                red.set_high();
                green.set_low();
                warn!("vbat = {} mV (raw {}) — LOW", mv, raw);
            }
            BatteryStatus::Critical => {
                red.set_high();
                green.set_high();
                error!("vbat = {} mV (raw {}) — CRITICAL", mv, raw);
            }
        }

        Timer::after_secs(1).await;
    }
}
