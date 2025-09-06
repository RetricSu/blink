#![no_std]

use core::fmt::Write;
use heapless::String as HString;
use heapless::Vec as HVec;

// Hardware abstraction modules
pub mod hardware;
pub mod renderer;

// 1. Define your States and Events as enums
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    DisplayingQuote,
    DisplayingCountdown,
}

#[derive(Debug, Clone)]
pub enum Event {
    ButtonPress,
    StartCountdown,
    CountdownTick,
    CountdownFinished,
}

// 2. Create a struct for your gadget
pub struct SmartGadget {
    pub state: State,
    pub current_quote: HString<128>,
    pub quotes: &'static [&'static str],
    pub current_quote_index: usize,
    pub countdown_seconds: u32,
    pub countdown_original: u32,
    pub quote_line_offset: usize, // For scrolling through long quotes
}

// 3. Implement a method to handle events
impl Default for SmartGadget {
    fn default() -> Self {
        Self::new()
    }
}

impl SmartGadget {
    pub fn new() -> Self {
        let quotes = &[
            "The only way to do great work is to love what you do.",
            "Life is what happens when you're busy making other plans.",
            "Success is not final, failure is not fatal: it is the courage to continue that counts.",
            "The future belongs to those who believe in the beauty of their dreams.",
            "In the middle of difficulty lies opportunity.",
            "Don't watch the clock; do what it does. Keep going.",
            "The best way to predict the future is to invent it.",
            "Everything you've ever wanted is on the other side of fear.",
        ];
        Self {
            state: State::DisplayingQuote,
            current_quote: HString::from(quotes[0]),
            quotes,
            current_quote_index: 0,
            countdown_seconds: 0,
            countdown_original: 0,
            quote_line_offset: 0,
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match (&self.state, event) {
            // If we're showing a quote and the button is pressed...
            (State::DisplayingQuote, Event::ButtonPress) => {
                self.switch_to_next_quote(); // Switch to next quote
                                             // ACTION: Display the new quote
            }

            // If we want to start countdown...
            (State::DisplayingQuote, Event::StartCountdown) => {
                self.start_countdown(30); // Start 30-second countdown
                self.state = State::DisplayingCountdown;
                // ACTION: Switch to countdown mode
            }

            // If we're in countdown and button is pressed...
            (State::DisplayingCountdown, Event::ButtonPress) => {
                self.state = State::DisplayingQuote;
                // ACTION: Go back to quote mode
            }

            // Countdown tick event
            (State::DisplayingCountdown, Event::CountdownTick) => {
                if self.countdown_seconds > 0 {
                    self.countdown_seconds -= 1;
                } else {
                    self.handle_event(Event::CountdownFinished);
                }
            }

            // Countdown finished
            (State::DisplayingCountdown, Event::CountdownFinished) => {
                self.state = State::DisplayingQuote;
                // ACTION: Countdown finished, return to quote
            }

            // Catch-all for unhandled combinations
            _ => { /* Do nothing for other combinations */ }
        }
    }

    pub fn get_next_quote(&mut self) -> HString<128> {
        let quote = self.quotes[self.current_quote_index];
        self.current_quote_index = (self.current_quote_index + 1) % self.quotes.len();
        HString::from(quote)
    }

    pub fn switch_to_next_quote(&mut self) {
        self.current_quote = self.get_next_quote();
        self.quote_line_offset = 0; // Reset scroll position for new quote
    }

    pub fn start_countdown(&mut self, seconds: u32) {
        self.countdown_seconds = seconds;
        self.countdown_original = seconds;
    }

    pub fn tick_countdown(&mut self) {
        self.handle_event(Event::CountdownTick);
    }

    pub fn scroll_quote(&mut self, total_lines: usize) {
        // Scroll through quote lines. When total_lines > 3, we show 2 lines at a time.
        if total_lines > 3 {
            // The number of lines displayed is 2. The last start_idx is total_lines - 2.
            // The number of possible start indices is total_lines - 1.
            self.quote_line_offset = (self.quote_line_offset + 1) % (total_lines - 1);
        }
    }
}

/// Utility functions for formatting text and time
pub mod util {
    use super::*;

    /// Formats a quote into displayable lines with configurable parameters
    ///
    /// # Arguments
    /// * `quote` - The quote text to format
    /// * `max_chars_per_line` - Maximum characters per line (default: 20)
    /// * `max_lines` - Maximum number of lines to generate (default: 8)
    pub fn format_quote_lines(
        quote: &str,
        max_chars_per_line: usize,
        max_lines: usize,
    ) -> HVec<HString<64>, 8> {
        let words: HVec<&str, 64> = quote.split_whitespace().collect();
        let mut lines: HVec<HString<64>, 8> = HVec::new();
        let mut current_line = HString::new();

        for word in words {
            // Handle words that are too long to fit on a single line
            if word.len() > max_chars_per_line {
                if !current_line.is_empty() && lines.push(current_line.clone()).is_err() {
                    break; // No more lines available
                }
                current_line = HString::new();

                // Break the long word into multiple lines instead of truncating
                let mut remaining_word = word;
                while !remaining_word.is_empty() {
                    if lines.len() >= max_lines {
                        break;
                    }
                    let mut end_idx = core::cmp::min(remaining_word.len(), max_chars_per_line);
                    while !remaining_word.is_char_boundary(end_idx) && end_idx > 0 {
                        end_idx -= 1;
                    }

                    let (chunk, rest) = remaining_word.split_at(end_idx);
                    if lines.push(HString::from(chunk)).is_err() {
                        break;
                    }
                    remaining_word = rest;
                }
                continue;
            }

            // Account for space character when checking length
            let space_needed = if current_line.is_empty() { 0 } else { 1 };
            if current_line.len() + space_needed + word.len() <= max_chars_per_line {
                // Add word to current line
                if !current_line.is_empty() {
                    // This is safe because HString capacity (64) > max_chars_per_line (e.g., 20)
                    let _ = current_line.push(' ');
                }
                // This is also safe due to the length check on the parent if-statement.
                let _ = current_line.push_str(word);
            } else {
                if !current_line.is_empty() && lines.push(current_line.clone()).is_err() {
                    // If we can't add more lines, break
                    break;
                }
                current_line = HString::from(word);
            }

            // Check if we've reached the maximum number of lines
            if lines.len() >= max_lines {
                break;
            }
        }
        if !current_line.is_empty() && lines.len() < max_lines {
            let _ = lines.push(current_line);
        }

        lines
    }

    /// Formats elapsed seconds into a time string with configurable format
    ///
    /// # Arguments
    /// * `total_seconds` - Total seconds to format
    /// * `show_hours` - Whether to include hours in the format (true: HH:MM:SS, false: MM:SS)
    pub fn format_time(total_seconds: u32, show_hours: bool) -> HString<16> {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        let mut time_str = HString::new();

        if show_hours {
            // Format as HH:MM:SS, ignoring potential errors if the string is full.
            let _ = write!(&mut time_str, "{:02}:", hours);
        }

        // Format minutes and seconds
        let _ = write!(&mut time_str, "{:02}:{:02}", minutes, seconds);

        time_str
    }
}
