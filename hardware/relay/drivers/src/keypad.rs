//! TCA8418 I2C Keypad Driver for Briolette Solar Relay
//!
//! Drives a 4x4 key matrix (13 keys populated) via the TCA8418 keypad scanner IC.
//! The TCA8418 handles matrix scanning, debouncing, and event queuing in hardware.
//!
//! Key layout (4 rows × 4 columns, 3 positions empty):
//!   Row 0: 1  2  3  CLR
//!   Row 1: 4  5  6  ---
//!   Row 2: 7  8  9  ---
//!   Row 3: .  0  OK ---

use embedded_hal::i2c::I2c;

const TCA8418_ADDR: u8 = 0x34;

// TCA8418 register addresses
const REG_CFG: u8 = 0x01;
const REG_INT_STAT: u8 = 0x02;
const REG_KEY_LCK_EC: u8 = 0x03;
const REG_KEY_EVENT_A: u8 = 0x04;
const REG_KP_GPIO1: u8 = 0x1D;
const REG_KP_GPIO2: u8 = 0x1E;
const REG_KP_GPIO3: u8 = 0x1F;

// CFG register bits
const CFG_KE_IEN: u8 = 0x01; // Key events interrupt enable
const CFG_INT_CFG: u8 = 0x10; // INT re-assert after clearing

// INT_STAT bits
const INT_STAT_K_INT: u8 = 0x01; // Key event interrupt

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Digit(u8), // 0-9
    Dot,
    Ok,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Press(Key),
    Release(Key),
}

/// Maps (row, col) to a Key. Returns None for unpopulated positions.
fn matrix_to_key(row: u8, col: u8) -> Option<Key> {
    match (row, col) {
        (0, 0) => Some(Key::Digit(1)),
        (0, 1) => Some(Key::Digit(2)),
        (0, 2) => Some(Key::Digit(3)),
        (0, 3) => Some(Key::Clear),
        (1, 0) => Some(Key::Digit(4)),
        (1, 1) => Some(Key::Digit(5)),
        (1, 2) => Some(Key::Digit(6)),
        (2, 0) => Some(Key::Digit(7)),
        (2, 1) => Some(Key::Digit(8)),
        (2, 2) => Some(Key::Digit(9)),
        (3, 0) => Some(Key::Dot),
        (3, 1) => Some(Key::Digit(0)),
        (3, 2) => Some(Key::Ok),
        _ => None,
    }
}

#[derive(Debug)]
pub enum Error<E> {
    I2c(E),
    InvalidEvent,
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::I2c(e)
    }
}

pub struct Keypad<I2C> {
    i2c: I2C,
}

impl<I2C, E> Keypad<I2C>
where
    I2C: I2c<Error = E>,
{
    pub fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, E> {
        let mut buf = [0u8];
        self.i2c.write_read(TCA8418_ADDR, &[reg], &mut buf)?;
        Ok(buf[0])
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), E> {
        self.i2c.write(TCA8418_ADDR, &[reg, val])
    }

    /// Initialize the TCA8418 for 4-row × 4-column keypad scanning.
    pub fn init(&mut self) -> Result<(), E> {
        // Configure ROW0-ROW3 (bits 0-3) as keypad rows
        self.write_reg(REG_KP_GPIO1, 0x0F)?;
        // Configure COL0-COL3 (bits 0-3 of GPIO2) as keypad columns
        self.write_reg(REG_KP_GPIO2, 0x0F)?;
        // No additional GPIOs used
        self.write_reg(REG_KP_GPIO3, 0x00)?;

        // Enable key event interrupt, re-assert INT on new events
        self.write_reg(REG_CFG, CFG_KE_IEN | CFG_INT_CFG)?;

        // Clear any pending interrupts
        self.write_reg(REG_INT_STAT, INT_STAT_K_INT)?;

        Ok(())
    }

    /// Read the next key event from the TCA8418 FIFO.
    /// Returns None if the FIFO is empty.
    pub fn read_event(&mut self) -> Result<Option<KeyEvent>, Error<E>> {
        // Check event count
        let ec = self.read_reg(REG_KEY_LCK_EC)? & 0x0F;
        if ec == 0 {
            return Ok(None);
        }

        let event = self.read_reg(REG_KEY_EVENT_A)?;
        if event == 0 {
            return Ok(None);
        }

        let pressed = (event & 0x80) != 0;
        let key_code = event & 0x7F;

        // TCA8418 key codes: row * 10 + col + 1 (1-indexed)
        if key_code == 0 || key_code > 80 {
            return Err(Error::InvalidEvent);
        }

        let code = key_code - 1;
        let row = code / 10;
        let col = code % 10;

        if let Some(key) = matrix_to_key(row, col) {
            Ok(Some(if pressed {
                KeyEvent::Press(key)
            } else {
                KeyEvent::Release(key)
            }))
        } else {
            // Ghost key in unpopulated position — ignore
            Ok(None)
        }
    }

    /// Clear the interrupt flag. Call after processing all events.
    pub fn clear_interrupt(&mut self) -> Result<(), E> {
        self.write_reg(REG_INT_STAT, INT_STAT_K_INT)
    }

    /// Drain all pending events from the FIFO.
    pub fn drain_events(&mut self) -> Result<(), Error<E>> {
        loop {
            let ec = self.read_reg(REG_KEY_LCK_EC)? & 0x0F;
            if ec == 0 {
                break;
            }
            let _ = self.read_reg(REG_KEY_EVENT_A)?;
        }
        self.clear_interrupt()?;
        Ok(())
    }

    /// Release the underlying I2C bus.
    pub fn release(self) -> I2C {
        self.i2c
    }
}

/// Amount entry state machine using the keypad.
pub struct AmountEntry {
    digits: [u8; 12],
    len: usize,
    dot_pos: Option<usize>,
}

impl AmountEntry {
    pub fn new() -> Self {
        Self {
            digits: [0; 12],
            len: 0,
            dot_pos: None,
        }
    }

    /// Process a key press. Returns Some(amount_cents) on OK, None otherwise.
    pub fn process(&mut self, key: Key) -> Option<u64> {
        match key {
            Key::Digit(d) => {
                if self.len < 12 {
                    // Limit to 2 decimal places
                    if let Some(dp) = self.dot_pos {
                        if self.len - dp > 2 {
                            return None;
                        }
                    }
                    self.digits[self.len] = d;
                    self.len += 1;
                }
                None
            }
            Key::Dot => {
                if self.dot_pos.is_none() && self.len < 11 {
                    self.dot_pos = Some(self.len);
                }
                None
            }
            Key::Clear => {
                self.len = 0;
                self.dot_pos = None;
                None
            }
            Key::Ok => {
                if self.len == 0 {
                    return None;
                }
                // Convert to cents (integer amount × 100)
                let mut whole: u64 = 0;
                let mut frac: u64 = 0;
                let dp = self.dot_pos.unwrap_or(self.len);

                for i in 0..dp {
                    whole = whole * 10 + self.digits[i] as u64;
                }

                let frac_digits = if self.dot_pos.is_some() {
                    self.len - dp
                } else {
                    0
                };

                for i in 0..frac_digits {
                    frac = frac * 10 + self.digits[dp + i] as u64;
                }

                // Pad fractional part to 2 digits
                match frac_digits {
                    0 => frac = 0,
                    1 => frac *= 10,
                    _ => {} // already 2 digits
                }

                let cents = whole * 100 + frac;
                // Reset for next entry
                self.len = 0;
                self.dot_pos = None;
                Some(cents)
            }
        }
    }

    /// Get the current display string (e.g., "12.50").
    pub fn display(&self, buf: &mut [u8]) -> usize {
        let mut pos = 0;
        if self.len == 0 {
            if pos < buf.len() {
                buf[pos] = b'0';
                pos += 1;
            }
            return pos;
        }
        for i in 0..self.len {
            if Some(i) == self.dot_pos {
                if pos < buf.len() {
                    buf[pos] = b'.';
                    pos += 1;
                }
            }
            if pos < buf.len() {
                buf[pos] = b'0' + self.digits[i];
                pos += 1;
            }
        }
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_mapping() {
        assert_eq!(matrix_to_key(0, 0), Some(Key::Digit(1)));
        assert_eq!(matrix_to_key(0, 3), Some(Key::Clear));
        assert_eq!(matrix_to_key(1, 0), Some(Key::Digit(4)));
        assert_eq!(matrix_to_key(3, 1), Some(Key::Digit(0)));
        assert_eq!(matrix_to_key(3, 2), Some(Key::Ok));
        assert_eq!(matrix_to_key(3, 0), Some(Key::Dot));
        // Empty positions
        assert_eq!(matrix_to_key(1, 3), None);
        assert_eq!(matrix_to_key(2, 3), None);
        assert_eq!(matrix_to_key(3, 3), None);
    }

    #[test]
    fn test_amount_entry_basic() {
        let mut entry = AmountEntry::new();
        assert_eq!(entry.process(Key::Digit(1)), None);
        assert_eq!(entry.process(Key::Digit(2)), None);
        assert_eq!(entry.process(Key::Dot), None);
        assert_eq!(entry.process(Key::Digit(5)), None);
        assert_eq!(entry.process(Key::Digit(0)), None);
        assert_eq!(entry.process(Key::Ok), Some(1250)); // 12.50 = 1250 cents
    }

    #[test]
    fn test_amount_entry_no_decimal() {
        let mut entry = AmountEntry::new();
        assert_eq!(entry.process(Key::Digit(5)), None);
        assert_eq!(entry.process(Key::Ok), Some(500)); // 5 = 500 cents
    }

    #[test]
    fn test_amount_entry_clear() {
        let mut entry = AmountEntry::new();
        entry.process(Key::Digit(9));
        entry.process(Key::Digit(9));
        assert_eq!(entry.process(Key::Clear), None);
        assert_eq!(entry.process(Key::Ok), None); // empty after clear
    }

    #[test]
    fn test_amount_entry_single_decimal() {
        let mut entry = AmountEntry::new();
        assert_eq!(entry.process(Key::Digit(3)), None);
        assert_eq!(entry.process(Key::Dot), None);
        assert_eq!(entry.process(Key::Digit(5)), None);
        assert_eq!(entry.process(Key::Ok), Some(350)); // 3.5 = 350 cents
    }

    #[test]
    fn test_amount_display() {
        let mut entry = AmountEntry::new();
        let mut buf = [0u8; 16];

        let n = entry.display(&mut buf);
        assert_eq!(&buf[..n], b"0");

        entry.process(Key::Digit(4));
        entry.process(Key::Digit(2));
        entry.process(Key::Dot);
        entry.process(Key::Digit(0));
        entry.process(Key::Digit(0));
        let n = entry.display(&mut buf);
        assert_eq!(&buf[..n], b"42.00");
    }
}
