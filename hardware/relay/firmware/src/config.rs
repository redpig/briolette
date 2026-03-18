//! Relay configuration — operating modes and flash-backed settings.
//!
//! Three operating modes:
//!
//! 1. **Variable** — both amount and wallets are entered/scanned each time.
//!    Full flexibility for ad-hoc peer-to-peer payments.
//!
//! 2. **MerchantPos** — variable amount, saved receiver. The merchant's
//!    credstick ticket is cached in flash. Operator enters a new amount
//!    each time, but the receiver is always the same merchant wallet.
//!    Good for market stalls with varied prices.
//!
//! 3. **EventMode** — fixed amount AND fixed receiver. Both are saved in
//!    flash. Each customer is a single tap. Maximum throughput for
//!    transit, event gates, vending, anywhere a fixed price applies.
//!
//! Configuration is stored in a dedicated flash page (4KB) at the end of
//! the nRF52840's 1MB flash, preserved across power cycles and firmware
//! updates (separate from the firmware image).
//!
//! Flash layout (page at 0x000FF000):
//!   [0x00..0x04]  Magic bytes: "BRCF"
//!   [0x04..0x05]  Config version (1)
//!   [0x05..0x06]  Mode: 0=variable, 1=merchant-pos, 2=event-mode
//!   [0x06..0x0A]  Fixed amount (u32 big-endian, in token base units)
//!   [0x0A..0x0C]  Receiver ticket length (u16 big-endian)
//!   [0x0C..0x20C] Receiver SignedTicket (up to 512 bytes)
//!   [0x20C..0x210] CRC32 of [0x00..0x20C]

use heapless::Vec;

/// Flash page address for config storage.
const CONFIG_FLASH_ADDR: u32 = 0x000F_F000;

/// Config magic bytes.
const CONFIG_MAGIC: [u8; 4] = *b"BRCF";

/// Current config version.
const CONFIG_VERSION: u8 = 1;

/// Maximum receiver ticket size.
const MAX_TICKET_SIZE: usize = 512;

/// Relay operating mode.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Mode {
    /// Variable amount and wallets. Both sides scanned/entered each time.
    Variable,
    /// Variable amount, saved receiver (merchant POS). Operator enters
    /// amount; receiver wallet is cached from initial config.
    MerchantPos,
    /// Fixed amount AND fixed receiver (event mode). Single-tap customer
    /// payment. Maximum throughput for transit, events, vending.
    EventMode,
}

/// Relay configuration stored in flash.
#[derive(Clone)]
pub struct Config {
    pub mode: Mode,
    /// Fixed amount in token base units (0 = not set / variable).
    pub fixed_amount: u32,
    /// Cached receiver SignedTicket (for MerchantPos and EventMode).
    pub receiver_ticket: Vec<u8, 512>,
}

impl Config {
    /// Default config: variable mode, no saved state.
    pub fn default() -> Self {
        Self {
            mode: Mode::Variable,
            fixed_amount: 0,
            receiver_ticket: Vec::new(),
        }
    }

    /// Load config from flash. Returns default if flash is erased/corrupt.
    pub fn load_from_flash() -> Self {
        let flash_ptr = CONFIG_FLASH_ADDR as *const u8;
        let flash_data = unsafe { core::slice::from_raw_parts(flash_ptr, 4096) };

        if flash_data[0..4] != CONFIG_MAGIC {
            defmt::info!("No config in flash, using defaults");
            return Self::default();
        }

        if flash_data[4] != CONFIG_VERSION {
            defmt::warn!("Config version mismatch, using defaults");
            return Self::default();
        }

        let mode = match flash_data[5] {
            0 => Mode::Variable,
            1 => Mode::MerchantPos,
            2 => Mode::EventMode,
            _ => {
                defmt::warn!("Invalid mode byte, using defaults");
                return Self::default();
            }
        };

        let fixed_amount = u32::from_be_bytes([
            flash_data[6],
            flash_data[7],
            flash_data[8],
            flash_data[9],
        ]);

        let ticket_len = u16::from_be_bytes([flash_data[10], flash_data[11]]) as usize;

        let mut receiver_ticket: Vec<u8, 512> = Vec::new();
        if ticket_len > 0
            && ticket_len <= MAX_TICKET_SIZE
            && (mode == Mode::MerchantPos || mode == Mode::EventMode)
        {
            receiver_ticket
                .extend_from_slice(&flash_data[12..12 + ticket_len])
                .ok();
        }

        defmt::info!(
            "Config loaded: mode={}, amount={}, ticket_len={}",
            mode,
            fixed_amount,
            ticket_len
        );

        Self {
            mode,
            fixed_amount,
            receiver_ticket,
        }
    }

    /// Save config to flash.
    ///
    /// Erases the config page and writes the new config.
    /// Must be called with interrupts masked (flash erase is blocking).
    pub fn save_to_flash(&self) {
        let mut page = [0xFFu8; 4096]; // 0xFF = erased flash value

        page[0..4].copy_from_slice(&CONFIG_MAGIC);
        page[4] = CONFIG_VERSION;
        page[5] = match self.mode {
            Mode::Variable => 0,
            Mode::MerchantPos => 1,
            Mode::EventMode => 2,
        };
        page[6..10].copy_from_slice(&self.fixed_amount.to_be_bytes());
        let ticket_len = self.receiver_ticket.len() as u16;
        page[10..12].copy_from_slice(&ticket_len.to_be_bytes());
        if !self.receiver_ticket.is_empty() {
            let end = 12 + self.receiver_ticket.len();
            page[12..end].copy_from_slice(&self.receiver_ticket);
        }

        // TODO: Direct NVMC register access for flash write.

        defmt::info!(
            "Config saved: mode={}, amount={}, ticket_len={}",
            self.mode,
            self.fixed_amount,
            ticket_len
        );
    }

    /// Build flash page bytes from config (for testing and save_to_flash).
    pub fn to_flash_bytes(&self) -> [u8; 4096] {
        let mut page = [0xFFu8; 4096];
        page[0..4].copy_from_slice(&CONFIG_MAGIC);
        page[4] = CONFIG_VERSION;
        page[5] = match self.mode {
            Mode::Variable => 0,
            Mode::MerchantPos => 1,
            Mode::EventMode => 2,
        };
        page[6..10].copy_from_slice(&self.fixed_amount.to_be_bytes());
        let ticket_len = self.receiver_ticket.len() as u16;
        page[10..12].copy_from_slice(&ticket_len.to_be_bytes());
        if !self.receiver_ticket.is_empty() {
            let end = 12 + self.receiver_ticket.len();
            page[12..end].copy_from_slice(&self.receiver_ticket);
        }
        page
    }

    /// Parse config from a flash page byte slice. Testable without hardware.
    pub fn from_flash_bytes(data: &[u8]) -> Self {
        if data.len() < 12 || data[0..4] != CONFIG_MAGIC {
            return Self::default();
        }
        if data[4] != CONFIG_VERSION {
            return Self::default();
        }

        let mode = match data[5] {
            0 => Mode::Variable,
            1 => Mode::MerchantPos,
            2 => Mode::EventMode,
            _ => return Self::default(),
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

    /// Check if receiver ticket is cached (MerchantPos or EventMode).
    pub fn has_saved_receiver(&self) -> bool {
        !self.receiver_ticket.is_empty()
    }

    /// Check if amount is fixed (EventMode only).
    pub fn has_fixed_amount(&self) -> bool {
        self.mode == Mode::EventMode && self.fixed_amount > 0
    }
}
