//! ATECC608B secure element I2C driver.
//!
//! DEPRECATED: Replaced by sim_card.rs (nano-SIM ISO 7816 interface).
//! This module is retained for reference only and is no longer compiled
//! into the firmware. See sim_card.rs for the current secure element driver.
//!
//! The ATECC608B provided:
//! - Manufacturer P-256 ECDSA key (generated and locked on-chip)
//! - Manufacturer certificate storage
//! - Hardware monotonic counter (for PIN attempt tracking)
//! - Challenge-response attestation
//!
//! All of these functions are now handled by the SIM card via ISO 7816.
//!
//! I2C address: 0x60 (default for ATECC608B).

use embassy_nrf::twim::Twim;
use embassy_nrf::peripherals;

/// Default I2C address for ATECC608B.
const I2C_ADDR: u8 = 0x60;

/// ATECC608B command opcodes.
mod cmd {
    pub const SIGN: u8 = 0x41;
    pub const GENKEY: u8 = 0x40;
    pub const READ: u8 = 0x02;
    pub const WRITE: u8 = 0x12;
    pub const COUNTER: u8 = 0x24;
    pub const NONCE: u8 = 0x16;
    pub const INFO: u8 = 0x30;
}

/// Counter slots for specific purposes.
mod counter_slot {
    /// PIN attempt counter. Incremented on each failed PIN entry.
    pub const PIN_ATTEMPTS: u8 = 0;
    /// Epoch counter. Incremented on each epoch transition.
    pub const EPOCH: u8 = 1;
}

/// Key slots.
mod key_slot {
    /// Manufacturer P-256 private key (generated, never exported).
    pub const MFR_KEY: u8 = 0;
    /// Manufacturer certificate (written during personalization).
    pub const MFR_CERT: u8 = 8;
}

pub struct Atecc608b<'d> {
    twi: Twim<'d, peripherals::TWISPI0>,
    /// Cached PIN attempt count (read on init).
    pin_attempts: u32,
}

impl<'d> Atecc608b<'d> {
    pub fn new(twi: Twim<'d, peripherals::TWISPI0>) -> Self {
        let mut atecc = Self {
            twi,
            pin_attempts: 0,
        };
        // Wake the device and read initial counter values.
        atecc.wake();
        atecc.pin_attempts = atecc.read_counter(counter_slot::PIN_ATTEMPTS);
        defmt::info!("ATECC608B: PIN attempt counter = {}", atecc.pin_attempts);
        atecc
    }

    /// Wake the ATECC608B from sleep.
    ///
    /// The device enters sleep automatically after 1.5s of inactivity.
    /// Wake protocol: send 0x00 at low I2C speed, wait 2.5ms, then
    /// check for wake response (0x04, 0x11, 0x33, 0x43).
    fn wake(&mut self) {
        // Send wake token (0x00 address write).
        let _ = self.twi.blocking_write(0x00, &[0x00]);
        // Wait for wake-up (tWHI = 2.5ms).
        cortex_m::asm::delay(64_000_000 / 400); // ~2.5ms at 64MHz
        // Read wake response.
        let mut resp = [0u8; 4];
        let _ = self.twi.blocking_read(I2C_ADDR, &mut resp);
        if resp == [0x04, 0x11, 0x33, 0x43] {
            defmt::debug!("ATECC608B: awake");
        }
    }

    /// Put the ATECC608B into sleep mode (~30nA).
    pub fn sleep(&mut self) {
        let _ = self.twi.blocking_write(I2C_ADDR, &[0x01]); // Sleep word address
    }

    /// Sign a challenge with the manufacturer P-256 key.
    ///
    /// Returns a DER-encoded ECDSA-SHA256 signature (~72 bytes).
    pub fn sign_challenge(&mut self, challenge: &[u8; 32]) -> Option<[u8; 64]> {
        self.wake();

        // Load challenge into TempKey via Nonce command.
        self.send_command(cmd::NONCE, 0x03, 0x00, challenge);
        self.wait_execution(60); // Nonce: ~60ms

        // Sign with key slot 0.
        self.send_command(cmd::SIGN, 0x80, key_slot::MFR_KEY as u16, &[]);
        self.wait_execution(115); // Sign: ~115ms

        // Read response (64 bytes: r || s).
        let mut sig = [0u8; 64];
        if self.read_response(&mut sig) {
            Some(sig)
        } else {
            None
        }
    }

    /// Read the manufacturer certificate from slot 8.
    pub fn read_certificate(&mut self) -> Option<[u8; 72]> {
        self.wake();

        // Read 72 bytes from data slot 8.
        let mut cert = [0u8; 72];
        self.send_command(cmd::READ, 0x82, key_slot::MFR_CERT as u16, &[]);
        self.wait_execution(5);

        if self.read_response(&mut cert) {
            Some(cert)
        } else {
            None
        }
    }

    /// Read the manufacturer public key (from GenKey in public mode).
    pub fn read_public_key(&mut self) -> Option<[u8; 64]> {
        self.wake();

        // GenKey in public key computation mode (mode=0x00).
        self.send_command(cmd::GENKEY, 0x00, key_slot::MFR_KEY as u16, &[]);
        self.wait_execution(115);

        let mut pubkey = [0u8; 64];
        if self.read_response(&mut pubkey) {
            Some(pubkey)
        } else {
            None
        }
    }

    /// Increment the PIN attempt counter.
    /// Returns the new counter value.
    pub fn increment_pin_counter(&mut self) -> u32 {
        self.wake();
        self.send_command(cmd::COUNTER, 0x01, counter_slot::PIN_ATTEMPTS as u16, &[]);
        self.wait_execution(20);

        let mut val = [0u8; 4];
        if self.read_response(&mut val) {
            self.pin_attempts = u32::from_le_bytes(val);
        }
        self.pin_attempts
    }

    /// Read a monotonic counter value.
    fn read_counter(&mut self, slot: u8) -> u32 {
        self.send_command(cmd::COUNTER, 0x00, slot as u16, &[]);
        self.wait_execution(20);

        let mut val = [0u8; 4];
        if self.read_response(&mut val) {
            u32::from_le_bytes(val)
        } else {
            0
        }
    }

    /// Get the current PIN attempt count.
    pub fn pin_attempt_count(&self) -> u32 {
        self.pin_attempts
    }

    // --- Low-level I2C protocol ---

    /// Send a command to the ATECC608B.
    fn send_command(&mut self, opcode: u8, param1: u8, param2: u16, data: &[u8]) {
        // ATECC608B command packet format:
        // [0x03] [length] [opcode] [param1] [param2_lo] [param2_hi] [data...] [crc16]
        let mut packet = [0u8; 128];
        let data_len = data.len();
        let total_len = 7 + data_len + 2; // word_addr + count + cmd + crc

        packet[0] = 0x03; // Command word address
        packet[1] = (total_len - 1) as u8; // Count (everything after word_addr)
        packet[2] = opcode;
        packet[3] = param1;
        packet[4] = (param2 & 0xFF) as u8;
        packet[5] = ((param2 >> 8) & 0xFF) as u8;

        if !data.is_empty() {
            packet[6..6 + data_len].copy_from_slice(data);
        }

        // CRC-16 over count..data.
        let crc = crc16(&packet[1..6 + data_len]);
        packet[6 + data_len] = (crc & 0xFF) as u8;
        packet[7 + data_len] = ((crc >> 8) & 0xFF) as u8;

        let _ = self.twi.blocking_write(I2C_ADDR, &packet[..8 + data_len]);
    }

    /// Wait for command execution (busy-poll).
    fn wait_execution(&self, max_ms: u32) {
        // Simple delay; in production use embassy_time::Timer.
        let cycles_per_ms = 64_000;
        for _ in 0..max_ms {
            cortex_m::asm::delay(cycles_per_ms);
        }
    }

    /// Read response from the ATECC608B.
    fn read_response(&mut self, buf: &mut [u8]) -> bool {
        let mut raw = [0u8; 130]; // Max response + count + CRC
        if self.twi.blocking_read(I2C_ADDR, &mut raw[..buf.len() + 3]).is_err() {
            return false;
        }

        let count = raw[0] as usize;
        if count < 4 || count > buf.len() + 3 {
            return false;
        }

        // Check CRC.
        let crc = crc16(&raw[..count - 2]);
        let expected = u16::from_le_bytes([raw[count - 2], raw[count - 1]]);
        if crc != expected {
            defmt::warn!("ATECC608B: CRC mismatch");
            return false;
        }

        // Copy data (skip count byte, exclude CRC).
        let data_len = count - 3;
        let copy_len = core::cmp::min(data_len, buf.len());
        buf[..copy_len].copy_from_slice(&raw[1..1 + copy_len]);
        true
    }
}

/// CRC-16 (polymod 0x8005, init 0x0000) used by ATECC608B.
fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        for bit in 0..8 {
            let data_bit = ((byte >> bit) & 1) as u16;
            let crc_bit = (crc >> 15) & 1;
            crc <<= 1;
            if data_bit != crc_bit {
                crc ^= 0x8005;
            }
        }
    }
    crc
}
