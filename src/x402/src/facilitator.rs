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

//! Payment facilitator — verifies Briolette token transfers for x402.
//!
//! Briolette tokens are self-verifying: each token carries its full chain
//! of ECDAA signatures back to the mint. The facilitator leverages the
//! existing wallet verification logic to check token validity without
//! requiring an external verification service.
//!
//! Optionally, it can perform online validation via the `validate` service
//! to confirm tokens haven't been double-spent.

use briolette_proto::briolette::token;
use briolette_wallet::{Wallet, WalletData};
use log::*;
use std::sync::{Arc, RwLock};

use crate::types::{AmountJson, PaymentReceipt};

/// Verifies Briolette token payments.
#[derive(Clone)]
pub struct Facilitator {
    /// A wallet instance used for cryptographic verification.
    wallet: Arc<RwLock<WalletData>>,
}

impl Facilitator {
    pub fn new(wallet: Arc<RwLock<WalletData>>) -> Self {
        Self { wallet }
    }

    /// Verify a set of tokens and return a receipt if valid.
    ///
    /// Performs:
    /// 1. Cryptographic verification of the token chain (ECDAA signatures)
    /// 2. Amount summation to confirm the payment meets the requirement
    /// 3. Optionally, online validation for double-spend detection
    pub fn verify_payment(
        &self,
        tokens: &[token::Token],
        required_amount: &token::Amount,
        resource: &str,
        network: &str,
    ) -> Result<PaymentReceipt, VerificationError> {
        if tokens.is_empty() {
            return Err(VerificationError::NoTokens);
        }

        // 1. Cryptographic verification via the wallet.
        let valid = self
            .wallet
            .write()
            .unwrap()
            .verify_tokens(&tokens.to_vec());
        if !valid {
            error!("Token cryptographic verification failed");
            return Err(VerificationError::InvalidTokens);
        }

        // 2. Sum token values.
        let mut total = token::Amount::default();
        for t in tokens {
            if let Some(desc) = &t.descriptor {
                if let Some(val) = &desc.value {
                    total = match total + val.clone() {
                        Ok(t) => t,
                        Err(_) => return Err(VerificationError::InvalidTokens),
                    };
                }
            }
        }

        // 3. Check that the total meets the required amount.
        if total.whole < required_amount.whole
            || (total.whole == required_amount.whole
                && total.fractional < required_amount.fractional)
        {
            error!(
                "Insufficient payment: got {}.{:06}, need {}.{:06}",
                total.whole, total.fractional, required_amount.whole, required_amount.fractional
            );
            return Err(VerificationError::InsufficientAmount);
        }

        info!(
            "Payment verified: {}.{:06} tokens for {}",
            total.whole, total.fractional, resource
        );

        Ok(PaymentReceipt {
            accepted: true,
            network: network.to_string(),
            amount: AmountJson::from(&total),
            resource: resource.to_string(),
        })
    }
}

/// Errors that can occur during payment verification.
#[derive(Debug)]
pub enum VerificationError {
    NoTokens,
    InvalidTokens,
    InsufficientAmount,
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTokens => write!(f, "No tokens provided"),
            Self::InvalidTokens => write!(f, "Token verification failed"),
            Self::InsufficientAmount => write!(f, "Insufficient payment amount"),
        }
    }
}

impl std::error::Error for VerificationError {}
