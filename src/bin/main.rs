#![no_std]
#![no_main]

use blink::{
    hardware::{Button, DisplayWrapper, Timer},
    renderer, Event, SmartGadget, State,
};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::i2c::master::I2c;
use esp_hal::main;
use esp_hal::time::RateExtU32;
use log::info;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

#[main]
fn main() -> ! {
    // Initialize logging and hardware
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // Initialize I2C for display
    let sda = peripherals.GPIO8;
    let scl = peripherals.GPIO9;
    let i2c_config = esp_hal::i2c::master::Config::default().with_frequency(100u32.kHz());
    let i2c = I2c::new(peripherals.I2C0, i2c_config)
        .unwrap()
        .with_scl(scl)
        .with_sda(sda);

    info!("Initializing display...");

    // Initialize display
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    // Initialize display with error handling
    match display.init() {
        Ok(_) => info!("Display initialized successfully"),
        Err(e) => {
            info!("Failed to initialize display: {:?}", e);
            // Continue anyway for demo purposes
        }
    }

    // Initialize hardware abstractions
    let mut display_wrapper = DisplayWrapper::new();
    let mut timer = Timer::new(100); // 100ms tick interval
    let mut countdown_timer = Timer::new(1000); // 1 second for countdown ticks
    let mut button = Button::new();

    // Initialize the state machine
    let mut gadget = SmartGadget::new();

    // Event loop state
    let mut counter = 0u32;

    info!("Starting main event loop...");

    loop {
        let current_time_ms = counter * 100; // Simple time tracking

        // === EVENT GENERATION ===

        // Check for button events (placeholder for now)
        if let Some(event) = button.check_press() {
            gadget.handle_event(event);
        }

        // Simulate button press every 5 seconds for demo
        if counter % 50 == 0 && counter > 0 {
            info!("Simulating button press!");
            gadget.handle_event(Event::ButtonPress);
        }

        // Start countdown after 3 quote cycles (15 seconds)
        if counter == 150 {
            info!("Starting countdown!");
            gadget.handle_event(Event::StartCountdown);
        }

        // Generate countdown tick events
        if gadget.state == State::DisplayingCountdown
            && countdown_timer.should_tick(current_time_ms)
        {
            gadget.tick_countdown();
        }

        // === RENDERING ===

        // Handle auto-scrolling for quotes
        if gadget.state == State::DisplayingQuote {
            renderer::handle_auto_scroll(&mut gadget, counter);
        }

        // Render current state
        let render_success =
            renderer::render(&gadget.state, &gadget, &mut display, &display_wrapper);

        if !render_success {
            info!("Rendering failed, skipping flush");
            timer.delay_ms(100);
            counter += 1;
            continue;
        }

        // Flush display with retry logic
        let flush_success = display_wrapper.try_flush(&mut || display.flush());

        if !flush_success {
            info!("All flush attempts failed");
        }

        // === TIMING ===

        // Sleep for the tick interval
        timer.delay_ms(100);
        counter += 1;

        // Handle countdown completion
        if gadget.state == State::DisplayingCountdown && gadget.countdown_seconds == 0 {
            info!("Countdown finished!");
            // Could add a beep or notification here
        }
    }
}
