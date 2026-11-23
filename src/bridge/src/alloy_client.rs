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

//! Production Ethereum client using the alloy library (v1.x).
//!
//! Connects to the BrioletteBridge.sol contract via JSON-RPC and implements
//! the `EthereumClient` trait for real L1/L2 interaction.

use crate::ethereum::{
    EthereumClient, EthereumError, L1DepositEvent, L1EpochReceipt, L1WithdrawalReceipt,
    L1WithdrawalState,
};
use alloy::network::EthereumWallet;
use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use async_trait::async_trait;
use log::*;
use std::sync::Arc;

// Generate type-safe bindings from the Solidity ABI.
sol! {
    #[sol(rpc)]
    interface IBrioletteBridge {
        // Deposit functions
        function deposit(bytes32 ticketHash) external payable;
        function markDepositProcessed(uint256 depositId) external;
        function deposits(uint256 id) external view returns (
            address depositor,
            uint256 amount,
            bytes32 ticketHash,
            uint256 timestamp,
            bool processed
        );
        function nextDepositId() external view returns (uint256);

        // Withdrawal functions
        function initiateWithdrawal(address recipient, uint256 amount)
            external returns (uint256 withdrawalId);
        function completeWithdrawal(uint256 withdrawalId) external;
        function withdrawals(uint256 id) external view returns (
            address recipient,
            uint256 amount,
            uint256 initiatedAt,
            bool completed,
            bool challenged
        );

        // Epoch functions
        function publishEpoch(uint64 epochNum, bytes32 dataHash) external;
        function latestEpoch() external view returns (uint64);

        // Key registry functions
        function getMintKeys() external view returns (bytes[] memory);
        function getTicketKeys() external view returns (bytes[] memory);
        function ttcGroupPublicKey() external view returns (bytes memory);
        function keyRegistryVersion() external view returns (uint256);
        function addMintKey(bytes calldata key) external;
        function addTicketKey(bytes calldata key) external;
        function setTtcGroupKey(bytes calldata key) external;

        // Events
        event Deposited(
            uint256 indexed depositId,
            address indexed depositor,
            uint256 amount,
            bytes32 ticketHash
        );
        event DepositProcessed(uint256 indexed depositId);
        event WithdrawalInitiated(
            uint256 indexed withdrawalId,
            address indexed recipient,
            uint256 amount
        );
        event KeyRegistryUpdated(uint256 indexed version);
    }
}

/// Concrete provider type used by the client.
type HttpProvider = alloy::providers::fillers::FillProvider<
    alloy::providers::fillers::JoinFill<
        alloy::providers::fillers::JoinFill<
            alloy::providers::Identity,
            alloy::providers::fillers::JoinFill<
                alloy::providers::fillers::GasFiller,
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::BlobGasFiller,
                    alloy::providers::fillers::JoinFill<
                        alloy::providers::fillers::NonceFiller,
                        alloy::providers::fillers::ChainIdFiller,
                    >,
                >,
            >,
        >,
        alloy::providers::fillers::WalletFiller<EthereumWallet>,
    >,
    RootProvider,
>;

/// Production Ethereum client backed by alloy JSON-RPC provider.
pub struct AlloyEthereumClient {
    provider: Arc<HttpProvider>,
    bridge_address: Address,
}

impl std::fmt::Debug for AlloyEthereumClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlloyEthereumClient")
            .field("bridge_address", &self.bridge_address)
            .finish()
    }
}

impl AlloyEthereumClient {
    pub fn new(
        rpc_url: String,
        bridge_address: Address,
        signer: PrivateKeySigner,
    ) -> Result<Self, EthereumError> {
        let wallet = EthereumWallet::from(signer);
        let url: url::Url = rpc_url
            .parse()
            .map_err(|e| EthereumError::ConnectionFailed(format!("invalid URL: {}", e)))?;

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(url);

        Ok(Self {
            provider: Arc::new(provider),
            bridge_address,
        })
    }

    /// Create a contract instance.
    fn contract(&self) -> IBrioletteBridge::IBrioletteBridgeInstance<&HttpProvider> {
        IBrioletteBridge::new(self.bridge_address, self.provider.as_ref())
    }

    // =========================================================================
    // Key Registry reads
    // =========================================================================

    /// Fetch all mint signing keys from the on-chain registry.
    pub async fn get_mint_keys(&self) -> Result<Vec<Vec<u8>>, EthereumError> {
        let result = self
            .contract()
            .getMintKeys()
            .call()
            .await
            .map_err(|e| EthereumError::ContractError(format!("{}", e)))?;

        Ok(result.into_iter().map(|b| b.to_vec()).collect())
    }

    /// Fetch all ticket signing keys from the on-chain registry.
    pub async fn get_ticket_keys(&self) -> Result<Vec<Vec<u8>>, EthereumError> {
        let result = self
            .contract()
            .getTicketKeys()
            .call()
            .await
            .map_err(|e| EthereumError::ContractError(format!("{}", e)))?;

        Ok(result.into_iter().map(|b| b.to_vec()).collect())
    }

    /// Fetch the TTC group public key from the on-chain registry.
    pub async fn get_ttc_group_key(&self) -> Result<Vec<u8>, EthereumError> {
        let result = self
            .contract()
            .ttcGroupPublicKey()
            .call()
            .await
            .map_err(|e| EthereumError::ContractError(format!("{}", e)))?;

        Ok(result.to_vec())
    }

    /// Get the current key registry version for change detection.
    pub async fn get_key_registry_version(&self) -> Result<u64, EthereumError> {
        let result: U256 = self
            .contract()
            .keyRegistryVersion()
            .call()
            .await
            .map_err(|e| EthereumError::ContractError(format!("{}", e)))?;

        Ok(result
            .try_into()
            .unwrap_or(u64::MAX))
    }
}

#[async_trait]
impl EthereumClient for AlloyEthereumClient {
    async fn get_deposits(&self, from_block: u64) -> Result<Vec<L1DepositEvent>, EthereumError> {
        let contract = self.contract();

        // Get the total number of deposits
        let next_id: U256 = contract
            .nextDepositId()
            .call()
            .await
            .map_err(|e| EthereumError::ContractError(format!("{}", e)))?;

        let next_id_u64: u64 = next_id
            .try_into()
            .map_err(|_| EthereumError::ContractError("deposit ID overflow".into()))?;

        let mut events = Vec::new();

        for deposit_id in from_block..next_id_u64 {
            let result = contract
                .deposits(U256::from(deposit_id))
                .call()
                .await
                .map_err(|e| EthereumError::EventQueryFailed(format!("{}", e)))?;

            if result.processed {
                continue;
            }

            let mut depositor = [0u8; 20];
            depositor.copy_from_slice(result.depositor.as_slice());

            let amount_wei: u64 = result
                .amount
                .try_into()
                .map_err(|_| EthereumError::ContractError("amount overflow u64".into()))?;

            events.push(L1DepositEvent {
                deposit_id,
                depositor,
                amount_wei,
                ticket_hash: result.ticketHash.into(),
                block_number: 0,
            });
        }

        Ok(events)
    }

    async fn mark_deposit_processed(&self, deposit_id: u64) -> Result<(), EthereumError> {
        let pending = self
            .contract()
            .markDepositProcessed(U256::from(deposit_id))
            .send()
            .await
            .map_err(|e| EthereumError::TransactionFailed(format!("{}", e)))?;

        let receipt = pending
            .get_receipt()
            .await
            .map_err(|e| EthereumError::TransactionFailed(format!("{}", e)))?;

        if !receipt.status() {
            return Err(EthereumError::TransactionFailed(
                "markDepositProcessed reverted".into(),
            ));
        }

        info!("deposit {} marked processed on-chain", deposit_id);
        Ok(())
    }

    async fn initiate_withdrawal(
        &self,
        recipient: [u8; 20],
        amount_wei: u64,
    ) -> Result<L1WithdrawalReceipt, EthereumError> {
        let recipient_addr = Address::from(recipient);

        let pending = self
            .contract()
            .initiateWithdrawal(recipient_addr, U256::from(amount_wei))
            .send()
            .await
            .map_err(|e| EthereumError::TransactionFailed(format!("{}", e)))?;

        let receipt = pending
            .get_receipt()
            .await
            .map_err(|e| EthereumError::TransactionFailed(format!("{}", e)))?;

        if !receipt.status() {
            return Err(EthereumError::TransactionFailed(
                "initiateWithdrawal reverted".into(),
            ));
        }

        let tx_hash: [u8; 32] = receipt.transaction_hash.into();
        let mut withdrawal_id = 0u64;

        for log in receipt.inner.logs() {
            if let Ok(event) = log.log_decode::<IBrioletteBridge::WithdrawalInitiated>() {
                withdrawal_id = event
                    .inner
                    .data
                    .withdrawalId
                    .try_into()
                    .unwrap_or(0);
                break;
            }
        }

        info!(
            "withdrawal initiated: id={}, amount_wei={}",
            withdrawal_id, amount_wei
        );

        Ok(L1WithdrawalReceipt {
            withdrawal_id,
            tx_hash,
        })
    }

    async fn get_withdrawal_state(
        &self,
        withdrawal_id: u64,
    ) -> Result<L1WithdrawalState, EthereumError> {
        let result = self
            .contract()
            .withdrawals(U256::from(withdrawal_id))
            .call()
            .await
            .map_err(|e| EthereumError::ContractError(format!("{}", e)))?;

        if result.challenged {
            Ok(L1WithdrawalState::Challenged)
        } else if result.completed {
            Ok(L1WithdrawalState::Completed)
        } else {
            Ok(L1WithdrawalState::Pending)
        }
    }

    async fn publish_epoch(
        &self,
        epoch_num: u64,
        data_hash: [u8; 32],
    ) -> Result<L1EpochReceipt, EthereumError> {
        let pending = self
            .contract()
            .publishEpoch(epoch_num, FixedBytes::from(data_hash))
            .send()
            .await
            .map_err(|e| EthereumError::TransactionFailed(format!("{}", e)))?;

        let receipt = pending
            .get_receipt()
            .await
            .map_err(|e| EthereumError::TransactionFailed(format!("{}", e)))?;

        if !receipt.status() {
            return Err(EthereumError::TransactionFailed(
                "publishEpoch reverted".into(),
            ));
        }

        let tx_hash: [u8; 32] = receipt.transaction_hash.into();
        info!("epoch {} published to L1", epoch_num);

        Ok(L1EpochReceipt { tx_hash })
    }

    async fn get_block_number(&self) -> Result<u64, EthereumError> {
        self.provider
            .get_block_number()
            .await
            .map_err(|e| EthereumError::ConnectionFailed(format!("{}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloy_client_creation() {
        let signer: PrivateKeySigner =
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .parse()
                .unwrap();
        let client = AlloyEthereumClient::new(
            "http://127.0.0.1:8545".to_string(),
            Address::ZERO,
            signer,
        );
        assert!(client.is_ok());
        let dbg = format!("{:?}", client.unwrap());
        assert!(dbg.contains("AlloyEthereumClient"));
    }
}
