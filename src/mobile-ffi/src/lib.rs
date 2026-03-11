//! UniFFI bridge for Briolette wallet operations.
//!
//! Exposes the wallet's async Rust API as synchronous functions that Kotlin
//! and Swift can call via UniFFI-generated bindings. Each function takes and
//! returns a `WalletState` (serialized JSON + cached summary), allowing the
//! mobile app to persist state between calls.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use prost::Message;

uniffi::include_scaffolding!("briolette");

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("wallet not initialized")]
    NotInitialized,
    #[error("network error")]
    NetworkError,
    #[error("insufficient funds")]
    InsufficientFunds,
    #[error("no tickets available")]
    NoTicketsAvailable,
    #[error("invalid data")]
    InvalidData,
    #[error("serialization error")]
    SerializationError,
    #[error("validation failed")]
    ValidationFailed,
}

// ---------------------------------------------------------------------------
// Data types (mirrored in UDL)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Balance {
    pub whole: i32,
    pub fractional: i32,
    pub currency: String,
    pub token_count: u32,
}

#[derive(Debug, Clone)]
pub struct WalletState {
    pub json: String,
    pub balance: Balance,
    pub ticket_count: u32,
    pub wallet_name: String,
}

#[derive(Debug, Clone)]
pub struct TransferResult {
    pub state: WalletState,
    pub tokens_b64: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub state: WalletState,
    pub all_valid: bool,
    pub valid_count: u32,
    pub invalid_count: u32,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a tokio runtime for blocking on async wallet operations.
fn runtime() -> Result<tokio::runtime::Runtime, WalletError> {
    tokio::runtime::Runtime::new()
        .map_err(|_| WalletError::NetworkError)
}

/// Extract a summary from wallet JSON without fully deserializing the wallet.
/// This parses the JSON to compute balance/ticket counts for the cached fields.
fn summarize_wallet(json: &str, name: &str) -> Result<WalletState, WalletError> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| WalletError::SerializationError)?;

    let tokens = v.get("tokens").and_then(|t| t.as_array());
    let tickets = v.get("tickets").and_then(|t| t.as_array());

    let mut whole_sum: i64 = 0;
    let mut frac_sum: i64 = 0;
    let mut currency = String::from("TEST");
    let token_count = tokens.map_or(0, |t| t.len()) as u32;

    if let Some(toks) = tokens {
        for tok in toks {
            whole_sum += tok.get("whole_value").and_then(|v| v.as_i64()).unwrap_or(0);
            // fractional_value is f32 in the wallet, representing micros
            let frac = tok.get("fractional_value")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            frac_sum += frac as i64;
            if let Some(code) = tok.get("value_code").and_then(|v| v.as_i64()) {
                currency = match code {
                    0 => "TEST".to_string(),
                    8888 => "ETH".to_string(),
                    840 => "USD".to_string(),
                    978 => "EUR".to_string(),
                    _ => format!("CODE_{code}"),
                };
            }
        }
    }

    // Normalize fractional overflow (1_000_000 micros = 1 whole)
    whole_sum += frac_sum / 1_000_000;
    frac_sum %= 1_000_000;

    Ok(WalletState {
        json: json.to_string(),
        balance: Balance {
            whole: whole_sum as i32,
            fractional: frac_sum as i32,
            currency,
            token_count,
        },
        ticket_count: tickets.map_or(0, |t| t.len()) as u32,
        wallet_name: name.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Public FFI functions
// ---------------------------------------------------------------------------

/// Create a new wallet, register with the network, sync epoch, and fetch
/// initial tickets. Returns serialized wallet state.
pub fn create_wallet(
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
) -> Result<String, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        // Generate a hardware ID from the wallet name.
        let hw_id = sha256::digest(name.as_bytes());

        let mut wallet = briolette_wallet::WalletData::new(
            registrar_uri,
            clerk_uri,
            mint_uri,
            validate_uri,
        )
        .map_err(|_| WalletError::InvalidData)?;

        if !wallet.initialize_keys(hw_id.as_bytes()) {
            return Err(WalletError::NotInitialized);
        }

        if !wallet.initialize_credential().await {
            return Err(WalletError::NetworkError);
        }

        if !wallet.synchronize().await {
            return Err(WalletError::NetworkError);
        }

        if !wallet.get_tickets(10).await {
            return Err(WalletError::NetworkError);
        }

        serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)
    })
}

/// Result of `init_wallet_keys` — contains the serialized wallet and the
/// attestation challenge preimage that must be hashed and used as the
/// attestation challenge for cryptographic binding.
#[derive(Debug, Clone)]
pub struct KeyInitResult {
    pub wallet_json: String,
    pub challenge_preimage_b64: String,
    // Card public key shares for split-key proof (base64).
    // Empty strings when not using split keys.
    pub nac_card_public_key_b64: String,
    pub ttc_card_public_key_b64: String,
}

#[derive(Debug, Clone)]
pub struct AttestationData {
    pub algorithm: i32,
    pub signature_b64: String,
    pub public_key_b64: String,
}

/// Initialize wallet keys and return the attestation challenge preimage.
///
/// Phase 1 of 2-phase attested registration. The returned base64 value is
/// `hw_id || nac_pk || ttc_pk` — the mobile app must SHA-256 hash this and
/// use the hash as the attestation challenge when generating Android Key
/// Attestation or iOS App Attest data. This cryptographically binds the
/// hardware attestation to the specific ECDAA credential public keys,
/// preventing attestation replay attacks.
///
/// The returned `wallet_json` must be passed to `register_wallet_with_attestation`
/// along with the attestation data to complete registration.
pub fn init_wallet_keys(
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
) -> Result<KeyInitResult, WalletError> {
    use briolette_wallet::Wallet;
    let hw_id = sha256::digest(name.as_bytes());

    let mut wallet = briolette_wallet::WalletData::new(
        registrar_uri,
        clerk_uri,
        mint_uri,
        validate_uri,
    )
    .map_err(|_| WalletError::InvalidData)?;

    if !wallet.initialize_keys(hw_id.as_bytes()) {
        return Err(WalletError::NotInitialized);
    }

    let challenge_preimage = wallet.attestation_challenge_preimage();
    let wallet_json = serde_json::to_string(&wallet)
        .map_err(|_| WalletError::SerializationError)?;

    Ok(KeyInitResult {
        wallet_json,
        challenge_preimage_b64: B64.encode(&challenge_preimage),
        nac_card_public_key_b64: String::new(),
        ttc_card_public_key_b64: String::new(),
    })
}

/// Complete attested wallet registration.
///
/// Phase 2 of 2-phase attested registration. Takes the wallet JSON from
/// `init_wallet_keys` and the attestation data generated using the
/// challenge preimage. Registers with the network, syncs epoch, and
/// fetches initial tickets.
pub fn register_wallet_with_attestation(
    wallet_json: String,
    attestation: AttestationData,
    nac_card_public_key_b64: String,
    ttc_card_public_key_b64: String,
) -> Result<String, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&wallet_json)
                .map_err(|_| WalletError::SerializationError)?;

        // Decode and set attestation data.
        let sig_bytes = B64
            .decode(&attestation.signature_b64)
            .map_err(|_| WalletError::InvalidData)?;
        let pk_bytes = B64
            .decode(&attestation.public_key_b64)
            .map_err(|_| WalletError::InvalidData)?;
        wallet.set_attestation_data(attestation.algorithm, sig_bytes, pk_bytes);

        // Set split-key proof if card public key shares are provided.
        if !nac_card_public_key_b64.is_empty() && !ttc_card_public_key_b64.is_empty() {
            let nac_card_pk = B64.decode(&nac_card_public_key_b64)
                .map_err(|_| WalletError::InvalidData)?;
            let ttc_card_pk = B64.decode(&ttc_card_public_key_b64)
                .map_err(|_| WalletError::InvalidData)?;
            wallet.set_split_key_proof(nac_card_pk, ttc_card_pk);
        }

        if !wallet.initialize_credential().await {
            return Err(WalletError::NetworkError);
        }

        if !wallet.synchronize().await {
            return Err(WalletError::NetworkError);
        }

        if !wallet.get_tickets(10).await {
            return Err(WalletError::NetworkError);
        }

        serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)
    })
}

/// Create a new wallet with hardware attestation data (legacy one-shot API).
///
/// NOTE: This function accepts pre-generated attestation data. For proper
/// cryptographic binding, use the 2-phase API: `init_wallet_keys` followed
/// by `register_wallet_with_attestation`, which allows the attestation
/// challenge to include the ECDAA public keys.
pub fn create_wallet_with_attestation(
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
    attestation: AttestationData,
) -> Result<String, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        let hw_id = sha256::digest(name.as_bytes());

        let mut wallet = briolette_wallet::WalletData::new(
            registrar_uri,
            clerk_uri,
            mint_uri,
            validate_uri,
        )
        .map_err(|_| WalletError::InvalidData)?;

        if !wallet.initialize_keys(hw_id.as_bytes()) {
            return Err(WalletError::NotInitialized);
        }

        // Decode and set attestation data.
        let sig_bytes = B64
            .decode(&attestation.signature_b64)
            .map_err(|_| WalletError::InvalidData)?;
        let pk_bytes = B64
            .decode(&attestation.public_key_b64)
            .map_err(|_| WalletError::InvalidData)?;
        wallet.set_attestation_data(attestation.algorithm, sig_bytes, pk_bytes);

        if !wallet.initialize_credential().await {
            return Err(WalletError::NetworkError);
        }

        if !wallet.synchronize().await {
            return Err(WalletError::NetworkError);
        }

        if !wallet.get_tickets(10).await {
            return Err(WalletError::NetworkError);
        }

        serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)
    })
}

// ---------------------------------------------------------------------------
// Split-key + attestation multi-step protocol
// ---------------------------------------------------------------------------
//
// For wallets using NFC smart cards (split-key mode), key generation requires
// interactive NFC communication. The card-derived combined public keys flow
// into the attestation challenge, cryptographically binding the attestation
// to both the phone hardware AND the specific NFC card.
//
// Protocol:
//
// Step 1: split_key_start(name, config)
//   → Returns base point B_ttc. App sends to TTC card:
//     PUBLIC_KEY_SHARE(B_ttc) → q_card_ttc, JOIN_COMMIT(B_ttc) → u_card_ttc
//
// Step 2a: split_key_after_ttc_commit(state, q_card_ttc, u_card_ttc)
//   → Returns c_ttc (send to TTC card: JOIN_RESPOND(c_ttc) → s_card_ttc)
//     and B_nac (send to NAC card: PUBLIC_KEY_SHARE, JOIN_COMMIT)
//
// Step 2b: split_key_after_nac_commit(state, q_card_nac, u_card_nac)
//   → Returns c_nac (send to NAC card: JOIN_RESPOND(c_nac) → s_card_nac)
//
// Step 3: split_key_complete(state, s_card_ttc, s_card_nac)
//   → Returns KeyInitResult with challenge_preimage_b64 bound to
//     card-derived combined public keys
//
// Step 4: register_wallet_with_attestation(wallet_json, attestation)
//   → Same as non-split path

/// Result of split_key_start.
#[derive(Debug, Clone)]
pub struct SplitKeyStep1Result {
    pub state_json: String,
    /// Base64 G1 point for TTC card PUBLIC_KEY_SHARE + JOIN_COMMIT.
    pub b_ttc_b64: String,
}

/// Result of split_key_after_ttc_commit.
#[derive(Debug, Clone)]
pub struct SplitKeyStep2aResult {
    pub state_json: String,
    /// Challenge for TTC card JOIN_RESPOND.
    pub c_ttc_b64: String,
    /// Base64 G1 point for NAC card PUBLIC_KEY_SHARE + JOIN_COMMIT.
    pub b_nac_b64: String,
}

/// Result of split_key_after_nac_commit.
#[derive(Debug, Clone)]
pub struct SplitKeyStep2bResult {
    pub state_json: String,
    /// Challenge for NAC card JOIN_RESPOND.
    pub c_nac_b64: String,
}

/// Step 1: Initialize wallet and compute TTC base point.
pub fn split_key_start(
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
) -> Result<SplitKeyStep1Result, WalletError> {
    let hw_id = sha256::digest(name.as_bytes());

    let wallet = briolette_wallet::WalletData::new(
        registrar_uri, clerk_uri, mint_uri, validate_uri,
    ).map_err(|_| WalletError::InvalidData)?;

    let hw_id_bytes = hw_id.into_bytes();
    let b_ttc = briolette_crypto::v0::split::split_base_point(&hw_id_bytes);

    let state = serde_json::json!({
        "wallet": serde_json::to_string(&wallet).map_err(|_| WalletError::SerializationError)?,
        "hw_id": B64.encode(&hw_id_bytes),
    });

    Ok(SplitKeyStep1Result {
        state_json: state.to_string(),
        b_ttc_b64: B64.encode(&b_ttc),
    })
}

/// Step 2a: After TTC card responses, compute TTC challenge and NAC base point.
pub fn split_key_after_ttc_commit(
    state_json: String,
    q_card_ttc_b64: String,
    u_card_ttc_b64: String,
) -> Result<SplitKeyStep2aResult, WalletError> {
    let state: serde_json::Value = serde_json::from_str(&state_json)
        .map_err(|_| WalletError::SerializationError)?;

    let hw_id = B64.decode(state["hw_id"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let q_card_ttc = B64.decode(&q_card_ttc_b64).map_err(|_| WalletError::InvalidData)?;
    let u_card_ttc = B64.decode(&u_card_ttc_b64).map_err(|_| WalletError::InvalidData)?;

    let (host_sk_ttc, host_r_ttc, c_ttc, q_ttc_combined) =
        briolette_crypto::v0::split::split_join_host_commit_and_challenge(
            &hw_id, &q_card_ttc, &u_card_ttc,
        ).ok_or(WalletError::InvalidData)?;

    let b_nac = briolette_crypto::v0::split::split_base_point(&q_ttc_combined);

    let updated_state = serde_json::json!({
        "wallet": state["wallet"],
        "hw_id": state["hw_id"],
        "host_sk_ttc": B64.encode(&host_sk_ttc),
        "host_r_ttc": B64.encode(&host_r_ttc),
        "q_card_ttc": q_card_ttc_b64,
        "u_card_ttc": u_card_ttc_b64,
        "c_ttc": B64.encode(&c_ttc),
        "q_ttc_combined": B64.encode(&q_ttc_combined),
    });

    Ok(SplitKeyStep2aResult {
        state_json: updated_state.to_string(),
        c_ttc_b64: B64.encode(&c_ttc),
        b_nac_b64: B64.encode(&b_nac),
    })
}

/// Step 2b: After NAC card responses, compute NAC challenge.
pub fn split_key_after_nac_commit(
    state_json: String,
    q_card_nac_b64: String,
    u_card_nac_b64: String,
) -> Result<SplitKeyStep2bResult, WalletError> {
    let state: serde_json::Value = serde_json::from_str(&state_json)
        .map_err(|_| WalletError::SerializationError)?;

    let q_ttc_combined = B64.decode(
        state["q_ttc_combined"].as_str().ok_or(WalletError::InvalidData)?
    ).map_err(|_| WalletError::InvalidData)?;
    let q_card_nac = B64.decode(&q_card_nac_b64).map_err(|_| WalletError::InvalidData)?;
    let u_card_nac = B64.decode(&u_card_nac_b64).map_err(|_| WalletError::InvalidData)?;

    let (host_sk_nac, host_r_nac, c_nac, _) =
        briolette_crypto::v0::split::split_join_host_commit_and_challenge(
            &q_ttc_combined, &q_card_nac, &u_card_nac,
        ).ok_or(WalletError::InvalidData)?;

    let mut updated_state: serde_json::Value = serde_json::from_str(&state_json)
        .map_err(|_| WalletError::SerializationError)?;
    updated_state["host_sk_nac"] = serde_json::Value::String(B64.encode(&host_sk_nac));
    updated_state["host_r_nac"] = serde_json::Value::String(B64.encode(&host_r_nac));
    updated_state["q_card_nac"] = serde_json::Value::String(q_card_nac_b64);
    updated_state["u_card_nac"] = serde_json::Value::String(u_card_nac_b64);
    updated_state["c_nac"] = serde_json::Value::String(B64.encode(&c_nac));

    Ok(SplitKeyStep2bResult {
        state_json: updated_state.to_string(),
        c_nac_b64: B64.encode(&c_nac),
    })
}

/// Step 3: Combine card responses into split-key wallet.
/// The challenge_preimage_b64 in the result is bound to the card-derived
/// combined public keys (hw_id || nac_pk || ttc_pk where nac_pk and ttc_pk
/// contain the card's public key share contributions).
pub fn split_key_complete(
    state_json: String,
    s_card_ttc_b64: String,
    s_card_nac_b64: String,
) -> Result<KeyInitResult, WalletError> {
    let state: serde_json::Value = serde_json::from_str(&state_json)
        .map_err(|_| WalletError::SerializationError)?;

    let wallet_json_str = state["wallet"].as_str().ok_or(WalletError::InvalidData)?;
    let mut wallet: briolette_wallet::WalletData =
        serde_json::from_str(wallet_json_str)
            .map_err(|_| WalletError::SerializationError)?;

    // Decode all intermediates
    let hw_id = B64.decode(state["hw_id"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let host_sk_ttc = B64.decode(state["host_sk_ttc"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let host_r_ttc = B64.decode(state["host_r_ttc"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let q_card_ttc = B64.decode(state["q_card_ttc"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let u_card_ttc = B64.decode(state["u_card_ttc"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let c_ttc = B64.decode(state["c_ttc"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let s_card_ttc = B64.decode(&s_card_ttc_b64).map_err(|_| WalletError::InvalidData)?;
    let q_ttc_combined = B64.decode(state["q_ttc_combined"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;

    let host_sk_nac = B64.decode(state["host_sk_nac"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let host_r_nac = B64.decode(state["host_r_nac"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let q_card_nac = B64.decode(state["q_card_nac"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let u_card_nac = B64.decode(state["u_card_nac"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let c_nac = B64.decode(state["c_nac"].as_str().ok_or(WalletError::InvalidData)?)
        .map_err(|_| WalletError::InvalidData)?;
    let s_card_nac = B64.decode(&s_card_nac_b64).map_err(|_| WalletError::InvalidData)?;

    // Finalize TTC key
    let ttc_pk = briolette_crypto::v0::split::split_join_finalize(
        &hw_id, &q_card_ttc, &u_card_ttc, &host_sk_ttc, &host_r_ttc,
        &c_ttc, &s_card_ttc,
    ).ok_or(WalletError::InvalidData)?;

    // Finalize NAC key
    let nac_pk = briolette_crypto::v0::split::split_join_finalize(
        &q_ttc_combined, &q_card_nac, &u_card_nac, &host_sk_nac, &host_r_nac,
        &c_nac, &s_card_nac,
    ).ok_or(WalletError::InvalidData)?;

    // Set the wallet's split keys
    wallet.set_split_keys(hw_id, nac_pk, host_sk_nac, ttc_pk, host_sk_ttc);

    let challenge_preimage = wallet.attestation_challenge_preimage();
    let final_json = serde_json::to_string(&wallet)
        .map_err(|_| WalletError::SerializationError)?;

    Ok(KeyInitResult {
        wallet_json: final_json,
        challenge_preimage_b64: B64.encode(&challenge_preimage),
        nac_card_public_key_b64: B64.encode(&q_card_nac),
        ttc_card_public_key_b64: B64.encode(&q_card_ttc),
    })
}

/// Load a wallet from its JSON representation.
pub fn load_wallet(json: String) -> Result<WalletState, WalletError> {
    // Validate that the JSON is parseable.
    let v: serde_json::Value = serde_json::from_str(&json)
        .map_err(|_| WalletError::SerializationError)?;

    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("unknown")
        .to_string();

    summarize_wallet(&json, &name)
}

/// Serialize wallet state to JSON.
pub fn save_wallet(state: WalletState) -> Result<String, WalletError> {
    // The state.json is already valid JSON; just return it.
    // Validate it first.
    let _: serde_json::Value = serde_json::from_str(&state.json)
        .map_err(|_| WalletError::SerializationError)?;
    Ok(state.json)
}

/// Synchronize epoch data from the clerk.
pub fn synchronize(state: WalletState, _clerk_uri: String) -> Result<WalletState, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&state.json)
                .map_err(|_| WalletError::SerializationError)?;

        if !wallet.synchronize().await {
            return Err(WalletError::NetworkError);
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)?;

        summarize_wallet(&json, &state.wallet_name)
    })
}

/// Request receiving tickets from the clerk.
pub fn request_tickets(
    state: WalletState,
    _clerk_uri: String,
    count: u32,
) -> Result<WalletState, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&state.json)
                .map_err(|_| WalletError::SerializationError)?;

        if !wallet.get_tickets(count).await {
            return Err(WalletError::NetworkError);
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)?;

        summarize_wallet(&json, &state.wallet_name)
    })
}

/// Withdraw (top up) tokens from the mint.
pub fn withdraw(
    state: WalletState,
    _mint_uri: String,
    amount: u32,
) -> Result<WalletState, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&state.json)
                .map_err(|_| WalletError::SerializationError)?;

        if wallet.tickets.is_empty() {
            return Err(WalletError::NoTicketsAvailable);
        }

        if !wallet.withdraw(amount).await {
            return Err(WalletError::NetworkError);
        }

        let json = serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)?;

        summarize_wallet(&json, &state.wallet_name)
    })
}

/// Transfer tokens to a recipient. Returns updated state + base64 tokens to send.
pub fn transfer_tokens(
    state: WalletState,
    recipient_ticket_b64: String,
    amount: u32,
) -> Result<TransferResult, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        let recipient_bytes = B64
            .decode(&recipient_ticket_b64)
            .map_err(|_| WalletError::InvalidData)?;

        let mut wallet: briolette_wallet::WalletData =
            serde_json::from_str(&state.json)
                .map_err(|_| WalletError::SerializationError)?;

        let balance_whole: i32 = wallet.tokens.iter().map(|t| t.whole_value).sum();
        if balance_whole < amount as i32 {
            return Err(WalletError::InsufficientFunds);
        }

        if !wallet.transfer(amount, recipient_bytes) {
            return Err(WalletError::InsufficientFunds);
        }

        // Extract pending tokens as base64 for the caller to deliver.
        let tokens_b64: Vec<String> = wallet
            .pending_tokens
            .iter()
            .map(|t| B64.encode(t))
            .collect();
        wallet.pending_tokens.clear();

        let json = serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)?;

        let updated_state = summarize_wallet(&json, &state.wallet_name)?;

        Ok(TransferResult {
            state: updated_state,
            tokens_b64,
        })
    })
}

/// Import received tokens (base64-encoded protobuf) into the wallet.
pub fn receive_tokens(
    state: WalletState,
    tokens_b64: Vec<String>,
) -> Result<WalletState, WalletError> {
    use briolette_proto::briolette::token::Token;

    let mut wallet: briolette_wallet::WalletData =
        serde_json::from_str(&state.json)
            .map_err(|_| WalletError::SerializationError)?;

    for b64 in &tokens_b64 {
        let bytes = B64
            .decode(b64)
            .map_err(|_| WalletError::InvalidData)?;

        // Decode the token protobuf and convert via From<Token> impl.
        let token = Token::decode(bytes.as_slice())
            .map_err(|_| WalletError::InvalidData)?;
        wallet.tokens.push(briolette_wallet::TokenEntry::from(token));
    }

    let json = serde_json::to_string(&wallet)
        .map_err(|_| WalletError::SerializationError)?;

    summarize_wallet(&json, &state.wallet_name)
}

/// Validate all held tokens with the network.
pub fn validate_tokens(
    state: WalletState,
    _validate_uri: String,
) -> Result<ValidationResult, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        use briolette_wallet::Wallet;

        let wallet: briolette_wallet::WalletData =
            serde_json::from_str(&state.json)
                .map_err(|_| WalletError::SerializationError)?;

        let total = wallet.tokens.len() as u32;

        if !wallet.validate().await {
            return Err(WalletError::ValidationFailed);
        }

        let valid_count = wallet.tokens.len() as u32;
        let invalid_count = total.saturating_sub(valid_count);

        let json = serde_json::to_string(&wallet)
            .map_err(|_| WalletError::SerializationError)?;

        let updated_state = summarize_wallet(&json, &state.wallet_name)?;

        Ok(ValidationResult {
            state: updated_state,
            all_valid: invalid_count == 0,
            valid_count,
            invalid_count,
        })
    })
}

/// Get a receiving ticket as base64 for QR code display.
///
/// Extracts the first ticket's raw bytes from the wallet JSON and
/// returns them as base64. The ticket bytes are the serialized
/// SignedTicket protobuf that a sender needs to target their payment.
pub fn get_receiving_ticket_b64(state: WalletState) -> Result<String, WalletError> {
    let v: serde_json::Value = serde_json::from_str(&state.json)
        .map_err(|_| WalletError::SerializationError)?;

    let ticket_arr = v
        .get("tickets")
        .and_then(|t| t.as_array())
        .ok_or(WalletError::NoTicketsAvailable)?;

    let first = ticket_arr.first().ok_or(WalletError::NoTicketsAvailable)?;

    // The "ticket" field is a JSON array of bytes (Vec<u8> serialized by serde).
    let ticket_bytes: Vec<u8> = first
        .get("ticket")
        .and_then(|t| serde_json::from_value(t.clone()).ok())
        .ok_or(WalletError::InvalidData)?;

    Ok(B64.encode(&ticket_bytes))
}

/// Compute the wallet balance from state.
pub fn get_balance(state: WalletState) -> Balance {
    state.balance.clone()
}

/// Get the number of available tickets.
pub fn get_ticket_count(state: WalletState) -> u32 {
    state.ticket_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_empty_wallet_json() {
        let json = r#"{"name":"test","tokens":[],"tickets":[]}"#;
        let state = summarize_wallet(json, "test").unwrap();
        assert_eq!(state.balance.whole, 0);
        assert_eq!(state.balance.token_count, 0);
        assert_eq!(state.ticket_count, 0);
        assert_eq!(state.wallet_name, "test");
    }

    #[test]
    fn summarize_wallet_with_tokens() {
        let json = r#"{
            "name": "alice",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 5, "fractional_value": 0, "value_code": 0},
                {"token": "", "credential": "", "whole_value": 3, "fractional_value": 500000, "value_code": 0}
            ],
            "tickets": [{"ticket": "", "credential": "", "group_number": 0, "created_on": 0, "lifetime": 1, "signature": ""}]
        }"#;
        let state = summarize_wallet(json, "alice").unwrap();
        assert_eq!(state.balance.whole, 8);
        assert_eq!(state.balance.fractional, 500000);
        assert_eq!(state.balance.token_count, 2);
        assert_eq!(state.ticket_count, 1);
    }

    #[test]
    fn balance_extract() {
        let state = WalletState {
            json: "{}".to_string(),
            balance: Balance {
                whole: 42,
                fractional: 0,
                currency: "TEST".to_string(),
                token_count: 3,
            },
            ticket_count: 5,
            wallet_name: "bob".to_string(),
        };
        let b = get_balance(state);
        assert_eq!(b.whole, 42);
        assert_eq!(b.token_count, 3);
    }

    #[test]
    fn ticket_count_extract() {
        let state = WalletState {
            json: "{}".to_string(),
            balance: Balance { whole: 0, fractional: 0, currency: "TEST".to_string(), token_count: 0 },
            ticket_count: 7,
            wallet_name: "carol".to_string(),
        };
        assert_eq!(get_ticket_count(state), 7);
    }

    #[test]
    fn load_wallet_valid_json() {
        let json = r#"{"name":"alice","tokens":[],"tickets":[]}"#.to_string();
        let state = load_wallet(json).unwrap();
        assert_eq!(state.wallet_name, "alice");
        assert_eq!(state.balance.whole, 0);
        assert_eq!(state.balance.token_count, 0);
        assert_eq!(state.ticket_count, 0);
    }

    #[test]
    fn load_wallet_invalid_json_returns_error() {
        let result = load_wallet("not valid json".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn load_wallet_missing_name_defaults_to_unknown() {
        let json = r#"{"tokens":[],"tickets":[]}"#.to_string();
        let state = load_wallet(json).unwrap();
        assert_eq!(state.wallet_name, "unknown");
    }

    #[test]
    fn save_wallet_roundtrip() {
        let json = r#"{"name":"test","tokens":[],"tickets":[]}"#.to_string();
        let state = WalletState {
            json: json.clone(),
            balance: Balance { whole: 0, fractional: 0, currency: "TEST".to_string(), token_count: 0 },
            ticket_count: 0,
            wallet_name: "test".to_string(),
        };
        let saved = save_wallet(state).unwrap();
        assert_eq!(saved, json);
    }

    #[test]
    fn save_wallet_invalid_json_returns_error() {
        let state = WalletState {
            json: "{{broken".to_string(),
            balance: Balance { whole: 0, fractional: 0, currency: "TEST".to_string(), token_count: 0 },
            ticket_count: 0,
            wallet_name: "test".to_string(),
        };
        assert!(save_wallet(state).is_err());
    }

    #[test]
    fn summarize_wallet_fractional_overflow_normalizes() {
        // 2_500_000 micros = 2 whole + 500_000 fractional
        let json = r#"{
            "name": "norm",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 0, "fractional_value": 2500000, "value_code": 0}
            ],
            "tickets": []
        }"#;
        let state = summarize_wallet(json, "norm").unwrap();
        assert_eq!(state.balance.whole, 2);
        assert_eq!(state.balance.fractional, 500_000);
    }

    #[test]
    fn summarize_wallet_multiple_currencies_uses_last() {
        let json = r#"{
            "name": "multi",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 1, "fractional_value": 0, "value_code": 0},
                {"token": "", "credential": "", "whole_value": 2, "fractional_value": 0, "value_code": 840}
            ],
            "tickets": []
        }"#;
        let state = summarize_wallet(json, "multi").unwrap();
        // Last token's code wins
        assert_eq!(state.balance.currency, "USD");
        assert_eq!(state.balance.whole, 3);
    }

    #[test]
    fn summarize_wallet_eth_currency_code() {
        let json = r#"{
            "name": "eth",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 1, "fractional_value": 0, "value_code": 8888}
            ],
            "tickets": []
        }"#;
        let state = summarize_wallet(json, "eth").unwrap();
        assert_eq!(state.balance.currency, "ETH");
    }

    #[test]
    fn summarize_wallet_eur_currency_code() {
        let json = r#"{
            "name": "eur",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 1, "fractional_value": 0, "value_code": 978}
            ],
            "tickets": []
        }"#;
        let state = summarize_wallet(json, "eur").unwrap();
        assert_eq!(state.balance.currency, "EUR");
    }

    #[test]
    fn summarize_wallet_unknown_currency_code() {
        let json = r#"{
            "name": "x",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 1, "fractional_value": 0, "value_code": 9999}
            ],
            "tickets": []
        }"#;
        let state = summarize_wallet(json, "x").unwrap();
        assert_eq!(state.balance.currency, "CODE_9999");
    }

    #[test]
    fn summarize_wallet_invalid_json_returns_error() {
        let result = summarize_wallet("not json", "test");
        assert!(result.is_err());
    }

    #[test]
    fn summarize_wallet_no_tokens_or_tickets_fields() {
        // JSON with no tokens/tickets keys at all
        let json = r#"{"name":"bare"}"#;
        let state = summarize_wallet(json, "bare").unwrap();
        assert_eq!(state.balance.whole, 0);
        assert_eq!(state.balance.token_count, 0);
        assert_eq!(state.ticket_count, 0);
    }

    #[test]
    fn get_receiving_ticket_b64_no_tickets_returns_error() {
        let state = WalletState {
            json: r#"{"tickets":[]}"#.to_string(),
            balance: Balance { whole: 0, fractional: 0, currency: "TEST".to_string(), token_count: 0 },
            ticket_count: 0,
            wallet_name: "test".to_string(),
        };
        assert!(get_receiving_ticket_b64(state).is_err());
    }

    #[test]
    fn get_receiving_ticket_b64_missing_tickets_key_returns_error() {
        let state = WalletState {
            json: r#"{}"#.to_string(),
            balance: Balance { whole: 0, fractional: 0, currency: "TEST".to_string(), token_count: 0 },
            ticket_count: 0,
            wallet_name: "test".to_string(),
        };
        assert!(get_receiving_ticket_b64(state).is_err());
    }

    #[test]
    fn get_receiving_ticket_b64_valid_ticket() {
        let state = WalletState {
            json: r#"{"tickets":[{"ticket":[1,2,3,4]}]}"#.to_string(),
            balance: Balance { whole: 0, fractional: 0, currency: "TEST".to_string(), token_count: 0 },
            ticket_count: 1,
            wallet_name: "test".to_string(),
        };
        let b64 = get_receiving_ticket_b64(state).unwrap();
        // [1,2,3,4] base64-encoded = "AQIDBA=="
        assert_eq!(b64, "AQIDBA==");
    }

    #[test]
    fn receive_tokens_invalid_base64_returns_error() {
        let state = WalletState {
            json: r#"{"name":"test","tokens":[],"tickets":[]}"#.to_string(),
            balance: Balance { whole: 0, fractional: 0, currency: "TEST".to_string(), token_count: 0 },
            ticket_count: 0,
            wallet_name: "test".to_string(),
        };
        let result = receive_tokens(state, vec!["not-base64!!!".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn split_key_protocol_produces_card_bound_challenge() {
        // Simulate the full split-key + attestation protocol using a MockCard.
        use briolette_crypto::v0::split::{MockCard, SmartCard};

        // Step 1: Start
        let step1 = split_key_start(
            "card-test".to_string(),
            "http://[::1]:50051".to_string(),
            "http://[::1]:50052".to_string(),
            "http://[::1]:50053".to_string(),
            "http://[::1]:50055".to_string(),
        ).expect("step1 failed");

        let b_ttc = B64.decode(&step1.b_ttc_b64).unwrap();
        assert!(!b_ttc.is_empty(), "TTC base point should not be empty");

        // Simulate TTC card: PUBLIC_KEY_SHARE + JOIN_COMMIT
        let mut ttc_card = MockCard::new();
        let q_card_ttc = ttc_card.public_key_share(&b_ttc);
        let u_card_ttc = ttc_card.join_commit(&b_ttc).unwrap();

        // Step 2a: After TTC commit
        let step2a = split_key_after_ttc_commit(
            step1.state_json,
            B64.encode(&q_card_ttc),
            B64.encode(&u_card_ttc),
        ).expect("step2a failed");

        // TTC card: JOIN_RESPOND
        let c_ttc = B64.decode(&step2a.c_ttc_b64).unwrap();
        let s_card_ttc = ttc_card.join_respond(&c_ttc).unwrap();

        // Simulate NAC card: PUBLIC_KEY_SHARE + JOIN_COMMIT
        let b_nac = B64.decode(&step2a.b_nac_b64).unwrap();
        let mut nac_card = MockCard::new();
        let q_card_nac = nac_card.public_key_share(&b_nac);
        let u_card_nac = nac_card.join_commit(&b_nac).unwrap();

        // Step 2b: After NAC commit
        let step2b = split_key_after_nac_commit(
            step2a.state_json,
            B64.encode(&q_card_nac),
            B64.encode(&u_card_nac),
        ).expect("step2b failed");

        // NAC card: JOIN_RESPOND
        let c_nac = B64.decode(&step2b.c_nac_b64).unwrap();
        let s_card_nac = nac_card.join_respond(&c_nac).unwrap();

        // Step 3: Complete
        let result = split_key_complete(
            step2b.state_json,
            B64.encode(&s_card_ttc),
            B64.encode(&s_card_nac),
        ).expect("split_key_complete failed");

        assert!(!result.wallet_json.is_empty());
        assert!(!result.challenge_preimage_b64.is_empty());

        // Verify the challenge preimage contains the hw_id and credential PKs
        let preimage = B64.decode(&result.challenge_preimage_b64).unwrap();
        // hw_id is SHA-256("card-test") as hex string bytes
        let hw_id = sha256::digest("card-test".as_bytes()).into_bytes();
        assert!(preimage.starts_with(&hw_id),
            "Challenge preimage must start with hw_id");
        assert!(preimage.len() > hw_id.len(),
            "Challenge preimage must include credential public keys");

        // Verify a second run with a different card produces a different challenge
        let mut ttc_card2 = MockCard::new();
        let q_card_ttc2 = ttc_card2.public_key_share(&b_ttc);
        let u_card_ttc2 = ttc_card2.join_commit(&b_ttc).unwrap();

        let step1_again = split_key_start(
            "card-test".to_string(),
            "http://[::1]:50051".to_string(),
            "http://[::1]:50052".to_string(),
            "http://[::1]:50053".to_string(),
            "http://[::1]:50055".to_string(),
        ).unwrap();

        let step2a_again = split_key_after_ttc_commit(
            step1_again.state_json,
            B64.encode(&q_card_ttc2),
            B64.encode(&u_card_ttc2),
        ).unwrap();

        let c_ttc2 = B64.decode(&step2a_again.c_ttc_b64).unwrap();
        let s_card_ttc2 = ttc_card2.join_respond(&c_ttc2).unwrap();

        let b_nac2 = B64.decode(&step2a_again.b_nac_b64).unwrap();
        let mut nac_card2 = MockCard::new();
        let q_card_nac2 = nac_card2.public_key_share(&b_nac2);
        let u_card_nac2 = nac_card2.join_commit(&b_nac2).unwrap();

        let step2b_again = split_key_after_nac_commit(
            step2a_again.state_json,
            B64.encode(&q_card_nac2),
            B64.encode(&u_card_nac2),
        ).unwrap();

        let c_nac2 = B64.decode(&step2b_again.c_nac_b64).unwrap();
        let s_card_nac2 = nac_card2.join_respond(&c_nac2).unwrap();

        let result2 = split_key_complete(
            step2b_again.state_json,
            B64.encode(&s_card_ttc2),
            B64.encode(&s_card_nac2),
        ).unwrap();

        // Different cards → different combined PKs → different challenge preimage
        assert_ne!(
            result.challenge_preimage_b64,
            result2.challenge_preimage_b64,
            "Different NFC cards must produce different attestation challenges"
        );
    }
}
