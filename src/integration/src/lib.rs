// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! High-level integration library for embedding Briolette wallet functionality
//! into applications (POS terminals, mobile messaging, backend services, etc.).
//!
//! This crate provides a stateless, async API over the Briolette wallet. All
//! operations take and return a [`BrioletteClient`] state snapshot that the
//! caller is responsible for persisting. This enables any application to
//! integrate Briolette payments without managing wallet internals.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐   ┌───────────────┐   ┌──────────────┐
//! │  Mobile App  │   │   POS System  │   │  Messaging   │
//! │  (UniFFI)    │   │   (Rust/C)    │   │  Plugin      │
//! └──────┬───────┘   └───────┬───────┘   └──────┬───────┘
//!        │                   │                   │
//!        └───────────┬───────┴───────────────────┘
//!                    │
//!          ┌─────────▼──────────┐
//!          │ briolette-integration │
//!          │  (this crate)       │
//!          └─────────┬──────────┘
//!                    │
//!          ┌─────────▼──────────┐
//!          │  briolette-wallet  │
//!          │  briolette-crypto  │
//!          │  briolette-proto   │
//!          └────────────────────┘
//! ```
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use briolette_integration::{BrioletteClient, ServiceConfig};
//!
//! # async fn example() -> Result<(), briolette_integration::Error> {
//! let config = ServiceConfig {
//!     registrar_uri: "http://[::1]:50051".into(),
//!     clerk_uri: "http://[::1]:50052".into(),
//!     mint_uri: "http://[::1]:50053".into(),
//!     validate_uri: "http://[::1]:50055".into(),
//! };
//!
//! // Create and register a new wallet
//! let client = BrioletteClient::create("my-wallet", &config).await?;
//!
//! // Persist the wallet state (app-specific storage)
//! let json = client.to_json()?;
//!
//! // Later: restore and use
//! let client = BrioletteClient::from_json(&json)?;
//! let balance = client.balance();
//! # Ok(())
//! # }
//! ```

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use prost::Message;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by integration operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("wallet not initialized")]
    NotInitialized,
    #[error("network error: {0}")]
    Network(String),
    #[error("insufficient funds: have {have}, need {need}")]
    InsufficientFunds { have: u32, need: u32 },
    #[error("no tickets available")]
    NoTicketsAvailable,
    #[error("invalid data: {0}")]
    InvalidData(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("validation failed")]
    ValidationFailed,
    #[error("wallet already registered")]
    AlreadyRegistered,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Network service endpoints for Briolette infrastructure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub registrar_uri: String,
    pub clerk_uri: String,
    pub mint_uri: String,
    pub validate_uri: String,
}

/// Hardware attestation data for registration.
#[derive(Debug, Clone)]
pub struct Attestation {
    /// Algorithm identifier (0=NONE, 1=ANDROID_KM, 2=IOS_APP_ATTEST).
    pub algorithm: i32,
    /// DER-encoded attestation signature.
    pub signature: Vec<u8>,
    /// DER-encoded attestation public key.
    pub public_key: Vec<u8>,
}

/// Card public key shares for split-key proof (HIGH security tier).
#[derive(Debug, Clone)]
pub struct SplitKeyProof {
    pub nac_card_public_key: Vec<u8>,
    pub ttc_card_public_key: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Balance
// ---------------------------------------------------------------------------

/// Wallet balance summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Balance {
    /// Whole currency units.
    pub whole: i32,
    /// Fractional units (micros, 1/1_000_000 of a whole unit).
    pub fractional: i32,
    /// Currency code (ISO 4217 name or "TEST", "ETH", etc.).
    pub currency: String,
    /// Number of individual tokens held.
    pub token_count: u32,
}

// ---------------------------------------------------------------------------
// Transfer result
// ---------------------------------------------------------------------------

/// Result of a token transfer operation.
#[derive(Debug, Clone)]
pub struct TransferResult {
    /// Updated client state after transfer.
    pub client: BrioletteClient,
    /// Encoded tokens to deliver to the recipient.
    /// Each entry is a protobuf-serialized `Token` message.
    pub tokens: Vec<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Validation result
// ---------------------------------------------------------------------------

/// Result of token validation.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Updated client state after validation.
    pub client: BrioletteClient,
    /// Whether all tokens are valid.
    pub all_valid: bool,
    /// Number of valid tokens.
    pub valid_count: u32,
    /// Number of invalid tokens (removed from wallet).
    pub invalid_count: u32,
}

// ---------------------------------------------------------------------------
// Key initialization result (for 2-phase registration)
// ---------------------------------------------------------------------------

/// Result of key initialization, containing the attestation challenge
/// preimage for cryptographic binding.
#[derive(Debug, Clone)]
pub struct KeyInitResult {
    /// The client with initialized keys (not yet registered).
    pub client: BrioletteClient,
    /// Attestation challenge preimage: `hw_id || nac_pk || ttc_pk`.
    /// SHA-256 hash this to get the attestation challenge.
    pub challenge_preimage: Vec<u8>,
    /// Card public key shares (empty if not using split keys).
    pub nac_card_public_key: Vec<u8>,
    pub ttc_card_public_key: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Split-key protocol intermediate results
// ---------------------------------------------------------------------------

/// Step 1 result: base point for TTC card.
#[derive(Debug, Clone)]
pub struct SplitKeyStep1 {
    /// Opaque state for the next step.
    pub state: SplitKeyState,
    /// G1 point for TTC card PUBLIC_KEY_SHARE + JOIN_COMMIT.
    pub b_ttc: Vec<u8>,
}

/// Step 2a result: TTC challenge and NAC base point.
#[derive(Debug, Clone)]
pub struct SplitKeyStep2a {
    pub state: SplitKeyState,
    /// Challenge for TTC card JOIN_RESPOND.
    pub c_ttc: Vec<u8>,
    /// G1 point for NAC card PUBLIC_KEY_SHARE + JOIN_COMMIT.
    pub b_nac: Vec<u8>,
}

/// Step 2b result: NAC challenge.
#[derive(Debug, Clone)]
pub struct SplitKeyStep2b {
    pub state: SplitKeyState,
    /// Challenge for NAC card JOIN_RESPOND.
    pub c_nac: Vec<u8>,
}

/// Opaque intermediate state for the split-key protocol.
/// Callers should not inspect or modify this; just pass it between steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitKeyState {
    wallet_json: String,
    hw_id: Vec<u8>,
    #[serde(default)]
    host_sk_ttc: Vec<u8>,
    #[serde(default)]
    host_r_ttc: Vec<u8>,
    #[serde(default)]
    q_card_ttc: Vec<u8>,
    #[serde(default)]
    u_card_ttc: Vec<u8>,
    #[serde(default)]
    c_ttc: Vec<u8>,
    #[serde(default)]
    q_ttc_combined: Vec<u8>,
    #[serde(default)]
    host_sk_nac: Vec<u8>,
    #[serde(default)]
    host_r_nac: Vec<u8>,
    #[serde(default)]
    q_card_nac: Vec<u8>,
    #[serde(default)]
    u_card_nac: Vec<u8>,
    #[serde(default)]
    c_nac: Vec<u8>,
}

// ---------------------------------------------------------------------------
// BrioletteClient — the main integration type
// ---------------------------------------------------------------------------

/// A Briolette wallet client.
///
/// This is the primary type for integrating Briolette payments into an
/// application. It wraps the underlying wallet state and provides a clean
/// async API for all wallet operations.
///
/// The client is stateless in the sense that each operation returns a new
/// client snapshot. The caller is responsible for persisting the state
/// between operations (via [`to_json`]/[`from_json`]).
///
/// [`to_json`]: BrioletteClient::to_json
/// [`from_json`]: BrioletteClient::from_json
#[derive(Debug, Clone)]
pub struct BrioletteClient {
    wallet_json: String,
    name: String,
}

impl BrioletteClient {
    // ===================================================================
    // Construction and persistence
    // ===================================================================

    /// Create a new wallet, register with the network, sync epoch, and
    /// fetch initial tickets. This is the simplest one-shot creation API.
    pub async fn create(name: &str, config: &ServiceConfig) -> Result<Self, Error> {
        use briolette_wallet::Wallet;

        let hw_id = sha256::digest(name.as_bytes());
        let mut wallet = briolette_wallet::WalletData::new(
            config.registrar_uri.clone(),
            config.clerk_uri.clone(),
            config.mint_uri.clone(),
            config.validate_uri.clone(),
        )
        .map_err(|e| Error::InvalidData(format!("{e}")))?;

        if !wallet.initialize_keys(hw_id.as_bytes()) {
            return Err(Error::NotInitialized);
        }
        if !wallet.initialize_credential().await {
            return Err(Error::Network("registration failed".into()));
        }
        if !wallet.synchronize().await {
            return Err(Error::Network("epoch sync failed".into()));
        }
        if !wallet.get_tickets(10).await {
            return Err(Error::Network("ticket fetch failed".into()));
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { wallet_json: json, name: name.to_string() })
    }

    /// Initialize keys without registering. Returns a [`KeyInitResult`]
    /// containing the attestation challenge preimage for 2-phase attested
    /// registration.
    pub fn init_keys(name: &str, config: &ServiceConfig) -> Result<KeyInitResult, Error> {
        use briolette_wallet::Wallet;

        let hw_id = sha256::digest(name.as_bytes());
        let mut wallet = briolette_wallet::WalletData::new(
            config.registrar_uri.clone(),
            config.clerk_uri.clone(),
            config.mint_uri.clone(),
            config.validate_uri.clone(),
        )
        .map_err(|e| Error::InvalidData(format!("{e}")))?;

        if !wallet.initialize_keys(hw_id.as_bytes()) {
            return Err(Error::NotInitialized);
        }

        let challenge_preimage = wallet.attestation_challenge_preimage();
        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(KeyInitResult {
            client: Self { wallet_json: json, name: name.to_string() },
            challenge_preimage,
            nac_card_public_key: Vec::new(),
            ttc_card_public_key: Vec::new(),
        })
    }

    /// Complete registration with hardware attestation data.
    /// Phase 2 of the 2-phase attested registration.
    pub async fn register_with_attestation(
        &self,
        attestation: &Attestation,
        split_key_proof: Option<&SplitKeyProof>,
    ) -> Result<Self, Error> {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        wallet.set_attestation_data(
            attestation.algorithm,
            attestation.signature.clone(),
            attestation.public_key.clone(),
        );

        if let Some(proof) = split_key_proof {
            wallet.set_split_key_proof(
                proof.nac_card_public_key.clone(),
                proof.ttc_card_public_key.clone(),
            );
        }

        if !wallet.initialize_credential().await {
            return Err(Error::Network("registration failed".into()));
        }
        if !wallet.synchronize().await {
            return Err(Error::Network("epoch sync failed".into()));
        }
        if !wallet.get_tickets(10).await {
            return Err(Error::Network("ticket fetch failed".into()));
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { wallet_json: json, name: self.name.clone() })
    }

    /// Restore a client from its JSON representation.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        // Validate parseable
        let v: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        let name = v.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown")
            .to_string();
        Ok(Self { wallet_json: json.to_string(), name })
    }

    /// Serialize the wallet state to JSON for persistence.
    pub fn to_json(&self) -> Result<String, Error> {
        // Validate it's still good JSON
        let _: serde_json::Value = serde_json::from_str(&self.wallet_json)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(self.wallet_json.clone())
    }

    /// Get the wallet display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    // ===================================================================
    // Balance and state queries
    // ===================================================================

    /// Compute the current wallet balance.
    pub fn balance(&self) -> Balance {
        compute_balance(&self.wallet_json)
    }

    /// Get the number of available receiving tickets.
    pub fn ticket_count(&self) -> u32 {
        let v: serde_json::Value = match serde_json::from_str(&self.wallet_json) {
            Ok(v) => v,
            Err(_) => return 0,
        };
        v.get("tickets")
            .and_then(|t| t.as_array())
            .map_or(0, |a| a.len() as u32)
    }

    /// Get a receiving ticket for sharing with a sender (e.g., as a QR code).
    /// Returns the raw protobuf-serialized `SignedTicket` bytes.
    pub fn receiving_ticket(&self) -> Result<Vec<u8>, Error> {
        let v: serde_json::Value = serde_json::from_str(&self.wallet_json)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        let ticket_arr = v.get("tickets")
            .and_then(|t| t.as_array())
            .ok_or(Error::NoTicketsAvailable)?;

        let first = ticket_arr.first()
            .ok_or(Error::NoTicketsAvailable)?;

        let ticket_bytes: Vec<u8> = first.get("ticket")
            .and_then(|t| serde_json::from_value(t.clone()).ok())
            .ok_or(Error::InvalidData("cannot decode ticket".into()))?;

        Ok(ticket_bytes)
    }

    /// Get a receiving ticket as a base64 string (convenient for QR codes).
    pub fn receiving_ticket_b64(&self) -> Result<String, Error> {
        self.receiving_ticket().map(|t| B64.encode(&t))
    }

    // ===================================================================
    // Network operations
    // ===================================================================

    /// Synchronize epoch data from the clerk.
    pub async fn synchronize(&self) -> Result<Self, Error> {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        if !wallet.synchronize().await {
            return Err(Error::Network("epoch sync failed".into()));
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { wallet_json: json, name: self.name.clone() })
    }

    /// Synchronize epoch data from a peer (gossip protocol, no network call).
    pub fn gossip_synchronize(&self, epoch_update_bytes: &[u8]) -> Result<Self, Error> {
        use briolette_wallet::Wallet;
        use briolette_proto::briolette::clerk::EpochUpdate;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        let epoch_update = EpochUpdate::decode(epoch_update_bytes)
            .map_err(|e| Error::InvalidData(format!("invalid epoch update: {e}")))?;

        if !wallet.gossip_synchronize(&epoch_update) {
            return Err(Error::InvalidData("epoch update rejected".into()));
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { wallet_json: json, name: self.name.clone() })
    }

    /// Request receiving tickets from the clerk.
    pub async fn request_tickets(&self, count: u32) -> Result<Self, Error> {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        if !wallet.get_tickets(count).await {
            return Err(Error::Network("ticket request failed".into()));
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { wallet_json: json, name: self.name.clone() })
    }

    /// Withdraw (mint) tokens from the network.
    pub async fn withdraw(&self, amount: u32) -> Result<Self, Error> {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        if wallet.tickets.is_empty() {
            return Err(Error::NoTicketsAvailable);
        }

        if !wallet.withdraw(amount).await {
            return Err(Error::Network("withdrawal failed".into()));
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { wallet_json: json, name: self.name.clone() })
    }

    /// Transfer tokens to a recipient.
    ///
    /// `recipient_ticket` is the raw protobuf `SignedTicket` bytes from the
    /// recipient (obtained via [`receiving_ticket`]).
    ///
    /// Returns a [`TransferResult`] containing the updated client and the
    /// token bytes to deliver to the recipient.
    ///
    /// [`receiving_ticket`]: BrioletteClient::receiving_ticket
    pub async fn transfer(
        &self,
        amount: u32,
        recipient_ticket: &[u8],
    ) -> Result<TransferResult, Error> {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        let balance_whole: i32 = wallet.tokens.iter().map(|t| t.whole_value).sum();
        if balance_whole < amount as i32 {
            return Err(Error::InsufficientFunds {
                have: balance_whole as u32,
                need: amount,
            });
        }

        if !wallet.transfer(amount, recipient_ticket.to_vec()) {
            return Err(Error::InsufficientFunds {
                have: balance_whole as u32,
                need: amount,
            });
        }

        let tokens: Vec<Vec<u8>> = wallet.pending_tokens.drain(..).collect();

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(TransferResult {
            client: Self { wallet_json: json, name: self.name.clone() },
            tokens,
        })
    }

    /// Import tokens received from a sender.
    ///
    /// `tokens` are raw protobuf-serialized `Token` messages (as returned
    /// by [`TransferResult::tokens`]).
    pub fn receive_tokens(&self, tokens: &[Vec<u8>]) -> Result<Self, Error> {
        use briolette_proto::briolette::token::Token;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        for raw in tokens {
            let token = Token::decode(raw.as_slice())
                .map_err(|e| Error::InvalidData(format!("invalid token: {e}")))?;
            wallet.tokens.push(briolette_wallet::TokenEntry::from(token));
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { wallet_json: json, name: self.name.clone() })
    }

    /// Validate all held tokens with the network.
    pub async fn validate(&self) -> Result<ValidationResult, Error> {
        use briolette_wallet::Wallet;

        let wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        let total = wallet.tokens.len() as u32;

        if !wallet.validate().await {
            return Err(Error::ValidationFailed);
        }

        let valid_count = wallet.tokens.len() as u32;
        let invalid_count = total.saturating_sub(valid_count);

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(ValidationResult {
            client: Self { wallet_json: json, name: self.name.clone() },
            all_valid: invalid_count == 0,
            valid_count,
            invalid_count,
        })
    }

    /// Migrate tokens bound to expired tickets to valid tickets.
    /// Returns the updated client and the count of tokens migrated.
    pub fn self_transfer_expired(&self) -> Result<(Self, usize), Error> {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        let count = wallet.self_transfer_expired();

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok((Self { wallet_json: json, name: self.name.clone() }, count))
    }

    /// Refresh specific tickets at the clerk.
    pub async fn refresh_tickets(&self, ticket_indices: &[usize]) -> Result<(Self, usize), Error> {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&self.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        let count = wallet.refresh_tickets(ticket_indices).await;

        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok((Self { wallet_json: json, name: self.name.clone() }, count))
    }

    // ===================================================================
    // Split-key protocol
    // ===================================================================

    /// Start the split-key protocol. Returns the TTC base point to send
    /// to the TTC smart card.
    pub fn split_key_start(name: &str, config: &ServiceConfig) -> Result<SplitKeyStep1, Error> {
        let hw_id = sha256::digest(name.as_bytes()).into_bytes();

        let wallet = briolette_wallet::WalletData::new(
            config.registrar_uri.clone(),
            config.clerk_uri.clone(),
            config.mint_uri.clone(),
            config.validate_uri.clone(),
        )
        .map_err(|e| Error::InvalidData(format!("{e}")))?;

        let b_ttc = briolette_crypto::v1::split::split_base_point(&hw_id);

        let wallet_json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(SplitKeyStep1 {
            state: SplitKeyState {
                wallet_json,
                hw_id,
                ..Default::default()
            },
            b_ttc,
        })
    }

    /// After TTC card responds with (q_card, u_card), compute TTC challenge
    /// and NAC base point.
    pub fn split_key_after_ttc_commit(
        state: &SplitKeyState,
        q_card_ttc: &[u8],
        u_card_ttc: &[u8],
    ) -> Result<SplitKeyStep2a, Error> {
        let (host_sk_ttc, host_r_ttc, c_ttc, q_ttc_combined) =
            briolette_crypto::v1::split::split_join_host_commit_and_challenge(
                &state.hw_id, q_card_ttc, u_card_ttc,
            )
            .ok_or(Error::InvalidData("TTC commit/challenge failed".into()))?;

        let b_nac = briolette_crypto::v1::split::split_base_point(&q_ttc_combined);

        Ok(SplitKeyStep2a {
            state: SplitKeyState {
                wallet_json: state.wallet_json.clone(),
                hw_id: state.hw_id.clone(),
                host_sk_ttc,
                host_r_ttc,
                q_card_ttc: q_card_ttc.to_vec(),
                u_card_ttc: u_card_ttc.to_vec(),
                c_ttc: c_ttc.clone(),
                q_ttc_combined,
                ..Default::default()
            },
            c_ttc,
            b_nac,
        })
    }

    /// After NAC card responds with (q_card, u_card), compute NAC challenge.
    pub fn split_key_after_nac_commit(
        state: &SplitKeyState,
        q_card_nac: &[u8],
        u_card_nac: &[u8],
    ) -> Result<SplitKeyStep2b, Error> {
        let (host_sk_nac, host_r_nac, c_nac, _) =
            briolette_crypto::v1::split::split_join_host_commit_and_challenge(
                &state.q_ttc_combined, q_card_nac, u_card_nac,
            )
            .ok_or(Error::InvalidData("NAC commit/challenge failed".into()))?;

        Ok(SplitKeyStep2b {
            state: SplitKeyState {
                wallet_json: state.wallet_json.clone(),
                hw_id: state.hw_id.clone(),
                host_sk_ttc: state.host_sk_ttc.clone(),
                host_r_ttc: state.host_r_ttc.clone(),
                q_card_ttc: state.q_card_ttc.clone(),
                u_card_ttc: state.u_card_ttc.clone(),
                c_ttc: state.c_ttc.clone(),
                q_ttc_combined: state.q_ttc_combined.clone(),
                host_sk_nac,
                host_r_nac,
                q_card_nac: q_card_nac.to_vec(),
                u_card_nac: u_card_nac.to_vec(),
                c_nac: c_nac.clone(),
            },
            c_nac,
        })
    }

    /// Complete the split-key protocol with card responses.
    pub fn split_key_complete(
        state: &SplitKeyState,
        s_card_ttc: &[u8],
        s_card_nac: &[u8],
    ) -> Result<KeyInitResult, Error> {
        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&state.wallet_json)
                .map_err(|e| Error::Serialization(e.to_string()))?;

        let ttc_pk = briolette_crypto::v1::split::split_join_finalize(
            &state.hw_id,
            &state.q_card_ttc,
            &state.u_card_ttc,
            &state.host_sk_ttc,
            &state.host_r_ttc,
            &state.c_ttc,
            s_card_ttc,
        )
        .ok_or(Error::InvalidData("TTC finalize failed".into()))?;

        let nac_pk = briolette_crypto::v1::split::split_join_finalize(
            &state.q_ttc_combined,
            &state.q_card_nac,
            &state.u_card_nac,
            &state.host_sk_nac,
            &state.host_r_nac,
            &state.c_nac,
            s_card_nac,
        )
        .ok_or(Error::InvalidData("NAC finalize failed".into()))?;

        wallet.set_split_keys(
            state.hw_id.clone(),
            nac_pk,
            state.host_sk_nac.clone(),
            ttc_pk,
            state.host_sk_ttc.clone(),
        );

        let challenge_preimage = wallet.attestation_challenge_preimage();
        let json = serde_json::to_string(&wallet)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(KeyInitResult {
            client: Self { wallet_json: json, name: "split-key".to_string() },
            challenge_preimage,
            nac_card_public_key: state.q_card_nac.clone(),
            ttc_card_public_key: state.q_card_ttc.clone(),
        })
    }

    // ===================================================================
    // Convenience: base64 encoding for transport
    // ===================================================================

    /// Encode raw bytes as base64 (for QR codes, message payloads, etc.).
    pub fn encode_b64(data: &[u8]) -> String {
        B64.encode(data)
    }

    /// Decode base64 to raw bytes.
    pub fn decode_b64(s: &str) -> Result<Vec<u8>, Error> {
        B64.decode(s)
            .map_err(|e| Error::InvalidData(format!("base64 decode: {e}")))
    }

    /// Import tokens from base64-encoded strings.
    pub fn receive_tokens_b64(&self, tokens_b64: &[String]) -> Result<Self, Error> {
        let tokens: Vec<Vec<u8>> = tokens_b64
            .iter()
            .map(|s| Self::decode_b64(s))
            .collect::<Result<_, _>>()?;
        self.receive_tokens(&tokens)
    }

    /// Transfer tokens, returning base64-encoded tokens for delivery.
    pub async fn transfer_b64(
        &self,
        amount: u32,
        recipient_ticket_b64: &str,
    ) -> Result<(Self, Vec<String>), Error> {
        let ticket = Self::decode_b64(recipient_ticket_b64)?;
        let result = self.transfer(amount, &ticket).await?;
        let tokens_b64: Vec<String> = result.tokens.iter()
            .map(|t| Self::encode_b64(t))
            .collect();
        Ok((result.client, tokens_b64))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl Default for SplitKeyState {
    fn default() -> Self {
        Self {
            wallet_json: String::new(),
            hw_id: Vec::new(),
            host_sk_ttc: Vec::new(),
            host_r_ttc: Vec::new(),
            q_card_ttc: Vec::new(),
            u_card_ttc: Vec::new(),
            c_ttc: Vec::new(),
            q_ttc_combined: Vec::new(),
            host_sk_nac: Vec::new(),
            host_r_nac: Vec::new(),
            q_card_nac: Vec::new(),
            u_card_nac: Vec::new(),
            c_nac: Vec::new(),
        }
    }
}

/// Compute balance from wallet JSON without deserializing the full wallet.
fn compute_balance(json: &str) -> Balance {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Balance::default(),
    };

    let tokens = v.get("tokens").and_then(|t| t.as_array());
    let mut whole_sum: i64 = 0;
    let mut frac_sum: i64 = 0;
    let mut currency = String::from("TEST");
    let token_count = tokens.map_or(0, |t| t.len()) as u32;

    if let Some(toks) = tokens {
        for tok in toks {
            whole_sum += tok.get("whole_value").and_then(|v| v.as_i64()).unwrap_or(0);
            let frac = tok.get("fractional_value")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            frac_sum += frac as i64;
            if let Some(code) = tok.get("value_code").and_then(|v| v.as_i64()) {
                currency = currency_name(code);
            }
        }
    }

    // Normalize fractional overflow (1_000_000 micros = 1 whole)
    whole_sum += frac_sum / 1_000_000;
    frac_sum %= 1_000_000;

    Balance {
        whole: whole_sum as i32,
        fractional: frac_sum as i32,
        currency,
        token_count,
    }
}

fn currency_name(code: i64) -> String {
    match code {
        0 => "TEST".to_string(),
        8888 => "ETH".to_string(),
        840 => "USD".to_string(),
        978 => "EUR".to_string(),
        _ => format!("CODE_{code}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_empty_wallet() {
        let json = r#"{"name":"test","tokens":[],"tickets":[]}"#;
        let client = BrioletteClient::from_json(json).unwrap();
        let b = client.balance();
        assert_eq!(b.whole, 0);
        assert_eq!(b.fractional, 0);
        assert_eq!(b.token_count, 0);
        assert_eq!(b.currency, "TEST");
    }

    #[test]
    fn balance_with_tokens() {
        let json = r#"{
            "name": "alice",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 5, "fractional_value": 0, "value_code": 0},
                {"token": "", "credential": "", "whole_value": 3, "fractional_value": 500000, "value_code": 0}
            ],
            "tickets": []
        }"#;
        let client = BrioletteClient::from_json(json).unwrap();
        let b = client.balance();
        assert_eq!(b.whole, 8);
        assert_eq!(b.fractional, 500_000);
        assert_eq!(b.token_count, 2);
    }

    #[test]
    fn balance_fractional_overflow() {
        let json = r#"{
            "name": "norm",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 0, "fractional_value": 2500000, "value_code": 0}
            ],
            "tickets": []
        }"#;
        let client = BrioletteClient::from_json(json).unwrap();
        let b = client.balance();
        assert_eq!(b.whole, 2);
        assert_eq!(b.fractional, 500_000);
    }

    #[test]
    fn ticket_count() {
        let json = r#"{
            "name": "bob",
            "tokens": [],
            "tickets": [{"ticket": [1,2,3]}, {"ticket": [4,5,6]}]
        }"#;
        let client = BrioletteClient::from_json(json).unwrap();
        assert_eq!(client.ticket_count(), 2);
    }

    #[test]
    fn receiving_ticket() {
        let json = r#"{"tickets":[{"ticket":[1,2,3,4]}]}"#;
        let client = BrioletteClient::from_json(json).unwrap();
        let ticket = client.receiving_ticket().unwrap();
        assert_eq!(ticket, vec![1, 2, 3, 4]);
        assert_eq!(client.receiving_ticket_b64().unwrap(), "AQIDBA==");
    }

    #[test]
    fn receiving_ticket_no_tickets() {
        let json = r#"{"tickets":[]}"#;
        let client = BrioletteClient::from_json(json).unwrap();
        assert!(client.receiving_ticket().is_err());
    }

    #[test]
    fn from_json_invalid() {
        assert!(BrioletteClient::from_json("not json").is_err());
    }

    #[test]
    fn roundtrip_json() {
        let json = r#"{"name":"test","tokens":[],"tickets":[]}"#;
        let client = BrioletteClient::from_json(json).unwrap();
        let saved = client.to_json().unwrap();
        assert_eq!(saved, json);
    }

    #[test]
    fn name_extraction() {
        let json = r#"{"name":"alice","tokens":[],"tickets":[]}"#;
        let client = BrioletteClient::from_json(json).unwrap();
        assert_eq!(client.name(), "alice");
    }

    #[test]
    fn name_missing_defaults_to_unknown() {
        let json = r#"{"tokens":[],"tickets":[]}"#;
        let client = BrioletteClient::from_json(json).unwrap();
        assert_eq!(client.name(), "unknown");
    }

    #[test]
    fn b64_roundtrip() {
        let data = vec![1, 2, 3, 4, 5];
        let encoded = BrioletteClient::encode_b64(&data);
        let decoded = BrioletteClient::decode_b64(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn b64_decode_invalid() {
        assert!(BrioletteClient::decode_b64("not valid b64!!!").is_err());
    }

    #[test]
    fn currency_codes() {
        assert_eq!(currency_name(0), "TEST");
        assert_eq!(currency_name(840), "USD");
        assert_eq!(currency_name(978), "EUR");
        assert_eq!(currency_name(8888), "ETH");
        assert_eq!(currency_name(9999), "CODE_9999");
    }

    #[test]
    fn split_key_state_default() {
        let state = SplitKeyState::default();
        assert!(state.hw_id.is_empty());
        assert!(state.wallet_json.is_empty());
    }

    #[test]
    fn split_key_protocol_produces_card_bound_challenge() {
        use briolette_crypto::v1::split::{MockCard, SmartCard};

        let config = ServiceConfig {
            registrar_uri: "http://[::1]:50051".into(),
            clerk_uri: "http://[::1]:50052".into(),
            mint_uri: "http://[::1]:50053".into(),
            validate_uri: "http://[::1]:50055".into(),
        };

        // Step 1
        let step1 = BrioletteClient::split_key_start("card-test", &config).unwrap();
        assert!(!step1.b_ttc.is_empty());

        // TTC card
        let mut ttc_card = MockCard::new();
        let q_card_ttc = ttc_card.public_key_share(&step1.b_ttc);
        let u_card_ttc = ttc_card.join_commit(&step1.b_ttc).unwrap();

        // Step 2a
        let step2a = BrioletteClient::split_key_after_ttc_commit(
            &step1.state, &q_card_ttc, &u_card_ttc,
        ).unwrap();
        let s_card_ttc = ttc_card.join_respond(&step2a.c_ttc).unwrap();

        // NAC card
        let mut nac_card = MockCard::new();
        let q_card_nac = nac_card.public_key_share(&step2a.b_nac);
        let u_card_nac = nac_card.join_commit(&step2a.b_nac).unwrap();

        // Step 2b
        let step2b = BrioletteClient::split_key_after_nac_commit(
            &step2a.state, &q_card_nac, &u_card_nac,
        ).unwrap();
        let s_card_nac = nac_card.join_respond(&step2b.c_nac).unwrap();

        // Complete
        let result = BrioletteClient::split_key_complete(
            &step2b.state, &s_card_ttc, &s_card_nac,
        ).unwrap();

        assert!(!result.client.wallet_json.is_empty());
        assert!(!result.challenge_preimage.is_empty());

        // Verify preimage starts with hw_id
        let hw_id = sha256::digest("card-test".as_bytes()).into_bytes();
        assert!(result.challenge_preimage.starts_with(&hw_id));
    }
}
