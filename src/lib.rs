#![no_std]

use heapless::String as HString;

pub mod hardware;
pub mod renderer;
pub mod util;

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

pub struct SmartGadget {
    pub state: State,
    pub current_quote: HString<128>,
    pub quotes: &'static [&'static str],
    pub current_quote_index: usize,
    pub countdown_seconds: u32,
    pub countdown_original: u32,
    pub quote_line_offset: usize, // For scrolling through long quotes
}

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
            (State::DisplayingQuote, Event::ButtonPress) => {
                self.switch_to_next_quote();
            }

            (State::DisplayingQuote, Event::StartCountdown) => {
                self.start_countdown(30); // Start 30-second countdown
                self.state = State::DisplayingCountdown;
            }

            (State::DisplayingCountdown, Event::ButtonPress) => {
                self.state = State::DisplayingQuote;
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
