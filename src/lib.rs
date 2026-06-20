#![cfg_attr(not(test), no_std)]

#[cfg(feature = "network")]
pub mod http;
#[cfg(feature = "network")]
pub mod wifi;
pub mod price;

use core::fmt::Write;
use heapless::String as HString;
use heapless::Vec as HVec;
use log::info;
use price::{Asset, ALL_ASSETS, format_price, simulate_fetch_price};

// 1. Define your States and Events as enums
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    DisplayingTime,
    DisplayingQuote,
    FetchingQuote,
    DisplayingCountdown,
    FetchingPrice,
    DisplayingPrice,
}

#[derive(Debug, Clone)]
pub enum Event {
    ButtonPress,
    QuoteReceived(HString<128>), // Event can carry data!
    FetchFailed,
    CountdownTick,
    CountdownFinished,
    PriceReceived(HString<128>),
    PriceFetchFailed,
    AssetTick,
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
    pub current_asset: Asset,
    pub current_price: Option<HString<128>>,
    pub asset_index: usize,
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
            current_asset: Asset::Btc,
            current_price: None,
            asset_index: 0,
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
                self.state = State::FetchingPrice;
                // ACTION: Start fetching price
            }

            // If we're fetching a price and it arrives...
            (State::FetchingPrice, Event::PriceReceived(price)) => {
                self.current_price = Some(price);
                self.state = State::DisplayingPrice;
                // ACTION: Display the price on the screen
            }

            // If we're fetching a price and it fails...
            (State::FetchingPrice, Event::PriceFetchFailed) => {
                self.state = State::DisplayingTime;
                // ACTION: Show error, then switch to time
            }

            // If we're showing price and the button is pressed...
            (State::DisplayingPrice, Event::ButtonPress) => {
                self.state = State::DisplayingTime;
                // ACTION: Go back to time mode
            }

            // Cycle to the next asset while displaying price
            (State::DisplayingPrice, Event::AssetTick) => {
                self.cycle_asset();
                self.state = State::FetchingPrice;
                // ACTION: Fetch next asset price
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
                info!("Countdown finished!");
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
        let safe_quote = if quote.len() > 128 {
            let mut limit = 128;
            while !quote.is_char_boundary(limit) {
                limit -= 1;
            }
            &quote[..limit]
        } else {
            quote
        };
        HString::from(safe_quote)
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
        // Scroll through quote lines. When total_lines > 3, we show 2 lines at a time.
        if total_lines > 3 {
            // The number of lines displayed is 2. The last start_idx is total_lines - 2.
            // The number of possible start indices is total_lines - 1.
            self.quote_line_offset = (self.quote_line_offset + 1) % (total_lines - 1);
        }
    }

    pub fn cycle_asset(&mut self) {
        self.asset_index = (self.asset_index + 1) % ALL_ASSETS.len();
        self.current_asset = ALL_ASSETS[self.asset_index];
    }

    pub fn simulate_price_fetch(&mut self) {
        let price = simulate_fetch_price(self.current_asset);
        let formatted = format_price(self.current_asset, price);
        self.handle_event(Event::PriceReceived(formatted));
    }
}

/// Utility functions for formatting text and time
pub mod util {
    use super::*;
    use core::str::FromStr;

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
                    if end_idx == 0 {
                        end_idx = remaining_word.chars().next().map_or(0, |c| c.len_utf8());
                    }

                    let (chunk, rest) = remaining_word.split_at(end_idx);
                    let hstr = match HString::from_str(chunk) {
                        Ok(hstr) => hstr,
                        Err(_) => break,
                    };
                    if lines.push(hstr).is_err() {
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
                current_line = if let Ok(hstr) = HString::from_str(word) {
                    hstr
                } else {
                    break;
                };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_displaying_time() {
        let gadget = SmartGadget::new();
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    #[test]
    fn button_press_from_time_goes_to_fetching() {
        let mut gadget = SmartGadget::new();
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::FetchingQuote);
    }

    #[test]
    fn quote_received_while_fetching_goes_to_displaying() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::FetchingQuote;
        let quote = HString::<128>::from("Hello, world!");
        gadget.handle_event(Event::QuoteReceived(quote.clone()));
        assert_eq!(gadget.state, State::DisplayingQuote);
        assert_eq!(gadget.current_quote, Some(quote));
        assert_eq!(gadget.quote_line_offset, 0);
    }

    #[test]
    fn button_press_from_quote_starts_countdown() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingQuote;
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::DisplayingCountdown);
        assert_eq!(gadget.countdown_seconds, 30);
        assert_eq!(gadget.countdown_original, 30);
    }

    #[test]
    fn button_press_during_countdown_goes_to_fetching_price() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingCountdown;
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::FetchingPrice);
    }

    #[test]
    fn fetch_failed_goes_back_to_time() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::FetchingQuote;
        gadget.handle_event(Event::FetchFailed);
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    #[test]
    fn countdown_tick_decrements() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingCountdown;
        gadget.countdown_seconds = 10;
        gadget.handle_event(Event::CountdownTick);
        assert_eq!(gadget.countdown_seconds, 9);
    }

    #[test]
    fn countdown_tick_at_zero_triggers_finished() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingCountdown;
        gadget.countdown_seconds = 0;
        gadget.handle_event(Event::CountdownTick);
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    #[test]
    fn countdown_finished_goes_to_time() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingCountdown;
        gadget.handle_event(Event::CountdownFinished);
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    #[test]
    fn unhandled_combinations_are_noop() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::FetchingQuote;
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::FetchingQuote);

        let mut g2 = SmartGadget::new();
        let q = HString::<128>::from("test");
        g2.handle_event(Event::QuoteReceived(q));
        assert_eq!(g2.state, State::DisplayingTime);
    }

    #[test]
    fn get_next_quote_cycles_through_quotes() {
        let mut gadget = SmartGadget::new();
        let first = gadget.get_next_quote();
        let second = gadget.get_next_quote();
        assert_ne!(first, second);
    }

    #[test]
    fn get_next_quote_wraps_around() {
        let mut gadget = SmartGadget::new();
        let num_quotes = gadget.quotes.len();
        for _ in 0..num_quotes {
            gadget.get_next_quote();
        }
        let after_wrap = gadget.get_next_quote();
        let mut g2 = SmartGadget::new();
        let first = g2.get_next_quote();
        assert_eq!(after_wrap, first);
    }

    #[test]
    fn start_countdown_sets_both_fields() {
        let mut gadget = SmartGadget::new();
        gadget.start_countdown(60);
        assert_eq!(gadget.countdown_seconds, 60);
        assert_eq!(gadget.countdown_original, 60);
    }

    #[test]
    fn tick_countdown_reduces_countdown() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingCountdown;
        gadget.start_countdown(5);
        gadget.tick_countdown();
        assert_eq!(gadget.countdown_seconds, 4);
    }

    #[test]
    fn tick_countdown_to_zero_finishes() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingCountdown;
        gadget.start_countdown(1);
        gadget.tick_countdown();
        assert_eq!(gadget.countdown_seconds, 0);
        assert_eq!(gadget.state, State::DisplayingCountdown);
        gadget.tick_countdown();
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    #[test]
    fn scroll_quote_does_not_scroll_for_short_text() {
        let mut gadget = SmartGadget::new();
        gadget.quote_line_offset = 0;
        gadget.scroll_quote(3);
        assert_eq!(gadget.quote_line_offset, 0);
    }

    #[test]
    fn scroll_quote_increments_for_long_text() {
        let mut gadget = SmartGadget::new();
        gadget.quote_line_offset = 0;
        gadget.scroll_quote(5);
        assert_eq!(gadget.quote_line_offset, 1);
    }

    #[test]
    fn scroll_quote_wraps_around() {
        let mut gadget = SmartGadget::new();
        gadget.quote_line_offset = 3; // last valid offset for 5 lines (total_lines-1 = 4, so offset 3)
        gadget.scroll_quote(5);
        // (3 + 1) % (5 - 1) = 4 % 4 = 0
        assert_eq!(gadget.quote_line_offset, 0);
    }

    // ── format_quote_lines ────────────────────────────────────────

    #[test]
    fn format_short_quote_single_line() {
        let lines = util::format_quote_lines("Hello", 20, 4);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].as_str(), "Hello");
    }

    #[test]
    fn format_quote_wraps_to_multiple_lines() {
        let lines =
            util::format_quote_lines("this is a very long sentence that should wrap", 10, 8);
        assert!(lines.len() > 1);
    }

    #[test]
    fn format_quote_respects_max_lines() {
        let long_text = "a b c d e f g h i j k l m n o p q r s t u v w x y z";
        let lines = util::format_quote_lines(long_text, 5, 3);
        assert!(lines.len() <= 3);
    }

    #[test]
    fn format_long_word_broken_across_lines() {
        let lines = util::format_quote_lines("supercalifragilisticexpialidocious", 10, 8);
        // The long word should be broken into chunks
        assert!(lines.len() > 1);
    }

    #[test]
    fn format_quote_preserves_words_when_possible() {
        let lines = util::format_quote_lines("hello world test", 20, 4);
        // All words fit on one line
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].as_str(), "hello world test");
    }

    // ── format_time ───────────────────────────────────────────────

    #[test]
    fn format_time_zero_seconds_no_hours() {
        let result = util::format_time(0, false);
        assert_eq!(result.as_str(), "00:00");
    }

    #[test]
    fn format_time_minutes_and_seconds() {
        let result = util::format_time(65, false);
        assert_eq!(result.as_str(), "01:05");
    }

    #[test]
    fn format_time_with_hours() {
        let result = util::format_time(3661, true); // 1h 1m 1s
        assert_eq!(result.as_str(), "01:01:01");
    }

    #[test]
    fn format_time_large_value_with_hours() {
        let result = util::format_time(86399, true); // 23:59:59
        assert_eq!(result.as_str(), "23:59:59");
    }

    #[test]
    fn format_time_midnight_rollover() {
        let result = util::format_time(86400, true); // 24:00:00
        assert_eq!(result.as_str(), "24:00:00");
    }

    // ── simulate_quote_fetch integration ──────────────────────────

    #[test]
    fn simulate_quote_fetch_transitions_state() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::FetchingQuote;
        gadget.simulate_quote_fetch();
        assert_eq!(gadget.state, State::DisplayingQuote);
        assert!(gadget.current_quote.is_some());
    }

    // ── full user-interaction flow ────────────────────────────────

    #[test]
    fn full_interaction_flow() {
        let mut gadget = SmartGadget::new();

        // 1. Start: DisplayingTime
        assert_eq!(gadget.state, State::DisplayingTime);

        // 2. Press button → FetchingQuote
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::FetchingQuote);

        // 3. Quote arrives → DisplayingQuote
        let quote =
            HString::<128>::from("A journey of a thousand miles begins with a single step.");
        gadget.handle_event(Event::QuoteReceived(quote));
        assert_eq!(gadget.state, State::DisplayingQuote);

        // 4. Press button → DisplayingCountdown (30s)
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::DisplayingCountdown);
        assert_eq!(gadget.countdown_seconds, 30);

        // 5. Countdown ticks down to zero
        for _ in 0..29 {
            gadget.tick_countdown();
        }
        assert_eq!(gadget.countdown_seconds, 1);
        gadget.tick_countdown();
        assert_eq!(gadget.countdown_seconds, 0);
        assert_eq!(gadget.state, State::DisplayingCountdown);
        gadget.tick_countdown();
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    // ── price display transitions ─────────────────────────────────

    #[test]
    fn button_press_from_countdown_goes_to_fetching_price() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingCountdown;
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::FetchingPrice);
    }

    #[test]
    fn price_received_while_fetching_goes_to_displaying() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::FetchingPrice;
        let price = HString::<128>::from("BTC: 65432.10");
        gadget.handle_event(Event::PriceReceived(price.clone()));
        assert_eq!(gadget.state, State::DisplayingPrice);
        assert_eq!(gadget.current_price, Some(price));
    }

    #[test]
    fn price_fetch_failed_goes_back_to_time() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::FetchingPrice;
        gadget.handle_event(Event::PriceFetchFailed);
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    #[test]
    fn button_press_from_price_goes_to_time() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingPrice;
        gadget.handle_event(Event::ButtonPress);
        assert_eq!(gadget.state, State::DisplayingTime);
    }

    #[test]
    fn asset_tick_cycles_asset_and_fetches() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::DisplayingPrice;
        gadget.handle_event(Event::AssetTick);
        assert_eq!(gadget.current_asset, Asset::Ckb);
        assert_eq!(gadget.state, State::FetchingPrice);
    }

    #[test]
    fn cycle_asset_wraps_around() {
        let mut gadget = SmartGadget::new();
        gadget.asset_index = ALL_ASSETS.len() - 1;
        gadget.cycle_asset();
        assert_eq!(gadget.asset_index, 0);
        assert_eq!(gadget.current_asset, Asset::Btc);
    }

    #[test]
    fn simulate_price_fetch_transitions_to_displaying() {
        let mut gadget = SmartGadget::new();
        gadget.state = State::FetchingPrice;
        gadget.simulate_price_fetch();
        assert_eq!(gadget.state, State::DisplayingPrice);
        assert!(gadget.current_price.is_some());
    }
}
