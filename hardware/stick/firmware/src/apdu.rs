//! Briolette APDU protocol handler.
//!
//! Implements the credstick side of the Briolette payment protocol,
//! mirroring `receiver.proto` RPCs as APDU commands:
//!
//! - INITIATE (0x10): Receive payment proposal (ticket + items + epoch)
//! - READ_TICKET (0x11): Return this credstick's SignedTicket
//! - GOSSIP (0x12): Exchange epoch updates
//! - TRANSACT (0x20): Propose unsigned tokens for settlement
//! - TRANSFER (0x30): Sign and commit proposed tokens
//! - RECEIVE (0x31): Accept incoming signed tokens
//! - GET_BALANCE (0x51): Return current balance
//! - SWEEP (0x50): Return accumulated tokens to merchant

use heapless::Vec;

use crate::bloom::BloomFilter;
use crate::button::PinSymbol;
use crate::ecdaa;
use crate::storage::Storage;

/// APDU class byte for Briolette commands.
const CLA_BRIOLETTE: u8 = 0x80;

/// APDU instruction codes — mirrors receiver.proto RPCs.
mod ins {
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
mod sw {
    pub const SUCCESS: [u8; 2] = [0x90, 0x00];
    pub const WRONG_LENGTH: [u8; 2] = [0x67, 0x00];
    pub const CONDITIONS_NOT_SATISFIED: [u8; 2] = [0x69, 0x85];
    pub const WRONG_DATA: [u8; 2] = [0x6A, 0x80];
    pub const INCORRECT_P1P2: [u8; 2] = [0x6A, 0x86];
    pub const INS_NOT_SUPPORTED: [u8; 2] = [0x6D, 0x00];
    pub const CLA_NOT_SUPPORTED: [u8; 2] = [0x6E, 0x00];
    pub const COMMAND_TIMEOUT: [u8; 2] = [0x64, 0x01];
    pub const PIN_REQUIRED: [u8; 2] = [0x63, 0xC0]; // 0x63CX = X retries remaining
    pub const LOCKED: [u8; 2] = [0x69, 0x83];
}

/// Transaction protocol phase — tracks state across NFC sessions.
#[derive(Clone)]
pub enum Phase {
    /// No active transaction.
    Idle,
    /// INITIATE received. Credstick has the proposal but hasn't
    /// revealed tokens yet (3-tap private mode), or has revealed
    /// unsigned tokens (2-tap fast mode).
    Proposed {
        amount: u32,
        desc: heapless::String<32>,
    },
    /// TRANSACT completed: unsigned tokens have been sent to the PoS.
    TokensSent,
    /// TRANSFER completed: tokens signed and committed.
    Signed { remaining: u32 },
    /// Transaction was rejected (double-spend detected, etc.).
    Rejected,
    /// Proposal expired (timeout).
    Expired,
}

impl Phase {
    pub fn is_proposed(&self) -> bool {
        matches!(self, Phase::Proposed { .. })
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Phase::Idle)
    }
}

/// Privacy mode for transaction flow.
#[derive(Clone, Copy, PartialEq, defmt::Format)]
pub enum PrivacyMode {
    /// 2-tap: INITIATE+TRANSACT on tap 1, TRANSFER on tap 2.
    Fast,
    /// 3-tap: INITIATE on tap 1, TRANSACT on tap 2, TRANSFER on tap 2/3.
    Private,
}

/// Transaction state machine.
///
/// Persists across NFC sessions in supercap-backed RAM.
pub struct TransactionState {
    phase: Phase,
    privacy_mode: PrivacyMode,
    tx_id: [u8; 16],
    /// The proposed amount for the current transaction.
    proposed_amount: u32,
    /// Tokens selected for the current proposal.
    proposed_tokens: Vec<u8, 2048>,
    /// Whether the proposed tokens have been signed.
    signed: bool,
    /// PIN authorization state.
    pin_authorized: bool,
    pin_buffer: Vec<PinSymbol, 16>,
    pin_attempts_failed: u8,
    /// Amount threshold below which PIN is not required.
    pin_threshold: u32,
    /// Persistent storage reference.
    storage: Storage,
    /// Bloom filter for basename double-spend tracking.
    bloom: BloomFilter,
    /// Tick counter for expiration (set on INITIATE, cleared on completion).
    initiated_at: Option<u64>,
}

impl TransactionState {
    pub fn new(storage: Storage) -> Self {
        Self {
            phase: Phase::Idle,
            privacy_mode: PrivacyMode::Fast,
            tx_id: [0u8; 16],
            proposed_amount: 0,
            proposed_tokens: Vec::new(),
            signed: false,
            pin_authorized: false,
            pin_buffer: Vec::new(),
            pin_attempts_failed: 0,
            pin_threshold: 10, // Default: no PIN below 10 tokens.
            storage,
            bloom: BloomFilter::new(),
            initiated_at: None,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn balance(&self) -> u32 {
        self.storage.balance()
    }

    pub fn last_amount(&self) -> u32 {
        self.proposed_amount
    }

    pub fn pin_required(&self) -> bool {
        self.proposed_amount >= self.pin_threshold
            && self.storage.has_pin()
            && !self.pin_authorized
    }

    pub fn pin_in_progress(&self) -> bool {
        self.pin_required() && !self.pin_buffer.is_empty()
    }

    pub fn pin_entered_count(&self) -> usize {
        self.pin_buffer.len()
    }

    pub fn pin_attempts_remaining(&self) -> u8 {
        10u8.saturating_sub(self.pin_attempts_failed)
    }

    pub fn pin_input(&mut self, symbol: PinSymbol) {
        let _ = self.pin_buffer.push(symbol);
    }

    pub fn verify_pin(&mut self) -> bool {
        if self.pin_attempts_remaining() == 0 {
            return false;
        }
        let ok = self.storage.verify_pin(&self.pin_buffer);
        self.pin_buffer.clear();
        if ok {
            self.pin_authorized = true;
            self.pin_attempts_failed = 0;
            true
        } else {
            self.pin_attempts_failed += 1;
            // Increment ATECC608B monotonic counter for tamper resistance.
            // (Done via atecc608b driver in production.)
            false
        }
    }

    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
        self.proposed_tokens.clear();
        self.signed = false;
        self.pin_authorized = false;
        self.pin_buffer.clear();
        self.initiated_at = None;
    }

    /// Handle an incoming APDU. Returns the response bytes.
    pub fn handle_apdu(&mut self, apdu: &[u8], response: &mut Vec<u8, 2048>) {
        if apdu.len() < 4 {
            response.extend_from_slice(&sw::WRONG_LENGTH).ok();
            return;
        }

        let cla = apdu[0];
        let ins = apdu[1];
        let _p1 = apdu[2];
        let _p2 = apdu[3];

        if cla != CLA_BRIOLETTE {
            response.extend_from_slice(&sw::CLA_NOT_SUPPORTED).ok();
            return;
        }

        // Extract Lc and data if present.
        let (data, _le) = if apdu.len() > 4 {
            let lc = apdu[4] as usize;
            let data_end = 5 + lc;
            if apdu.len() < data_end {
                response.extend_from_slice(&sw::WRONG_LENGTH).ok();
                return;
            }
            let le = if apdu.len() > data_end {
                Some(apdu[data_end])
            } else {
                None
            };
            (&apdu[5..data_end], le)
        } else {
            (&[] as &[u8], None)
        };

        match ins {
            ins::INITIATE => self.handle_initiate(data, response),
            ins::READ_TICKET => self.handle_read_ticket(response),
            ins::GOSSIP => self.handle_gossip(data, response),
            ins::TRANSACT => self.handle_transact(data, response),
            ins::TRANSFER => self.handle_transfer(data, response),
            ins::RECEIVE => self.handle_receive(data, response),
            ins::SWEEP => self.handle_sweep(data, response),
            ins::GET_BALANCE => self.handle_get_balance(response),
            _ => {
                response.extend_from_slice(&sw::INS_NOT_SUPPORTED).ok();
            }
        }
    }

    /// INITIATE (0x10): Receive payment proposal.
    ///
    /// Data in: ticket (protobuf) + items (protobuf) + epoch data
    /// Data out (2-tap fast mode): tx_id + unsigned tokens
    /// Data out (3-tap private mode): tx_id only
    ///
    /// Mirrors `InitiateReply` from receiver.proto.
    fn handle_initiate(&mut self, data: &[u8], response: &mut Vec<u8, 2048>) {
        if !self.phase.is_idle() {
            // Already have a pending proposal. Reject.
            response.extend_from_slice(&sw::CONDITIONS_NOT_SATISFIED).ok();
            return;
        }

        // Parse the proposal.
        // Format: [4B amount_whole][4B amount_micros][NB ticket][NB items]
        if data.len() < 8 {
            response.extend_from_slice(&sw::WRONG_DATA).ok();
            return;
        }

        let amount = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        // Micros in data[4..8] — ignored for now (whole tokens only).

        // Extract description from items (simplified: first N bytes as UTF-8).
        let desc_bytes = &data[8..core::cmp::min(data.len(), 40)];
        let mut desc = heapless::String::new();
        for &b in desc_bytes {
            if b.is_ascii_graphic() || b == b' ' {
                desc.push(b as char).ok();
            }
        }

        // Generate tx_id.
        // In production: SHA-256(salt || timestamp || amount).
        // For now: incrementing counter + amount.
        let tx_id = self.generate_tx_id(amount);
        self.tx_id = tx_id;
        self.proposed_amount = amount;
        self.initiated_at = Some(0); // TODO: use embassy_time::Instant.

        // Response: tx_id.
        response.extend_from_slice(&tx_id).ok();

        if self.privacy_mode == PrivacyMode::Fast {
            // 2-tap mode: also select and return unsigned tokens now.
            self.select_tokens_for_proposal(amount);
            response.extend_from_slice(&self.proposed_tokens).ok();
        }

        self.phase = Phase::Proposed { amount, desc };
        response.extend_from_slice(&sw::SUCCESS).ok();

        defmt::info!("INITIATE: amount={}, mode={}", amount, self.privacy_mode);
    }

    /// READ_TICKET (0x11): Return this credstick's SignedTicket.
    fn handle_read_ticket(&self, response: &mut Vec<u8, 2048>) {
        let ticket = self.storage.signed_ticket();
        response.extend_from_slice(ticket).ok();
        response.extend_from_slice(&sw::SUCCESS).ok();
    }

    /// GOSSIP (0x12): Exchange epoch updates.
    fn handle_gossip(&mut self, data: &[u8], response: &mut Vec<u8, 2048>) {
        // Parse incoming EpochUpdate.
        // If their epoch is newer, update ours.
        // Return our EpochUpdate if ours is newer.
        if data.len() < 4 {
            response.extend_from_slice(&sw::WRONG_DATA).ok();
            return;
        }
        let their_epoch = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let our_epoch = self.storage.epoch_number();

        if their_epoch > our_epoch {
            // Accept their epoch data.
            self.storage.update_epoch(data);
            // Reset bloom filter for new epoch.
            self.bloom.reset_for_epoch(their_epoch);
            defmt::info!("GOSSIP: updated epoch {} -> {}", our_epoch, their_epoch);
        }

        // Return our epoch data.
        let epoch_data = self.storage.epoch_data();
        response.extend_from_slice(epoch_data).ok();
        response.extend_from_slice(&sw::SUCCESS).ok();
    }

    /// TRANSACT (0x20): Propose unsigned tokens for settlement.
    ///
    /// In 2-tap mode: this was already done during INITIATE (no-op here).
    /// In 3-tap mode: user has seen the proposal and consented to reveal tokens.
    ///
    /// Mirrors `TransactRequest` from receiver.proto.
    fn handle_transact(&mut self, data: &[u8], response: &mut Vec<u8, 2048>) {
        match &self.phase {
            Phase::Proposed { amount, .. } => {
                let amount = *amount;
                if self.privacy_mode == PrivacyMode::Private {
                    // 3-tap mode: select tokens now.
                    // Verify tx_id matches.
                    if data.len() >= 16 && data[..16] != self.tx_id {
                        response.extend_from_slice(&sw::WRONG_DATA).ok();
                        return;
                    }
                    self.select_tokens_for_proposal(amount);
                    response.extend_from_slice(&self.proposed_tokens).ok();
                } else {
                    // 2-tap mode: tokens were already sent in INITIATE.
                    // Return them again in case the PoS needs a refresh.
                    response.extend_from_slice(&self.proposed_tokens).ok();
                }

                self.phase = Phase::TokensSent;
                response.extend_from_slice(&sw::SUCCESS).ok();
                defmt::info!("TRANSACT: {} tokens proposed", amount);
            }
            _ => {
                response.extend_from_slice(&sw::CONDITIONS_NOT_SATISFIED).ok();
            }
        }
    }

    /// TRANSFER (0x30): Sign proposed tokens and commit.
    ///
    /// Data in: tx_id + accept (1 byte) + validation_status
    /// Data out: signatures for each proposed token
    ///
    /// The credstick only signs if:
    /// 1. A valid proposal exists (INITIATE was received).
    /// 2. Tokens were proposed (TRANSACT completed, or 2-tap mode).
    /// 3. PIN was entered (if required by policy).
    /// 4. The PoS accepts (accept byte = 0x01).
    ///
    /// Mirrors `TransferRequest` from receiver.proto — the credstick
    /// returns the signed Token History entries.
    fn handle_transfer(&mut self, data: &[u8], response: &mut Vec<u8, 2048>) {
        // Must be in TokensSent phase (or Proposed in 2-tap mode where
        // TRANSACT was implicit during INITIATE).
        let can_sign = matches!(self.phase, Phase::TokensSent)
            || (self.privacy_mode == PrivacyMode::Fast
                && matches!(self.phase, Phase::Proposed { .. }));

        if !can_sign {
            response.extend_from_slice(&sw::CONDITIONS_NOT_SATISFIED).ok();
            return;
        }

        // Check PIN authorization.
        if self.pin_required() {
            // PIN not yet entered. Return PIN_REQUIRED with retry count.
            let retries = self.pin_attempts_remaining();
            response
                .extend_from_slice(&[0x63, 0xC0 | (retries & 0x0F)])
                .ok();
            return;
        }

        // Check if locked out.
        if self.pin_attempts_remaining() == 0 {
            response.extend_from_slice(&sw::LOCKED).ok();
            return;
        }

        // Parse accept/reject.
        if data.is_empty() {
            response.extend_from_slice(&sw::WRONG_DATA).ok();
            return;
        }
        let accepted = data[0] == 0x01;

        if !accepted {
            // PoS rejected (e.g., double-spend detected during validation).
            self.phase = Phase::Rejected;
            self.proposed_tokens.clear();
            self.pin_authorized = false;
            response.extend_from_slice(&sw::SUCCESS).ok();
            defmt::info!("TRANSFER: rejected by PoS");
            return;
        }

        // Sign the proposed tokens using ECDAA split-key.
        let signatures = ecdaa::sign_tokens(
            &self.proposed_tokens,
            self.storage.ecdaa_secret_key(),
            &mut self.bloom,
        );

        match signatures {
            Some(sigs) => {
                response.extend_from_slice(&sigs).ok();

                // Deduct tokens from balance.
                self.storage.deduct(self.proposed_amount);
                let remaining = self.storage.balance();

                self.phase = Phase::Signed { remaining };
                self.proposed_tokens.clear();
                self.signed = true;
                self.pin_authorized = false;
                self.initiated_at = None;

                response.extend_from_slice(&sw::SUCCESS).ok();
                defmt::info!(
                    "TRANSFER: signed {} tokens, {} remaining",
                    self.proposed_amount,
                    remaining
                );
            }
            None => {
                // Signing failed (bloom filter hit = double spend attempt,
                // or crypto error).
                self.phase = Phase::Rejected;
                response.extend_from_slice(&sw::CONDITIONS_NOT_SATISFIED).ok();
                defmt::warn!("TRANSFER: signing failed (bloom/crypto)");
            }
        }
    }

    /// RECEIVE (0x31): Accept incoming signed tokens.
    ///
    /// Used when this credstick is the receiver in a credstick-to-credstick
    /// payment. The PoS/relay sends us signed tokens from the sender.
    fn handle_receive(&mut self, data: &[u8], response: &mut Vec<u8, 2048>) {
        if data.is_empty() {
            response.extend_from_slice(&sw::WRONG_DATA).ok();
            return;
        }

        // Verify token signatures cryptographically (BLS pairing checks).
        // In the minimal firmware, we accept on faith and mark as unvalidated.
        // Full validation requires pairing operations which are expensive on
        // Cortex-M4 — defer to the next online sync.
        let token_count = self.storage.receive_tokens(data);

        // Return acceptance.
        response.push(0x01).ok(); // accepted = true
        response.extend_from_slice(&sw::SUCCESS).ok();
        defmt::info!("RECEIVE: accepted {} token(s)", token_count);
    }

    /// SWEEP (0x50): Return accumulated tokens to merchant credstick.
    fn handle_sweep(&mut self, _data: &[u8], response: &mut Vec<u8, 2048>) {
        let tokens = self.storage.sweep_received_tokens();
        response.extend_from_slice(&tokens).ok();
        response.extend_from_slice(&sw::SUCCESS).ok();
    }

    /// GET_BALANCE (0x51): Return current token balance.
    fn handle_get_balance(&self, response: &mut Vec<u8, 2048>) {
        let balance = self.storage.balance();
        response.extend_from_slice(&balance.to_be_bytes()).ok();
        response.extend_from_slice(&sw::SUCCESS).ok();
    }

    // --- Internal helpers ---

    fn generate_tx_id(&self, amount: u32) -> [u8; 16] {
        // Simplified tx_id: hash of amount + counter.
        // Production: SHA-256(device_id || counter || amount || timestamp).
        let mut id = [0u8; 16];
        id[0..4].copy_from_slice(&amount.to_be_bytes());
        // TODO: incorporate monotonic counter and RNG.
        id
    }

    fn select_tokens_for_proposal(&mut self, amount: u32) {
        // Select tokens from storage to fulfill the requested amount.
        // Returns serialized unsigned Token[] (without signatures).
        self.proposed_tokens.clear();
        self.storage
            .select_tokens(amount, &mut self.proposed_tokens);
    }
}
