#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

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

    info!("LED test started");

    loop {
        info!("green on");
        green.set_high();
        Timer::after_millis(500).await;
        green.set_low();
        Timer::after_millis(200).await;

        info!("red on");
        red.set_high();
        Timer::after_millis(500).await;
        red.set_low();
        Timer::after_millis(200).await;

        info!("both on");
        green.set_high();
        red.set_high();
        Timer::after_millis(500).await;
        green.set_low();
        red.set_low();
        Timer::after_millis(500).await;
    }
}
