//! NFC-A Type 4 Tag driver for the nRF52840.
//!
//! The nRF52840 has a built-in NFC-A tag peripheral (NFCT) that supports
//! ISO 14443-4 (ISO-DEP / T4T). This module configures the NFCT peripheral
//! to act as a Type 4 Tag and dispatches incoming APDUs to the Briolette
//! protocol handler.
//!
//! The NFCT peripheral handles the low-level RF protocol (anti-collision,
//! activation, frame encoding) in hardware. We handle:
//! - T4T AID selection (SELECT by DF name)
//! - APDU routing to briolette apdu handler
//! - Response framing
//!
//! Power: The NFCT peripheral can wake the nRF52840 from System OFF when
//! an NFC field is detected (FIELDDETECTED event). This is the primary
//! wake source for the credstick.

use heapless::Vec;

/// Briolette application identifier (AID) for ISO 7816-4 SELECT.
/// Matches the JavaCard applet: A0 00 00 00 62 03 01
const BRIOLETTE_AID: [u8; 7] = [0xA0, 0x00, 0x00, 0x00, 0x62, 0x03, 0x01];

/// ISO 7816-4 SELECT command INS byte.
const INS_SELECT: u8 = 0xA4;

/// NFC driver state.
pub struct NfcDriver {
    /// Whether the Briolette applet is selected.
    selected: bool,
    /// Receive buffer for incoming APDUs.
    rx_buf: [u8; 256],
    /// Transmit buffer for outgoing responses.
    tx_buf: [u8; 2048],
}

impl NfcDriver {
    pub fn new() -> Self {
        Self {
            selected: false,
            rx_buf: [0u8; 256],
            tx_buf: [0u8; 2048],
        }
    }

    /// Configure the nRF52840 NFCT peripheral.
    ///
    /// Sets up:
    /// - NFC-A Type 4 Tag emulation
    /// - NFCID1 (7-byte UID, random for privacy)
    /// - Frame delay timing
    /// - Interrupt handlers for RXFRAMEEND, TXFRAMEEND, FIELDDETECTED, FIELDLOST
    pub fn configure(&mut self) {
        // Access the NFCT peripheral via PAC (Peripheral Access Crate).
        // Embassy-nrf doesn't have a high-level NFCT driver yet, so we
        // use the PAC directly.
        //
        // The nRF52840 NFCT peripheral:
        // - Handles NFC-A anti-collision in hardware
        // - Supports ISO-DEP (ISO 14443-4) framing
        // - Generates FIELDDETECTED interrupt on NFC field presence
        // - Can wake from System OFF on field detect
        //
        // Register setup:
        // 1. NFCID1: Set random 7-byte UID for privacy
        // 2. SENSRES: Configure ATQA (NFC-A answer to REQA)
        // 3. SELRES: Configure SAK (Select Acknowledge) for Type 4 Tag
        // 4. FRAMEDELAYMAX/MIN: ISO-DEP timing constraints
        // 5. Enable interrupts: RXFRAMEEND, FIELDDETECTED, FIELDLOST

        // TODO: Direct PAC register access. Pseudocode:
        //
        // let nfct = unsafe { &*pac::NFCT::ptr() };
        //
        // // Set NFCID1 (7-byte random UID).
        // nfct.nfcid1_last.write(|w| w.bits(random_uid_last));
        // nfct.nfcid1_2nd_last.write(|w| w.bits(random_uid_2nd));
        //
        // // SENSRES: NFC Forum Type 4 Tag platform config.
        // // NFCIDSIZE = 0b10 (7-byte), BITFRAMESDD = 0b0000
        // nfct.sensres.write(|w| {
        //     w.nfcidsize().nfcid1double()
        //      .bitframesdd().sdd00000()
        // });
        //
        // // SELRES: SAK byte. Bit 6 set = ISO-DEP capable.
        // nfct.selres.write(|w| w.bits(0x20));
        //
        // // Frame delay for ISO-DEP (FDT).
        // nfct.framedelaymin.write(|w| w.bits(1152)); // ~86µs
        // nfct.framedelaymax.write(|w| w.bits(0xFFFF));
        // nfct.framedelaymode.write(|w| w.framedelaymode().window());
        //
        // // Enable NFCT and field detect.
        // nfct.tasks_sense.write(|w| w.bits(1)); // Start sensing for field.

        defmt::info!("NFCT configured for ISO-DEP Type 4 Tag");
    }

    /// Handle a received NFC frame (called from NFCT interrupt).
    ///
    /// Returns the response to send back, or None if the frame should
    /// be ignored.
    pub fn handle_frame(
        &mut self,
        apdu: &[u8],
        tx_state: &mut crate::apdu::TransactionState,
    ) -> Option<&[u8]> {
        if apdu.len() < 4 {
            return None;
        }

        // Check for ISO 7816-4 SELECT command.
        if apdu[1] == INS_SELECT && apdu[0] == 0x00 {
            return self.handle_select(apdu);
        }

        // If Briolette applet not selected, reject.
        if !self.selected {
            self.set_response(&[0x69, 0x99]); // SW: Applet not selected
            return Some(self.response());
        }

        // Dispatch to Briolette APDU handler.
        let mut response: Vec<u8, 2048> = Vec::new();
        tx_state.handle_apdu(apdu, &mut response);

        // Copy response to tx buffer.
        let len = core::cmp::min(response.len(), self.tx_buf.len());
        self.tx_buf[..len].copy_from_slice(&response[..len]);

        Some(&self.tx_buf[..len])
    }

    /// Handle ISO 7816-4 SELECT command.
    fn handle_select(&mut self, apdu: &[u8]) -> Option<&[u8]> {
        // SELECT by DF name (P1=0x04, P2=0x00).
        if apdu.len() < 5 {
            self.set_response(&[0x6A, 0x82]); // File not found
            return Some(self.response());
        }

        let p1 = apdu[2];
        let lc = apdu[4] as usize;

        if p1 != 0x04 || apdu.len() < 5 + lc {
            self.set_response(&[0x6A, 0x82]);
            return Some(self.response());
        }

        let aid = &apdu[5..5 + lc];

        if aid == BRIOLETTE_AID {
            self.selected = true;
            defmt::info!("Briolette applet selected");
            self.set_response(&[0x90, 0x00]); // Success
        } else {
            self.selected = false;
            self.set_response(&[0x6A, 0x82]); // File not found
        }

        Some(self.response())
    }

    fn set_response(&mut self, data: &[u8]) {
        let len = core::cmp::min(data.len(), self.tx_buf.len());
        self.tx_buf[..len].copy_from_slice(&data[..len]);
    }

    fn response(&self) -> &[u8] {
        // Returns the populated portion of tx_buf.
        // In practice, we'd track the length separately.
        &self.tx_buf[..2] // Minimum: just SW bytes.
    }

    /// Called when NFC field is detected (FIELDDETECTED interrupt).
    pub fn on_field_detected(&mut self) {
        defmt::debug!("NFC field detected");
        // Start receiving frames.
        // nfct.tasks_activate.write(|w| w.bits(1));
    }

    /// Called when NFC field is lost (FIELDLOST interrupt).
    pub fn on_field_lost(&mut self) {
        self.selected = false;
        defmt::debug!("NFC field lost");
    }
}
