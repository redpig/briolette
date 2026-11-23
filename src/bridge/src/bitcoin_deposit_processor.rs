// Copyright 2023 The Briolette Authors.
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

//! Bitcoin Deposit-to-Mint pipeline.
//!
//! The `BitcoinDepositProcessor` polls Bitcoin for new deposits (UTXOs sent
//! to the bridge Taproot address) and triggers Briolette token minting.
//!
//! This is the Bitcoin equivalent of `deposit_processor.rs` (Ethereum).
//!
//! Key differences from the Ethereum deposit processor:
//!
//! | Aspect             | Ethereum                          | Bitcoin                          |
//! |--------------------|-----------------------------------|----------------------------------|
//! | Deposit detection  | Contract events via JSON-RPC      | UTXO scanning via Bitcoin RPC    |
//! | Confirmation model | Block number + finality           | 6+ confirmations (configurable)  |
//! | Amount unit        | Wei (10^18 per ETH)               | Satoshis (10^8 per BTC)          |
//! | Processed marker   | On-chain `markDepositProcessed()` | Local state (no on-chain cost)   |
//! | Polling interval   | ~12s (1 ETH block)                | ~60s (checking for new blocks)   |
//! | Ticket hash source | Contract event field              | OP_RETURN output in deposit tx   |

use crate::deposit_processor::TicketRegistry;
use crate::l1::L1Client;
use briolette_proto::briolette::mint::mint_client::MintClient;
use briolette_proto::briolette::mint::GetTokensRequest;
use briolette_proto::briolette::token;
use briolette_proto::briolette::Version;
use log::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{self, Duration};

/// Satoshis per whole BTC unit.
const SATS_PER_WHOLE: u64 = 100_000_000;
/// Satoshis per micro-BTC (for fractional Briolette amounts).
/// 1 micro-BTC = 100 satoshis.
const SATS_PER_MICRO: u64 = 100;

/// Converts a satoshi amount to Briolette Amount with BTC currency type.
///
/// Maps satoshis to Briolette's Amount struct:
///   - whole: number of full BTC
///   - fractional: remaining micro-BTC (1 micro = 100 sats)
///   - code: AmountType::Btc
///
/// Note: AmountType::Btc needs to be added to the proto enum. For this
/// reference implementation, we use a placeholder value (2).
pub fn sats_to_briolette_amount(sats: u64) -> token::Amount {
    let whole = (sats / SATS_PER_WHOLE) as i32;
    let remainder = sats % SATS_PER_WHOLE;
    let fractional = (remainder / SATS_PER_MICRO) as i32;

    token::Amount {
        whole,
        fractional,
        // AmountType::Btc - needs proto enum extension.
        // Using value 2 as placeholder (0=Unspecified, 1=Eth).
        code: 2,
    }
}

/// Configuration for the Bitcoin deposit processor.
pub struct BitcoinDepositProcessorConfig {
    /// URI of the Briolette Mint gRPC service.
    pub mint_uri: String,
    /// Polling interval for checking new Bitcoin blocks.
    /// Bitcoin blocks arrive ~every 10 minutes, so we poll more frequently
    /// to catch confirmations as they accumulate.
    pub poll_interval: Duration,
    /// Number of tokens to mint per deposit.
    pub tokens_per_deposit: u32,
}

impl Default for BitcoinDepositProcessorConfig {
    fn default() -> Self {
        Self {
            mint_uri: "http://127.0.0.1:50054".to_string(),
            poll_interval: Duration::from_secs(60), // Check every minute
            tokens_per_deposit: 1,
        }
    }
}

/// The Bitcoin deposit processor polls for BTC deposits and triggers minting.
pub struct BitcoinDepositProcessor {
    l1_client: Arc<dyn L1Client>,
    ticket_registry: Arc<RwLock<TicketRegistry>>,
    config: BitcoinDepositProcessorConfig,
    /// Last block height we've scanned.
    last_scanned_height: u64,
}

impl BitcoinDepositProcessor {
    pub fn new(
        l1_client: Arc<dyn L1Client>,
        ticket_registry: Arc<RwLock<TicketRegistry>>,
        config: BitcoinDepositProcessorConfig,
    ) -> Self {
        Self {
            l1_client,
            ticket_registry,
            config,
            last_scanned_height: 0,
        }
    }

    /// Run the deposit processing loop.
    pub async fn run(&mut self) {
        info!(
            "Bitcoin deposit processor starting, mint_uri={}, poll_interval={:?}",
            self.config.mint_uri, self.config.poll_interval
        );

        let mut interval = time::interval(self.config.poll_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.process_pending_deposits().await {
                error!("Bitcoin deposit processing error: {}", e);
            }
        }
    }

    /// Process all pending deposits in a single pass.
    async fn process_pending_deposits(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let deposits = self
            .l1_client
            .get_deposits(self.last_scanned_height)
            .await?;

        if deposits.is_empty() {
            return Ok(());
        }

        info!("found {} confirmed Bitcoin deposits", deposits.len());

        for deposit in deposits {
            match self.process_single_deposit(&deposit).await {
                Ok(()) => {
                    info!(
                        "Bitcoin deposit {} processed: {} sats minted as BTC tokens",
                        deposit.deposit_id, deposit.amount
                    );
                    if deposit.block_height >= self.last_scanned_height {
                        self.last_scanned_height = deposit.block_height + 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "failed to process Bitcoin deposit {}: {}",
                        deposit.deposit_id, e
                    );
                }
            }
        }

        Ok(())
    }

    /// Process a single deposit: look up ticket, mint tokens, mark processed.
    async fn process_single_deposit(
        &self,
        deposit: &crate::l1::L1Deposit,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Look up the ticket for this deposit.
        let ticket = {
            let registry = self.ticket_registry.read().await;
            registry
                .get(&deposit.ticket_hash)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "no ticket registered for hash {:?} (Bitcoin deposit {})",
                        &deposit.ticket_hash[..8],
                        deposit.deposit_id
                    )
                })?
        };

        // 2. Convert satoshis to Briolette amount.
        let amount = sats_to_briolette_amount(deposit.amount);

        trace!(
            "Bitcoin deposit {}: {} sats -> {} whole + {} fractional BTC",
            deposit.deposit_id,
            deposit.amount,
            amount.whole,
            amount.fractional
        );

        // 3. Call the Mint service to create tokens.
        let mut mint_client = MintClient::connect(self.config.mint_uri.clone())
            .await
            .map_err(|e| format!("mint connection failed: {}", e))?;

        let mint_request = GetTokensRequest {
            version: Version::Current as i32,
            amount: Some(amount),
            tags: vec![],
            count: self.config.tokens_per_deposit,
            ticket: Some(ticket),
        };

        let response = mint_client
            .get_tokens(mint_request)
            .await
            .map_err(|e| format!("mint request failed: {}", e))?;

        let reply = response.into_inner();
        if reply.tokens.is_empty() {
            return Err("mint returned no tokens".into());
        }

        info!(
            "minted {} tokens for Bitcoin deposit {}",
            reply.tokens.len(),
            deposit.deposit_id
        );

        // 4. Mark deposit as processed (local state only — no on-chain cost!).
        self.l1_client
            .mark_deposit_processed(deposit.deposit_id)
            .await?;

        // 5. Remove the used ticket from the registry.
        {
            let mut registry = self.ticket_registry.write().await;
            registry.remove(&deposit.ticket_hash);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sats_to_briolette_amount_whole_btc() {
        let amount = sats_to_briolette_amount(100_000_000); // 1 BTC
        assert_eq!(amount.whole, 1);
        assert_eq!(amount.fractional, 0);
        assert_eq!(amount.code, 2); // BTC
    }

    #[test]
    fn test_sats_to_briolette_amount_fractional() {
        // 0.5 BTC = 50,000,000 sats
        let amount = sats_to_briolette_amount(50_000_000);
        assert_eq!(amount.whole, 0);
        assert_eq!(amount.fractional, 500_000); // 0.5 = 500,000 micros
    }

    #[test]
    fn test_sats_to_briolette_amount_mixed() {
        // 2.12345600 BTC = 212,345,600 sats
        let amount = sats_to_briolette_amount(212_345_600);
        assert_eq!(amount.whole, 2);
        assert_eq!(amount.fractional, 123_456); // 12,345,600 remaining / 100 = 123,456
    }

    #[test]
    fn test_sats_to_briolette_amount_zero() {
        let amount = sats_to_briolette_amount(0);
        assert_eq!(amount.whole, 0);
        assert_eq!(amount.fractional, 0);
    }

    #[test]
    fn test_sats_to_briolette_amount_dust() {
        // 546 sats (dust limit) = 0.00000546 BTC
        let amount = sats_to_briolette_amount(546);
        assert_eq!(amount.whole, 0);
        assert_eq!(amount.fractional, 5); // 546 / 100 = 5 micros
    }

    #[test]
    fn test_sats_to_briolette_amount_large() {
        // 21,000,000 BTC = 2,100,000,000,000,000 sats (max supply)
        let amount = sats_to_briolette_amount(2_100_000_000_000_000);
        assert_eq!(amount.whole, 21_000_000);
        assert_eq!(amount.fractional, 0);
    }
}
