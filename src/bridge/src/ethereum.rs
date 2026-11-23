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

//! Trait-based Ethereum L1 interaction layer for the Briolette bridge.
//!
//! The `EthereumClient` trait abstracts all L1 interactions so that:
//! - Production code can use ethers-rs, alloy, or any JSON-RPC library
//! - Tests can use a mock implementation
//! - The bridge service logic stays Ethereum-library-agnostic

use async_trait::async_trait;
use std::fmt;

/// Represents an L1 deposit event from the BrioletteBridge contract.
#[derive(Debug, Clone)]
pub struct L1DepositEvent {
    pub deposit_id: u64,
    pub depositor: [u8; 20],
    pub amount_wei: u64,
    pub ticket_hash: [u8; 32],
    pub block_number: u64,
}

/// Represents an L1 withdrawal state.
#[derive(Debug, Clone, PartialEq)]
pub enum L1WithdrawalState {
    Pending,
    Challenged,
    Completed,
}

/// Result of initiating a withdrawal on L1.
#[derive(Debug, Clone)]
pub struct L1WithdrawalReceipt {
    pub withdrawal_id: u64,
    pub tx_hash: [u8; 32],
}

/// Result of publishing an epoch on L1.
#[derive(Debug, Clone)]
pub struct L1EpochReceipt {
    pub tx_hash: [u8; 32],
}

#[derive(Debug)]
pub enum EthereumError {
    ConnectionFailed(String),
    TransactionFailed(String),
    EventQueryFailed(String),
    ContractError(String),
}

impl fmt::Display for EthereumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EthereumError::ConnectionFailed(s) => write!(f, "connection failed: {}", s),
            EthereumError::TransactionFailed(s) => write!(f, "transaction failed: {}", s),
            EthereumError::EventQueryFailed(s) => write!(f, "event query failed: {}", s),
            EthereumError::ContractError(s) => write!(f, "contract error: {}", s),
        }
    }
}

impl std::error::Error for EthereumError {}

/// Trait abstracting Ethereum L1 interactions for the Briolette bridge.
///
/// Implementations connect to the BrioletteBridge.sol contract deployed on L1
/// and execute bridge operations. This trait allows the bridge service to be
/// tested without a live Ethereum node.
#[async_trait]
pub trait EthereumClient: Send + Sync {
    /// Query new deposit events from the bridge contract since `from_block`.
    async fn get_deposits(&self, from_block: u64) -> Result<Vec<L1DepositEvent>, EthereumError>;

    /// Mark a deposit as processed on the bridge contract.
    async fn mark_deposit_processed(&self, deposit_id: u64) -> Result<(), EthereumError>;

    /// Initiate a withdrawal on the bridge contract.
    async fn initiate_withdrawal(
        &self,
        recipient: [u8; 20],
        amount_wei: u64,
    ) -> Result<L1WithdrawalReceipt, EthereumError>;

    /// Query the state of a withdrawal.
    async fn get_withdrawal_state(
        &self,
        withdrawal_id: u64,
    ) -> Result<L1WithdrawalState, EthereumError>;

    /// Publish an epoch commitment hash to the bridge contract.
    async fn publish_epoch(
        &self,
        epoch_num: u64,
        data_hash: [u8; 32],
    ) -> Result<L1EpochReceipt, EthereumError>;

    /// Get the latest block number on L1.
    async fn get_block_number(&self) -> Result<u64, EthereumError>;
}

/// Mock Ethereum client for testing.
#[derive(Debug, Default)]
pub struct MockEthereumClient {
    pub deposits: Vec<L1DepositEvent>,
    pub next_withdrawal_id: u64,
    pub next_block: u64,
}

#[async_trait]
impl EthereumClient for MockEthereumClient {
    async fn get_deposits(&self, _from_block: u64) -> Result<Vec<L1DepositEvent>, EthereumError> {
        Ok(self.deposits.clone())
    }

    async fn mark_deposit_processed(&self, _deposit_id: u64) -> Result<(), EthereumError> {
        Ok(())
    }

    async fn initiate_withdrawal(
        &self,
        _recipient: [u8; 20],
        _amount_wei: u64,
    ) -> Result<L1WithdrawalReceipt, EthereumError> {
        Ok(L1WithdrawalReceipt {
            withdrawal_id: self.next_withdrawal_id,
            tx_hash: [0u8; 32],
        })
    }

    async fn get_withdrawal_state(
        &self,
        _withdrawal_id: u64,
    ) -> Result<L1WithdrawalState, EthereumError> {
        Ok(L1WithdrawalState::Pending)
    }

    async fn publish_epoch(
        &self,
        _epoch_num: u64,
        _data_hash: [u8; 32],
    ) -> Result<L1EpochReceipt, EthereumError> {
        Ok(L1EpochReceipt {
            tx_hash: [0u8; 32],
        })
    }

    async fn get_block_number(&self) -> Result<u64, EthereumError> {
        Ok(self.next_block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_client_deposits() {
        let client = MockEthereumClient {
            deposits: vec![L1DepositEvent {
                deposit_id: 0,
                depositor: [1u8; 20],
                amount_wei: 1_000_000_000_000_000_000, // 1 ETH
                ticket_hash: [2u8; 32],
                block_number: 100,
            }],
            ..Default::default()
        };

        let deposits = client.get_deposits(0).await.unwrap();
        assert_eq!(deposits.len(), 1);
        assert_eq!(deposits[0].amount_wei, 1_000_000_000_000_000_000);
    }

    #[tokio::test]
    async fn test_mock_client_withdrawal() {
        let client = MockEthereumClient {
            next_withdrawal_id: 42,
            ..Default::default()
        };

        let receipt = client
            .initiate_withdrawal([3u8; 20], 500_000_000)
            .await
            .unwrap();
        assert_eq!(receipt.withdrawal_id, 42);
    }

    #[tokio::test]
    async fn test_mock_client_epoch() {
        let client = MockEthereumClient::default();
        let receipt = client.publish_epoch(1, [0xAB; 32]).await.unwrap();
        assert_eq!(receipt.tx_hash, [0u8; 32]);
    }
}
