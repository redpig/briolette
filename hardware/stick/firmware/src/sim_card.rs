//! ISO 7816 SIM card driver for the credstick secure element.
//!
//! Replaces the ATECC608B with a standard nano-SIM card in a low-profile
//! push-push connector (Molex 78800-0001). The SIM card provides:
//! - Manufacturer P-256 ECDSA key (generated on-card, never exported)
//! - Manufacturer certificate storage
//! - Hardware monotonic counter (for PIN attempt tracking)
//! - Challenge-response attestation
//! - Tamper-resistant key storage (CC EAL4+ certified)
//!
//! Advantages over ATECC608B:
//! - User-replaceable: swap SIM to transfer identity
//! - Standard interface (ISO 7816 T=0/T=1)
//! - Widely available, multiple vendor options
//! - JavaCard applet ecosystem for custom SE applications
//!
//! Interface: ISO 7816-3 T=0 protocol over nRF52840 UARTE.
//! - SIM_IO  (P0.26): Half-duplex data (UARTE TX/RX with direction control)
//! - SIM_CLK (P0.27): Card clock, 3.25 MHz via PWM
//! - SIM_RST (P0.22): Card reset, active low
//! - SIM_DET (P0.24): Card detect, active low when inserted

use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::peripherals;
use embassy_nrf::uarte::{self, Uarte};
use embassy_time::{Duration, Timer};

/// ISO 7816 APDU class byte for the Briolette SE applet.
const CLA_BRIOLETTE_SE: u8 = 0x80;

/// Standard ISO 7816 CLA for interindustry commands.
const CLA_ISO: u8 = 0x00;

/// ISO 7816-4 instruction codes for SE applet operations.
mod ins {
    /// SELECT command (ISO 7816-4).
    pub const SELECT: u8 = 0xA4;
    /// VERIFY PIN (ISO 7816-4).
    pub const VERIFY: u8 = 0x20;
    /// INTERNAL AUTHENTICATE — sign challenge with manufacturer key.
    pub const INTERNAL_AUTH: u8 = 0x88;
    /// GET DATA — read certificate or public key.
    pub const GET_DATA: u8 = 0xCA;
    /// Increment monotonic counter (applet-specific).
    pub const INCREMENT_COUNTER: u8 = 0xD0;
    /// Read monotonic counter value (applet-specific).
    pub const READ_COUNTER: u8 = 0xD2;
}

/// Briolette SE applet AID (application identifier).
/// Registered under a test RID; production would use a real RID.
const APPLET_AID: &[u8] = &[0xF0, 0x42, 0x52, 0x49, 0x4F, 0x4C, 0x01]; // F0 "BRIOL" 01

/// GET DATA tags for different objects.
mod tag {
    /// Manufacturer P-256 public key (uncompressed, 65 bytes).
    pub const MFR_PUBKEY: u16 = 0xDF01;
    /// Manufacturer certificate (DER-encoded X.509, variable length).
    pub const MFR_CERT: u16 = 0xDF02;
}

/// Counter slot identifiers (matches applet's internal counter indices).
mod counter {
    pub const PIN_ATTEMPTS: u8 = 0x00;
    pub const EPOCH: u8 = 0x01;
}

/// ISO 7816 status words.
mod sw {
    pub const SUCCESS: u16 = 0x9000;
    pub const WRONG_LENGTH: u16 = 0x6700;
    pub const SECURITY_NOT_SATISFIED: u16 = 0x6982;
    pub const AUTH_BLOCKED: u16 = 0x6983;
    pub const WRONG_DATA: u16 = 0x6A80;
    pub const FILE_NOT_FOUND: u16 = 0x6A82;
    pub const WRONG_P1P2: u16 = 0x6A86;
    pub const INS_NOT_SUPPORTED: u16 = 0x6D00;
}

pub struct SimCard<'d> {
    uart: Uarte<'d, peripherals::UARTE0>,
    rst: Output<'d>,
    det: Input<'d>,
    /// ATR (Answer to Reset) cached after card initialization.
    atr: [u8; 33],
    atr_len: usize,
    /// Whether the Briolette SE applet has been selected.
    applet_selected: bool,
    /// Cached PIN attempt count.
    pin_attempts: u32,
}

impl<'d> SimCard<'d> {
    /// Create a new SIM card driver.
    ///
    /// Requires: UARTE0 for SIM_IO, GPIO for RST and card detect.
    /// The SIM_CLK (3.25 MHz) must be started separately via PWM before
    /// calling this constructor.
    pub fn new(
        uart: Uarte<'d, peripherals::UARTE0>,
        rst: Output<'d>,
        det: Input<'d>,
    ) -> Self {
        Self {
            uart,
            rst,
            det,
            atr: [0u8; 33],
            atr_len: 0,
            applet_selected: false,
            pin_attempts: 0,
        }
    }

    /// Check if a SIM card is physically present in the connector.
    pub fn card_present(&self) -> bool {
        // Card detect pin is active low (grounded when card inserted).
        self.det.is_low()
    }

    /// Initialize the SIM card: perform reset, read ATR, select applet.
    ///
    /// Returns true if the card is present and the applet was selected.
    pub async fn init(&mut self) -> bool {
        if !self.card_present() {
            defmt::warn!("SIM: no card detected");
            return false;
        }

        // Cold reset sequence (ISO 7816-3 §6.2.2):
        // 1. Assert RST low
        // 2. Wait >= 400 clock cycles (at 3.25 MHz: ~123µs)
        // 3. Release RST high
        // 4. Card sends ATR within 40,000 clock cycles (~12.3ms)
        self.rst.set_low();
        Timer::after(Duration::from_micros(200)).await;
        self.rst.set_high();

        // Read ATR (Answer to Reset).
        Timer::after(Duration::from_millis(20)).await;
        let mut atr_buf = [0u8; 33];
        match self.uart.read(&mut atr_buf).await {
            Ok(()) => {
                // Find actual ATR length from format byte TS + T0.
                self.atr_len = parse_atr_length(&atr_buf);
                self.atr[..self.atr_len].copy_from_slice(&atr_buf[..self.atr_len]);
                defmt::info!("SIM: ATR received, {} bytes", self.atr_len);
            }
            Err(_) => {
                defmt::warn!("SIM: ATR read failed");
                return false;
            }
        }

        // Select the Briolette SE applet.
        self.applet_selected = self.select_applet().await;
        if !self.applet_selected {
            defmt::warn!("SIM: applet selection failed");
            return false;
        }

        // Read initial counter values.
        self.pin_attempts = self.read_counter(counter::PIN_ATTEMPTS).await;
        defmt::info!("SIM: initialized, PIN attempts = {}", self.pin_attempts);
        true
    }

    /// Put the SIM card into a low-power state.
    ///
    /// ISO 7816-3 §6.9: clock stop mode. We stop the CLK signal
    /// (handled externally by disabling PWM) and the card enters
    /// its idle state (~5µA typical for modern SIM cards).
    pub async fn sleep(&mut self) {
        // CLK stop is handled by the caller (disable PWM output).
        // Card will retain state until next clock edge.
        defmt::debug!("SIM: entering clock-stop mode");
    }

    /// Sign a 32-byte challenge with the manufacturer P-256 key.
    ///
    /// Uses INTERNAL AUTHENTICATE (INS 0x88) which is the standard
    /// ISO 7816-4 command for challenge-response signing.
    ///
    /// Returns a 64-byte ECDSA signature (r || s) or None on failure.
    pub async fn sign_challenge(&mut self, challenge: &[u8; 32]) -> Option<[u8; 64]> {
        if !self.applet_selected {
            return None;
        }

        // INTERNAL AUTHENTICATE: CLA=00 INS=88 P1=00 P2=00 Lc=20 Data=challenge Le=40
        let mut response = [0u8; 66]; // 64 bytes sig + 2 bytes SW
        let len = self
            .send_apdu(CLA_ISO, ins::INTERNAL_AUTH, 0x00, 0x00, challenge, &mut response)
            .await?;

        if len < 66 {
            return None;
        }

        let status = u16::from_be_bytes([response[64], response[65]]);
        if status != sw::SUCCESS {
            defmt::warn!("SIM: INTERNAL_AUTH failed, SW={=u16:04X}", status);
            return None;
        }

        let mut sig = [0u8; 64];
        sig.copy_from_slice(&response[..64]);
        Some(sig)
    }

    /// Read the manufacturer certificate from the SE applet.
    pub async fn read_certificate(&mut self) -> Option<[u8; 72]> {
        if !self.applet_selected {
            return None;
        }

        // GET DATA: CLA=80 INS=CA P1P2=tag Lc=0 Le=48
        let p1 = (tag::MFR_CERT >> 8) as u8;
        let p2 = (tag::MFR_CERT & 0xFF) as u8;

        let mut response = [0u8; 74]; // 72 bytes cert + 2 bytes SW
        let len = self
            .send_apdu(CLA_BRIOLETTE_SE, ins::GET_DATA, p1, p2, &[], &mut response)
            .await?;

        if len < 74 {
            return None;
        }

        let status = u16::from_be_bytes([response[72], response[73]]);
        if status != sw::SUCCESS {
            return None;
        }

        let mut cert = [0u8; 72];
        cert.copy_from_slice(&response[..72]);
        Some(cert)
    }

    /// Read the manufacturer public key from the SE applet.
    pub async fn read_public_key(&mut self) -> Option<[u8; 64]> {
        if !self.applet_selected {
            return None;
        }

        let p1 = (tag::MFR_PUBKEY >> 8) as u8;
        let p2 = (tag::MFR_PUBKEY & 0xFF) as u8;

        let mut response = [0u8; 67]; // 1 byte format + 64 bytes key + 2 bytes SW
        let len = self
            .send_apdu(CLA_BRIOLETTE_SE, ins::GET_DATA, p1, p2, &[], &mut response)
            .await?;

        if len < 67 {
            return None;
        }

        let status = u16::from_be_bytes([response[65], response[66]]);
        if status != sw::SUCCESS {
            return None;
        }

        // Skip the 0x04 uncompressed point prefix.
        let mut pubkey = [0u8; 64];
        pubkey.copy_from_slice(&response[1..65]);
        Some(pubkey)
    }

    /// Increment the PIN attempt counter on the SIM card.
    ///
    /// The SIM card's monotonic counter cannot be rolled back, even
    /// with physical access to the nRF52840 — it's inside the
    /// tamper-resistant SIM silicon.
    pub async fn increment_pin_counter(&mut self) -> u32 {
        if !self.applet_selected {
            return self.pin_attempts;
        }

        let data = [counter::PIN_ATTEMPTS];
        let mut response = [0u8; 6]; // 4 bytes counter + 2 bytes SW
        if let Some(len) = self
            .send_apdu(CLA_BRIOLETTE_SE, ins::INCREMENT_COUNTER, 0x00, 0x00, &data, &mut response)
            .await
        {
            if len >= 6 {
                let status = u16::from_be_bytes([response[4], response[5]]);
                if status == sw::SUCCESS {
                    self.pin_attempts = u32::from_be_bytes([
                        response[0],
                        response[1],
                        response[2],
                        response[3],
                    ]);
                }
            }
        }
        self.pin_attempts
    }

    /// Read a monotonic counter value from the SIM card.
    async fn read_counter(&mut self, slot: u8) -> u32 {
        let data = [slot];
        let mut response = [0u8; 6]; // 4 bytes counter + 2 bytes SW
        if let Some(len) = self
            .send_apdu(CLA_BRIOLETTE_SE, ins::READ_COUNTER, 0x00, 0x00, &data, &mut response)
            .await
        {
            if len >= 6 {
                let status = u16::from_be_bytes([response[4], response[5]]);
                if status == sw::SUCCESS {
                    return u32::from_be_bytes([
                        response[0],
                        response[1],
                        response[2],
                        response[3],
                    ]);
                }
            }
        }
        0
    }

    /// Get the cached PIN attempt count.
    pub fn pin_attempt_count(&self) -> u32 {
        self.pin_attempts
    }

    /// Select the Briolette SE applet on the SIM card.
    async fn select_applet(&mut self) -> bool {
        // SELECT by AID: CLA=00 INS=A4 P1=04 P2=00 Lc=len Data=AID Le=00
        let mut response = [0u8; 2];
        if let Some(len) = self
            .send_apdu(CLA_ISO, ins::SELECT, 0x04, 0x00, APPLET_AID, &mut response)
            .await
        {
            if len >= 2 {
                let status = u16::from_be_bytes([response[0], response[1]]);
                if status == sw::SUCCESS {
                    defmt::info!("SIM: Briolette SE applet selected");
                    return true;
                }
                defmt::warn!("SIM: SELECT failed, SW={=u16:04X}", status);
            }
        }
        false
    }

    /// Send an ISO 7816-4 APDU to the SIM card and receive the response.
    ///
    /// Uses T=0 protocol: send command header, handle procedure bytes,
    /// then transfer data.
    ///
    /// Returns the number of bytes written to `response`, or None on error.
    async fn send_apdu(
        &mut self,
        cla: u8,
        ins: u8,
        p1: u8,
        p2: u8,
        data: &[u8],
        response: &mut [u8],
    ) -> Option<usize> {
        // Build command header: CLA INS P1 P2 [Lc data...] [Le]
        let mut cmd = [0u8; 261]; // Max short APDU: 5 + 255 + 1
        let lc = data.len();
        let mut cmd_len = 5;

        cmd[0] = cla;
        cmd[1] = ins;
        cmd[2] = p1;
        cmd[3] = p2;

        if !data.is_empty() {
            cmd[4] = lc as u8;
            cmd[5..5 + lc].copy_from_slice(data);
            cmd_len = 5 + lc;
        } else {
            // Case 1 or Case 2: no command data
            cmd[4] = response.len() as u8; // Le
        }

        // Send command bytes via UART (ISO 7816 T=0).
        if self.uart.write(&cmd[..cmd_len]).await.is_err() {
            defmt::warn!("SIM: UART write failed");
            return None;
        }

        // T=0 protocol: wait for procedure byte from card.
        let mut proc_byte = [0u8; 1];
        // Wait for procedure byte with timeout.
        match embassy_time::with_timeout(
            Duration::from_millis(1000),
            self.uart.read(&mut proc_byte),
        )
        .await
        {
            Ok(Ok(())) => {}
            _ => {
                defmt::warn!("SIM: procedure byte timeout");
                return None;
            }
        }

        // Handle procedure byte:
        // 0x60: NULL — wait and read next procedure byte
        // INS: ACK — transfer remaining data
        // ~INS: transfer one byte, then wait for next procedure byte
        // 0x6X/0x9X: SW1 — read SW2 and return status
        let pb = proc_byte[0];

        if pb == 0x60 {
            // NULL byte — card needs more time, re-read procedure byte.
            // Simplified: just read the response after a delay.
            Timer::after(Duration::from_millis(50)).await;
            return self.read_response(response).await;
        }

        if pb == ins || pb == !ins {
            // ACK — card ready for data transfer.
            // If we have command data remaining, send it.
            // Then read the response.
            return self.read_response(response).await;
        }

        if (pb & 0xF0) == 0x60 || (pb & 0xF0) == 0x90 {
            // SW1 received — read SW2.
            let mut sw2 = [0u8; 1];
            if self.uart.read(&mut sw2).await.is_err() {
                return None;
            }
            // Store status word in response.
            if response.len() >= 2 {
                response[0] = pb;
                response[1] = sw2[0];
                return Some(2);
            }
            return None;
        }

        // Unexpected procedure byte.
        defmt::warn!("SIM: unexpected procedure byte 0x{=u8:02X}", pb);
        None
    }

    /// Read the response data and status word from the SIM card.
    async fn read_response(&mut self, response: &mut [u8]) -> Option<usize> {
        match embassy_time::with_timeout(
            Duration::from_millis(2000),
            self.uart.read(response),
        )
        .await
        {
            Ok(Ok(())) => Some(response.len()),
            Ok(Err(e)) => {
                defmt::warn!("SIM: read error");
                None
            }
            Err(_) => {
                defmt::warn!("SIM: read timeout");
                None
            }
        }
    }
}

/// Parse ATR length from the TS and T0 bytes.
///
/// ISO 7816-3 §8.1: ATR format is TS T0 [TA1..TD1] [TA2..TD2] ... [TCK]
/// T0 encodes the number of historical bytes (lower nibble) and which
/// interface bytes follow (upper nibble).
fn parse_atr_length(atr: &[u8]) -> usize {
    if atr.is_empty() {
        return 0;
    }

    let t0 = if atr.len() > 1 { atr[1] } else { return 2 };
    let historical_len = (t0 & 0x0F) as usize;

    // Count interface bytes from T0's upper nibble.
    let mut idx = 2;
    let mut td = t0;

    loop {
        // Each set bit in upper nibble of TD means one interface byte follows.
        if td & 0x10 != 0 { idx += 1; } // TA
        if td & 0x20 != 0 { idx += 1; } // TB
        if td & 0x40 != 0 { idx += 1; } // TC
        if td & 0x80 != 0 {
            // TD present — read it for next round.
            if idx < atr.len() {
                td = atr[idx];
                idx += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Add historical bytes + optional check byte.
    let total = idx + historical_len + 1; // +1 for TCK
    core::cmp::min(total, atr.len())
}
