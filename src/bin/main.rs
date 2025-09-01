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

    // Create I2C with proper configuration
    let i2c_config = esp_hal::i2c::master::Config::default().with_frequency(100u32.kHz()); // Try 100 kHz for better compatibility

    let i2c = I2c::new(peripherals.I2C0, i2c_config)
        .unwrap()
        .with_scl(scl)
        .with_sda(sda);

    info!("Initializing I2C display...");

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    // Initialize display with error handling
    match display.init() {
        Ok(_) => info!("Display initialized successfully"),
        Err(e) => {
            info!("Failed to initialize display: {:?}", e);
            // Continue anyway, but log the error
        }
    }

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
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                if let Err(e) =
                    Text::new("Time Mode", Point::new(10, 12), text_style).draw(&mut display)
                {
                    info!("Failed to draw text: {:?}", e);
                    continue;
                }

                if let Err(e) =
                    Text::new("Press for quote", Point::new(10, 24), text_style).draw(&mut display)
                {
                    info!("Failed to draw text: {:?}", e);
                    continue;
                }

                // Flush with retry logic
                let mut flush_success = false;
                for attempt in 1..=3 {
                    match display.flush() {
                        Ok(_) => {
                            flush_success = true;
                            break;
                        }
                        Err(e) => {
                            info!("Failed to flush display on attempt {}: {:?}", attempt, e);
                            if attempt < 3 {
                                delay.delay_millis(10);
                            }
                        }
                    }
                }
                if !flush_success {
                    info!("All flush attempts failed for time display");
                }
            }

            State::FetchingQuote => {
                info!("Fetching quote");
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                if let Err(e) =
                    Text::new("Loading...", Point::new(10, 16), text_style).draw(&mut display)
                {
                    info!("Failed to draw text: {:?}", e);
                    continue;
                }

                // Flush with retry logic
                let mut flush_success = false;
                for attempt in 1..=3 {
                    match display.flush() {
                        Ok(_) => {
                            flush_success = true;
                            break;
                        }
                        Err(e) => {
                            info!("Failed to flush display on attempt {}: {:?}", attempt, e);
                            if attempt < 3 {
                                delay.delay_millis(10);
                            }
                        }
                    }
                }
                if !flush_success {
                    info!("All flush attempts failed for loading display");
                }

                // Simulate quote fetching
                delay.delay_millis(1000);
                gadget.simulate_quote_fetch();
            }

            State::DisplayingQuote => {
                info!("Displaying quote: {:?}", gadget.current_quote);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                if let Some(quote) = &gadget.current_quote {
                    // Split quote into lines for display
                    let lines = format_quote_lines(quote);

                    // Display lines (only 2 lines for 128x32 display)
                    for (i, line) in lines.iter().take(2).enumerate() {
                        // Max 2 lines for 32px height
                        if let Err(e) =
                            Text::new(line, Point::new(5, 12 + ((i as i32) * 10)), text_style)
                                .draw(&mut display)
                        {
                            info!("Failed to draw quote line {}: {:?}", i, e);
                            continue;
                        }
                    }
                }

                if let Err(e) =
                    Text::new("Press for time", Point::new(10, 24), text_style).draw(&mut display)
                {
                    info!("Failed to draw text: {:?}", e);
                    continue;
                }

                // Flush with retry logic
                let mut flush_success = false;
                for attempt in 1..=3 {
                    match display.flush() {
                        Ok(_) => {
                            flush_success = true;
                            break;
                        }
                        Err(e) => {
                            info!("Failed to flush display on attempt {}: {:?}", attempt, e);
                            if attempt < 3 {
                                delay.delay_millis(10);
                            }
                        }
                    }
                }
                if !flush_success {
                    info!("All flush attempts failed for quote display");
                }
            }
        }

        delay.delay_millis(100); // Small delay for button debouncing
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.23.1/examples/src/bin
}
