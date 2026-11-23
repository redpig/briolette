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

//! Chain-agnostic L1 interaction layer for the Briolette bridge.
//!
//! This module defines the `L1Client` trait which abstracts all L1
//! interactions regardless of the underlying chain (Ethereum, Bitcoin, etc.).
//!
//! Both the Ethereum (`ethereum.rs`) and Bitcoin (`bitcoin.rs`) modules
//! implement this trait, allowing the bridge server to operate with either
//! chain via feature flags.
//!
//! # Architecture
//!
//! ```text
//!                    ┌──────────┐
//!                    │ L1Client │  (this trait)
//!                    └────┬─────┘
//!               ┌─────────┴──────────┐
//!               │                    │
//!     ┌─────────┴────────┐  ┌───────┴──────────┐
//!     │  EthereumClient  │  │  BitcoinClient   │
//!     │  (ethereum.rs)   │  │  (bitcoin.rs)    │
//!     │                  │  │                  │
//!     │  - Solidity ABI  │  │  - Taproot UTXO  │
//!     │  - EVM events    │  │  - OP_RETURN     │
//!     │  - bn256 pairing │  │  - Federated     │
//!     │    fraud proofs  │  │    fraud proofs  │
//!     └──────────────────┘  └──────────────────┘
//! ```
//!
//! # Trade-offs
//!
//! | Aspect              | Ethereum                    | Bitcoin                        |
//! |---------------------|-----------------------------|--------------------------------|
//! | Deposit mechanism   | Solidity `deposit()` call   | Send BTC to Taproot address    |
//! | Withdrawal          | Smart contract + challenge  | Operator Taproot spend + delay |
//! | Epoch anchoring     | Contract storage (32 bytes) | OP_RETURN (32 bytes)           |
//! | Fraud proofs        | On-chain ECDAA pairing      | Federated / BitVM              |
//! | Key registry        | Contract storage arrays     | OP_RETURN or off-chain + hash  |
//! | Trust model         | Trustless (smart contract)  | Federated (n-of-m multisig)    |
//! | Cost per deposit    | ~65,000 gas (~$7 at 30gwei) | ~$1-4 (depends on fee market)  |
//! | Programmability     | Full EVM                    | Limited Script                 |

use async_trait::async_trait;
use std::fmt;

/// Represents an L1 deposit detected by the bridge.
///
/// This is chain-agnostic: on Ethereum, `depositor` is a 20-byte address;
/// on Bitcoin, it's a 32-byte Taproot x-only public key.
#[derive(Debug, Clone)]
pub struct L1Deposit {
    pub deposit_id: u64,
    /// Depositor identifier (chain-specific: ETH address or BTC pubkey).
    pub depositor: Vec<u8>,
    /// Deposit amount in the chain's smallest unit (wei or satoshis).
    pub amount: u64,
    /// Hash of the Briolette ticket to receive tokens.
    pub ticket_hash: [u8; 32],
    /// L1 block/height at which the deposit was confirmed.
    pub block_height: u64,
}

/// Represents an L1 withdrawal state.
#[derive(Debug, Clone, PartialEq)]
pub enum L1WithdrawalState {
    /// Withdrawal initiated, challenge period active.
    Pending,
    /// Withdrawal was challenged with a fraud proof.
    Challenged,
    /// Withdrawal completed and funds released.
    Completed,
}

/// Result of initiating a withdrawal on L1.
#[derive(Debug, Clone)]
pub struct L1WithdrawalReceipt {
    pub withdrawal_id: u64,
    /// Transaction identifier (chain-specific: ETH tx hash or BTC txid).
    pub tx_id: [u8; 32],
}

/// Result of publishing an epoch on L1.
#[derive(Debug, Clone)]
pub struct L1EpochReceipt {
    /// Transaction identifier.
    pub tx_id: [u8; 32],
}

/// Chain type for configuration and logging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChainType {
    Ethereum,
    Bitcoin,
}

impl fmt::Display for ChainType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainType::Ethereum => write!(f, "Ethereum"),
            ChainType::Bitcoin => write!(f, "Bitcoin"),
        }
    }
}

#[derive(Debug)]
pub enum L1Error {
    ConnectionFailed(String),
    TransactionFailed(String),
    EventQueryFailed(String),
    ContractError(String),
    /// Insufficient confirmations for a deposit (Bitcoin-specific).
    InsufficientConfirmations { have: u32, need: u32 },
}

impl fmt::Display for L1Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            L1Error::ConnectionFailed(s) => write!(f, "connection failed: {}", s),
            L1Error::TransactionFailed(s) => write!(f, "transaction failed: {}", s),
            L1Error::EventQueryFailed(s) => write!(f, "event query failed: {}", s),
            L1Error::ContractError(s) => write!(f, "contract error: {}", s),
            L1Error::InsufficientConfirmations { have, need } => {
                write!(f, "insufficient confirmations: have {}, need {}", have, need)
            }
        }
    }
}

impl std::error::Error for L1Error {}

/// Chain-agnostic L1 client trait.
///
/// This trait abstracts all L1 interactions for the Briolette bridge,
/// allowing the same bridge server logic to operate against Ethereum
/// or Bitcoin (or future chains).
///
/// Implementations:
/// - `EthereumClient` in `ethereum.rs` (existing, wraps this via adapter)
/// - `BitcoinL1Client` in `bitcoin.rs` (Taproot + OP_RETURN)
#[async_trait]
pub trait L1Client: Send + Sync {
    /// Which chain this client connects to.
    fn chain_type(&self) -> ChainType;

    /// Query new deposits since the given cursor (block height or deposit ID).
    async fn get_deposits(&self, from_cursor: u64) -> Result<Vec<L1Deposit>, L1Error>;

    /// Mark a deposit as processed (chain-specific acknowledgment).
    ///
    /// On Ethereum: calls `markDepositProcessed()` on the contract.
    /// On Bitcoin: records the deposit as consumed in the operator's state
    /// (no on-chain action needed — the UTXO is already locked).
    async fn mark_deposit_processed(&self, deposit_id: u64) -> Result<(), L1Error>;

    /// Initiate a withdrawal on L1.
    ///
    /// On Ethereum: calls `initiateWithdrawal()` on the bridge contract.
    /// On Bitcoin: creates a timelocked Taproot transaction that releases
    /// funds to the recipient after the challenge period.
    async fn initiate_withdrawal(
        &self,
        recipient: &[u8],
        amount: u64,
    ) -> Result<L1WithdrawalReceipt, L1Error>;

    /// Query the state of a withdrawal.
    async fn get_withdrawal_state(
        &self,
        withdrawal_id: u64,
    ) -> Result<L1WithdrawalState, L1Error>;

    /// Publish an epoch commitment hash to L1.
    ///
    /// On Ethereum: calls `publishEpoch()` on the bridge contract.
    /// On Bitcoin: creates an OP_RETURN transaction with the epoch hash.
    async fn publish_epoch(
        &self,
        epoch_num: u64,
        data_hash: [u8; 32],
    ) -> Result<L1EpochReceipt, L1Error>;

    /// Get the current L1 block height.
    async fn get_block_height(&self) -> Result<u64, L1Error>;

    /// Get the smallest unit name for logging (e.g., "wei" or "satoshi").
    fn unit_name(&self) -> &'static str;
}

/// Mock L1 client for testing (chain-agnostic).
#[derive(Debug, Default)]
pub struct MockL1Client {
    pub deposits: Vec<L1Deposit>,
    pub next_withdrawal_id: u64,
    pub next_block: u64,
    pub chain: Option<ChainType>,
}

#[async_trait]
impl L1Client for MockL1Client {
    fn chain_type(&self) -> ChainType {
        self.chain.unwrap_or(ChainType::Ethereum)
    }

    async fn get_deposits(&self, _from_cursor: u64) -> Result<Vec<L1Deposit>, L1Error> {
        Ok(self.deposits.clone())
    }

    async fn mark_deposit_processed(&self, _deposit_id: u64) -> Result<(), L1Error> {
        Ok(())
    }

    async fn initiate_withdrawal(
        &self,
        _recipient: &[u8],
        _amount: u64,
    ) -> Result<L1WithdrawalReceipt, L1Error> {
        Ok(L1WithdrawalReceipt {
            withdrawal_id: self.next_withdrawal_id,
            tx_id: [0u8; 32],
        })
    }

    async fn get_withdrawal_state(
        &self,
        _withdrawal_id: u64,
    ) -> Result<L1WithdrawalState, L1Error> {
        Ok(L1WithdrawalState::Pending)
    }

    async fn publish_epoch(
        &self,
        _epoch_num: u64,
        _data_hash: [u8; 32],
    ) -> Result<L1EpochReceipt, L1Error> {
        Ok(L1EpochReceipt {
            tx_id: [0u8; 32],
        })
    }

    async fn get_block_height(&self) -> Result<u64, L1Error> {
        Ok(self.next_block)
    }

    fn unit_name(&self) -> &'static str {
        match self.chain_type() {
            ChainType::Ethereum => "wei",
            ChainType::Bitcoin => "sat",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_l1_client_deposits() {
        let client = MockL1Client {
            deposits: vec![L1Deposit {
                deposit_id: 0,
                depositor: vec![1u8; 20],
                amount: 1_000_000,
                ticket_hash: [2u8; 32],
                block_height: 100,
            }],
            ..Default::default()
        };

        let deposits = client.get_deposits(0).await.unwrap();
        assert_eq!(deposits.len(), 1);
        assert_eq!(deposits[0].amount, 1_000_000);
    }

    #[tokio::test]
    async fn test_mock_l1_client_chain_types() {
        let eth = MockL1Client {
            chain: Some(ChainType::Ethereum),
            ..Default::default()
        };
        assert_eq!(eth.chain_type(), ChainType::Ethereum);
        assert_eq!(eth.unit_name(), "wei");

        let btc = MockL1Client {
            chain: Some(ChainType::Bitcoin),
            ..Default::default()
        };
        assert_eq!(btc.chain_type(), ChainType::Bitcoin);
        assert_eq!(btc.unit_name(), "sat");
    }

    #[tokio::test]
    async fn test_mock_l1_client_withdrawal() {
        let client = MockL1Client {
            next_withdrawal_id: 42,
            ..Default::default()
        };
        let receipt = client
            .initiate_withdrawal(&[3u8; 20], 500_000)
            .await
            .unwrap();
        assert_eq!(receipt.withdrawal_id, 42);
    }

    #[tokio::test]
    async fn test_mock_l1_client_epoch() {
        let client = MockL1Client::default();
        let receipt = client.publish_epoch(1, [0xAB; 32]).await.unwrap();
        assert_eq!(receipt.tx_id, [0u8; 32]);
    }
}
