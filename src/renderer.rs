use embedded_graphics::{pixelcolor::BinaryColor, prelude::*, text::Text};
use log::info;

use crate::{hardware::DisplayWrapper, util, SmartGadget, State};

/// Macro for safe text drawing with error handling
macro_rules! draw_text {
    ($display:expr, $text:expr, $point:expr, $style:expr, $err_msg:expr) => {
        if let Err(_e) = Text::new($text, $point, $style).draw($display) {
            info!(concat!($err_msg, ": error drawing text"));
            return false;
        }
    };
}

/// Render the current state of the SmartGadget to the display
/// Returns true if rendering was successful, false otherwise
pub fn render<D>(
    state: &State,
    gadget: &SmartGadget,
    display: &mut D,
    display_wrapper: &DisplayWrapper,
) -> bool
where
    D: embedded_graphics::draw_target::DrawTarget<Color = BinaryColor>
        + embedded_graphics::draw_target::DrawTargetExt,
{
    // Clear the display first
    if let Err(_e) = display.clear(BinaryColor::Off) {
        info!("Failed to clear display: error clearing");
        return false;
    }

    match state {
        State::DisplayingQuote => render_quote(gadget, display, display_wrapper),
        State::DisplayingCountdown => render_countdown(gadget, display, display_wrapper),
    }
}

/// Render quote display state
fn render_quote<D>(gadget: &SmartGadget, display: &mut D, display_wrapper: &DisplayWrapper) -> bool
where
    D: embedded_graphics::draw_target::DrawTarget<Color = BinaryColor>,
{
    info!("Rendering quote: {:?}", gadget.current_quote);

    let text_style = display_wrapper.text_style();

    // Split quote into lines for display
    let lines = util::format_quote_lines(&gadget.current_quote, 20, 8);

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
            "Press for next",
            Point::new(2, 28),
            text_style,
            "Failed to draw instruction text"
        );
    }

    true
}

/// Render countdown display state
fn render_countdown<D>(
    gadget: &SmartGadget,
    display: &mut D,
    display_wrapper: &DisplayWrapper,
) -> bool
where
    D: embedded_graphics::draw_target::DrawTarget<Color = BinaryColor>,
{
    info!("Rendering countdown: {}", gadget.countdown_seconds);

    let text_style = display_wrapper.text_style();

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

    true
}

/// Handle auto-scrolling for long quotes
/// Returns true if scrolling occurred, false otherwise
pub fn handle_auto_scroll(gadget: &mut SmartGadget, counter: u32) -> bool {
    let lines = util::format_quote_lines(&gadget.current_quote, 20, 8);

    // Auto-scroll through long quotes every 4 seconds for better readability
    if counter % 40 == 0 && lines.len() > 3 {
        gadget.scroll_quote(lines.len());
        return true;
    }

    false
}
