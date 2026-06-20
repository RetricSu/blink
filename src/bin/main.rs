#![no_std]
#![no_main]

use blink::{util, Event, SmartGadget, State};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
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
    macro_rules! draw_text {
        ($display:expr, $text:expr, $point:expr, $style:expr, $err_msg:expr) => {
            if let Err(e) = Text::new($text, $point, $style).draw(&mut $display) {
                info!(concat!($err_msg, ": {:?}"), e);
            }
        };
    }

    macro_rules! flush_display {
        ($display:expr, $delay:expr, $context:expr) => {
            let mut flush_success = false;
            for attempt in 1..=3 {
                match $display.flush() {
                    Ok(_) => {
                        flush_success = true;
                        break;
                    }
                    Err(e) => {
                        info!("Failed to flush display on attempt {}: {:?}", attempt, e);
                        if attempt < 3 {
                            $delay.delay_millis(10);
                        }
                    }
                }
            }
            if !flush_success {
                info!("All flush attempts failed for {}", $context);
            }
        };
    }

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
    gadget.handle_event(Event::ButtonPress);
    gadget.simulate_quote_fetch();

    let mut counter = 0;
    let mut seconds_elapsed = 0u32; // Track seconds since boot

    loop {
        // Simulate button press every 5 seconds for demo
        counter += 1;
        if counter % 50 == 0 {
            // Every 5 seconds (50 * 100ms)
            info!("Simulating button press!");
            gadget.handle_event(Event::ButtonPress);
        }

        // Update time tracking every 10 iterations (approximately every second)
        if counter % 10 == 0 {
            seconds_elapsed += 1;
            // Also tick countdown if we're in countdown mode
            if gadget.state == State::DisplayingCountdown {
                gadget.tick_countdown();
            }
        }

        // Handle state transitions
        match gadget.state {
            State::DisplayingTime => {
                info!("Displaying time: {}", seconds_elapsed);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                // Format and display the current time - centered for 128x32 screen
                let time_string = util::format_time(seconds_elapsed, true);
                draw_text!(
                    display,
                    &time_string,
                    Point::new(30, 8),
                    text_style,
                    "Failed to draw time"
                );

                draw_text!(
                    display,
                    "Press for quote",
                    Point::new(2, 22),
                    text_style,
                    "Failed to draw text"
                );

                // Flush with retry logic
                flush_display!(display, delay, "time display");
            }

            State::FetchingQuote => {
                info!("Fetching quote");
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                draw_text!(
                    display,
                    "Loading...",
                    Point::new(30, 16),
                    text_style,
                    "Failed to draw text"
                );

                // Flush with retry logic
                flush_display!(display, delay, "loading display");

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
                    let lines = util::format_quote_lines(quote, 20, 8);

                    // Auto-scroll through long quotes every 4 seconds for better readability
                    if counter % 40 == 0 && lines.len() > 3 {
                        gadget.scroll_quote(lines.len());
                    }

                    // Display current lines based on scroll offset
                    let start_idx = gadget.quote_line_offset;

                    // Optimized layout for 128x32 screen with 6x10 font
                    let lines_to_display = if lines.len() > 3 { 2 } else { 3 };
                    for (i, line) in lines
                        .iter()
                        .skip(start_idx)
                        .take(lines_to_display)
                        .enumerate()
                    {
                        // y=8 for first line, then +10 for each subsequent line
                        let y = 8 + (i as i32 * 10);
                        draw_text!(
                            display,
                            line,
                            Point::new(2, y),
                            text_style,
                            "Failed to draw quote line"
                        );
                    }

                    // Show scroll indicator if there are more than 3 lines
                    if lines.len() > 3 {
                        let indicator = "...";
                        draw_text!(
                            display,
                            indicator,
                            Point::new(110, 28),
                            text_style,
                            "Failed to draw scroll indicator"
                        );
                    }

                    // Show instruction only if we have 2 lines or less
                    if lines.len() <= 2 {
                        draw_text!(
                            display,
                            "Press countdown",
                            Point::new(2, 28),
                            text_style,
                            "Failed to draw instruction text"
                        );
                    }
                }

                // Flush with retry logic
                flush_display!(display, delay, "quote display");
            }

            State::DisplayingCountdown => {
                info!("Displaying countdown: {}", gadget.countdown_seconds);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                // Display "COUNTDOWN" label - centered for 128x32 screen
                draw_text!(
                    display,
                    "COUNTDOWN",
                    Point::new(20, 8),
                    text_style,
                    "Failed to draw countdown label"
                );

                // Format and display the countdown time
                let countdown_string = util::format_time(gadget.countdown_seconds, false);
                draw_text!(
                    display,
                    &countdown_string,
                    Point::new(35, 22),
                    text_style,
                    "Failed to draw countdown time"
                );

                // Flush with retry logic
                flush_display!(display, delay, "countdown display");

                // Check if countdown finished
                if gadget.countdown_seconds == 0 {
                    // Could add a beep or notification here
                }
            }

            State::FetchingPrice => {
                info!("Fetching price for {:?}", gadget.current_asset);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                draw_text!(
                    display,
                    "Loading price...",
                    Point::new(10, 16),
                    text_style,
                    "Failed to draw text"
                );

                flush_display!(display, delay, "price loading display");

                // Basic version uses simulation because the HTTP client is HTTP-only
                // and Binance requires HTTPS/TLS.
                delay.delay_millis(800);
                gadget.simulate_price_fetch();
            }

            State::DisplayingPrice => {
                info!("Displaying price: {:?}", gadget.current_price);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                // Asset label at the top
                draw_text!(
                    display,
                    gadget.current_asset.display_name(),
                    Point::new(50, 8),
                    text_style,
                    "Failed to draw asset label"
                );

                // Price or fallback text
                let price_text: heapless::String<128> = gadget
                    .current_price
                    .clone()
                    .unwrap_or_else(|| heapless::String::from("--"));
                draw_text!(
                    display,
                    &price_text,
                    Point::new(10, 24),
                    text_style,
                    "Failed to draw price"
                );

                flush_display!(display, delay, "price display");

                // Auto-cycle asset every 5 seconds while in price mode
                if counter % 50 == 0 {
                    info!("Auto-cycling asset");
                    gadget.handle_event(Event::AssetTick);
                }
            }
        }

        delay.delay_millis(100); // Small delay for button debouncing
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.23.1/examples/src/bin
}
