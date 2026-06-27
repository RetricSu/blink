#![no_std]
#![no_main]

use blink::{util, Event, SmartGadget, State};
use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{
        ascii::{FONT_10X20, FONT_5X7, FONT_6X10},
        MonoTextStyle,
    },
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle},
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

#[cfg(feature = "network")]
use blink::price::fetch_price;
#[cfg(feature = "network")]
use blink::wifi::NetworkStack;

#[cfg(feature = "network")]
const WIFI_SSID: &str = match option_env!("BLINK_WIFI_SSID") {
    Some(ssid) => ssid,
    None => "YOUR_SSID",
};
#[cfg(feature = "network")]
const WIFI_PASSWORD: &str = match option_env!("BLINK_WIFI_PASSWORD") {
    Some(password) => password,
    None => "YOUR_PASSWORD",
};

/// Display dimensions (must match the `DisplaySize128x32` used below).
const SCREEN_WIDTH: i32 = 128;
const SCREEN_HEIGHT: i32 = 32;

/// Dimensions of the large price font (`FONT_10X20`).
const PRICE_FONT_WIDTH: i32 = 10;

/// Top-left position of the small asset label.
const LABEL_X: i32 = 2;
const LABEL_Y: i32 = 7;

/// Top-right position of the status label.
const STATUS_TEXT: &str = "LIVE";
const STATUS_X: i32 = SCREEN_WIDTH - (STATUS_TEXT.len() as i32 * 5) - 2;
const STATUS_Y: i32 = LABEL_Y;

/// Horizontal rule separating the status line from the price.
const HEADER_RULE_Y: i32 = 10;

/// Baseline Y position for the large price text.
const PRICE_Y: i32 = SCREEN_HEIGHT - 1;

fn draw_price_screen<D>(
    display: &mut D,
    asset_label: &str,
    price_text: &str,
    label_style: MonoTextStyle<'_, BinaryColor>,
    price_style: MonoTextStyle<'_, BinaryColor>,
) where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    if let Err(e) = display.clear(BinaryColor::Off) {
        info!("Failed to clear display: {:?}", e);
        return;
    }

    if let Err(e) = Text::new(asset_label, Point::new(LABEL_X, LABEL_Y), label_style).draw(display)
    {
        info!("Failed to draw asset label: {:?}", e);
    }

    if let Err(e) =
        Text::new(STATUS_TEXT, Point::new(STATUS_X, STATUS_Y), label_style).draw(display)
    {
        info!("Failed to draw status label: {:?}", e);
    }

    if let Err(e) = Line::new(
        Point::new(2, HEADER_RULE_Y),
        Point::new(SCREEN_WIDTH - 3, HEADER_RULE_Y),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
    .draw(display)
    {
        info!("Failed to draw header rule: {:?}", e);
    }

    let price_width = price_text.len() as i32 * PRICE_FONT_WIDTH;
    let price_x = (SCREEN_WIDTH - price_width) / 2;
    if let Err(e) = Text::new(price_text, Point::new(price_x, PRICE_Y), price_style).draw(display) {
        info!("Failed to draw price: {:?}", e);
    }
}

fn init_heap() {
    const HEAP_SIZE: usize = 64 * 1024;
    static mut HEAP: core::mem::MaybeUninit<[u8; HEAP_SIZE]> = core::mem::MaybeUninit::uninit();
    unsafe {
        let ptr = core::ptr::addr_of_mut!(HEAP) as *mut core::mem::MaybeUninit<[u8; HEAP_SIZE]>;
        let heap_bottom = (*ptr).as_mut_ptr() as *mut u8;
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            heap_bottom,
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
}

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

    init_heap();

    // ── Network stack (when `network` feature is enabled) ──────
    #[cfg(feature = "network")]
    let mut network_stack: Option<NetworkStack<'_>> = {
        match NetworkStack::init(
            peripherals.TIMG0,
            peripherals.RNG,
            peripherals.RADIO_CLK,
            peripherals.WIFI,
        ) {
            Ok(stack) => {
                info!("WiFi hardware initialized");
                Some(stack)
            }
            Err(e) => {
                info!("WiFi init failed: {:?}, will use simulation", e);
                None
            }
        }
    };

    // ── Display (uses remaining peripherals) ───────────────────
    let sda = peripherals.GPIO8;
    let scl = peripherals.GPIO9;

    let i2c_config = esp_hal::i2c::master::Config::default().with_frequency(100u32.kHz());

    let i2c = I2c::new(peripherals.I2C0, i2c_config)
        .unwrap()
        .with_scl(scl)
        .with_sda(sda);

    info!("Initializing I2C display...");

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    match display.init() {
        Ok(_) => info!("Display initialized successfully"),
        Err(e) => {
            info!("Failed to initialize display: {:?}", e);
        }
    }

    let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let label_style = MonoTextStyle::new(&FONT_5X7, BinaryColor::On);
    let price_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let delay = Delay::new();

    // ── Connect WiFi (when network is available) ───────────────
    #[cfg(feature = "network")]
    {
        if let Some(ref mut stack) = network_stack {
            if let Err(e) = display.clear(BinaryColor::Off) {
                info!("Failed to clear display: {:?}", e);
            }
            draw_text!(
                display,
                "Connecting WiFi",
                Point::new(0, 16),
                text_style,
                "Failed to draw"
            );
            flush_display!(display, delay, "wifi connecting");

            match stack.connect(WIFI_SSID, WIFI_PASSWORD) {
                Ok(_) => info!("WiFi associated, waiting for DHCP..."),
                Err(e) => info!("WiFi connect failed: {:?}, using simulation", e),
            }

            // Wait for DHCP to assign an IP (up to ~15 seconds)
            for _ in 0..150 {
                stack.poll();
                if stack.is_network_ready() {
                    info!("Network ready (DHCP configured)");
                    break;
                }
                delay.delay_millis(100);
            }
            if !stack.is_network_ready() {
                info!("DHCP timeout, will use simulation for prices");
            }
        }
    }

    // ── State machine init ─────────────────────────────────────
    let mut gadget = SmartGadget::new();

    gadget.state = State::FetchingPrice;

    let mut counter = 0;
    let mut seconds_elapsed = 0u32;
    let mut price_dirty = true;

    loop {
        // Poll the network stack every iteration
        #[cfg(feature = "network")]
        {
            if let Some(ref mut stack) = network_stack {
                stack.poll();
            }
        }

        counter += 1;
        if counter % 10 == 0 {
            seconds_elapsed += 1;
            if gadget.state == State::DisplayingCountdown {
                gadget.tick_countdown();
            }
        }

        match gadget.state {
            State::DisplayingTime => {
                info!("Displaying time: {}", seconds_elapsed);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

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

                flush_display!(display, delay, "loading display");

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
                    let lines = util::format_quote_lines(quote, 20, 8);

                    if counter % 40 == 0 && lines.len() > 3 {
                        gadget.scroll_quote(lines.len());
                    }

                    let start_idx = gadget.quote_line_offset;

                    let lines_to_display = if lines.len() > 3 { 2 } else { 3 };
                    for (i, line) in lines
                        .iter()
                        .skip(start_idx)
                        .take(lines_to_display)
                        .enumerate()
                    {
                        let y = 8 + (i as i32 * 10);
                        draw_text!(
                            display,
                            line,
                            Point::new(2, y),
                            text_style,
                            "Failed to draw quote line"
                        );
                    }

                    if lines.len() > 3 {
                        draw_text!(
                            display,
                            "...",
                            Point::new(110, 28),
                            text_style,
                            "Failed to draw scroll indicator"
                        );
                    }

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

                flush_display!(display, delay, "quote display");
            }

            State::DisplayingCountdown => {
                info!("Displaying countdown: {}", gadget.countdown_seconds);
                if let Err(e) = display.clear(BinaryColor::Off) {
                    info!("Failed to clear display: {:?}", e);
                    continue;
                }

                draw_text!(
                    display,
                    "COUNTDOWN",
                    Point::new(20, 8),
                    text_style,
                    "Failed to draw countdown label"
                );

                let countdown_string = util::format_time(gadget.countdown_seconds, false);
                draw_text!(
                    display,
                    &countdown_string,
                    Point::new(35, 22),
                    text_style,
                    "Failed to draw countdown time"
                );

                flush_display!(display, delay, "countdown display");

                if gadget.countdown_seconds == 0 {}
            }

            State::FetchingPrice => {
                info!("Fetching price for {:?}", gadget.current_asset);

                // ── Real fetch (network feature) vs simulation ──
                #[cfg(feature = "network")]
                {
                    let fetched = if let Some(ref mut stack) = network_stack {
                        if stack.is_network_ready() {
                            match fetch_price(stack, gadget.current_asset) {
                                Ok(price) => {
                                    let formatted = blink::price::format_asset_price(
                                        gadget.current_asset,
                                        price,
                                    );
                                    gadget.handle_event(Event::PriceReceived(formatted));
                                    true
                                }
                                Err(e) => {
                                    info!("Price fetch failed: {:?}, using simulation", e);
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if !fetched {
                        delay.delay_millis(800);
                        gadget.simulate_price_fetch();
                    }
                }

                #[cfg(not(feature = "network"))]
                {
                    delay.delay_millis(800);
                    gadget.simulate_price_fetch();
                }

                price_dirty = true;
            }

            State::DisplayingPrice => {
                if price_dirty {
                    info!("Displaying price: {:?}", gadget.current_price);
                    let price_text: &str = gadget.current_price.as_deref().unwrap_or("--");
                    draw_price_screen(
                        &mut display,
                        gadget.current_asset.display_name(),
                        price_text,
                        label_style,
                        price_style,
                    );

                    flush_display!(display, delay, "price display");
                    price_dirty = false;
                }

                if counter % 50 == 25 {
                    info!("Auto-cycling asset");
                    gadget.handle_event(Event::AssetTick);
                }
            }
        }

        delay.delay_millis(100);
    }
}
