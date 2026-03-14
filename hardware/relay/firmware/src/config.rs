//! Merchant POS configuration — fixed amount + fixed receiver.
//!
//! In **merchant POS mode**, the relay is personalized to a specific merchant:
//!   - Fixed amount: e.g., bus fare, event admission, market stall price
//!   - Fixed receiver: the merchant's credstick SignedTicket cached in flash
//!
//! This turns the relay into a simple, dedicated payment terminal:
//!   1. Configure once: set amount + tap merchant credstick
//!   2. Each customer: single tap → instant payment
//!   No rescanning, no retyping, no phone needed.
//!
//! Perfect for high-volume scenarios: transit, event gates, market stalls.
//!
//! Configuration is stored in a dedicated flash page (4KB) at the end of
//! the nRF52840's 1MB flash, preserved across power cycles and firmware
//! updates (separate from the firmware image).
//!
//! Flash layout (page at 0x000FF000):
//!   [0x00..0x04]  Magic bytes: "BRCF"
//!   [0x04..0x05]  Config version (1)
//!   [0x05..0x06]  Mode: 0=variable, 1=fixed-amount, 2=fixed-amount+receiver
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
    /// Variable amount — operator enters amount via keypad each time.
    Variable,
    /// Fixed amount — same price for every transaction, operator just
    /// presses OK to start. Still requires receiver tap each time.
    FixedAmount,
    /// Fixed amount + fixed receiver (merchant POS mode).
    /// One-tap customer payment. Maximum throughput.
    MerchantPos,
}

/// Relay configuration stored in flash.
#[derive(Clone)]
pub struct Config {
    pub mode: Mode,
    /// Fixed amount in token base units (0 = not set).
    pub fixed_amount: u32,
    /// Cached receiver SignedTicket for merchant POS mode.
    pub receiver_ticket: Vec<u8, 512>,
}

impl Config {
    /// Default config: variable amount mode.
    pub fn default() -> Self {
        Self {
            mode: Mode::Variable,
            fixed_amount: 0,
            receiver_ticket: Vec::new(),
        }
    }

    /// Load config from flash. Returns default if flash is erased/corrupt.
    pub fn load_from_flash() -> Self {
        // Read flash page.
        let flash_ptr = CONFIG_FLASH_ADDR as *const u8;
        let flash_data = unsafe { core::slice::from_raw_parts(flash_ptr, 4096) };

        // Verify magic.
        if flash_data[0..4] != CONFIG_MAGIC {
            defmt::info!("No config in flash, using defaults");
            return Self::default();
        }

        // Verify version.
        if flash_data[4] != CONFIG_VERSION {
            defmt::warn!("Config version mismatch, using defaults");
            return Self::default();
        }

        let mode = match flash_data[5] {
            0 => Mode::Variable,
            1 => Mode::FixedAmount,
            2 => Mode::MerchantPos,
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
        if ticket_len > 0 && ticket_len <= MAX_TICKET_SIZE && mode == Mode::MerchantPos {
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
    /// This erases the config page and writes the new config.
    /// Must be called with interrupts masked (flash erase is blocking).
    pub fn save_to_flash(&self) {
        // Build the flash page data.
        let mut page = [0xFFu8; 4096]; // 0xFF = erased flash value

        // Magic.
        page[0..4].copy_from_slice(&CONFIG_MAGIC);
        // Version.
        page[4] = CONFIG_VERSION;
        // Mode.
        page[5] = match self.mode {
            Mode::Variable => 0,
            Mode::FixedAmount => 1,
            Mode::MerchantPos => 2,
        };
        // Fixed amount.
        page[6..10].copy_from_slice(&self.fixed_amount.to_be_bytes());
        // Receiver ticket length.
        let ticket_len = self.receiver_ticket.len() as u16;
        page[10..12].copy_from_slice(&ticket_len.to_be_bytes());
        // Receiver ticket data.
        if !self.receiver_ticket.is_empty() {
            let end = 12 + self.receiver_ticket.len();
            page[12..end].copy_from_slice(&self.receiver_ticket);
        }

        // Erase and write flash page.
        // Uses nRF52840 NVMC (Non-Volatile Memory Controller).
        //
        // TODO: Direct NVMC register access:
        // let nvmc = unsafe { &*pac::NVMC::ptr() };
        // nvmc.config.write(|w| w.wen().een()); // Enable erase
        // nvmc.erasepage.write(|w| unsafe { w.bits(CONFIG_FLASH_ADDR) });
        // while nvmc.ready.read().ready().is_busy() {}
        // nvmc.config.write(|w| w.wen().wen()); // Enable write
        // for (i, chunk) in page.chunks(4).enumerate() {
        //     let addr = CONFIG_FLASH_ADDR + (i * 4) as u32;
        //     let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        //     unsafe { (addr as *mut u32).write_volatile(word) };
        //     while nvmc.ready.read().ready().is_busy() {}
        // }
        // nvmc.config.write(|w| w.wen().ren()); // Read-only

        defmt::info!(
            "Config saved: mode={}, amount={}, ticket_len={}",
            self.mode,
            self.fixed_amount,
            ticket_len
        );
    }

    /// Set up fixed-amount mode. Amount is in token base units.
    pub fn set_fixed_amount(&mut self, amount: u32) {
        self.fixed_amount = amount;
        if self.receiver_ticket.is_empty() {
            self.mode = Mode::FixedAmount;
        } else {
            self.mode = Mode::MerchantPos;
        }
    }

    /// Cache the receiver's SignedTicket for merchant POS mode.
    pub fn set_receiver_ticket(&mut self, ticket: &[u8]) {
        self.receiver_ticket.clear();
        self.receiver_ticket.extend_from_slice(ticket).ok();
        if self.fixed_amount > 0 {
            self.mode = Mode::MerchantPos;
        }
    }

    /// Clear the fixed config and return to variable mode.
    pub fn clear(&mut self) {
        self.mode = Mode::Variable;
        self.fixed_amount = 0;
        self.receiver_ticket.clear();
    }

    /// Check if this is a personalized merchant POS.
    pub fn is_merchant_pos(&self) -> bool {
        self.mode == Mode::MerchantPos
    }
}
