#![no_std]
#![no_main]

use blink::{Event, SmartGadget, State};
use core::fmt::Write;
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

/// Formats a quote into displayable lines, wrapping at approximately 18 characters
/// to fit better on the 128x32 display
fn format_quote_lines(quote: &str) -> HVec<HString<64>, 8> {
    let words: HVec<&str, 64> = quote.split_whitespace().collect();
    let mut lines: HVec<HString<64>, 8> = HVec::new();
    let mut current_line = HString::new();

    for word in words {
        // Skip words that are too long to fit on a single line
        if word.len() > 18 {
            // Truncate very long words
            let truncated = if word.len() > 15 { &word[..15] } else { word };
            if !current_line.is_empty() {
                if let Ok(_) = lines.push(current_line.clone()) {
                    current_line = HString::from(truncated);
                }
            } else {
                current_line = HString::from(truncated);
            }
            continue;
        }

        // Account for space character when checking length
        let space_needed = if current_line.is_empty() { 0 } else { 1 };
        if current_line.len() + space_needed + word.len() <= 18 {
            // ~18 chars per line to fit better on 128px width
            if !current_line.is_empty() {
                if let Err(_) = current_line.push(' ') {
                    // If we can't add space, push current line and start new one
                    if let Ok(_) = lines.push(current_line.clone()) {
                        current_line = HString::new();
                        let _ = current_line.push_str(word);
                    }
                    continue;
                }
            }
            if let Err(_) = current_line.push_str(word) {
                // If we can't add word, push current line and start new one
                if let Ok(_) = lines.push(current_line.clone()) {
                    current_line = HString::from(word);
                }
            }
        } else {
            if !current_line.is_empty() {
                if let Err(_) = lines.push(current_line.clone()) {
                    // If we can't add more lines, break
                    break;
                }
            }
            current_line = HString::from(word);
        }
    }
    if !current_line.is_empty() {
        let _ = lines.push(current_line);
    }

    lines
}

/// Formats elapsed seconds into a time string (HH:MM:SS)
fn format_time(total_seconds: u32) -> HString<16> {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    let mut time_str = HString::new();
    // Format as HH:MM:SS
    if hours < 10 {
        time_str.push('0').unwrap();
    }
    write!(&mut time_str, "{}:", hours).unwrap();

    if minutes < 10 {
        time_str.push('0').unwrap();
    }
    write!(&mut time_str, "{}:", minutes).unwrap();

    if seconds < 10 {
        time_str.push('0').unwrap();
    }
    write!(&mut time_str, "{}", seconds).unwrap();

    time_str
}

/// Formats countdown seconds into MM:SS format
fn format_countdown(total_seconds: u32) -> HString<16> {
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;

    let mut countdown_str = HString::new();
    // Format as MM:SS
    if minutes < 10 {
        countdown_str.push('0').unwrap();
    }
    write!(&mut countdown_str, "{}:", minutes).unwrap();

    if seconds < 10 {
        countdown_str.push('0').unwrap();
    }
    write!(&mut countdown_str, "{}", seconds).unwrap();

    countdown_str
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

                // Format and display the current time
                let time_string = format_time(seconds_elapsed);
                if let Err(e) =
                    Text::new(&time_string, Point::new(25, 10), text_style).draw(&mut display)
                {
                    info!("Failed to draw time: {:?}", e);
                    continue;
                }

                if let Err(e) =
                    Text::new("Press for quote", Point::new(2, 24), text_style).draw(&mut display)
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
                    Text::new("Loading...", Point::new(25, 16), text_style).draw(&mut display)
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

                    // Auto-scroll through long quotes every 3 seconds
                    if counter % 30 == 0 && lines.len() > 2 {
                        gadget.scroll_quote(lines.len());
                    }

                    // Display current lines based on scroll offset
                    let start_idx = gadget.quote_line_offset;

                    // Show first line
                    if let Some(first_line) = lines.get(start_idx) {
                        if let Err(e) =
                            Text::new(first_line, Point::new(2, 10), text_style).draw(&mut display)
                        {
                            info!("Failed to draw quote line: {:?}", e);
                            continue;
                        }
                    }

                    // Show second line if available
                    if let Some(second_line) = lines.get(start_idx + 1) {
                        if let Err(e) =
                            Text::new(second_line, Point::new(2, 20), text_style).draw(&mut display)
                        {
                            info!("Failed to draw second quote line: {:?}", e);
                            continue;
                        }
                    }

                    // Show scroll indicator if there are more lines
                    if lines.len() > 2 {
                        let indicator = if start_idx + 2 < lines.len() {
                            "..."
                        } else {
                            "..."
                        };
                        if let Err(e) =
                            Text::new(indicator, Point::new(110, 20), text_style).draw(&mut display)
                        {
                            info!("Failed to draw scroll indicator: {:?}", e);
                            continue;
                        }
                    }

                    // Only show instruction if we have 1 line or less (to avoid overlap)
                    if lines.len() <= 1 {
                        if let Err(e) = Text::new("Press countdown", Point::new(2, 24), text_style)
                            .draw(&mut display)
                        {
                            info!("Failed to draw instruction text: {:?}", e);
                            continue;
                        }
                    }
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

            State::DisplayingCountdown => {
                info!("Displaying countdown: {}", gadget.countdown_seconds);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                // Display "COUNTDOWN" label
                if let Err(e) =
                    Text::new("COUNTDOWN", Point::new(25, 10), text_style).draw(&mut display)
                {
                    info!("Failed to draw countdown label: {:?}", e);
                    continue;
                }

                // Format and display the countdown time
                let countdown_string = format_countdown(gadget.countdown_seconds);
                if let Err(e) =
                    Text::new(&countdown_string, Point::new(35, 22), text_style).draw(&mut display)
                {
                    info!("Failed to draw countdown time: {:?}", e);
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
                    info!("All flush attempts failed for countdown display");
                }

                // Check if countdown finished
                if gadget.countdown_seconds == 0 {
                    info!("Countdown finished!");
                    // Could add a beep or notification here
                }
            }
        }

        delay.delay_millis(100); // Small delay for button debouncing
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.23.1/examples/src/bin
}
