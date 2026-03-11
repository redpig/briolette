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
}
