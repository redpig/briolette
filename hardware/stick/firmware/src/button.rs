//! Button input handler for PIN entry and UI navigation.
//!
//! Two buttons (Left/Right) with timing differentiation:
//! - Short press (<500ms): "dit" / Left or Right symbol
//! - Long press (≥500ms): "dah" / Left-long or Right-long symbol
//! - Both buttons held simultaneously (≥1s): Submit/Confirm
//!
//! The same buttons serve multiple roles depending on context:
//! | Context      | Left          | Right          |
//! |--------------|---------------|----------------|
//! | PIN entry    | Left symbol   | Right symbol   |
//! | Balance view | Prev token    | Next token     |
//! | Transaction  | Decline       | Accept         |
//! | Confirm      | (hold both)   | (hold both)    |
//!
//! PIN entry happens between NFC taps, not during. The user lifts the
//! credstick from the reader, sees the proposal on e-ink, enters their
//! PIN via buttons, then taps again to confirm.

use embassy_nrf::gpio::Input;
use embassy_time::{Duration, Instant, Timer};

use crate::Event;

/// PIN symbol: Left or Right, short or long press.
#[derive(Clone, Copy, defmt::Format)]
pub enum PinSymbol {
    Left { long: bool },
    Right { long: bool },
}

impl PinSymbol {
    /// Encode a PIN symbol as a single byte for hashing.
    pub fn to_byte(&self) -> u8 {
        match self {
            PinSymbol::Left { long: false } => 0x00,
            PinSymbol::Left { long: true } => 0x01,
            PinSymbol::Right { long: false } => 0x02,
            PinSymbol::Right { long: true } => 0x03,
        }
    }
}

/// Threshold for long press detection.
const LONG_PRESS_MS: u64 = 500;
/// Threshold for simultaneous press detection.
const BOTH_PRESS_MS: u64 = 1000;
/// Debounce time.
const DEBOUNCE_MS: u64 = 50;

/// Button handler task.
///
/// Runs as an Embassy async task, monitoring GPIO pins for button presses.
/// Sends events to the main event channel.
#[embassy_executor::task]
pub async fn button_task(
    mut btn_left: Input<'static>,
    mut btn_right: Input<'static>,
) {
    loop {
        // Wait for either button to be pressed (active low with pull-up).
        let left_pressed = btn_left.is_low();
        let right_pressed = btn_right.is_low();

        if left_pressed && right_pressed {
            // Both buttons pressed — wait for hold duration.
            Timer::after(Duration::from_millis(BOTH_PRESS_MS)).await;
            if btn_left.is_low() && btn_right.is_low() {
                crate::EVENT_CHANNEL.send(Event::ButtonBoth).await;
                // Wait for release.
                while btn_left.is_low() || btn_right.is_low() {
                    Timer::after(Duration::from_millis(DEBOUNCE_MS)).await;
                }
            }
        } else if left_pressed {
            let press_start = Instant::now();
            // Wait for release or long-press threshold.
            while btn_left.is_low() {
                Timer::after(Duration::from_millis(DEBOUNCE_MS)).await;
            }
            let held_ms = press_start.elapsed().as_millis();
            let long = held_ms >= LONG_PRESS_MS;
            crate::EVENT_CHANNEL
                .send(Event::ButtonLeft { long })
                .await;
        } else if right_pressed {
            let press_start = Instant::now();
            while btn_right.is_low() {
                Timer::after(Duration::from_millis(DEBOUNCE_MS)).await;
            }
            let held_ms = press_start.elapsed().as_millis();
            let long = held_ms >= LONG_PRESS_MS;
            crate::EVENT_CHANNEL
                .send(Event::ButtonRight { long })
                .await;
        }

        // Poll interval when no buttons pressed.
        Timer::after(Duration::from_millis(DEBOUNCE_MS)).await;
    }
}
