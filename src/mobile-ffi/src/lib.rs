//! UniFFI bridge for Briolette wallet operations.
//!
//! This is a thin synchronous wrapper over `briolette-integration`, which
//! provides the actual wallet logic. Each function blocks on a tokio runtime
//! to call the async integration API, then converts types to the UniFFI-
//! compatible structs defined in `briolette.udl`.
//!
//! Application developers who want a native Rust (async) API should depend
//! on `briolette-integration` directly instead of this crate.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use briolette_integration as bri;

uniffi::include_scaffolding!("briolette");

// ---------------------------------------------------------------------------
// Error type (must match UDL)
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

impl From<bri::Error> for WalletError {
    fn from(e: bri::Error) -> Self {
        match e {
            bri::Error::NotInitialized => WalletError::NotInitialized,
            bri::Error::Network(_) => WalletError::NetworkError,
            bri::Error::InsufficientFunds { .. } => WalletError::InsufficientFunds,
            bri::Error::NoTicketsAvailable => WalletError::NoTicketsAvailable,
            bri::Error::InvalidData(_) => WalletError::InvalidData,
            bri::Error::Serialization(_) => WalletError::SerializationError,
            bri::Error::ValidationFailed => WalletError::ValidationFailed,
            bri::Error::AlreadyRegistered => WalletError::InvalidData,
        }
    }
}

// ---------------------------------------------------------------------------
// Data types (must match UDL)
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

#[derive(Debug, Clone)]
pub struct KeyInitResult {
    pub wallet_json: String,
    pub challenge_preimage_b64: String,
    pub nac_card_public_key_b64: String,
    pub ttc_card_public_key_b64: String,
}

#[derive(Debug, Clone)]
pub struct AttestationData {
    pub algorithm: i32,
    pub signature_b64: String,
    pub public_key_b64: String,
}

#[derive(Debug, Clone)]
pub struct SplitKeyStep1Result {
    pub state_json: String,
    pub b_ttc_b64: String,
}

#[derive(Debug, Clone)]
pub struct SplitKeyStep2aResult {
    pub state_json: String,
    pub c_ttc_b64: String,
    pub b_nac_b64: String,
}

#[derive(Debug, Clone)]
pub struct SplitKeyStep2bResult {
    pub state_json: String,
    pub c_nac_b64: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn runtime() -> Result<tokio::runtime::Runtime, WalletError> {
    tokio::runtime::Runtime::new().map_err(|_| WalletError::NetworkError)
}

fn to_wallet_state(client: &bri::BrioletteClient) -> Result<WalletState, WalletError> {
    let bal = client.balance();
    Ok(WalletState {
        json: client.to_json().map_err(WalletError::from)?,
        balance: Balance {
            whole: bal.whole,
            fractional: bal.fractional,
            currency: bal.currency,
            token_count: bal.token_count,
        },
        ticket_count: client.ticket_count(),
        wallet_name: client.name().to_string(),
    })
}

fn from_wallet_state(state: &WalletState) -> Result<bri::BrioletteClient, WalletError> {
    bri::BrioletteClient::from_json(&state.json).map_err(WalletError::from)
}

fn config_from(
    registrar_uri: &str,
    clerk_uri: &str,
    mint_uri: &str,
    validate_uri: &str,
) -> bri::ServiceConfig {
    bri::ServiceConfig {
        registrar_uri: registrar_uri.to_string(),
        clerk_uri: clerk_uri.to_string(),
        mint_uri: mint_uri.to_string(),
        validate_uri: validate_uri.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Public FFI functions
// ---------------------------------------------------------------------------

pub fn create_wallet(
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
) -> Result<String, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        let config = config_from(&registrar_uri, &clerk_uri, &mint_uri, &validate_uri);
        let client = bri::BrioletteClient::create(&name, &config).await
            .map_err(WalletError::from)?;
        client.to_json().map_err(WalletError::from)
    })
}

pub fn init_wallet_keys(
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
) -> Result<KeyInitResult, WalletError> {
    let config = config_from(&registrar_uri, &clerk_uri, &mint_uri, &validate_uri);
    let result = bri::BrioletteClient::init_keys(&name, &config)
        .map_err(WalletError::from)?;

    Ok(KeyInitResult {
        wallet_json: result.client.to_json().map_err(WalletError::from)?,
        challenge_preimage_b64: B64.encode(&result.challenge_preimage),
        nac_card_public_key_b64: String::new(),
        ttc_card_public_key_b64: String::new(),
    })
}

pub fn register_wallet_with_attestation(
    wallet_json: String,
    attestation: AttestationData,
    nac_card_public_key_b64: String,
    ttc_card_public_key_b64: String,
    card_attestation_b64: String,
) -> Result<String, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        let client = bri::BrioletteClient::from_json(&wallet_json)
            .map_err(WalletError::from)?;

        let att = bri::Attestation {
            algorithm: attestation.algorithm,
            signature: B64.decode(&attestation.signature_b64)
                .map_err(|_| WalletError::InvalidData)?,
            public_key: B64.decode(&attestation.public_key_b64)
                .map_err(|_| WalletError::InvalidData)?,
        };

        let proof = if !nac_card_public_key_b64.is_empty()
            && !ttc_card_public_key_b64.is_empty()
        {
            Some(bri::SplitKeyProof {
                nac_card_public_key: B64.decode(&nac_card_public_key_b64)
                    .map_err(|_| WalletError::InvalidData)?,
                ttc_card_public_key: B64.decode(&ttc_card_public_key_b64)
                    .map_err(|_| WalletError::InvalidData)?,
            })
        } else {
            None
        };

        let card_attest = if !card_attestation_b64.is_empty() {
            Some(B64.decode(&card_attestation_b64)
                .map_err(|_| WalletError::InvalidData)?)
        } else {
            None
        };

        let registered = client
            .register_with_attestation_and_card(
                &att,
                proof.as_ref(),
                card_attest.as_deref(),
            )
            .await
            .map_err(WalletError::from)?;

        registered.to_json().map_err(WalletError::from)
    })
}

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
        let config = config_from(&registrar_uri, &clerk_uri, &mint_uri, &validate_uri);
        let init = bri::BrioletteClient::init_keys(&name, &config)
            .map_err(WalletError::from)?;

        let att = bri::Attestation {
            algorithm: attestation.algorithm,
            signature: B64.decode(&attestation.signature_b64)
                .map_err(|_| WalletError::InvalidData)?,
            public_key: B64.decode(&attestation.public_key_b64)
                .map_err(|_| WalletError::InvalidData)?,
        };

        let registered = init.client
            .register_with_attestation(&att, None)
            .await
            .map_err(WalletError::from)?;

        registered.to_json().map_err(WalletError::from)
    })
}

pub fn load_wallet(json: String) -> Result<WalletState, WalletError> {
    let client = bri::BrioletteClient::from_json(&json).map_err(WalletError::from)?;
    to_wallet_state(&client)
}

pub fn save_wallet(state: WalletState) -> Result<String, WalletError> {
    let client = from_wallet_state(&state)?;
    client.to_json().map_err(WalletError::from)
}

pub fn synchronize(state: WalletState, _clerk_uri: String) -> Result<WalletState, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        let client = from_wallet_state(&state)?;
        let updated = client.synchronize().await.map_err(WalletError::from)?;
        to_wallet_state(&updated)
    })
}

pub fn request_tickets(
    state: WalletState,
    _clerk_uri: String,
    count: u32,
) -> Result<WalletState, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        let client = from_wallet_state(&state)?;
        let updated = client.request_tickets(count).await.map_err(WalletError::from)?;
        to_wallet_state(&updated)
    })
}

pub fn withdraw(
    state: WalletState,
    _mint_uri: String,
    amount: u32,
) -> Result<WalletState, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        let client = from_wallet_state(&state)?;
        let updated = client.withdraw(amount).await.map_err(WalletError::from)?;
        to_wallet_state(&updated)
    })
}

pub fn transfer_tokens(
    state: WalletState,
    recipient_ticket_b64: String,
    amount: u32,
) -> Result<TransferResult, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        let client = from_wallet_state(&state)?;
        let (updated, tokens_b64) = client
            .transfer_b64(amount, &recipient_ticket_b64)
            .await
            .map_err(WalletError::from)?;
        let ws = to_wallet_state(&updated)?;
        Ok(TransferResult { state: ws, tokens_b64 })
    })
}

pub fn receive_tokens(
    state: WalletState,
    tokens_b64: Vec<String>,
) -> Result<WalletState, WalletError> {
    let client = from_wallet_state(&state)?;
    let updated = client.receive_tokens_b64(&tokens_b64).map_err(WalletError::from)?;
    to_wallet_state(&updated)
}

pub fn validate_tokens(
    state: WalletState,
    _validate_uri: String,
) -> Result<ValidationResult, WalletError> {
    let rt = runtime()?;
    rt.block_on(async {
        let client = from_wallet_state(&state)?;
        let result = client.validate().await.map_err(WalletError::from)?;
        let ws = to_wallet_state(&result.client)?;
        Ok(ValidationResult {
            state: ws,
            all_valid: result.all_valid,
            valid_count: result.valid_count,
            invalid_count: result.invalid_count,
        })
    })
}

pub fn get_receiving_ticket_b64(state: WalletState) -> Result<String, WalletError> {
    let client = from_wallet_state(&state)?;
    client.receiving_ticket_b64().map_err(WalletError::from)
}

pub fn get_balance(state: WalletState) -> Balance {
    state.balance.clone()
}

pub fn get_ticket_count(state: WalletState) -> u32 {
    state.ticket_count
}

// ---------------------------------------------------------------------------
// Split-key protocol
// ---------------------------------------------------------------------------

pub fn split_key_start(
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
) -> Result<SplitKeyStep1Result, WalletError> {
    let config = config_from(&registrar_uri, &clerk_uri, &mint_uri, &validate_uri);
    let step = bri::BrioletteClient::split_key_start(&name, &config)
        .map_err(WalletError::from)?;

    let state_json = serde_json::to_string(&step.state)
        .map_err(|_| WalletError::SerializationError)?;

    Ok(SplitKeyStep1Result {
        state_json,
        b_ttc_b64: B64.encode(&step.b_ttc),
    })
}

pub fn split_key_after_ttc_commit(
    state_json: String,
    q_card_ttc_b64: String,
    u_card_ttc_b64: String,
) -> Result<SplitKeyStep2aResult, WalletError> {
    let state: bri::SplitKeyState = serde_json::from_str(&state_json)
        .map_err(|_| WalletError::SerializationError)?;

    let q = B64.decode(&q_card_ttc_b64).map_err(|_| WalletError::InvalidData)?;
    let u = B64.decode(&u_card_ttc_b64).map_err(|_| WalletError::InvalidData)?;

    let step = bri::BrioletteClient::split_key_after_ttc_commit(&state, &q, &u)
        .map_err(WalletError::from)?;

    let new_state_json = serde_json::to_string(&step.state)
        .map_err(|_| WalletError::SerializationError)?;

    Ok(SplitKeyStep2aResult {
        state_json: new_state_json,
        c_ttc_b64: B64.encode(&step.c_ttc),
        b_nac_b64: B64.encode(&step.b_nac),
    })
}

pub fn split_key_after_nac_commit(
    state_json: String,
    q_card_nac_b64: String,
    u_card_nac_b64: String,
) -> Result<SplitKeyStep2bResult, WalletError> {
    let state: bri::SplitKeyState = serde_json::from_str(&state_json)
        .map_err(|_| WalletError::SerializationError)?;

    let q = B64.decode(&q_card_nac_b64).map_err(|_| WalletError::InvalidData)?;
    let u = B64.decode(&u_card_nac_b64).map_err(|_| WalletError::InvalidData)?;

    let step = bri::BrioletteClient::split_key_after_nac_commit(&state, &q, &u)
        .map_err(WalletError::from)?;

    let new_state_json = serde_json::to_string(&step.state)
        .map_err(|_| WalletError::SerializationError)?;

    Ok(SplitKeyStep2bResult {
        state_json: new_state_json,
        c_nac_b64: B64.encode(&step.c_nac),
    })
}

pub fn split_key_complete(
    state_json: String,
    s_card_ttc_b64: String,
    s_card_nac_b64: String,
) -> Result<KeyInitResult, WalletError> {
    let state: bri::SplitKeyState = serde_json::from_str(&state_json)
        .map_err(|_| WalletError::SerializationError)?;

    let s_ttc = B64.decode(&s_card_ttc_b64).map_err(|_| WalletError::InvalidData)?;
    let s_nac = B64.decode(&s_card_nac_b64).map_err(|_| WalletError::InvalidData)?;

    let result = bri::BrioletteClient::split_key_complete(&state, &s_ttc, &s_nac)
        .map_err(WalletError::from)?;

    Ok(KeyInitResult {
        wallet_json: result.client.to_json().map_err(WalletError::from)?,
        challenge_preimage_b64: B64.encode(&result.challenge_preimage),
        nac_card_public_key_b64: B64.encode(&result.nac_card_public_key),
        ttc_card_public_key_b64: B64.encode(&result.ttc_card_public_key),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_empty_wallet_json() {
        let json = r#"{"name":"test","tokens":[],"tickets":[]}"#.to_string();
        let state = load_wallet(json).unwrap();
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
        }"#.to_string();
        let state = load_wallet(json).unwrap();
        assert_eq!(state.balance.whole, 8);
        assert_eq!(state.balance.fractional, 500000);
        assert_eq!(state.balance.token_count, 2);
        assert_eq!(state.ticket_count, 1);
    }

    #[test]
    fn balance_extract() {
        let state = WalletState {
            json: r#"{"name":"bob","tokens":[],"tickets":[]}"#.to_string(),
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
            json: r#"{"name":"carol","tokens":[],"tickets":[]}"#.to_string(),
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
        let json = r#"{
            "name": "norm",
            "tokens": [
                {"token": "", "credential": "", "whole_value": 0, "fractional_value": 2500000, "value_code": 0}
            ],
            "tickets": []
        }"#.to_string();
        let state = load_wallet(json).unwrap();
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
        }"#.to_string();
        let state = load_wallet(json).unwrap();
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
        }"#.to_string();
        let state = load_wallet(json).unwrap();
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
        }"#.to_string();
        let state = load_wallet(json).unwrap();
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
        }"#.to_string();
        let state = load_wallet(json).unwrap();
        assert_eq!(state.balance.currency, "CODE_9999");
    }

    #[test]
    fn summarize_wallet_invalid_json_returns_error() {
        let result = load_wallet("not json".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn summarize_wallet_no_tokens_or_tickets_fields() {
        let json = r#"{"name":"bare"}"#.to_string();
        let state = load_wallet(json).unwrap();
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
        use briolette_crypto::v1::split::{MockCard, SmartCard};

        // Step 1
        let step1 = split_key_start(
            "card-test".to_string(),
            "http://[::1]:50051".to_string(),
            "http://[::1]:50052".to_string(),
            "http://[::1]:50053".to_string(),
            "http://[::1]:50055".to_string(),
        ).expect("step1 failed");

        let b_ttc = B64.decode(&step1.b_ttc_b64).unwrap();
        assert!(!b_ttc.is_empty());

        // TTC card
        let mut ttc_card = MockCard::new();
        let q_card_ttc = ttc_card.public_key_share(&b_ttc);
        let u_card_ttc = ttc_card.join_commit(&b_ttc).unwrap();

        // Step 2a
        let step2a = split_key_after_ttc_commit(
            step1.state_json,
            B64.encode(&q_card_ttc),
            B64.encode(&u_card_ttc),
        ).expect("step2a failed");

        let c_ttc = B64.decode(&step2a.c_ttc_b64).unwrap();
        let s_card_ttc = ttc_card.join_respond(&c_ttc).unwrap();

        // NAC card
        let b_nac = B64.decode(&step2a.b_nac_b64).unwrap();
        let mut nac_card = MockCard::new();
        let q_card_nac = nac_card.public_key_share(&b_nac);
        let u_card_nac = nac_card.join_commit(&b_nac).unwrap();

        // Step 2b
        let step2b = split_key_after_nac_commit(
            step2a.state_json,
            B64.encode(&q_card_nac),
            B64.encode(&u_card_nac),
        ).expect("step2b failed");

        let c_nac = B64.decode(&step2b.c_nac_b64).unwrap();
        let s_card_nac = nac_card.join_respond(&c_nac).unwrap();

        // Complete
        let result = split_key_complete(
            step2b.state_json,
            B64.encode(&s_card_ttc),
            B64.encode(&s_card_nac),
        ).expect("split_key_complete failed");

        assert!(!result.wallet_json.is_empty());
        assert!(!result.challenge_preimage_b64.is_empty());

        let preimage = B64.decode(&result.challenge_preimage_b64).unwrap();
        let hw_id = sha256::digest("card-test".as_bytes()).into_bytes();
        assert!(preimage.starts_with(&hw_id));
        assert!(preimage.len() > hw_id.len());

        // Second run with different card produces different challenge
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

        assert_ne!(
            result.challenge_preimage_b64,
            result2.challenge_preimage_b64,
            "Different NFC cards must produce different attestation challenges"
        );
    }
}
