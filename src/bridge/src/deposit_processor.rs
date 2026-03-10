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

//! Deposit-to-Mint pipeline.
//!
//! The `DepositProcessor` polls the Ethereum bridge contract for new deposit
//! events and triggers Briolette token minting for each unprocessed deposit.
//!
//! Flow:
//! 1. Poll L1 contract for unprocessed deposits
//! 2. For each deposit, convert wei → Briolette Amount (ETH currency type)
//! 3. Look up the ticket matching the deposit's ticket_hash
//! 4. Call the Briolette Mint service to create tokens
//! 5. Mark the deposit as processed on L1

use crate::ethereum::EthereumClient;
use briolette_proto::briolette::mint::mint_client::MintClient;
use briolette_proto::briolette::mint::GetTokensRequest;
use briolette_proto::briolette::token;
use briolette_proto::briolette::Version;
use log::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{self, Duration};

/// Wei-to-Briolette conversion constants.
/// 1 ETH = 10^18 wei = 1 whole unit + 0 fractional micros.
const WEI_PER_WHOLE: u64 = 1_000_000_000_000_000_000;
const WEI_PER_MICRO: u64 = 1_000_000_000_000;

/// Maps ticket hashes to signed tickets for deposit fulfillment.
/// Wallets register their ticket with the bridge before depositing on L1.
pub struct TicketRegistry {
    /// ticket_hash (SHA256 of SignedTicket bytes) -> SignedTicket
    tickets: HashMap<[u8; 32], token::SignedTicket>,
}

impl TicketRegistry {
    pub fn new() -> Self {
        Self {
            tickets: HashMap::new(),
        }
    }

    /// Register a ticket for deposit fulfillment.
    pub fn register(&mut self, ticket: token::SignedTicket) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let ticket_bytes = prost::Message::encode_to_vec(&ticket);
        let hash: [u8; 32] = Sha256::digest(&ticket_bytes).into();
        self.tickets.insert(hash, ticket);
        hash
    }

    /// Look up a ticket by its hash.
    pub fn get(&self, hash: &[u8; 32]) -> Option<&token::SignedTicket> {
        self.tickets.get(hash)
    }

    /// Remove a ticket after it has been used.
    pub fn remove(&mut self, hash: &[u8; 32]) -> Option<token::SignedTicket> {
        self.tickets.remove(hash)
    }
}

/// Converts a wei amount to Briolette Amount with ETH currency type.
pub fn wei_to_briolette_amount(wei: u64) -> token::Amount {
    let whole = (wei / WEI_PER_WHOLE) as i32;
    let remainder = wei % WEI_PER_WHOLE;
    let fractional = (remainder / WEI_PER_MICRO) as i32;

    token::Amount {
        whole,
        fractional,
        code: token::AmountType::Eth as i32,
    }
}

/// Configuration for the deposit processor.
pub struct DepositProcessorConfig {
    /// URI of the Briolette Mint gRPC service.
    pub mint_uri: String,
    /// Polling interval for checking new deposits.
    pub poll_interval: Duration,
    /// Number of tokens to mint per deposit (each token = 1 unit of the amount).
    /// For simplicity, we mint a single token per deposit with the full amount.
    pub tokens_per_deposit: u32,
}

impl Default for DepositProcessorConfig {
    fn default() -> Self {
        Self {
            mint_uri: "http://127.0.0.1:50054".to_string(),
            poll_interval: Duration::from_secs(12), // ~1 Ethereum block
            tokens_per_deposit: 1,
        }
    }
}

/// The deposit processor polls L1 for deposits and triggers minting.
pub struct DepositProcessor {
    eth_client: Arc<dyn EthereumClient>,
    ticket_registry: Arc<RwLock<TicketRegistry>>,
    config: DepositProcessorConfig,
    /// Last deposit ID we've processed (acts as cursor).
    last_processed_id: u64,
}

impl DepositProcessor {
    pub fn new(
        eth_client: Arc<dyn EthereumClient>,
        ticket_registry: Arc<RwLock<TicketRegistry>>,
        config: DepositProcessorConfig,
    ) -> Self {
        Self {
            eth_client,
            ticket_registry,
            config,
            last_processed_id: 0,
        }
    }

    /// Run the deposit processing loop. This should be spawned as a background task.
    pub async fn run(&mut self) {
        info!(
            "deposit processor starting, mint_uri={}",
            self.config.mint_uri
        );

        let mut interval = time::interval(self.config.poll_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.process_pending_deposits().await {
                error!("deposit processing error: {}", e);
            }
        }
    }

    /// Process all pending deposits in a single pass.
    async fn process_pending_deposits(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let deposits = self
            .eth_client
            .get_deposits(self.last_processed_id)
            .await?;

        if deposits.is_empty() {
            return Ok(());
        }

        info!("found {} unprocessed deposits", deposits.len());

        for deposit in deposits {
            match self.process_single_deposit(&deposit).await {
                Ok(()) => {
                    info!(
                        "deposit {} processed: {} wei minted as ETH tokens",
                        deposit.deposit_id, deposit.amount_wei
                    );
                    // Update cursor past this deposit
                    if deposit.deposit_id >= self.last_processed_id {
                        self.last_processed_id = deposit.deposit_id + 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "failed to process deposit {}: {}",
                        deposit.deposit_id, e
                    );
                    // Don't advance cursor — retry on next poll
                }
            }
        }

        Ok(())
    }

    /// Process a single deposit: look up ticket, mint tokens, mark processed.
    async fn process_single_deposit(
        &self,
        deposit: &crate::ethereum::L1DepositEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Look up the ticket for this deposit.
        let ticket = {
            let registry = self.ticket_registry.read().await;
            registry
                .get(&deposit.ticket_hash)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "no ticket registered for hash {:?} (deposit {})",
                        deposit.ticket_hash, deposit.deposit_id
                    )
                })?
        };

        // 2. Convert wei to Briolette amount.
        let amount = wei_to_briolette_amount(deposit.amount_wei);

        trace!(
            "deposit {}: {} wei -> {} whole + {} fractional ETH",
            deposit.deposit_id,
            deposit.amount_wei,
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
            "minted {} tokens for deposit {}",
            reply.tokens.len(),
            deposit.deposit_id
        );

        // 4. Mark deposit as processed on L1.
        self.eth_client
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
    fn test_wei_to_briolette_amount_whole_eth() {
        let amount = wei_to_briolette_amount(1_000_000_000_000_000_000); // 1 ETH
        assert_eq!(amount.whole, 1);
        assert_eq!(amount.fractional, 0);
        assert_eq!(amount.code, token::AmountType::Eth as i32);
    }

    #[test]
    fn test_wei_to_briolette_amount_fractional() {
        // 0.5 ETH = 500_000_000_000_000_000 wei
        let amount = wei_to_briolette_amount(500_000_000_000_000_000);
        assert_eq!(amount.whole, 0);
        assert_eq!(amount.fractional, 500_000); // 0.5 = 500,000 micros
        assert_eq!(amount.code, token::AmountType::Eth as i32);
    }

    #[test]
    fn test_wei_to_briolette_amount_mixed() {
        // 2.123456 ETH
        let wei = 2_123_456_000_000_000_000u64;
        let amount = wei_to_briolette_amount(wei);
        assert_eq!(amount.whole, 2);
        assert_eq!(amount.fractional, 123_456);
    }

    #[test]
    fn test_wei_to_briolette_amount_zero() {
        let amount = wei_to_briolette_amount(0);
        assert_eq!(amount.whole, 0);
        assert_eq!(amount.fractional, 0);
    }

    #[test]
    fn test_ticket_registry_roundtrip() {
        let mut registry = TicketRegistry::new();

        let ticket = token::SignedTicket {
            ticket: Some(token::Ticket {
                credential: vec![1, 2, 3],
                tags: Some(token::TicketData {
                    group_number: 0,
                    lifetime: 1,
                    created_on: 100,
                }),
            }),
            signature: vec![4, 5, 6],
        };

        let hash = registry.register(ticket.clone());
        assert!(registry.get(&hash).is_some());

        let retrieved = registry.get(&hash).unwrap();
        assert_eq!(retrieved.signature, ticket.signature);

        let removed = registry.remove(&hash);
        assert!(removed.is_some());
        assert!(registry.get(&hash).is_none());
    }

    #[test]
    fn test_ticket_registry_unknown_hash() {
        let registry = TicketRegistry::new();
        assert!(registry.get(&[0u8; 32]).is_none());
    }
}
