#![no_std]
#![no_main]

use blink::{Event, SmartGadget, State};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Text,
};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use heapless::String as HString;
use heapless::Vec as HVec;

use esp_hal::i2c::master::I2c;
use esp_hal::main;
use esp_hal::time::RateExtU32;
use log::info;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

/// Formats a quote into displayable lines, wrapping at approximately 20 characters
fn format_quote_lines(quote: &str) -> HVec<HString<32>, 8> {
    let words: HVec<&str, 32> = quote.split_whitespace().collect();
    let mut lines: HVec<HString<32>, 8> = HVec::new();
    let mut current_line = HString::new();

    for word in words {
        if current_line.len() + word.len() < 20 {
            // ~20 chars per line
            if !current_line.is_empty() {
                current_line.push(' ').unwrap();
            }
            current_line.push_str(word).unwrap();
        } else {
            if !current_line.is_empty() {
                lines.push(current_line.clone()).unwrap();
            }
            current_line = HString::from(word);
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line).unwrap();
    }

    lines
}

#[main]
fn main() -> ! {
    // generator version: 0.2.2
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

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

    let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let delay = Delay::new();

    // Initialize the state machine
    let mut gadget = SmartGadget::new();

    // Start with displaying a quote
    gadget.simulate_quote_fetch();

    let mut counter = 0;

    loop {
        // Simulate button press every 5 seconds for demo
        counter += 1;
        if counter % 50 == 0 {
            // Every 5 seconds (50 * 100ms)
            info!("Simulating button press!");
            gadget.handle_event(Event::ButtonPress);
        }

        // Handle state transitions
        match gadget.state {
            State::DisplayingTime => {
                info!("Displaying time");
                display.clear(BinaryColor::Off).unwrap();
                Text::new("Time Mode", Point::new(10, 20), text_style)
                    .draw(&mut display)
                    .unwrap();
                Text::new("Press for quote", Point::new(10, 35), text_style)
                    .draw(&mut display)
                    .unwrap();
                display.flush().unwrap();
            }

            State::FetchingQuote => {
                info!("Fetching quote");
                display.clear(BinaryColor::Off).unwrap();
                Text::new("Loading...", Point::new(10, 20), text_style)
                    .draw(&mut display)
                    .unwrap();
                display.flush().unwrap();

                // Simulate quote fetching
                delay.delay_millis(1000);
                gadget.simulate_quote_fetch();
            }

            State::DisplayingQuote => {
                info!("Displaying quote: {:?}", gadget.current_quote);
                display.clear(BinaryColor::Off).unwrap();

                if let Some(quote) = &gadget.current_quote {
                    // Split quote into lines for display
                    let lines = format_quote_lines(quote);

                    // Display lines
                    for (i, line) in lines.iter().take(4).enumerate() {
                        // Max 4 lines
                        Text::new(line, Point::new(5, 15 + ((i as i32) * 12)), text_style)
                            .draw(&mut display)
                            .unwrap();
                    }
                }

                Text::new("Press for time", Point::new(10, 55), text_style)
                    .draw(&mut display)
                    .unwrap();
                display.flush().unwrap();
            }
        }

        delay.delay_millis(100); // Small delay for button debouncing
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.23.1/examples/src/bin
}
