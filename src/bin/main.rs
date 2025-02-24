#![no_std]
#![no_main]

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Baseline,
    text::Text,
};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::i2c::master::I2c;
use esp_hal::main;
use esp_hal::time::RateExtU32;
use log::info;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

#[main]
fn main() -> ! {
    // generator version: 0.2.2

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let system = peripherals.SYSTEM.bt_lpck_div_frac();

    esp_println::logger::init_logger_from_env();

    let sda = peripherals.GPIO8;
    let scl = peripherals.GPIO9;

    let i2c = I2c::new(
        peripherals.I2C0,
        esp_hal::i2c::master::Config::default().with_frequency(100u32.Hz()),
    )
    .unwrap()
    .with_scl(scl)
    .with_sda(sda);

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    display.init().unwrap();

    let text_style = MonoTextStyleBuilder::new()
        .text_color(BinaryColor::On)
        .build();
    Text::with_baseline(
        "Click stick\nto start! ",
        Point::zero(),
        text_style,
        Baseline::Top,
    )
    .draw(&mut display)
    .unwrap();
    display.flush().unwrap();

    let delay = Delay::new();
    loop {
        info!("Hello world!");
        delay.delay_millis(500);
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.23.1/examples/src/bin
}
