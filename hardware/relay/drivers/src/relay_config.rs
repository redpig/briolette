//! Relay configuration serialization/deserialization (host-testable).
//!
//! This module contains the pure logic for config flash layout parsing
//! and serialization. It's in the library crate so it can be tested
//! on a host without Embassy or defmt.
//!
//! Three operating modes:
//!   1. Variable — both amount and wallets entered/scanned each time
//!   2. MerchantPos — variable amount, saved receiver wallet
//!   3. EventMode — fixed amount AND fixed receiver (maximum throughput)

use heapless::Vec;

/// Config magic bytes.
pub const CONFIG_MAGIC: [u8; 4] = *b"BRCF";

/// Current config version.
pub const CONFIG_VERSION: u8 = 1;

/// Maximum receiver ticket size.
pub const MAX_TICKET_SIZE: usize = 512;

/// Flash page size.
pub const CONFIG_PAGE_SIZE: usize = 4096;

/// Relay operating mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Variable amount and wallets.
    Variable,
    /// Variable amount, saved receiver (merchant POS).
    MerchantPos,
    /// Fixed amount AND fixed receiver (event mode).
    EventMode,
}

impl Mode {
    pub fn to_byte(self) -> u8 {
        match self {
            Mode::Variable => 0,
            Mode::MerchantPos => 1,
            Mode::EventMode => 2,
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Mode::Variable),
            1 => Some(Mode::MerchantPos),
            2 => Some(Mode::EventMode),
            _ => None,
        }
    }
}

/// Relay configuration (serializable to/from flash).
#[derive(Clone, Debug)]
pub struct RelayConfig {
    pub mode: Mode,
    pub fixed_amount: u32,
    pub receiver_ticket: Vec<u8, 512>,
}

impl RelayConfig {
    /// Default config: variable mode, no saved state.
    pub fn default() -> Self {
        Self {
            mode: Mode::Variable,
            fixed_amount: 0,
            receiver_ticket: Vec::new(),
        }
    }

    /// Serialize config to a flash page.
    pub fn to_flash_bytes(&self) -> [u8; CONFIG_PAGE_SIZE] {
        let mut page = [0xFFu8; CONFIG_PAGE_SIZE];

        page[0..4].copy_from_slice(&CONFIG_MAGIC);
        page[4] = CONFIG_VERSION;
        page[5] = self.mode.to_byte();
        page[6..10].copy_from_slice(&self.fixed_amount.to_be_bytes());

        let ticket_len = self.receiver_ticket.len() as u16;
        page[10..12].copy_from_slice(&ticket_len.to_be_bytes());

        if !self.receiver_ticket.is_empty() {
            let end = 12 + self.receiver_ticket.len();
            page[12..end].copy_from_slice(&self.receiver_ticket);
        }

        page
    }

    /// Deserialize config from a flash page. Returns default on invalid data.
    pub fn from_flash_bytes(data: &[u8]) -> Self {
        if data.len() < 12 || data[0..4] != CONFIG_MAGIC {
            return Self::default();
        }

        if data[4] != CONFIG_VERSION {
            return Self::default();
        }

        let mode = match Mode::from_byte(data[5]) {
            Some(m) => m,
            None => return Self::default(),
        };

        let fixed_amount =
            u32::from_be_bytes([data[6], data[7], data[8], data[9]]);

        let ticket_len = u16::from_be_bytes([data[10], data[11]]) as usize;

        let mut receiver_ticket: Vec<u8, 512> = Vec::new();
        if ticket_len > 0
            && ticket_len <= MAX_TICKET_SIZE
            && data.len() >= 12 + ticket_len
            && (mode == Mode::MerchantPos || mode == Mode::EventMode)
        {
            receiver_ticket
                .extend_from_slice(&data[12..12 + ticket_len])
                .ok();
        }

        Self {
            mode,
            fixed_amount,
            receiver_ticket,
        }
    }

    /// Configure as merchant POS: save receiver ticket, amount stays variable.
    pub fn set_merchant_pos(&mut self, ticket: &[u8]) {
        self.receiver_ticket.clear();
        self.receiver_ticket.extend_from_slice(ticket).ok();
        self.fixed_amount = 0;
        self.mode = Mode::MerchantPos;
    }

    /// Configure as event mode: fixed amount + fixed receiver.
    pub fn set_event_mode(&mut self, amount: u32, ticket: &[u8]) {
        self.fixed_amount = amount;
        self.receiver_ticket.clear();
        self.receiver_ticket.extend_from_slice(ticket).ok();
        self.mode = Mode::EventMode;
    }

    /// Clear config and return to variable mode.
    pub fn clear(&mut self) {
        self.mode = Mode::Variable;
        self.fixed_amount = 0;
        self.receiver_ticket.clear();
    }

    /// Check if receiver ticket is cached.
    pub fn has_saved_receiver(&self) -> bool {
        !self.receiver_ticket.is_empty()
    }

    /// Check if amount is fixed (EventMode only).
    pub fn has_fixed_amount(&self) -> bool {
        self.mode == Mode::EventMode && self.fixed_amount > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Mode byte conversion ---

    #[test]
    fn test_mode_roundtrip() {
        for mode in [Mode::Variable, Mode::MerchantPos, Mode::EventMode] {
            assert_eq!(Mode::from_byte(mode.to_byte()), Some(mode));
        }
    }

    #[test]
    fn test_mode_from_invalid_byte() {
        assert_eq!(Mode::from_byte(3), None);
        assert_eq!(Mode::from_byte(0xFF), None);
    }

    // --- Default config ---

    #[test]
    fn test_default_config() {
        let c = RelayConfig::default();
        assert_eq!(c.mode, Mode::Variable);
        assert_eq!(c.fixed_amount, 0);
        assert!(c.receiver_ticket.is_empty());
        assert!(!c.has_saved_receiver());
        assert!(!c.has_fixed_amount());
    }

    // --- Flash serialization roundtrip ---

    #[test]
    fn test_variable_mode_roundtrip() {
        let original = RelayConfig::default();
        let bytes = original.to_flash_bytes();
        let loaded = RelayConfig::from_flash_bytes(&bytes);

        assert_eq!(loaded.mode, Mode::Variable);
        assert_eq!(loaded.fixed_amount, 0);
        assert!(loaded.receiver_ticket.is_empty());
    }

    #[test]
    fn test_merchant_pos_roundtrip() {
        let mut original = RelayConfig::default();
        let ticket = [0xCA, 0xFE, 0xBA, 0xBE, 0x01, 0x02, 0x03, 0x04];
        original.set_merchant_pos(&ticket);

        let bytes = original.to_flash_bytes();
        let loaded = RelayConfig::from_flash_bytes(&bytes);

        assert_eq!(loaded.mode, Mode::MerchantPos);
        assert_eq!(loaded.fixed_amount, 0);
        assert_eq!(loaded.receiver_ticket.as_slice(), &ticket);
        assert!(loaded.has_saved_receiver());
        assert!(!loaded.has_fixed_amount());
    }

    #[test]
    fn test_event_mode_roundtrip() {
        let mut original = RelayConfig::default();
        let ticket = [0xDE, 0xAD, 0xBE, 0xEF];
        original.set_event_mode(500, &ticket);

        let bytes = original.to_flash_bytes();
        let loaded = RelayConfig::from_flash_bytes(&bytes);

        assert_eq!(loaded.mode, Mode::EventMode);
        assert_eq!(loaded.fixed_amount, 500);
        assert_eq!(loaded.receiver_ticket.as_slice(), &ticket);
        assert!(loaded.has_saved_receiver());
        assert!(loaded.has_fixed_amount());
    }

    #[test]
    fn test_event_mode_large_ticket() {
        let mut original = RelayConfig::default();
        let ticket = [0xAB; 256]; // 256-byte ticket
        original.set_event_mode(1000, &ticket);

        let bytes = original.to_flash_bytes();
        let loaded = RelayConfig::from_flash_bytes(&bytes);

        assert_eq!(loaded.mode, Mode::EventMode);
        assert_eq!(loaded.fixed_amount, 1000);
        assert_eq!(loaded.receiver_ticket.len(), 256);
        assert_eq!(loaded.receiver_ticket[0], 0xAB);
        assert_eq!(loaded.receiver_ticket[255], 0xAB);
    }

    // --- Flash layout verification ---

    #[test]
    fn test_flash_layout_magic() {
        let c = RelayConfig::default();
        let bytes = c.to_flash_bytes();
        assert_eq!(&bytes[0..4], b"BRCF");
    }

    #[test]
    fn test_flash_layout_version() {
        let c = RelayConfig::default();
        let bytes = c.to_flash_bytes();
        assert_eq!(bytes[4], CONFIG_VERSION);
    }

    #[test]
    fn test_flash_layout_mode_byte() {
        let mut c = RelayConfig::default();
        assert_eq!(c.to_flash_bytes()[5], 0); // Variable

        c.set_merchant_pos(&[0x01]);
        assert_eq!(c.to_flash_bytes()[5], 1); // MerchantPos

        c.set_event_mode(10, &[0x01]);
        assert_eq!(c.to_flash_bytes()[5], 2); // EventMode
    }

    #[test]
    fn test_flash_layout_amount_encoding() {
        let mut c = RelayConfig::default();
        c.set_event_mode(0x01020304, &[0x01]);
        let bytes = c.to_flash_bytes();
        assert_eq!(&bytes[6..10], &[0x01, 0x02, 0x03, 0x04]); // big-endian
    }

    #[test]
    fn test_flash_layout_ticket_length() {
        let mut c = RelayConfig::default();
        let ticket = [0xFF; 100];
        c.set_event_mode(1, &ticket);
        let bytes = c.to_flash_bytes();
        assert_eq!(&bytes[10..12], &[0x00, 0x64]); // 100 big-endian
    }

    #[test]
    fn test_flash_rest_is_erased() {
        let c = RelayConfig::default();
        let bytes = c.to_flash_bytes();
        // After the header (12 bytes, no ticket), rest should be 0xFF.
        for &b in &bytes[12..] {
            assert_eq!(b, 0xFF);
        }
    }

    // --- Invalid flash data handling ---

    #[test]
    fn test_erased_flash_returns_default() {
        let erased = [0xFF; CONFIG_PAGE_SIZE];
        let c = RelayConfig::from_flash_bytes(&erased);
        assert_eq!(c.mode, Mode::Variable);
        assert_eq!(c.fixed_amount, 0);
    }

    #[test]
    fn test_wrong_magic_returns_default() {
        let mut data = [0u8; CONFIG_PAGE_SIZE];
        data[0..4].copy_from_slice(b"JUNK");
        let c = RelayConfig::from_flash_bytes(&data);
        assert_eq!(c.mode, Mode::Variable);
    }

    #[test]
    fn test_wrong_version_returns_default() {
        let mut data = [0u8; CONFIG_PAGE_SIZE];
        data[0..4].copy_from_slice(&CONFIG_MAGIC);
        data[4] = 99; // wrong version
        let c = RelayConfig::from_flash_bytes(&data);
        assert_eq!(c.mode, Mode::Variable);
    }

    #[test]
    fn test_invalid_mode_returns_default() {
        let mut data = [0u8; CONFIG_PAGE_SIZE];
        data[0..4].copy_from_slice(&CONFIG_MAGIC);
        data[4] = CONFIG_VERSION;
        data[5] = 7; // invalid mode
        let c = RelayConfig::from_flash_bytes(&data);
        assert_eq!(c.mode, Mode::Variable);
    }

    #[test]
    fn test_truncated_flash_returns_default() {
        let c = RelayConfig::from_flash_bytes(&[0x42, 0x52]); // too short
        assert_eq!(c.mode, Mode::Variable);
    }

    #[test]
    fn test_ticket_not_loaded_for_variable_mode() {
        // Even if ticket data is present, Variable mode doesn't load it.
        let mut c = RelayConfig::default();
        c.set_event_mode(100, &[0xAA; 32]);
        let mut bytes = c.to_flash_bytes();
        bytes[5] = 0; // Override mode to Variable

        let loaded = RelayConfig::from_flash_bytes(&bytes);
        assert_eq!(loaded.mode, Mode::Variable);
        assert!(loaded.receiver_ticket.is_empty()); // ticket NOT loaded
    }

    #[test]
    fn test_ticket_truncated_in_flash() {
        // Ticket length says 100, but flash data only has 50 bytes.
        let mut data = [0xFFu8; 64]; // short buffer
        data[0..4].copy_from_slice(&CONFIG_MAGIC);
        data[4] = CONFIG_VERSION;
        data[5] = 2; // EventMode
        data[6..10].copy_from_slice(&100u32.to_be_bytes());
        data[10..12].copy_from_slice(&100u16.to_be_bytes()); // claims 100 bytes

        let loaded = RelayConfig::from_flash_bytes(&data);
        // Should fall back because data.len() < 12 + 100.
        assert!(loaded.receiver_ticket.is_empty());
    }

    // --- Config mutation ---

    #[test]
    fn test_clear_resets_to_variable() {
        let mut c = RelayConfig::default();
        c.set_event_mode(999, &[0xBB; 16]);
        assert_eq!(c.mode, Mode::EventMode);
        assert!(c.has_fixed_amount());

        c.clear();
        assert_eq!(c.mode, Mode::Variable);
        assert_eq!(c.fixed_amount, 0);
        assert!(c.receiver_ticket.is_empty());
    }

    #[test]
    fn test_set_merchant_pos_clears_amount() {
        let mut c = RelayConfig::default();
        c.set_event_mode(500, &[0xCC; 8]);
        assert_eq!(c.fixed_amount, 500);

        c.set_merchant_pos(&[0xDD; 8]);
        assert_eq!(c.mode, Mode::MerchantPos);
        assert_eq!(c.fixed_amount, 0); // cleared
        assert_eq!(c.receiver_ticket.len(), 8);
    }

    #[test]
    fn test_set_event_mode_sets_both() {
        let mut c = RelayConfig::default();
        c.set_event_mode(42, &[0xEE; 4]);
        assert_eq!(c.mode, Mode::EventMode);
        assert_eq!(c.fixed_amount, 42);
        assert_eq!(c.receiver_ticket.as_slice(), &[0xEE; 4]);
    }

    #[test]
    fn test_has_fixed_amount_only_event_mode() {
        let mut c = RelayConfig::default();
        c.fixed_amount = 100;
        c.mode = Mode::Variable;
        assert!(!c.has_fixed_amount()); // not event mode

        c.mode = Mode::MerchantPos;
        assert!(!c.has_fixed_amount()); // not event mode

        c.mode = Mode::EventMode;
        assert!(c.has_fixed_amount()); // event mode with amount
    }

    #[test]
    fn test_event_mode_zero_amount() {
        let mut c = RelayConfig::default();
        c.set_event_mode(0, &[0x01]);
        assert_eq!(c.mode, Mode::EventMode);
        assert!(!c.has_fixed_amount()); // 0 is not a valid fixed amount
    }
}
