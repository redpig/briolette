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

//! x402 client — handles HTTP 402 responses by constructing and sending payments.
//!
//! When an HTTP request receives a 402 response, this client:
//! 1. Parses the `PaymentRequirements` from the response
//! 2. Uses the wallet to prepare a token transfer to the `pay_to` ticket
//! 3. Retries the request with an `X-PAYMENT` header containing the payment

use crate::scheme;
use crate::types::{BriolettePaymentData, PaymentPayload, PaymentRequirements};
use briolette_proto::briolette::token;
use briolette_wallet::{Wallet, WalletData};
use log::*;
use prost::Message;
use std::sync::{Arc, RwLock};

/// An x402-aware HTTP client that automatically handles payment challenges.
pub struct X402Client {
    wallet: Arc<RwLock<WalletData>>,
    http_client: reqwest::Client,
    network: String,
}

impl X402Client {
    pub fn new(wallet: Arc<RwLock<WalletData>>, network: String) -> Self {
        Self {
            wallet,
            http_client: reqwest::Client::new(),
            network,
        }
    }

    /// Make an HTTP GET request, automatically handling 402 payment challenges.
    ///
    /// If the server returns 402, this method will:
    /// 1. Parse the payment requirements
    /// 2. Prepare a token transfer
    /// 3. Retry with the payment header
    ///
    /// Returns the final response (either the original non-402, or the retry result).
    pub async fn get(&self, url: &str) -> Result<reqwest::Response, X402Error> {
        let response = self
            .http_client
            .get(url)
            .send()
            .await
            .map_err(X402Error::Http)?;

        if response.status() != reqwest::StatusCode::PAYMENT_REQUIRED {
            return Ok(response);
        }

        info!("Received 402, attempting payment for {}", url);

        // Parse payment requirements from the response body.
        let requirements: PaymentRequirements = response.json().await.map_err(|e| {
            X402Error::Protocol(format!("Failed to parse payment requirements: {}", e))
        })?;

        // Prepare the payment.
        let payment = self.prepare_payment(&requirements)?;
        let payment_json =
            serde_json::to_string(&payment).map_err(|e| X402Error::Protocol(e.to_string()))?;

        // Retry with payment header.
        let retry_response = self
            .http_client
            .get(url)
            .header(scheme::PAYMENT_HEADER, &payment_json)
            .send()
            .await
            .map_err(X402Error::Http)?;

        Ok(retry_response)
    }

    /// Prepare a payment payload for the given requirements.
    fn prepare_payment(
        &self,
        requirements: &PaymentRequirements,
    ) -> Result<PaymentPayload, X402Error> {
        // Extract the recipient ticket from the requirements.
        let ticket_bytes = serde_json::to_vec(&requirements.pay_to)
            .map_err(|e| X402Error::Protocol(format!("Invalid pay_to ticket: {}", e)))?;

        let amount = requirements.max_amount_required.whole as u32;

        // Use the wallet to prepare the transfer.
        {
            let mut wallet = self.wallet.write().unwrap();
            if !wallet.transfer(amount, ticket_bytes) {
                return Err(X402Error::InsufficientFunds);
            }
        }

        // Collect the pending tokens.
        let tokens: Vec<serde_json::Value> = {
            let wallet = self.wallet.read().unwrap();
            wallet
                .pending_tokens
                .iter()
                .map(|t| {
                    let token = token::Token::decode(t.as_slice())
                        .expect("pending token should be valid protobuf");
                    serde_json::to_value(&token).expect("token should serialize to JSON")
                })
                .collect()
        };

        Ok(PaymentPayload {
            scheme: scheme::SCHEME_NAME.to_string(),
            network: self.network.clone(),
            payload: BriolettePaymentData { tokens },
        })
    }
}

/// Errors from x402 client operations.
#[derive(Debug)]
pub enum X402Error {
    Http(reqwest::Error),
    Protocol(String),
    InsufficientFunds,
}

impl std::fmt::Display for X402Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {}", e),
            Self::Protocol(s) => write!(f, "Protocol error: {}", s),
            Self::InsufficientFunds => write!(f, "Insufficient funds in wallet"),
        }
    }
}

impl std::error::Error for X402Error {}
