//! Relay-side APDU transaction engine.
//!
//! The relay orchestrates credstick-to-credstick payments by shuttling APDUs
//! between a sender and receiver credstick. It is a **reader** — it builds
//! and sends APDUs, the inverse of the credstick's `apdu.rs` which receives them.
//!
//! Transaction flow (2-tap for sender):
//!   1. Tap receiver → READ_TICKET (0x11) → get receiver's SignedTicket
//!   2. Tap sender   → INITIATE (0x10) + TRANSACT (0x20) → get unsigned tokens
//!      [sender lifts, reads e-ink, enters PIN if needed]
//!   3. Tap sender   → TRANSFER (0x30) → get signed tokens
//!   4. Tap receiver → RECEIVE (0x31) → deliver signed tokens
//!
//! Operating modes affect which steps are needed:
//!   - **Variable**: all 4 steps every time
//!   - **MerchantPos**: step 1 done once at config; steps 2-4 per transaction
//!   - **EventMode**: step 1 done once at config + amount is fixed;
//!     operator just presses OK, then steps 2-4 run automatically
//!
//! Power note: Transaction sizes depend on token history length.
//! Long-disconnected credsticks carry heavier token histories,
//! producing larger APDUs and requiring more RF time per transaction.

use heapless::Vec;

/// APDU class byte for Briolette commands.
const CLA_BRIOLETTE: u8 = 0x80;

/// APDU instruction codes — same as credstick side (we're the sender now).
pub mod ins {
    pub const INITIATE: u8 = 0x10;
    pub const READ_TICKET: u8 = 0x11;
    pub const GOSSIP: u8 = 0x12;
    pub const TRANSACT: u8 = 0x20;
    pub const TRANSFER: u8 = 0x30;
    pub const RECEIVE: u8 = 0x31;
    pub const GET_BALANCE: u8 = 0x51;
}

/// ISO 7816-4 status word bytes.
const SW_SUCCESS: [u8; 2] = [0x90, 0x00];

/// Relay transaction phase.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Phase {
    /// No active transaction. Waiting for operator to start.
    Idle,
    /// Receiver ticket acquired. Ready for sender tap.
    ReceiverReady,
    /// INITIATE sent to sender. Waiting for sender to confirm (lift + re-tap).
    SenderProposed,
    /// TRANSFER completed. Have signed tokens, need to deliver to receiver.
    TokensSigned,
    /// RECEIVE sent to receiver. Transaction complete.
    Complete,
    /// Transaction failed at some step.
    Failed,
}

/// Result of a completed transaction.
#[derive(Clone, defmt::Format)]
pub struct TransactionResult {
    pub amount: u32,
    pub success: bool,
}

/// Relay transaction state machine.
///
/// Holds intermediate state between taps. Persists in RAM across
/// the multi-tap flow (supercap-backed, no flash writes needed).
pub struct TransactionEngine {
    phase: Phase,
    /// Amount for this transaction (entered via keypad or fixed config).
    amount: u32,
    /// Receiver's SignedTicket (from READ_TICKET or cached config).
    receiver_ticket: Vec<u8, 512>,
    /// Transaction ID returned by sender's INITIATE.
    tx_id: Vec<u8, 16>,
    /// Unsigned tokens from sender's TRANSACT response.
    unsigned_tokens: Vec<u8, 2048>,
    /// Signed tokens from sender's TRANSFER response.
    signed_tokens: Vec<u8, 2048>,
    /// Epoch data for GOSSIP exchange.
    epoch_data: Vec<u8, 256>,
    /// Number of completed transactions this session.
    tx_count: u32,
    /// Last transaction result (for LED feedback).
    last_result: Option<TransactionResult>,
}

impl TransactionEngine {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            amount: 0,
            receiver_ticket: Vec::new(),
            tx_id: Vec::new(),
            unsigned_tokens: Vec::new(),
            signed_tokens: Vec::new(),
            epoch_data: Vec::new(),
            tx_count: 0,
            last_result: None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn amount(&self) -> u32 {
        self.amount
    }

    pub fn tx_count(&self) -> u32 {
        self.tx_count
    }

    pub fn last_result(&self) -> Option<&TransactionResult> {
        self.last_result.as_ref()
    }

    /// Set the transaction amount (from keypad entry or fixed config).
    pub fn set_amount(&mut self, amount: u32) {
        self.amount = amount;
    }

    /// Pre-load a receiver ticket (for MerchantPos / EventMode).
    /// This allows skipping the receiver-tap step.
    pub fn set_cached_receiver_ticket(&mut self, ticket: &[u8]) {
        self.receiver_ticket.clear();
        self.receiver_ticket.extend_from_slice(ticket).ok();
        if !ticket.is_empty() {
            self.phase = Phase::ReceiverReady;
            defmt::info!("Receiver ticket cached ({} bytes)", ticket.len());
        }
    }

    /// Check if we have a cached receiver ticket.
    pub fn has_cached_receiver(&self) -> bool {
        !self.receiver_ticket.is_empty()
    }

    /// Start a new transaction. Resets per-transaction state but preserves
    /// cached receiver ticket if present.
    pub fn start(&mut self, amount: u32) {
        self.amount = amount;
        self.tx_id.clear();
        self.unsigned_tokens.clear();
        self.signed_tokens.clear();

        if self.has_cached_receiver() {
            self.phase = Phase::ReceiverReady;
        } else {
            self.phase = Phase::Idle;
        }

        self.last_result = None;
    }

    /// Reset the transaction state completely, clearing cached receiver.
    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
        self.amount = 0;
        self.receiver_ticket.clear();
        self.tx_id.clear();
        self.unsigned_tokens.clear();
        self.signed_tokens.clear();
        self.last_result = None;
    }

    // --- APDU builders (relay builds APDUs to send TO credsticks) ---

    /// Build READ_TICKET APDU to send to receiver credstick.
    pub fn build_read_ticket(&self) -> Vec<u8, 16> {
        let mut apdu: Vec<u8, 16> = Vec::new();
        apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::READ_TICKET, 0x00, 0x00, 0x00])
            .ok();
        apdu
    }

    /// Build INITIATE APDU to send to sender credstick.
    /// Includes the receiver's ticket and the requested amount.
    pub fn build_initiate(&self) -> Vec<u8, 256> {
        let mut apdu: Vec<u8, 256> = Vec::new();
        apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::INITIATE, 0x00, 0x00])
            .ok();

        // Payload: [4B amount][4B micros=0][receiver_ticket]
        let payload_len = 8 + self.receiver_ticket.len();
        apdu.push(payload_len as u8).ok();

        apdu.extend_from_slice(&self.amount.to_be_bytes()).ok();
        apdu.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]).ok();
        apdu.extend_from_slice(&self.receiver_ticket).ok();

        apdu
    }

    /// Build TRANSACT APDU (for 3-tap private mode).
    pub fn build_transact(&self) -> Vec<u8, 32> {
        let mut apdu: Vec<u8, 32> = Vec::new();
        apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::TRANSACT, 0x00, 0x00])
            .ok();

        if !self.tx_id.is_empty() {
            apdu.push(self.tx_id.len() as u8).ok();
            apdu.extend_from_slice(&self.tx_id).ok();
        }

        apdu
    }

    /// Build TRANSFER APDU to send to sender (accept=true).
    pub fn build_transfer(&self) -> Vec<u8, 16> {
        let mut apdu: Vec<u8, 16> = Vec::new();
        apdu.extend_from_slice(&[
            CLA_BRIOLETTE,
            ins::TRANSFER,
            0x00,
            0x00,
            0x01, // Lc = 1
            0x01, // accept = true
        ])
        .ok();
        apdu
    }

    /// Build RECEIVE APDU to send to receiver with signed tokens.
    pub fn build_receive(&self) -> Vec<u8, 2048> {
        let mut apdu: Vec<u8, 2048> = Vec::new();
        apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::RECEIVE, 0x00, 0x00])
            .ok();

        // Extended length encoding for large payloads.
        if self.signed_tokens.len() > 255 {
            apdu.push(0x00).ok();
            let len = self.signed_tokens.len() as u16;
            apdu.push((len >> 8) as u8).ok();
            apdu.push((len & 0xFF) as u8).ok();
        } else {
            apdu.push(self.signed_tokens.len() as u8).ok();
        }

        apdu.extend_from_slice(&self.signed_tokens).ok();
        apdu
    }

    /// Build GOSSIP APDU for epoch exchange.
    pub fn build_gossip(&self) -> Vec<u8, 256> {
        let mut apdu: Vec<u8, 256> = Vec::new();
        apdu.extend_from_slice(&[CLA_BRIOLETTE, ins::GOSSIP, 0x00, 0x00])
            .ok();
        if !self.epoch_data.is_empty() {
            apdu.push(self.epoch_data.len() as u8).ok();
            apdu.extend_from_slice(&self.epoch_data).ok();
        }
        apdu
    }

    // --- Response handlers ---

    /// Process READ_TICKET response from receiver.
    pub fn handle_read_ticket_response(&mut self, response: &[u8]) -> Result<(), ()> {
        if !check_sw(response) {
            defmt::warn!("READ_TICKET failed");
            self.phase = Phase::Failed;
            return Err(());
        }

        let ticket_data = &response[..response.len() - 2];
        self.receiver_ticket.clear();
        self.receiver_ticket
            .extend_from_slice(ticket_data)
            .map_err(|_| ())?;

        self.phase = Phase::ReceiverReady;
        defmt::info!("Receiver ticket acquired ({} bytes)", ticket_data.len());
        Ok(())
    }

    /// Process INITIATE response from sender.
    /// In 2-tap fast mode, this also contains unsigned tokens.
    pub fn handle_initiate_response(&mut self, response: &[u8]) -> Result<(), ()> {
        if !check_sw(response) {
            defmt::warn!(
                "INITIATE failed: SW={=[u8]:02X}",
                &response[response.len().saturating_sub(2)..]
            );
            self.phase = Phase::Failed;
            return Err(());
        }

        let payload = &response[..response.len() - 2];

        if payload.len() < 16 {
            defmt::warn!("INITIATE response too short");
            self.phase = Phase::Failed;
            return Err(());
        }

        self.tx_id.clear();
        self.tx_id.extend_from_slice(&payload[..16]).map_err(|_| ())?;

        if payload.len() > 16 {
            self.unsigned_tokens.clear();
            self.unsigned_tokens
                .extend_from_slice(&payload[16..])
                .map_err(|_| ())?;
            defmt::info!(
                "INITIATE: got tx_id + {} bytes unsigned tokens",
                payload.len() - 16
            );
        } else {
            defmt::info!("INITIATE: got tx_id (3-tap mode)");
        }

        self.phase = Phase::SenderProposed;
        Ok(())
    }

    /// Process TRANSFER response from sender (signed tokens).
    pub fn handle_transfer_response(&mut self, response: &[u8]) -> Result<(), ()> {
        if !check_sw(response) {
            // Check for PIN_REQUIRED (0x63Cx).
            if response.len() >= 2 && response[response.len() - 2] == 0x63 {
                let retries = response[response.len() - 1] & 0x0F;
                defmt::info!("TRANSFER: PIN required ({} retries left)", retries);
                // Stay in SenderProposed — user needs to enter PIN and re-tap.
                return Err(());
            }

            defmt::warn!("TRANSFER failed");
            self.phase = Phase::Failed;
            return Err(());
        }

        let payload = &response[..response.len() - 2];
        self.signed_tokens.clear();
        self.signed_tokens
            .extend_from_slice(payload)
            .map_err(|_| ())?;

        self.phase = Phase::TokensSigned;
        defmt::info!("TRANSFER: got {} bytes signed tokens", payload.len());
        Ok(())
    }

    /// Process RECEIVE response from receiver (delivery confirmation).
    pub fn handle_receive_response(&mut self, response: &[u8]) -> Result<(), ()> {
        if !check_sw(response) {
            defmt::warn!("RECEIVE failed");
            self.phase = Phase::Failed;
            self.last_result = Some(TransactionResult {
                amount: self.amount,
                success: false,
            });
            return Err(());
        }

        let payload = &response[..response.len() - 2];
        let accepted = !payload.is_empty() && payload[0] == 0x01;

        if accepted {
            self.tx_count += 1;
            self.phase = Phase::Complete;
            self.last_result = Some(TransactionResult {
                amount: self.amount,
                success: true,
            });
            defmt::info!(
                "Transaction #{} complete: {} tokens",
                self.tx_count,
                self.amount
            );
        } else {
            self.phase = Phase::Failed;
            self.last_result = Some(TransactionResult {
                amount: self.amount,
                success: false,
            });
            defmt::warn!("RECEIVE: receiver rejected tokens");
        }

        Ok(())
    }

    /// Update epoch data (acquired from GOSSIP exchange).
    pub fn update_epoch(&mut self, data: &[u8]) {
        self.epoch_data.clear();
        self.epoch_data.extend_from_slice(data).ok();
    }
}

/// Check if an APDU response ends with SW 90 00 (success).
pub fn check_sw(response: &[u8]) -> bool {
    response.len() >= 2
        && response[response.len() - 2] == SW_SUCCESS[0]
        && response[response.len() - 1] == SW_SUCCESS[1]
}
