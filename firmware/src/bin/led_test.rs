#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::Config;
use embassy_time::Timer;

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hse = Some(Hse {
            freq: embassy_stm32::time::Hertz(8_000_000),
            mode: HseMode::Bypass,
        });
        config.rcc.pll_src = PllSource::Hse;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::Div4,
            mul: PllMul::Mul100,
            divp: Some(PllPDiv::Div2),
            divq: None,
            divr: None,
        });
        config.rcc.sys = Sysclk::Pll1P;
        config.rcc.ahb_pre = AHBPrescaler::Div1;
        config.rcc.apb1_pre = APBPrescaler::Div2;
        config.rcc.apb2_pre = APBPrescaler::Div1;
    }
    let p = embassy_stm32::init(config);

    let mut green = Output::new(p.PB3, Level::Low, Speed::Low);
    let mut red = Output::new(p.PB5, Level::Low, Speed::Low);

    info!("LED test started");

    loop {
        info!("green on");
        green.set_high();
        Timer::after_millis(100).await;
        green.set_low();
        Timer::after_millis(50).await;

        info!("red on");
        red.set_high();
        Timer::after_millis(100).await;
        red.set_low();
        Timer::after_millis(50).await;

        info!("both on");
        green.set_high();
        red.set_high();
        Timer::after_millis(100).await;
        green.set_low();
        red.set_low();
        Timer::after_millis(50).await;
    }
}