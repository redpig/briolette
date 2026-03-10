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

//! Briolette L2 Bridge service implementation.
//!
//! The bridge service sits between Ethereum L1 and the Briolette system:
//! - Monitors L1 for deposit events, triggering token minting
//! - Accepts tokens for L1 withdrawal, verifying the token chain
//! - Publishes epoch commitments to L1 for auditability

use crate::ethereum::{EthereumClient, L1WithdrawalState};
use briolette_proto::briolette::bridge::*;
use briolette_proto::briolette::token;
use briolette_proto::briolette::token::TokenVerify;
use briolette_proto::briolette::Version;
use briolette_proto::briolette::{Error as BrioletteError, ErrorCode as BrioletteErrorCode};
use log::*;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Wei-to-micros conversion factor.
/// 1 ETH = 10^18 wei, 1 whole unit = 10^6 micros.
/// We map 1 ETH = 1 whole unit, so 1 wei = 10^-12 micros.
/// For simplicity: amount_wei / 10^12 = micros, amount_wei / 10^18 = whole.
const WEI_PER_WHOLE: u64 = 1_000_000_000_000_000_000;
const WEI_PER_MICRO: u64 = 1_000_000_000_000;

#[derive(Clone)]
pub struct BrioletteBridge {
    eth_client: Arc<dyn EthereumClient>,
    ttc_group_public_key: Vec<u8>,
    mint_signing_keys: Vec<Vec<u8>>,
    ticket_signing_keys: Vec<Vec<u8>>,
    validate_uri: String,
    /// Last L1 block we've scanned for deposit events.
    last_scanned_block: u64,
}

impl std::fmt::Debug for BrioletteBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrioletteBridge")
            .field("validate_uri", &self.validate_uri)
            .field("last_scanned_block", &self.last_scanned_block)
            .finish()
    }
}

impl BrioletteBridge {
    pub fn new(
        eth_client: Arc<dyn EthereumClient>,
        ttc_group_public_key: Vec<u8>,
        mint_signing_keys: Vec<Vec<u8>>,
        ticket_signing_keys: Vec<Vec<u8>>,
        validate_uri: String,
    ) -> Self {
        Self {
            eth_client,
            ttc_group_public_key,
            mint_signing_keys,
            ticket_signing_keys,
            validate_uri,
            last_scanned_block: 0,
        }
    }

    /// Process a withdrawal request: verify the token chain, then initiate
    /// a withdrawal on the L1 bridge contract.
    pub async fn withdraw_to_l1_impl(
        &self,
        request: &WithdrawRequest,
    ) -> Result<WithdrawReply, BrioletteError> {
        trace!("withdraw_to_l1: request = {:?}", &request);

        if request.version != Version::Current as i32 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidVersion.into(),
            });
        }

        if request.tokens.is_empty() || request.recipient_address.len() != 20 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }

        // 1. Verify each token in the chain.
        let mut total_amount = token::Amount::default();
        for token in request.tokens.iter() {
            token.verify(
                &self.ttc_group_public_key,
                &self.mint_signing_keys,
                &self.ticket_signing_keys,
            )?;
            total_amount =
                total_amount + token.descriptor.as_ref().unwrap().value.clone().unwrap();
        }

        // 2. Convert Briolette amount to wei.
        let whole_wei = (total_amount.whole as u64) * WEI_PER_WHOLE;
        let frac_wei = (total_amount.fractional as u64) * WEI_PER_MICRO;
        let total_wei = whole_wei + frac_wei;

        if total_wei == 0 {
            return Ok(WithdrawReply {
                accepted: false,
                withdrawal_id: 0,
                error: Some(BrioletteError {
                    code: BrioletteErrorCode::InvalidAmountType.into(),
                }),
            });
        }

        // 3. Initiate withdrawal on L1.
        let mut recipient = [0u8; 20];
        recipient.copy_from_slice(&request.recipient_address);

        match self.eth_client.initiate_withdrawal(recipient, total_wei).await {
            Ok(receipt) => {
                info!(
                    "L1 withdrawal initiated: id={}, amount_wei={}",
                    receipt.withdrawal_id, total_wei
                );
                Ok(WithdrawReply {
                    accepted: true,
                    withdrawal_id: receipt.withdrawal_id,
                    error: None,
                })
            }
            Err(e) => {
                error!("L1 withdrawal failed: {}", e);
                Ok(WithdrawReply {
                    accepted: false,
                    withdrawal_id: 0,
                    error: Some(BrioletteError {
                        code: BrioletteErrorCode::InvalidServerState.into(),
                    }),
                })
            }
        }
    }

    /// Query the status of an L1 deposit.
    pub async fn get_deposit_status_impl(
        &self,
        request: &DepositStatusRequest,
    ) -> Result<DepositStatusReply, BrioletteError> {
        // For the prototype, we return a minimal response.
        // A full implementation would query the L1 contract state.
        let deposits = self
            .eth_client
            .get_deposits(0)
            .await
            .map_err(|_| BrioletteError {
                code: BrioletteErrorCode::InvalidServerState.into(),
            })?;

        for dep in deposits.iter() {
            if dep.deposit_id == request.deposit_id {
                return Ok(DepositStatusReply {
                    deposit: Some(L1Deposit {
                        deposit_id: dep.deposit_id,
                        depositor_address: dep.depositor.to_vec(),
                        amount_wei: dep.amount_wei,
                        ticket_hash: dep.ticket_hash.to_vec(),
                        block_number: dep.block_number,
                        timestamp: 0,
                    }),
                    processed: false,
                });
            }
        }

        Err(BrioletteError {
            code: BrioletteErrorCode::InvalidMissingFields.into(),
        })
    }

    /// Query the status of an L1 withdrawal.
    pub async fn get_withdrawal_status_impl(
        &self,
        request: &WithdrawalStatusRequest,
    ) -> Result<WithdrawalStatusReply, BrioletteError> {
        let state = self
            .eth_client
            .get_withdrawal_state(request.withdrawal_id)
            .await
            .map_err(|_| BrioletteError {
                code: BrioletteErrorCode::InvalidServerState.into(),
            })?;

        let proto_state = match state {
            L1WithdrawalState::Pending => WithdrawalState::WithdrawalPending,
            L1WithdrawalState::Challenged => WithdrawalState::WithdrawalChallenged,
            L1WithdrawalState::Completed => WithdrawalState::WithdrawalCompleted,
        };

        Ok(WithdrawalStatusReply {
            withdrawal_id: request.withdrawal_id,
            state: proto_state.into(),
            amount_wei: 0,
            recipient_address: vec![],
            initiated_at: 0,
        })
    }

    /// Publish an epoch commitment to L1.
    /// Called by the clerk's epoch generation process.
    pub async fn publish_epoch_to_l1(
        &self,
        epoch_num: u64,
        epoch_data_bytes: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data_hash: [u8; 32] = Sha256::digest(epoch_data_bytes).into();
        let receipt = self.eth_client.publish_epoch(epoch_num, data_hash).await?;
        info!(
            "epoch {} published to L1, tx_hash={:?}",
            epoch_num,
            hex::encode(receipt.tx_hash)
        );
        Ok(())
    }
}

/// Hex encoding helper (no external dep needed).
mod hex {
    pub fn encode(data: [u8; 32]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethereum::MockEthereumClient;

    fn make_bridge() -> BrioletteBridge {
        let client = Arc::new(MockEthereumClient {
            next_withdrawal_id: 7,
            ..Default::default()
        });
        BrioletteBridge::new(
            client,
            vec![1, 2, 3], // dummy keys for testing
            vec![vec![4, 5, 6]],
            vec![vec![7, 8, 9]],
            "http://127.0.0.1:50055".to_string(),
        )
    }

    #[tokio::test]
    async fn test_withdraw_rejects_empty_tokens() {
        let bridge = make_bridge();
        let request = WithdrawRequest {
            version: Version::Current as i32,
            tokens: vec![],
            recipient_address: vec![0u8; 20],
        };
        let result = bridge.withdraw_to_l1_impl(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_withdraw_rejects_bad_address() {
        let bridge = make_bridge();
        let request = WithdrawRequest {
            version: Version::Current as i32,
            tokens: vec![token::Token::default()],
            recipient_address: vec![0u8; 10], // too short
        };
        let result = bridge.withdraw_to_l1_impl(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_withdrawal_status() {
        let bridge = make_bridge();
        let request = WithdrawalStatusRequest { withdrawal_id: 0 };
        let reply = bridge.get_withdrawal_status_impl(&request).await.unwrap();
        assert_eq!(reply.state, WithdrawalState::WithdrawalPending as i32);
    }

    #[tokio::test]
    async fn test_publish_epoch() {
        let bridge = make_bridge();
        let result = bridge.publish_epoch_to_l1(1, b"test epoch data").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_deposit_status_not_found() {
        let bridge = make_bridge();
        let request = DepositStatusRequest { deposit_id: 999 };
        let result = bridge.get_deposit_status_impl(&request).await;
        assert!(result.is_err());
    }
}
