//! Flash storage for persistent credstick state.
//!
//! Stores:
//! - ECDAA secret key half (card_sk, 32 bytes)
//! - SignedTicket (variable length, up to ~256 bytes)
//! - Token inventory (owned tokens, variable)
//! - Received tokens (unswept, variable)
//! - PIN hash (Argon2id output, 32 bytes)
//! - Epoch data (cached, variable)
//! - Configuration (privacy mode, PIN threshold, etc.)
//!
//! Uses the nRF52840's internal flash (1MB). Flash pages are 4KB on nRF52840.
//! We use a simple log-structured storage to minimize wear:
//! - Append new state entries to the current page
//! - When page is full, compact live entries to a new page
//! - Erase the old page
//!
//! Flash endurance: ~10,000 erase cycles per page. With 256 4KB pages
//! and round-robin allocation, this gives ~2.56M total erase operations.
//! At 10 transactions/day with 1 erase/transaction: ~700 years.

use heapless::Vec;

use crate::button::PinSymbol;

/// Flash page size on nRF52840.
const PAGE_SIZE: usize = 4096;

/// Storage region: last 64KB of flash (16 pages), leaving ~960KB for firmware.
const STORAGE_BASE: usize = 0x000F_0000;
const STORAGE_PAGES: usize = 16;

/// Key storage slots.
mod slot {
    pub const ECDAA_SK: u8 = 0x01;
    pub const SIGNED_TICKET: u8 = 0x02;
    pub const PIN_HASH: u8 = 0x03;
    pub const EPOCH_DATA: u8 = 0x04;
    pub const CONFIG: u8 = 0x05;
    pub const TOKEN_INVENTORY: u8 = 0x10;
    pub const RECEIVED_TOKENS: u8 = 0x20;
}

/// In-memory state (loaded from flash on init).
pub struct Storage {
    /// ECDAA secret key half (32 bytes).
    ecdaa_sk: [u8; 32],
    /// Whether a key has been generated.
    key_initialized: bool,
    /// Token balance (whole tokens, simplified).
    balance: u32,
    /// SignedTicket (serialized protobuf).
    signed_ticket: Vec<u8, 256>,
    /// Epoch data (serialized).
    epoch_data: Vec<u8, 512>,
    /// Current epoch number.
    epoch_number: u32,
    /// PIN hash (Argon2id output).
    pin_hash: Option<[u8; 32]>,
    /// Received tokens awaiting sweep.
    received_tokens: Vec<u8, 4096>,
    /// Received token count.
    received_count: u32,
}

impl Storage {
    /// Initialize storage: read persistent state from flash.
    pub fn init() -> Self {
        let mut storage = Self {
            ecdaa_sk: [0u8; 32],
            key_initialized: false,
            balance: 0,
            signed_ticket: Vec::new(),
            epoch_data: Vec::new(),
            epoch_number: 0,
            pin_hash: None,
            received_tokens: Vec::new(),
            received_count: 0,
        };

        storage.load_from_flash();
        storage
    }

    pub fn balance(&self) -> u32 {
        self.balance
    }

    pub fn ecdaa_secret_key(&self) -> &[u8] {
        &self.ecdaa_sk
    }

    pub fn signed_ticket(&self) -> &[u8] {
        &self.signed_ticket
    }

    pub fn epoch_data(&self) -> &[u8] {
        &self.epoch_data
    }

    pub fn epoch_number(&self) -> u32 {
        self.epoch_number
    }

    pub fn has_pin(&self) -> bool {
        self.pin_hash.is_some()
    }

    /// Verify a PIN sequence against the stored hash.
    pub fn verify_pin(&self, pin: &[PinSymbol]) -> bool {
        let hash = match &self.pin_hash {
            Some(h) => h,
            None => return true, // No PIN set = always passes.
        };

        // Hash the PIN input with Argon2id.
        // On Cortex-M4, Argon2id is expensive (~100ms with reduced parameters).
        // For the credstick, use reduced parameters:
        //   - m=256 (256 KB memory — fits in nRF52840's 256KB RAM)
        //   - t=1 (single iteration)
        //   - p=1 (single thread)
        //
        // TODO: Implement Argon2id. For now, use SHA-256 as placeholder.
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for symbol in pin {
            hasher.update(&[symbol.to_byte()]);
        }
        // Include device-specific salt (ATECC608B serial would be ideal).
        hasher.update(b"briolette-credstick-pin-salt");
        let computed = hasher.finalize();

        // Constant-time comparison.
        let mut diff = 0u8;
        for i in 0..32 {
            diff |= computed[i] ^ hash[i];
        }
        diff == 0
    }

    /// Select tokens from inventory to fulfill an amount.
    ///
    /// Writes serialized unsigned Token[] to `out`.
    /// Simple greedy selection: pick tokens until sum >= amount.
    pub fn select_tokens(&self, amount: u32, out: &mut Vec<u8, 2048>) {
        // Token inventory format (simplified):
        // [4B count] [per token: [4B value][48B descriptor][variable history]]
        //
        // For the unsigned proposal, we strip signatures and send:
        // [4B count] [per token: [48B S_point][48B basename_base]]
        //
        // TODO: Implement actual token selection from inventory.
        // Placeholder: write a dummy token count.
        let _ = out.extend_from_slice(&(0u32).to_be_bytes());
    }

    /// Deduct tokens from balance after a successful TRANSFER.
    pub fn deduct(&mut self, amount: u32) {
        self.balance = self.balance.saturating_sub(amount);
        self.save_to_flash();
    }

    /// Receive signed tokens from a sender.
    /// Returns the number of tokens received.
    pub fn receive_tokens(&mut self, token_data: &[u8]) -> u32 {
        // Append to received_tokens buffer.
        let _ = self.received_tokens.extend_from_slice(token_data);
        self.received_count += 1;

        // Update balance (optimistic — mark as unvalidated).
        // TODO: Parse actual token values from the data.
        // For now, assume 1 token per receive call.
        self.balance += 1;

        self.save_to_flash();
        self.received_count
    }

    /// Sweep all received tokens (return them for merchant collection).
    /// Returns the serialized tokens and clears the received buffer.
    pub fn sweep_received_tokens(&mut self) -> Vec<u8, 4096> {
        let tokens = self.received_tokens.clone();
        self.received_tokens.clear();
        self.received_count = 0;
        self.save_to_flash();
        tokens
    }

    /// Update epoch data from a GOSSIP exchange.
    pub fn update_epoch(&mut self, data: &[u8]) {
        self.epoch_data.clear();
        let _ = self.epoch_data.extend_from_slice(data);
        if data.len() >= 4 {
            self.epoch_number = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        }
        self.save_to_flash();
    }

    // --- Flash I/O ---

    /// Load all state from flash into RAM.
    fn load_from_flash(&mut self) {
        // Scan storage pages for the latest valid entries.
        // Each entry: [1B slot_id][2B length][data...][4B CRC32]
        //
        // TODO: Implement flash scanning.
        // For now, start with empty state.
        defmt::info!("Storage: loaded from flash (placeholder — empty state)");
    }

    /// Persist current state to flash.
    fn save_to_flash(&self) {
        // Append current state as log entries to the active flash page.
        // If the page is full, compact and move to the next page.
        //
        // TODO: Implement flash writing.
        // nRF52840 flash write:
        //   1. Erase page (4KB) — sets all bits to 1
        //   2. Write words (4 bytes at a time) — clears bits to 0
        //   3. Cannot set bits back to 1 without erasing entire page
        //
        // Use embassy-nrf's NVMC (Non-Volatile Memory Controller) driver:
        //   let mut nvmc = Nvmc::new(p.NVMC);
        //   nvmc.erase(page_addr)?;
        //   nvmc.write(addr, &data)?;
        defmt::debug!("Storage: saved to flash (placeholder)");
    }
}
