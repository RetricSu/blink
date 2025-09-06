use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
};
use esp_hal::delay::Delay;
use log::info;

/// Hardware abstraction for timing operations
pub struct Timer {
    delay: Delay,
    last_tick_ms: u32,
    tick_interval_ms: u32,
}

impl Timer {
    pub fn new(tick_interval_ms: u32) -> Self {
        Self {
            delay: Delay::new(),
            last_tick_ms: 0,
            tick_interval_ms,
        }
    }

    /// Delay for the specified number of milliseconds
    pub fn delay_ms(&mut self, ms: u32) {
        self.delay.delay_millis(ms);
    }

    /// Check if it's time for the next tick based on the interval
    /// This is a simple implementation - in a real system you'd use hardware timers
    pub fn should_tick(&mut self, current_time_ms: u32) -> bool {
        if current_time_ms - self.last_tick_ms >= self.tick_interval_ms {
            self.last_tick_ms = current_time_ms;
            true
        } else {
            false
        }
    }

    /// Get the current tick interval
    pub fn tick_interval_ms(&self) -> u32 {
        self.tick_interval_ms
    }
}

/// Hardware abstraction for button input (placeholder for future implementation)
/// This will be used when a physical button is added
pub struct Button {
    // GPIO pin will be added here when button is implemented
    // For now, this is a placeholder
}

impl Button {
    pub fn new() -> Self {
        // TODO: Initialize GPIO pin for button input
        // TODO: Configure internal pull-up resistor
        Self {}
    }

    /// Check if button is pressed (placeholder implementation)
    /// Returns true if button press is detected
    pub fn is_pressed(&self) -> bool {
        // TODO: Read GPIO pin state
        // TODO: Implement debouncing logic
        false
    }

    /// Get button press event if one occurred
    /// Returns Some(ButtonPress) if a new press is detected, None otherwise
    pub fn check_press(&mut self) -> Option<crate::Event> {
        // TODO: Implement proper button debouncing and edge detection
        None
    }
}

impl Default for Button {
    fn default() -> Self {
        Self::new()
    }
}

/// Display driver wrapper with retry logic
pub struct DisplayWrapper {
    text_style: MonoTextStyle<'static, BinaryColor>,
    delay: Delay,
}

impl DisplayWrapper {
    pub fn new() -> Self {
        let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
        let delay = Delay::new();

        Self { text_style, delay }
    }

    /// Get the text style for consistent rendering
    pub fn text_style(&self) -> MonoTextStyle<'static, BinaryColor> {
        self.text_style
    }

    /// Flush the display buffer with retry logic
    /// Returns true if successful, false if all retries failed
    pub fn try_flush<D, E>(&mut self, display: &mut D) -> bool
    where
        D: FnMut() -> Result<(), E>,
        E: core::fmt::Debug,
    {
        const MAX_RETRIES: u8 = 3;
        const RETRY_DELAY_MS: u32 = 10;

        for attempt in 1..=MAX_RETRIES {
            match display() {
                Ok(_) => return true,
                Err(e) => {
                    info!("Failed to flush display on attempt {}: {:?}", attempt, e);
                    if attempt < MAX_RETRIES {
                        self.delay.delay_millis(RETRY_DELAY_MS);
                    }
                }
            }
        }

        info!("All flush attempts failed");
        false
    }
}

impl Default for DisplayWrapper {
    fn default() -> Self {
        Self::new()
    }
}
