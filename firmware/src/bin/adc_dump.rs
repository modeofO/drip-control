#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

//! ADC dump — prints raw and converted readings continuously.
//!
//! Pin map (matches the main firmware):
//!   A0 / PA0  — soil sensor 1
//!   A1 / PA1  — soil sensor 2
//!   A2 / PA4  — battery voltage (via 100K/100K divider)
//!
//! Useful for:
//!   - Calibrating SOIL_DRY / SOIL_WET thresholds: drop a sensor in dry vs
//!     freshly watered soil and read the value here.
//!   - Verifying the battery divider math against a multimeter.
//!   - Sanity-checking ADC channel mapping before flashing the main firmware.
//!
//! Run:  cargo run --bin adc-dump --release

use defmt::*;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::Config;
use embassy_time::{Delay, Timer};

use irrigation_controller::{adc_to_battery_mv, classify_battery, is_sensor_fault};

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Config::default());

    let mut delay = Delay;
    let mut adc = Adc::new(p.ADC1, &mut delay);
    adc.set_sample_time(SampleTime::Cycles480);

    let mut soil1 = p.PA0;
    let mut soil2 = p.PA1;
    let mut vbat = p.PA4;

    info!("ADC dump — soil1(PA0), soil2(PA1), vbat(PA4) every 500ms");

    loop {
        let s1 = adc.read(&mut soil1);
        let s2 = adc.read(&mut soil2);
        let v_raw = adc.read(&mut vbat);
        let v_mv = adc_to_battery_mv(v_raw);

        info!(
            "s1={} (fault={}) | s2={} (fault={}) | vbat raw={} mv={} status={}",
            s1,
            is_sensor_fault(s1),
            s2,
            is_sensor_fault(s2),
            v_raw,
            v_mv,
            classify_battery(v_mv) as u8,
        );

        Timer::after_millis(500).await;
    }
}
