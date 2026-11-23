//! Briolette APDU protocol constants and builders (host-testable).
//!
//! This module contains the pure logic for building and parsing APDUs
//! that the relay sends to credsticks. It's in the library crate so
//! it can be tested on a host without Embassy or defmt.

use heapless::Vec;

/// APDU class byte for Briolette commands.
pub const CLA_BRIOLETTE: u8 = 0x80;

/// Briolette application AID for ISO 7816-4 SELECT.
pub const BRIOLETTE_AID: [u8; 7] = [0xA0, 0x00, 0x00, 0x00, 0x62, 0x03, 0x01];

/// APDU instruction codes — mirrors receiver.proto RPCs.
pub mod ins {
    pub const INITIATE: u8 = 0x10;
    pub const READ_TICKET: u8 = 0x11;
    pub const GOSSIP: u8 = 0x12;
    pub const TRANSACT: u8 = 0x20;
    pub const TRANSFER: u8 = 0x30;
    pub const RECEIVE: u8 = 0x31;
    pub const SWEEP: u8 = 0x50;
    pub const GET_BALANCE: u8 = 0x51;
}

/// ISO 7816-4 status words.
pub mod sw {
    pub const SUCCESS: [u8; 2] = [0x90, 0x00];
    pub const WRONG_LENGTH: [u8; 2] = [0x67, 0x00];
    pub const CONDITIONS_NOT_SATISFIED: [u8; 2] = [0x69, 0x85];
    pub const WRONG_DATA: [u8; 2] = [0x6A, 0x80];
    pub const INS_NOT_SUPPORTED: [u8; 2] = [0x6D, 0x00];
    pub const CLA_NOT_SUPPORTED: [u8; 2] = [0x6E, 0x00];
}

/// Check if an APDU response ends with SW 90 00 (success).
pub fn check_sw(response: &[u8]) -> bool {
    response.len() >= 2
        && response[response.len() - 2] == sw::SUCCESS[0]
        && response[response.len() - 1] == sw::SUCCESS[1]
}

/// Check if a response indicates PIN_REQUIRED (0x63Cx).
pub fn is_pin_required(response: &[u8]) -> Option<u8> {
    if response.len() >= 2 && response[response.len() - 2] == 0x63 {
        Some(response[response.len() - 1] & 0x0F)
    } else {
        None
    }
}

/// Extract SW1SW2 from the end of a response.
pub fn extract_sw(response: &[u8]) -> Option<[u8; 2]> {
    if response.len() >= 2 {
        Some([response[response.len() - 2], response[response.len() - 1]])
    } else {
        None
    }
}

/// Build a SELECT APDU for the Briolette applet.
pub fn build_select_apdu() -> Vec<u8, 16> {
    let mut apdu: Vec<u8, 16> = Vec::new();
    apdu.extend_from_slice(&[0x00, 0xA4, 0x04, 0x00]).ok();
    apdu.push(BRIOLETTE_AID.len() as u8).ok();
    apdu.extend_from_slice(&BRIOLETTE_AID).ok();
    apdu
}

/// Build a READ_TICKET APDU.
pub fn build_read_ticket() -> Vec<u8, 16> {
    let mut apdu: Vec<u8, 16> = Vec::new();
    apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::READ_TICKET, 0x00, 0x00, 0x00])
        .ok();
    apdu
}

/// Build an INITIATE APDU with amount and receiver ticket.
pub fn build_initiate(amount: u32, receiver_ticket: &[u8]) -> Vec<u8, 256> {
    let mut apdu: Vec<u8, 256> = Vec::new();
    apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::INITIATE, 0x00, 0x00])
        .ok();

    let payload_len = 8 + receiver_ticket.len();
    apdu.push(payload_len as u8).ok();
    apdu.extend_from_slice(&amount.to_be_bytes()).ok();
    apdu.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]).ok(); // micros
    apdu.extend_from_slice(receiver_ticket).ok();

    apdu
}

/// Build a TRANSACT APDU with optional tx_id.
pub fn build_transact(tx_id: &[u8]) -> Vec<u8, 32> {
    let mut apdu: Vec<u8, 32> = Vec::new();
    apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::TRANSACT, 0x00, 0x00])
        .ok();
    if !tx_id.is_empty() {
        apdu.push(tx_id.len() as u8).ok();
        apdu.extend_from_slice(tx_id).ok();
    }
    apdu
}

/// Build a TRANSFER APDU (accept=true).
pub fn build_transfer_accept() -> Vec<u8, 16> {
    let mut apdu: Vec<u8, 16> = Vec::new();
    apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::TRANSFER, 0x00, 0x00, 0x01, 0x01])
        .ok();
    apdu
}

/// Build a RECEIVE APDU with signed token data.
pub fn build_receive(signed_tokens: &[u8]) -> Vec<u8, 2048> {
    let mut apdu: Vec<u8, 2048> = Vec::new();
    apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::RECEIVE, 0x00, 0x00])
        .ok();

    if signed_tokens.len() > 255 {
        apdu.push(0x00).ok();
        let len = signed_tokens.len() as u16;
        apdu.push((len >> 8) as u8).ok();
        apdu.push((len & 0xFF) as u8).ok();
    } else {
        apdu.push(signed_tokens.len() as u8).ok();
    }

    apdu.extend_from_slice(signed_tokens).ok();
    apdu
}

/// Build a GOSSIP APDU with epoch data.
pub fn build_gossip(epoch_data: &[u8]) -> Vec<u8, 256> {
    let mut apdu: Vec<u8, 256> = Vec::new();
    apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::GOSSIP, 0x00, 0x00])
        .ok();
    if !epoch_data.is_empty() {
        apdu.push(epoch_data.len() as u8).ok();
        apdu.extend_from_slice(epoch_data).ok();
    }
    apdu
}

/// Parse an INITIATE response: tx_id (16 bytes) + optional unsigned tokens.
pub fn parse_initiate_response(response: &[u8]) -> Result<(&[u8], &[u8]), ()> {
    if !check_sw(response) {
        return Err(());
    }
    let payload = &response[..response.len() - 2];
    if payload.len() < 16 {
        return Err(());
    }
    let tx_id = &payload[..16];
    let tokens = &payload[16..];
    Ok((tx_id, tokens))
}

/// Parse a TRANSFER response: signed token data.
pub fn parse_transfer_response(response: &[u8]) -> Result<&[u8], ()> {
    if !check_sw(response) {
        return Err(());
    }
    Ok(&response[..response.len() - 2])
}

/// Parse a RECEIVE response: accepted flag.
pub fn parse_receive_response(response: &[u8]) -> Result<bool, ()> {
    if !check_sw(response) {
        return Err(());
    }
    let payload = &response[..response.len() - 2];
    Ok(!payload.is_empty() && payload[0] == 0x01)
}

/// NCI Message Type constants (for PN7150 driver).
pub mod nci {
    pub const MT_DATA: u8 = 0x00;
    pub const MT_COMMAND: u8 = 0x20;
    pub const MT_RESPONSE: u8 = 0x40;
    pub const MT_NOTIFICATION: u8 = 0x60;

    pub const GID_CORE: u8 = 0x00;
    pub const GID_RF: u8 = 0x01;

    pub const OID_CORE_RESET: u8 = 0x00;
    pub const OID_CORE_INIT: u8 = 0x01;
    pub const OID_RF_DISCOVER_MAP: u8 = 0x00;
    pub const OID_RF_DISCOVER: u8 = 0x03;
    pub const OID_RF_DEACTIVATE: u8 = 0x06;
    pub const OID_RF_INTF_ACTIVATED: u8 = 0x05;

    pub const STATUS_OK: u8 = 0x00;

    /// Build NCI CORE_RESET command.
    pub fn build_core_reset() -> [u8; 4] {
        [MT_COMMAND | GID_CORE, OID_CORE_RESET, 0x01, 0x01]
    }

    /// Build NCI CORE_INIT command.
    pub fn build_core_init() -> [u8; 3] {
        [MT_COMMAND | GID_CORE, OID_CORE_INIT, 0x00]
    }

    /// Build NCI RF_DISCOVER_MAP for NFC-A ISO-DEP.
    pub fn build_discover_map_nfca_isodep() -> [u8; 7] {
        [
            MT_COMMAND | GID_RF,
            OID_RF_DISCOVER_MAP,
            0x04, // payload length
            0x01, // 1 mapping entry
            0x00, // NFC-A passive poll
            0x01, // poll mode
            0x02, // ISO-DEP interface
        ]
    }

    /// Build NCI RF_DISCOVER for NFC-A poll.
    pub fn build_discover_nfca() -> [u8; 6] {
        [
            MT_COMMAND | GID_RF,
            OID_RF_DISCOVER,
            0x03,
            0x01, // 1 config
            0x00, // NFC-A passive poll
            0x01, // every cycle
        ]
    }

    /// Build NCI RF_DEACTIVATE (idle).
    pub fn build_deactivate_idle() -> [u8; 4] {
        [MT_COMMAND | GID_RF, OID_RF_DEACTIVATE, 0x01, 0x00]
    }

    /// Check if an NCI packet is an RF_INTF_ACTIVATED notification.
    pub fn is_rf_activated(packet: &[u8]) -> bool {
        packet.len() >= 3
            && (packet[0] & 0xE0) == MT_NOTIFICATION
            && (packet[0] & 0x0F) == GID_RF
            && packet[1] == OID_RF_INTF_ACTIVATED
    }

    /// Check NCI response status (byte 3).
    pub fn check_status(resp: &[u8]) -> bool {
        resp.len() > 3 && resp[3] == STATUS_OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SW checking ---

    #[test]
    fn test_check_sw_success() {
        assert!(check_sw(&[0x01, 0x02, 0x90, 0x00]));
        assert!(check_sw(&[0x90, 0x00]));
    }

    #[test]
    fn test_check_sw_failure() {
        assert!(!check_sw(&[0x6A, 0x80]));
        assert!(!check_sw(&[0x01, 0x02, 0x69, 0x85]));
        assert!(!check_sw(&[0x90])); // too short
        assert!(!check_sw(&[]));
    }

    #[test]
    fn test_is_pin_required() {
        assert_eq!(is_pin_required(&[0x63, 0xC3]), Some(3));
        assert_eq!(is_pin_required(&[0x63, 0xCA]), Some(10));
        assert_eq!(is_pin_required(&[0x90, 0x00]), None);
        assert_eq!(is_pin_required(&[0x69, 0x85]), None);
    }

    #[test]
    fn test_extract_sw() {
        assert_eq!(extract_sw(&[0x01, 0x90, 0x00]), Some([0x90, 0x00]));
        assert_eq!(extract_sw(&[0x90, 0x00]), Some([0x90, 0x00]));
        assert_eq!(extract_sw(&[0x00]), None);
        assert_eq!(extract_sw(&[]), None);
    }

    // --- APDU builders ---

    #[test]
    fn test_build_select_apdu() {
        let apdu = build_select_apdu();
        assert_eq!(apdu[0], 0x00); // CLA
        assert_eq!(apdu[1], 0xA4); // INS = SELECT
        assert_eq!(apdu[2], 0x04); // P1 = by name
        assert_eq!(apdu[3], 0x00); // P2
        assert_eq!(apdu[4], 0x07); // Lc = AID length
        assert_eq!(&apdu[5..12], &BRIOLETTE_AID);
        assert_eq!(apdu.len(), 12);
    }

    #[test]
    fn test_build_read_ticket() {
        let apdu = build_read_ticket();
        assert_eq!(apdu[0], CLA_BRIOLETTE);
        assert_eq!(apdu[1], ins::READ_TICKET);
        assert_eq!(apdu[2], 0x00);
        assert_eq!(apdu[3], 0x00);
        assert_eq!(apdu[4], 0x00); // Le = 0 (return all)
        assert_eq!(apdu.len(), 5);
    }

    #[test]
    fn test_build_initiate_with_ticket() {
        let ticket = [0xAA, 0xBB, 0xCC, 0xDD];
        let apdu = build_initiate(100, &ticket);

        assert_eq!(apdu[0], CLA_BRIOLETTE);
        assert_eq!(apdu[1], ins::INITIATE);
        assert_eq!(apdu[2], 0x00);
        assert_eq!(apdu[3], 0x00);
        assert_eq!(apdu[4], 12); // Lc = 8 (amount+micros) + 4 (ticket)

        // Amount big-endian.
        assert_eq!(&apdu[5..9], &100u32.to_be_bytes());
        // Micros = 0.
        assert_eq!(&apdu[9..13], &[0, 0, 0, 0]);
        // Receiver ticket.
        assert_eq!(&apdu[13..17], &ticket);
        assert_eq!(apdu.len(), 17);
    }

    #[test]
    fn test_build_initiate_empty_ticket() {
        let apdu = build_initiate(42, &[]);
        assert_eq!(apdu[4], 8); // Lc = 8 (amount+micros only)
        assert_eq!(apdu.len(), 13);
    }

    #[test]
    fn test_build_transact_with_tx_id() {
        let tx_id = [0x01, 0x02, 0x03, 0x04];
        let apdu = build_transact(&tx_id);
        assert_eq!(apdu[0], CLA_BRIOLETTE);
        assert_eq!(apdu[1], ins::TRANSACT);
        assert_eq!(apdu[4], 4); // Lc
        assert_eq!(&apdu[5..9], &tx_id);
    }

    #[test]
    fn test_build_transact_empty() {
        let apdu = build_transact(&[]);
        assert_eq!(apdu.len(), 4); // CLA INS P1 P2 only
    }

    #[test]
    fn test_build_transfer_accept() {
        let apdu = build_transfer_accept();
        assert_eq!(apdu[0], CLA_BRIOLETTE);
        assert_eq!(apdu[1], ins::TRANSFER);
        assert_eq!(apdu[4], 0x01); // Lc
        assert_eq!(apdu[5], 0x01); // accept = true
        assert_eq!(apdu.len(), 6);
    }

    #[test]
    fn test_build_receive_short() {
        let tokens = [0xDE, 0xAD, 0xBE, 0xEF];
        let apdu = build_receive(&tokens);
        assert_eq!(apdu[0], CLA_BRIOLETTE);
        assert_eq!(apdu[1], ins::RECEIVE);
        assert_eq!(apdu[4], 4); // short Lc
        assert_eq!(&apdu[5..9], &tokens);
    }

    #[test]
    fn test_build_receive_extended_length() {
        // Build a payload > 255 bytes.
        let tokens = [0xAB; 300];
        let apdu = build_receive(&tokens);
        assert_eq!(apdu[4], 0x00); // extended Lc marker
        assert_eq!(apdu[5], 0x01); // 300 >> 8 = 1
        assert_eq!(apdu[6], 0x2C); // 300 & 0xFF = 44
        assert_eq!(apdu.len(), 7 + 300);
    }

    #[test]
    fn test_build_gossip_with_data() {
        let epoch = [0x00, 0x00, 0x00, 0x05]; // epoch 5
        let apdu = build_gossip(&epoch);
        assert_eq!(apdu[0], CLA_BRIOLETTE);
        assert_eq!(apdu[1], ins::GOSSIP);
        assert_eq!(apdu[4], 4);
        assert_eq!(&apdu[5..9], &epoch);
    }

    #[test]
    fn test_build_gossip_empty() {
        let apdu = build_gossip(&[]);
        assert_eq!(apdu.len(), 4);
    }

    // --- Response parsers ---

    #[test]
    fn test_parse_initiate_response_fast_mode() {
        // 16 bytes tx_id + 4 bytes tokens + SW 9000
        let mut resp = [0u8; 22];
        resp[..16].copy_from_slice(&[0x01; 16]); // tx_id
        resp[16..20].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // tokens
        resp[20] = 0x90;
        resp[21] = 0x00;

        let (tx_id, tokens) = parse_initiate_response(&resp).unwrap();
        assert_eq!(tx_id, &[0x01; 16]);
        assert_eq!(tokens, &[0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_parse_initiate_response_private_mode() {
        // 16 bytes tx_id only + SW 9000
        let mut resp = [0u8; 18];
        resp[..16].copy_from_slice(&[0x02; 16]);
        resp[16] = 0x90;
        resp[17] = 0x00;

        let (tx_id, tokens) = parse_initiate_response(&resp).unwrap();
        assert_eq!(tx_id, &[0x02; 16]);
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_parse_initiate_response_too_short() {
        // Only 10 bytes payload + SW — too short for tx_id.
        let mut resp = [0u8; 12];
        resp[10] = 0x90;
        resp[11] = 0x00;
        assert!(parse_initiate_response(&resp).is_err());
    }

    #[test]
    fn test_parse_initiate_response_failure_sw() {
        let resp = [0x69, 0x85]; // CONDITIONS_NOT_SATISFIED
        assert!(parse_initiate_response(&resp).is_err());
    }

    #[test]
    fn test_parse_transfer_response() {
        let mut resp = [0u8; 6];
        resp[..4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        resp[4] = 0x90;
        resp[5] = 0x00;

        let sigs = parse_transfer_response(&resp).unwrap();
        assert_eq!(sigs, &[0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn test_parse_transfer_response_failure() {
        assert!(parse_transfer_response(&[0x69, 0x85]).is_err());
    }

    #[test]
    fn test_parse_receive_response_accepted() {
        let resp = [0x01, 0x90, 0x00];
        assert_eq!(parse_receive_response(&resp), Ok(true));
    }

    #[test]
    fn test_parse_receive_response_rejected() {
        let resp = [0x00, 0x90, 0x00];
        assert_eq!(parse_receive_response(&resp), Ok(false));
    }

    #[test]
    fn test_parse_receive_response_empty_payload() {
        // Just SW, no accepted byte.
        let resp = [0x90, 0x00];
        assert_eq!(parse_receive_response(&resp), Ok(false));
    }

    #[test]
    fn test_parse_receive_response_failure() {
        assert!(parse_receive_response(&[0x6A, 0x80]).is_err());
    }

    // --- NCI builders ---

    #[test]
    fn test_nci_core_reset() {
        let cmd = nci::build_core_reset();
        assert_eq!(cmd[0], nci::MT_COMMAND | nci::GID_CORE);
        assert_eq!(cmd[1], nci::OID_CORE_RESET);
        assert_eq!(cmd[2], 0x01); // payload length
        assert_eq!(cmd[3], 0x01); // keep config
    }

    #[test]
    fn test_nci_core_init() {
        let cmd = nci::build_core_init();
        assert_eq!(cmd[0], nci::MT_COMMAND | nci::GID_CORE);
        assert_eq!(cmd[1], nci::OID_CORE_INIT);
        assert_eq!(cmd[2], 0x00); // no payload
    }

    #[test]
    fn test_nci_discover_map() {
        let cmd = nci::build_discover_map_nfca_isodep();
        assert_eq!(cmd[0], nci::MT_COMMAND | nci::GID_RF);
        assert_eq!(cmd[1], nci::OID_RF_DISCOVER_MAP);
        assert_eq!(cmd[2], 0x04); // payload length
        assert_eq!(cmd[3], 0x01); // 1 entry
        assert_eq!(cmd[4], 0x00); // NFC-A passive poll
        assert_eq!(cmd[5], 0x01); // poll mode
        assert_eq!(cmd[6], 0x02); // ISO-DEP
    }

    #[test]
    fn test_nci_discover_nfca() {
        let cmd = nci::build_discover_nfca();
        assert_eq!(cmd[0], nci::MT_COMMAND | nci::GID_RF);
        assert_eq!(cmd[1], nci::OID_RF_DISCOVER);
    }

    #[test]
    fn test_nci_deactivate_idle() {
        let cmd = nci::build_deactivate_idle();
        assert_eq!(cmd[0], nci::MT_COMMAND | nci::GID_RF);
        assert_eq!(cmd[1], nci::OID_RF_DEACTIVATE);
        assert_eq!(cmd[3], 0x00); // idle
    }

    #[test]
    fn test_nci_is_rf_activated() {
        // Valid RF_INTF_ACTIVATED notification.
        let packet = [nci::MT_NOTIFICATION | nci::GID_RF, nci::OID_RF_INTF_ACTIVATED, 0x05, 0x01, 0x02];
        assert!(nci::is_rf_activated(&packet));

        // Not a notification (command).
        let cmd = [nci::MT_COMMAND | nci::GID_RF, nci::OID_RF_INTF_ACTIVATED, 0x00];
        assert!(!nci::is_rf_activated(&cmd));

        // Wrong OID.
        let wrong = [nci::MT_NOTIFICATION | nci::GID_RF, nci::OID_RF_DISCOVER, 0x00];
        assert!(!nci::is_rf_activated(&wrong));

        // Too short.
        assert!(!nci::is_rf_activated(&[0x61]));
        assert!(!nci::is_rf_activated(&[]));
    }

    #[test]
    fn test_nci_check_status() {
        assert!(nci::check_status(&[0x40, 0x00, 0x01, 0x00])); // OK
        assert!(!nci::check_status(&[0x40, 0x00, 0x01, 0x01])); // REJECTED
        assert!(!nci::check_status(&[0x40, 0x00, 0x01])); // too short
    }
}
