#![no_std]

use heapless::String as HString;

// 1. Define your States and Events as enums
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    DisplayingTime,
    DisplayingQuote,
    FetchingQuote,
}

#[derive(Debug, Clone)]
pub enum Event {
    ButtonPress,
    QuoteReceived(HString<128>), // Event can carry data!
    FetchFailed,
}

// 2. Create a struct for your gadget
pub struct SmartGadget {
    pub state: State,
    pub current_quote: Option<HString<128>>,
    pub quotes: &'static [&'static str],
    pub current_quote_index: usize,
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
                self.state = State::DisplayingQuote;
                // ACTION: Display the new quote on the screen
            }

            // If we're showing a quote and the button is pressed...
            (State::DisplayingQuote, Event::ButtonPress) => {
                self.state = State::DisplayingTime;
                // ACTION: Clear screen and show the time
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
}
