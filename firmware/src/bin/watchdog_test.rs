#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

//! Watchdog bench test.
//!
//! Sequence:
//!   1. Boot — green LED on, watchdog armed (30s timeout).
//!   2. For 20 seconds, the firmware pets the watchdog every second.
//!      Green stays solid the whole time.
//!   3. After 20s the firmware stops petting and turns green off + red on.
//!   4. ~30s after the last pet, IWDG fires → MCU resets → boot sequence
//!      repeats.
//!
//! What to verify:
//!   - On reset, the MOSFET gate sits at 0V (the 10K pull-down + LOW GPIO
//!     default ensures the solenoid drops out). Probe PA10 with a meter.
//!   - The log prints "BOOTED" on every reset; the cycle should repeat
//!     forever, roughly every 50 seconds.
//!
//! Run:  cargo run --bin watchdog-test --release

use defmt::*;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::Config;
use embassy_time::Timer;

use irrigation_controller::config::WDG_TIMEOUT_US;

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Config::default());

    // Drive the solenoid gate explicitly LOW so the bench test can't leave the
    // valve open across a watchdog reset cycle.
    let _solenoid = Output::new(p.PA10, Level::Low, Speed::Low);

    let mut green = Output::new(p.PB3, Level::Low, Speed::Low);
    let mut red = Output::new(p.PB5, Level::Low, Speed::Low);

    let mut wdg = IndependentWatchdog::new(p.IWDG, WDG_TIMEOUT_US);
    wdg.unleash();

    info!("BOOTED — petting watchdog for 20s");
    green.set_high();
    red.set_low();

    for i in 1..=20 {
        wdg.pet();
        Timer::after_secs(1).await;
        info!("pet {}/20", i);
    }

    info!("stopped petting — IWDG should fire within ~30s");
    green.set_low();
    red.set_high();

    // Spin without petting; the watchdog will reset us.
    loop {
        Timer::after_secs(1).await;
    }
}
