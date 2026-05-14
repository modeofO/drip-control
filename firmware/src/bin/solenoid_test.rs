#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

//! Solenoid bench test.
//!
//! Toggles the MOSFET gate (PA10 / D2) on and off every 2 seconds.
//! Useful for:
//!   - Listening for the valve "click" on each transition.
//!   - Measuring gate voltage with a multimeter (should swing 0V ↔ 3.3V).
//!   - Measuring drain voltage (should swing 12V ↔ ~0V when energised).
//!   - Confirming the 10K gate pull-down works: when the MCU is held in
//!     reset, gate should sit at 0V and the solenoid should drop out.
//!
//! Run:  cargo run --bin solenoid-test --release

use defmt::*;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::Config;
use embassy_time::Timer;

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Config::default());
    let mut solenoid = Output::new(p.PA10, Level::Low, Speed::Low);
    let mut green = Output::new(p.PB3, Level::Low, Speed::Low);

    info!("solenoid bench test — toggling PA10 every 2s");

    let mut on = false;
    loop {
        on = !on;
        if on {
            solenoid.set_high();
            green.set_high();
            info!("solenoid: ON  (gate HIGH, drain should be ~0V)");
        } else {
            solenoid.set_low();
            green.set_low();
            info!("solenoid: OFF (gate LOW, drain should be ~12V)");
        }
        Timer::after_secs(2).await;
    }
}
