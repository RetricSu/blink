#![no_std]

use core::fmt::Write;
use heapless::String as HString;
use heapless::Vec as HVec;

// 1. Define your States and Events as enums
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    DisplayingTime,
    DisplayingQuote,
    FetchingQuote,
    DisplayingCountdown,
}

#[derive(Debug, Clone)]
pub enum Event {
    ButtonPress,
    QuoteReceived(HString<128>), // Event can carry data!
    FetchFailed,
    CountdownTick,
    CountdownFinished,
}

// 2. Create a struct for your gadget
pub struct SmartGadget {
    pub state: State,
    pub current_quote: Option<HString<128>>,
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
        Self {
            state: State::DisplayingTime,
            current_quote: None,
            quotes: &[
                "The only way to do great work is to love what you do.",
                "Life is what happens when you're busy making other plans.",
                "Success is not final, failure is not fatal: it is the courage to continue that counts.",
                "The future belongs to those who believe in the beauty of their dreams.",
                "In the middle of difficulty lies opportunity.",
                "Don't watch the clock; do what it does. Keep going.",
                "The best way to predict the future is to invent it.",
                "Everything you've ever wanted is on the other side of fear.",
            ],
            current_quote_index: 0,
            countdown_seconds: 0,
            countdown_original: 0,
            quote_line_offset: 0,
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match (&self.state, event) {
            // If we're showing time and the button is pressed...
            (State::DisplayingTime, Event::ButtonPress) => {
                self.state = State::FetchingQuote;
                // ACTION: Start the network request, show "Loading..."
            }

            // If we're fetching a quote and it arrives...
            (State::FetchingQuote, Event::QuoteReceived(quote)) => {
                self.current_quote = Some(quote);
                self.quote_line_offset = 0; // Reset scroll position for new quote
                self.state = State::DisplayingQuote;
                // ACTION: Display the new quote on the screen
            }

            // If we're showing a quote and the button is pressed...
            (State::DisplayingQuote, Event::ButtonPress) => {
                self.start_countdown(30); // Start 30-second countdown
                self.state = State::DisplayingCountdown;
                // ACTION: Switch to countdown mode
            }

            // If we're in countdown and button is pressed...
            (State::DisplayingCountdown, Event::ButtonPress) => {
                self.state = State::DisplayingTime;
                // ACTION: Go back to time mode
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
                self.state = State::DisplayingTime;
                // ACTION: Countdown finished, return to time
            }

            // You can handle error cases, too
            (State::FetchingQuote, Event::FetchFailed) => {
                self.state = State::DisplayingTime;
                // ACTION: Show an error icon, then switch to time
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

    pub fn simulate_quote_fetch(&mut self) {
        // Simulate fetching a quote (in real implementation, this would be async)
        let quote = self.get_next_quote();
        self.handle_event(Event::QuoteReceived(quote));
    }

    pub fn start_countdown(&mut self, seconds: u32) {
        self.countdown_seconds = seconds;
        self.countdown_original = seconds;
    }

    pub fn tick_countdown(&mut self) {
        self.handle_event(Event::CountdownTick);
    }

    pub fn scroll_quote(&mut self, total_lines: usize) {
        // Scroll through quote lines, showing 3 lines at a time for 128x32 screen
        if total_lines > 3 {
            self.quote_line_offset = (self.quote_line_offset + 1) % (total_lines - 2);
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
            // Skip words that are too long to fit on a single line
            if word.len() > max_chars_per_line {
                // Truncate very long words to fit the specified width
                let truncated = if word.len() > max_chars_per_line - 2 {
                    &word[..max_chars_per_line - 2]
                } else {
                    word
                };
                if !current_line.is_empty() {
                    if lines.push(current_line.clone()).is_ok() {
                        current_line = HString::from(truncated);
                    }
                } else {
                    current_line = HString::from(truncated);
                }
                continue;
            }

            // Account for space character when checking length
            let space_needed = if current_line.is_empty() { 0 } else { 1 };
            if current_line.len() + space_needed + word.len() <= max_chars_per_line {
                // Add word to current line
                if !current_line.is_empty() && current_line.push(' ').is_err() {
                    // If we can't add space, push current line and start new one
                    if lines.push(current_line.clone()).is_ok() {
                        current_line = HString::new();
                        let _ = current_line.push_str(word);
                    }
                    continue;
                }
                if current_line.push_str(word).is_err() {
                    // If we can't add word, push current line and start new one
                    if lines.push(current_line.clone()).is_ok() {
                        current_line = HString::from(word);
                    }
                }
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
            // Format as HH:MM:SS
            if hours < 10 {
                time_str.push('0').unwrap();
            }
            write!(&mut time_str, "{}:", hours).unwrap();
        }

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

    /// Formats countdown seconds into time format with configurable display
    ///
    /// # Arguments
    /// * `total_seconds` - Total seconds for countdown
    /// * `show_hours` - Whether to include hours in the format (true: HH:MM:SS, false: MM:SS)
    pub fn format_countdown(total_seconds: u32, show_hours: bool) -> HString<16> {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        let mut countdown_str = HString::new();

        if show_hours {
            // Format as HH:MM:SS
            if hours < 10 {
                countdown_str.push('0').unwrap();
            }
            write!(&mut countdown_str, "{}:", hours).unwrap();
        }

        // Format minutes
        if minutes < 10 {
            countdown_str.push('0').unwrap();
        }
        write!(&mut countdown_str, "{}:", minutes).unwrap();

        // Format seconds
        if seconds < 10 {
            countdown_str.push('0').unwrap();
        }
        write!(&mut countdown_str, "{}", seconds).unwrap();

        countdown_str
    }
}
