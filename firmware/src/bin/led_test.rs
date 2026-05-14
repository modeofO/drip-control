#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

//! LED bench test.
//!
//! Walks through every LED state the firmware can produce:
//!   1. Green only       — heartbeat
//!   2. Red only         — low battery
//!   3. Both on          — watering + low battery
//!   4. Red fast blink   — sensor fault
//!   5. Both off         — sleep
//!
//! Use it to verify the 330Ω current-limit resistors, LED polarity, and
//! GPIO pin mapping (PB3 = green / D3, PB5 = red / D4).
//!
//! Run:  cargo run --bin led-test --release

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
    let mut green = Output::new(p.PB3, Level::Low, Speed::Low);
    let mut red = Output::new(p.PB5, Level::Low, Speed::Low);

    info!("LED bench test starting");

    loop {
        info!("1/5: GREEN solid (heartbeat)");
        green.set_high();
        red.set_low();
        Timer::after_secs(2).await;

        info!("2/5: RED solid (low battery)");
        green.set_low();
        red.set_high();
        Timer::after_secs(2).await;

        info!("3/5: BOTH solid (watering + low battery)");
        green.set_high();
        red.set_high();
        Timer::after_secs(2).await;

        info!("4/5: RED fast blink (sensor fault)");
        green.set_low();
        for _ in 0..10 {
            red.set_high();
            Timer::after_millis(100).await;
            red.set_low();
            Timer::after_millis(100).await;
        }

        info!("5/5: BOTH off (sleep)");
        Timer::after_secs(2).await;
    }
}
