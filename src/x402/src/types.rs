// Copyright 2025 The Briolette Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! x402 protocol types for the Briolette payment scheme.
//!
//! These types define how payment requirements, payloads, and receipts
//! are represented in HTTP headers and JSON bodies.

use briolette_proto::briolette::token;
use serde::{Deserialize, Serialize};

/// Payment requirements returned in a 402 response.
///
/// Tells the client what payment is needed to access the resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    /// Scheme identifier — always "briolette" for this implementation.
    pub scheme: String,
    /// Network identifier (e.g., "testnet", operator name).
    pub network: String,
    /// The recipient's SignedTicket, serialized as JSON.
    /// Tokens must be transferred to this ticket to complete payment.
    pub pay_to: serde_json::Value,
    /// Maximum amount required, as an Amount JSON object.
    pub max_amount_required: AmountJson,
    /// The resource path being paywalled.
    pub resource: String,
    /// Human-readable description of what's being paid for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Additional Briolette-specific details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<BriolettePaymentDetails>,
}

/// Briolette-specific payment details included in requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriolettePaymentDetails {
    /// Current epoch for the receiver.
    pub epoch: u64,
    /// Accepted mint public keys (empty = accept default mint).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_mint_keys: Vec<serde_json::Value>,
}

/// JSON-friendly representation of a Briolette Amount.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmountJson {
    pub whole: i32,
    pub fractional: i32,
    pub code: i32,
}

impl From<&token::Amount> for AmountJson {
    fn from(a: &token::Amount) -> Self {
        Self {
            whole: a.whole,
            fractional: a.fractional,
            code: a.code,
        }
    }
}

impl From<token::Amount> for AmountJson {
    fn from(a: token::Amount) -> Self {
        AmountJson::from(&a)
    }
}

/// Payment payload sent by the client in the X-PAYMENT header.
///
/// Contains the signed token transfer proving payment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    /// Scheme identifier — must be "briolette".
    pub scheme: String,
    /// Network identifier — must match the requirements.
    pub network: String,
    /// The actual payment data.
    pub payload: BriolettePaymentData,
}

/// The Briolette-specific payment data within a PaymentPayload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriolettePaymentData {
    /// The transferred tokens, serialized as JSON (using serde-derived Token types).
    pub tokens: Vec<serde_json::Value>,
}

/// Receipt returned after successful payment verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentReceipt {
    /// Whether the payment was accepted.
    pub accepted: bool,
    /// Network the payment was verified on.
    pub network: String,
    /// Amount that was paid.
    pub amount: AmountJson,
    /// The resource that was unlocked.
    pub resource: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_requirements_round_trip() {
        let req = PaymentRequirements {
            scheme: "briolette".to_string(),
            network: "testnet".to_string(),
            pay_to: serde_json::json!({"ticket": "..."}),
            max_amount_required: AmountJson {
                whole: 5,
                fractional: 0,
                code: 0,
            },
            resource: "/api/data".to_string(),
            description: Some("Access to data endpoint".to_string()),
            extra: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: PaymentRequirements = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.scheme, "briolette");
        assert_eq!(decoded.max_amount_required.whole, 5);
    }

    #[test]
    fn payment_payload_round_trip() {
        let payload = PaymentPayload {
            scheme: "briolette".to_string(),
            network: "testnet".to_string(),
            payload: BriolettePaymentData {
                tokens: vec![serde_json::json!({"descriptor": {"version": 0}})],
            },
        };
        let json = serde_json::to_string(&payload).unwrap();
        let decoded: PaymentPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.payload.tokens.len(), 1);
    }
}
