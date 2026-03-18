//! RGB LED driver for the Briolette solar relay.
//!
//! Three discrete LEDs (red, green, blue) driven via PWM for brightness
//! control, or simple GPIO on/off for minimal power usage.
//!
//! LED semantics:
//!   Green = power/ready/success
//!   Blue  = NFC activity (tag detected, data exchange)
//!   Red   = error/low power/failure
//!
//! Patterns:
//!   Solid green    = ready for transaction
//!   Blink green 3× = transaction success
//!   Blink blue     = NFC data exchange in progress
//!   Solid blue     = tag connected
//!   Blink red 3×   = transaction failed
//!   Slow blink red = low power
//!   Green pulse every 10s = idle charge indicator

use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_time::{Duration, Timer};

/// LED color selection.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Color {
    Red,
    Green,
    Blue,
}

/// LED pattern for async playback.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Pattern {
    /// All LEDs off.
    Off,
    /// Solid single color.
    Solid(Color),
    /// Blink N times (color, count, on_ms, off_ms).
    Blink(Color, u8, u16, u16),
    /// Brief pulse (for idle status indication).
    Pulse(Color, u16),
}

/// RGB LED controller.
pub struct Leds<'a> {
    red: Output<'a>,
    green: Output<'a>,
    blue: Output<'a>,
}

impl<'a> Leds<'a> {
    pub fn new(red: Output<'a>, green: Output<'a>, blue: Output<'a>) -> Self {
        Self { red, green, blue }
    }

    /// Turn all LEDs off.
    pub fn all_off(&mut self) {
        self.red.set_high(); // Active low (LED cathode to GPIO).
        self.green.set_high();
        self.blue.set_high();
    }

    /// Set a single LED on, others off.
    pub fn set(&mut self, color: Color) {
        self.all_off();
        match color {
            Color::Red => self.red.set_low(),
            Color::Green => self.green.set_low(),
            Color::Blue => self.blue.set_low(),
        }
    }

    /// Turn a single LED off without affecting others.
    pub fn clear(&mut self, color: Color) {
        match color {
            Color::Red => self.red.set_high(),
            Color::Green => self.green.set_high(),
            Color::Blue => self.blue.set_high(),
        }
    }

    /// Play a pattern asynchronously (blocking the caller until complete).
    pub async fn play(&mut self, pattern: Pattern) {
        match pattern {
            Pattern::Off => {
                self.all_off();
            }
            Pattern::Solid(color) => {
                self.set(color);
            }
            Pattern::Blink(color, count, on_ms, off_ms) => {
                for _ in 0..count {
                    self.set(color);
                    Timer::after(Duration::from_millis(on_ms as u64)).await;
                    self.all_off();
                    Timer::after(Duration::from_millis(off_ms as u64)).await;
                }
            }
            Pattern::Pulse(color, duration_ms) => {
                self.set(color);
                Timer::after(Duration::from_millis(duration_ms as u64)).await;
                self.all_off();
            }
        }
    }
}

/// Predefined patterns for common relay states.
pub mod patterns {
    use super::*;

    /// Transaction ready (solid green).
    pub const READY: Pattern = Pattern::Solid(Color::Green);

    /// NFC tag detected (solid blue).
    pub const TAG_CONNECTED: Pattern = Pattern::Solid(Color::Blue);

    /// NFC data exchange (fast blue blink).
    pub const NFC_ACTIVE: Pattern = Pattern::Blink(Color::Blue, 1, 100, 100);

    /// Transaction success (3× green blink).
    pub const SUCCESS: Pattern = Pattern::Blink(Color::Green, 3, 200, 200);

    /// Transaction failed (3× red blink).
    pub const FAILURE: Pattern = Pattern::Blink(Color::Red, 3, 200, 200);

    /// Low power warning (slow red blink).
    pub const LOW_POWER: Pattern = Pattern::Blink(Color::Red, 1, 500, 500);

    /// Idle charge indicator (brief green pulse).
    pub const IDLE_PULSE: Pattern = Pattern::Pulse(Color::Green, 50);

    /// Waiting for sender re-tap (slow blue blink).
    pub const WAITING_SENDER: Pattern = Pattern::Blink(Color::Blue, 1, 300, 700);
}
