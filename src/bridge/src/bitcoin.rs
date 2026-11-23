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

//! Bitcoin L1 interaction layer for the Briolette bridge.
//!
//! This module implements the `L1Client` trait for Bitcoin, enabling Briolette
//! to operate as a Bitcoin L2 using:
//!
//! - **Taproot multisig** for deposit locking (operator + federation keys)
//! - **OP_RETURN** for epoch commitment anchoring (32-byte hash per epoch)
//! - **Timelocked Taproot spends** for withdrawals with a challenge window
//! - **Federated fraud proofs** (off-chain ECDAA verification by federation)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Bitcoin L1                                  │
//! │                                                                     │
//! │  Deposits:                                                          │
//! │    User sends BTC to a Taproot address derived from:                │
//! │      internal_key = operator_pubkey                                  │
//! │      script_path  = OP_CHECKSIGVERIFY <federation_key>              │
//! │                     OP_CHECKLOCKTIMEVERIFY (recovery after timeout) │
//! │    The OP_RETURN in the deposit tx encodes the ticket_hash.         │
//! │                                                                     │
//! │  Withdrawals:                                                       │
//! │    Operator constructs a Taproot spend from the deposit pool:       │
//! │      - Key path: operator signs immediately                         │
//! │      - Output: recipient address with timelock (challenge period)   │
//! │      - Federation co-signs after verifying the token chain          │
//! │                                                                     │
//! │  Epoch anchoring:                                                   │
//! │    Operator publishes OP_RETURN tx:                                  │
//! │      OP_RETURN <"BRI\x01"> <epoch_num: 8 bytes> <hash: 32 bytes>   │
//! │    Total: 44 bytes (within 80-byte OP_RETURN limit)                 │
//! │                                                                     │
//! │  Fraud proofs:                                                      │
//! │    ECDAA pairing verification is done OFF-CHAIN by the federation.  │
//! │    If fraud is detected, the federation refuses to co-sign          │
//! │    withdrawals and publishes a revocation OP_RETURN.                │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Trust model comparison
//!
//! | Property               | Ethereum bridge         | Bitcoin bridge            |
//! |------------------------|-------------------------|---------------------------|
//! | Deposit custody        | Smart contract (code)   | Taproot multisig (keys)   |
//! | Fraud proof execution  | On-chain (bn256 pairing)| Off-chain (federation)    |
//! | Challenge mechanism    | Anyone can challenge    | Federation vetoes         |
//! | Withdrawal finality    | After challenge period  | After timelock expires    |
//! | Operator misbehavior   | Slashable on-chain      | Federation stops signing  |
//! | Worst case (collusion) | Funds frozen in contract| Funds stolen if n-of-m    |
//!
//! The Bitcoin bridge trades the trustless fraud proof model for a federated
//! model. This is acceptable when:
//! - The operator is a known, regulated entity (e.g., central bank)
//! - The federation members are independent and incentive-aligned
//! - The off-chain system (ECDAA double-spend detection) is the primary defense

use crate::l1::{ChainType, L1Client, L1Deposit, L1EpochReceipt, L1Error, L1WithdrawalReceipt, L1WithdrawalState};
use async_trait::async_trait;
use log::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Satoshis per BTC.
pub const SATS_PER_BTC: u64 = 100_000_000;

/// Briolette OP_RETURN prefix: "BRI" + version byte.
pub const OP_RETURN_PREFIX: &[u8; 4] = b"BRI\x01";

/// Minimum confirmations required for a deposit to be considered final.
pub const MIN_DEPOSIT_CONFIRMATIONS: u32 = 6;

/// Challenge period for withdrawals in Bitcoin blocks (~7 days at 144 blocks/day).
pub const CHALLENGE_PERIOD_BLOCKS: u32 = 1008;

/// Configuration for the Bitcoin L1 client.
#[derive(Debug, Clone)]
pub struct BitcoinConfig {
    /// Bitcoin Core RPC URL (e.g., "http://127.0.0.1:8332").
    pub rpc_url: String,
    /// RPC authentication (user:password).
    pub rpc_auth: String,
    /// Bitcoin network (mainnet, testnet, signet, regtest).
    pub network: BitcoinNetwork,
    /// Operator's x-only public key (32 bytes, for Taproot key path).
    pub operator_pubkey: [u8; 32],
    /// Federation public keys for multisig (x-only, 32 bytes each).
    pub federation_pubkeys: Vec<[u8; 32]>,
    /// Minimum federation signatures required (n of m).
    pub federation_threshold: u32,
    /// Minimum confirmations for deposit finality.
    pub min_confirmations: u32,
}

impl Default for BitcoinConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://127.0.0.1:8332".to_string(),
            rpc_auth: String::new(),
            network: BitcoinNetwork::Regtest,
            operator_pubkey: [0u8; 32],
            federation_pubkeys: vec![],
            federation_threshold: 1,
            min_confirmations: MIN_DEPOSIT_CONFIRMATIONS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BitcoinNetwork {
    Mainnet,
    Testnet,
    Signet,
    Regtest,
}

impl BitcoinNetwork {
    pub fn as_str(&self) -> &'static str {
        match self {
            BitcoinNetwork::Mainnet => "mainnet",
            BitcoinNetwork::Testnet => "testnet",
            BitcoinNetwork::Signet => "signet",
            BitcoinNetwork::Regtest => "regtest",
        }
    }
}

/// Represents a Bitcoin UTXO locked in the bridge deposit address.
#[derive(Debug, Clone)]
pub struct DepositUtxo {
    /// The Bitcoin transaction ID (big-endian, display order).
    pub txid: [u8; 32],
    /// Output index within the transaction.
    pub vout: u32,
    /// Amount in satoshis.
    pub amount_sats: u64,
    /// Ticket hash extracted from the OP_RETURN output.
    pub ticket_hash: [u8; 32],
    /// Block height at which this was confirmed.
    pub block_height: u64,
    /// Number of confirmations.
    pub confirmations: u32,
}

/// Represents a pending Bitcoin withdrawal.
#[derive(Debug, Clone)]
struct PendingWithdrawal {
    pub recipient_script: Vec<u8>,
    pub amount_sats: u64,
    pub initiated_height: u64,
    pub txid: [u8; 32],
    pub state: L1WithdrawalState,
}

/// Encodes an epoch commitment as an OP_RETURN payload.
///
/// Format: `"BRI\x01" || epoch_num (8 bytes BE) || data_hash (32 bytes)`
/// Total: 44 bytes (within Bitcoin's 80-byte OP_RETURN limit).
pub fn encode_epoch_op_return(epoch_num: u64, data_hash: &[u8; 32]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(44);
    payload.extend_from_slice(OP_RETURN_PREFIX);
    payload.extend_from_slice(&epoch_num.to_be_bytes());
    payload.extend_from_slice(data_hash);
    payload
}

/// Decodes an OP_RETURN payload into epoch number and data hash.
/// Returns None if the payload doesn't match the expected format.
pub fn decode_epoch_op_return(payload: &[u8]) -> Option<(u64, [u8; 32])> {
    if payload.len() != 44 {
        return None;
    }
    if &payload[0..4] != OP_RETURN_PREFIX {
        return None;
    }
    let epoch_num = u64::from_be_bytes(payload[4..12].try_into().ok()?);
    let mut data_hash = [0u8; 32];
    data_hash.copy_from_slice(&payload[12..44]);
    Some((epoch_num, data_hash))
}

/// Encodes a deposit OP_RETURN payload containing the ticket hash.
///
/// Format: `"BRI\x02" || ticket_hash (32 bytes)`
/// Total: 36 bytes.
pub fn encode_deposit_op_return(ticket_hash: &[u8; 32]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(36);
    payload.extend_from_slice(b"BRI\x02");
    payload.extend_from_slice(ticket_hash);
    payload
}

/// Decodes a deposit OP_RETURN payload.
pub fn decode_deposit_op_return(payload: &[u8]) -> Option<[u8; 32]> {
    if payload.len() != 36 {
        return None;
    }
    if &payload[0..4] != b"BRI\x02" {
        return None;
    }
    let mut ticket_hash = [0u8; 32];
    ticket_hash.copy_from_slice(&payload[4..36]);
    Some(ticket_hash)
}

/// Constructs the Taproot deposit script tree.
///
/// The deposit address is a Taproot output with:
/// - Internal key: operator's x-only pubkey (key path spend for normal operations)
/// - Script path: federation threshold signature + timelock recovery
///
/// This function returns the raw script bytes for the script path leaf.
/// In production, this would use a proper Bitcoin library (rust-bitcoin)
/// to construct the full Taproot tree and derive the address.
pub fn build_deposit_script(
    operator_pubkey: &[u8; 32],
    federation_pubkeys: &[[u8; 32]],
    federation_threshold: u32,
) -> Vec<u8> {
    // This is a conceptual representation of the Taproot script tree.
    // A production implementation would use rust-bitcoin to construct:
    //
    // Script path leaf 1 (normal withdrawal):
    //   <operator_pubkey> OP_CHECKSIGVERIFY
    //   <federation_key_1> OP_CHECKSIG
    //   <federation_key_2> OP_CHECKSIGADD
    //   ...
    //   <threshold> OP_NUMEQUAL
    //
    // Script path leaf 2 (emergency recovery after extended timelock):
    //   <very_long_timelock> OP_CHECKLOCKTIMEVERIFY OP_DROP
    //   <recovery_pubkey> OP_CHECKSIG
    //
    // For the reference implementation, we encode the structure as tagged bytes
    // that document the intended script without pulling in the full bitcoin
    // library dependency (which would be required for production use).
    let mut script = Vec::new();

    // Tag: script version
    script.push(0x01);

    // Operator pubkey (32 bytes)
    script.extend_from_slice(operator_pubkey);

    // Federation threshold
    script.extend_from_slice(&federation_threshold.to_le_bytes());

    // Federation pubkeys
    script.extend_from_slice(&(federation_pubkeys.len() as u32).to_le_bytes());
    for pk in federation_pubkeys {
        script.extend_from_slice(pk);
    }

    script
}

/// Bitcoin L1 client for the Briolette bridge.
///
/// This implementation uses Bitcoin Core's JSON-RPC API to:
/// - Watch for deposits to the Taproot bridge address
/// - Construct and broadcast withdrawal transactions
/// - Publish epoch commitments via OP_RETURN
///
/// In a production deployment, this would use the `bitcoincore-rpc` or
/// `rust-bitcoin` crates. This reference implementation provides the
/// structure and demonstrates the protocol without those dependencies.
pub struct BitcoinL1Client {
    config: BitcoinConfig,
    /// Operator's local state: processed deposits.
    processed_deposits: Arc<RwLock<HashMap<u64, bool>>>,
    /// Operator's local state: pending withdrawals.
    withdrawals: Arc<RwLock<Vec<PendingWithdrawal>>>,
    /// Simulated deposit UTXOs (for testing without a real Bitcoin node).
    simulated_deposits: Arc<RwLock<Vec<DepositUtxo>>>,
    /// Current block height (for testing).
    current_height: Arc<RwLock<u64>>,
}

impl BitcoinL1Client {
    pub fn new(config: BitcoinConfig) -> Self {
        Self {
            config,
            processed_deposits: Arc::new(RwLock::new(HashMap::new())),
            withdrawals: Arc::new(RwLock::new(Vec::new())),
            simulated_deposits: Arc::new(RwLock::new(Vec::new())),
            current_height: Arc::new(RwLock::new(0)),
        }
    }

    /// Get the deposit address for this bridge instance.
    ///
    /// Returns the raw script bytes. A production implementation would
    /// encode this as a bech32m address (bc1p...).
    pub fn deposit_script(&self) -> Vec<u8> {
        build_deposit_script(
            &self.config.operator_pubkey,
            &self.config.federation_pubkeys,
            self.config.federation_threshold,
        )
    }

    /// Scan Bitcoin blocks for deposit transactions to the bridge address.
    ///
    /// In production, this would call Bitcoin Core's `listsinceblock` or
    /// `scantxoutset` RPC methods. The reference implementation uses
    /// simulated deposits for testing.
    async fn scan_deposits(
        &self,
        from_height: u64,
    ) -> Result<Vec<DepositUtxo>, L1Error> {
        // In production:
        //   1. Call `getblockcount` to get current height
        //   2. For each block from from_height to current:
        //      a. Call `getblock <hash> 2` for full tx data
        //      b. Check each tx output against the bridge Taproot address
        //      c. If match, extract ticket_hash from OP_RETURN output
        //   3. Filter by minimum confirmations
        //
        // For the reference implementation, return simulated deposits.
        let deposits = self.simulated_deposits.read()
            .map_err(|e| L1Error::EventQueryFailed(format!("lock: {}", e)))?;

        Ok(deposits
            .iter()
            .filter(|d| d.block_height >= from_height)
            .filter(|d| d.confirmations >= self.config.min_confirmations)
            .cloned()
            .collect())
    }

    /// Construct and broadcast a withdrawal transaction.
    ///
    /// The withdrawal creates a timelocked output that can be spent by
    /// the recipient after CHALLENGE_PERIOD_BLOCKS blocks.
    ///
    /// In production:
    ///   1. Select UTXOs from the deposit pool
    ///   2. Construct a transaction with:
    ///      - Input: deposit UTXO(s) signed via Taproot key path
    ///      - Output 1: recipient with OP_CHECKSEQUENCEVERIFY timelock
    ///      - Output 2: change back to bridge address
    ///      - Output 3: OP_RETURN with withdrawal metadata
    ///   3. Get federation co-signatures
    ///   4. Broadcast the transaction
    async fn construct_withdrawal(
        &self,
        recipient: &[u8],
        amount_sats: u64,
    ) -> Result<[u8; 32], L1Error> {
        info!(
            "constructing withdrawal: {} sats to {:?} (network: {})",
            amount_sats,
            &recipient[..std::cmp::min(8, recipient.len())],
            self.config.network.as_str()
        );

        // In production, this would:
        // 1. Create the transaction using rust-bitcoin
        // 2. Sign with operator key
        // 3. Collect federation signatures
        // 4. Broadcast via sendrawtransaction RPC

        // For the reference implementation, return a deterministic txid.
        let mut txid = [0u8; 32];
        let height = *self.current_height.read()
            .map_err(|e| L1Error::TransactionFailed(format!("lock: {}", e)))?;
        txid[0..8].copy_from_slice(&height.to_be_bytes());
        txid[8..16].copy_from_slice(&amount_sats.to_be_bytes());

        Ok(txid)
    }

    /// Broadcast an OP_RETURN transaction for epoch anchoring.
    ///
    /// In production:
    ///   1. Create a transaction with a small UTXO input
    ///   2. Add OP_RETURN output with the epoch payload
    ///   3. Sign and broadcast
    async fn broadcast_op_return(
        &self,
        payload: &[u8],
    ) -> Result<[u8; 32], L1Error> {
        if payload.len() > 80 {
            return Err(L1Error::TransactionFailed(format!(
                "OP_RETURN payload too large: {} bytes (max 80)",
                payload.len()
            )));
        }

        info!(
            "broadcasting OP_RETURN: {} bytes (network: {})",
            payload.len(),
            self.config.network.as_str()
        );

        // In production: construct, sign, and broadcast via RPC.
        // Reference implementation returns a deterministic txid.
        use sha2::{Digest, Sha256};
        let txid: [u8; 32] = Sha256::digest(payload).into();
        Ok(txid)
    }

    // === Test helpers ===

    /// Add a simulated deposit (for testing without a real Bitcoin node).
    #[cfg(test)]
    pub fn add_simulated_deposit(&self, utxo: DepositUtxo) {
        self.simulated_deposits.write().unwrap().push(utxo);
    }

    /// Set the simulated block height.
    #[cfg(test)]
    pub fn set_height(&self, height: u64) {
        *self.current_height.write().unwrap() = height;
    }
}

impl std::fmt::Debug for BitcoinL1Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitcoinL1Client")
            .field("network", &self.config.network)
            .field("rpc_url", &self.config.rpc_url)
            .finish()
    }
}

#[async_trait]
impl L1Client for BitcoinL1Client {
    fn chain_type(&self) -> ChainType {
        ChainType::Bitcoin
    }

    async fn get_deposits(&self, from_cursor: u64) -> Result<Vec<L1Deposit>, L1Error> {
        let utxos = self.scan_deposits(from_cursor).await?;

        Ok(utxos
            .into_iter()
            .enumerate()
            .map(|(i, utxo)| L1Deposit {
                deposit_id: from_cursor + i as u64,
                depositor: utxo.txid.to_vec(), // Use txid as depositor identifier
                amount: utxo.amount_sats,
                ticket_hash: utxo.ticket_hash,
                block_height: utxo.block_height,
            })
            .collect())
    }

    async fn mark_deposit_processed(&self, deposit_id: u64) -> Result<(), L1Error> {
        // On Bitcoin, marking a deposit as processed is purely local state.
        // The UTXO is already locked — we just record that we've minted
        // the corresponding Briolette tokens.
        //
        // This is a key difference from Ethereum where markDepositProcessed()
        // is an on-chain transaction. On Bitcoin, the deposit UTXO stays
        // unspent in the bridge address until a withdrawal consumes it.
        self.processed_deposits
            .write()
            .map_err(|e| L1Error::ContractError(format!("lock: {}", e)))?
            .insert(deposit_id, true);

        info!("deposit {} marked processed (local state)", deposit_id);
        Ok(())
    }

    async fn initiate_withdrawal(
        &self,
        recipient: &[u8],
        amount: u64,
    ) -> Result<L1WithdrawalReceipt, L1Error> {
        let txid = self.construct_withdrawal(recipient, amount).await?;

        let height = *self.current_height.read()
            .map_err(|e| L1Error::TransactionFailed(format!("lock: {}", e)))?;

        let withdrawal_id = {
            let mut withdrawals = self.withdrawals.write()
                .map_err(|e| L1Error::TransactionFailed(format!("lock: {}", e)))?;
            let id = withdrawals.len() as u64;
            withdrawals.push(PendingWithdrawal {
                recipient_script: recipient.to_vec(),
                amount_sats: amount,
                initiated_height: height,
                txid,
                state: L1WithdrawalState::Pending,
            });
            id
        };

        info!(
            "Bitcoin withdrawal initiated: id={}, amount={} sats, txid={:?}",
            withdrawal_id,
            amount,
            &txid[..8]
        );

        Ok(L1WithdrawalReceipt {
            withdrawal_id,
            tx_id: txid,
        })
    }

    async fn get_withdrawal_state(
        &self,
        withdrawal_id: u64,
    ) -> Result<L1WithdrawalState, L1Error> {
        let withdrawals = self.withdrawals.read()
            .map_err(|e| L1Error::ContractError(format!("lock: {}", e)))?;

        let withdrawal = withdrawals
            .get(withdrawal_id as usize)
            .ok_or_else(|| L1Error::ContractError("withdrawal not found".into()))?;

        // In production, check if the timelock has expired and if the
        // federation has challenged (by refusing to sign or publishing
        // a revocation OP_RETURN).
        let current_height = *self.current_height.read()
            .map_err(|e| L1Error::ContractError(format!("lock: {}", e)))?;

        if withdrawal.state == L1WithdrawalState::Challenged {
            return Ok(L1WithdrawalState::Challenged);
        }

        if current_height >= withdrawal.initiated_height + CHALLENGE_PERIOD_BLOCKS as u64 {
            return Ok(L1WithdrawalState::Completed);
        }

        Ok(L1WithdrawalState::Pending)
    }

    async fn publish_epoch(
        &self,
        epoch_num: u64,
        data_hash: [u8; 32],
    ) -> Result<L1EpochReceipt, L1Error> {
        let payload = encode_epoch_op_return(epoch_num, &data_hash);
        let txid = self.broadcast_op_return(&payload).await?;

        info!(
            "epoch {} published to Bitcoin via OP_RETURN, txid={:?}",
            epoch_num,
            &txid[..8]
        );

        Ok(L1EpochReceipt { tx_id: txid })
    }

    async fn get_block_height(&self) -> Result<u64, L1Error> {
        // In production: call `getblockcount` RPC.
        let height = *self.current_height.read()
            .map_err(|e| L1Error::ConnectionFailed(format!("lock: {}", e)))?;
        Ok(height)
    }

    fn unit_name(&self) -> &'static str {
        "sat"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BitcoinConfig {
        BitcoinConfig {
            rpc_url: "http://127.0.0.1:18443".to_string(),
            rpc_auth: "user:pass".to_string(),
            network: BitcoinNetwork::Regtest,
            operator_pubkey: [0xAA; 32],
            federation_pubkeys: vec![[0xBB; 32], [0xCC; 32]],
            federation_threshold: 2,
            min_confirmations: 6,
        }
    }

    #[test]
    fn test_encode_decode_epoch_op_return() {
        let epoch_num = 42u64;
        let data_hash = [0xAB; 32];

        let payload = encode_epoch_op_return(epoch_num, &data_hash);
        assert_eq!(payload.len(), 44);
        assert_eq!(&payload[0..4], OP_RETURN_PREFIX);

        let (decoded_epoch, decoded_hash) = decode_epoch_op_return(&payload).unwrap();
        assert_eq!(decoded_epoch, epoch_num);
        assert_eq!(decoded_hash, data_hash);
    }

    #[test]
    fn test_decode_epoch_op_return_bad_prefix() {
        let mut payload = encode_epoch_op_return(1, &[0; 32]);
        payload[0] = b'X';
        assert!(decode_epoch_op_return(&payload).is_none());
    }

    #[test]
    fn test_decode_epoch_op_return_bad_length() {
        assert!(decode_epoch_op_return(&[0; 10]).is_none());
    }

    #[test]
    fn test_encode_decode_deposit_op_return() {
        let ticket_hash = [0xCD; 32];
        let payload = encode_deposit_op_return(&ticket_hash);
        assert_eq!(payload.len(), 36);

        let decoded = decode_deposit_op_return(&payload).unwrap();
        assert_eq!(decoded, ticket_hash);
    }

    #[test]
    fn test_build_deposit_script() {
        let config = test_config();
        let script = build_deposit_script(
            &config.operator_pubkey,
            &config.federation_pubkeys,
            config.federation_threshold,
        );

        // Version byte + 32 operator + 4 threshold + 4 count + 2*32 federation
        assert_eq!(script.len(), 1 + 32 + 4 + 4 + 64);
        assert_eq!(script[0], 0x01); // version
        assert_eq!(&script[1..33], &[0xAA; 32]); // operator
    }

    #[tokio::test]
    async fn test_bitcoin_client_deposits() {
        let client = BitcoinL1Client::new(test_config());
        client.set_height(100);

        client.add_simulated_deposit(DepositUtxo {
            txid: [1u8; 32],
            vout: 0,
            amount_sats: 50_000,
            ticket_hash: [2u8; 32],
            block_height: 90,
            confirmations: 10,
        });

        let deposits = client.get_deposits(0).await.unwrap();
        assert_eq!(deposits.len(), 1);
        assert_eq!(deposits[0].amount, 50_000);
        assert_eq!(deposits[0].ticket_hash, [2u8; 32]);
    }

    #[tokio::test]
    async fn test_bitcoin_client_insufficient_confirmations() {
        let client = BitcoinL1Client::new(test_config());
        client.set_height(100);

        client.add_simulated_deposit(DepositUtxo {
            txid: [1u8; 32],
            vout: 0,
            amount_sats: 50_000,
            ticket_hash: [2u8; 32],
            block_height: 98,
            confirmations: 2, // Below min_confirmations (6)
        });

        let deposits = client.get_deposits(0).await.unwrap();
        assert_eq!(deposits.len(), 0); // Filtered out
    }

    #[tokio::test]
    async fn test_bitcoin_client_mark_deposit_processed() {
        let client = BitcoinL1Client::new(test_config());
        // Should succeed (local state only, no on-chain tx)
        client.mark_deposit_processed(0).await.unwrap();
        assert!(client.processed_deposits.read().unwrap().contains_key(&0));
    }

    #[tokio::test]
    async fn test_bitcoin_client_withdrawal() {
        let client = BitcoinL1Client::new(test_config());
        client.set_height(100);

        let receipt = client
            .initiate_withdrawal(&[0xBE; 32], 25_000)
            .await
            .unwrap();
        assert_eq!(receipt.withdrawal_id, 0);

        // Before challenge period expires
        let state = client.get_withdrawal_state(0).await.unwrap();
        assert_eq!(state, L1WithdrawalState::Pending);

        // After challenge period
        client.set_height(100 + CHALLENGE_PERIOD_BLOCKS as u64);
        let state = client.get_withdrawal_state(0).await.unwrap();
        assert_eq!(state, L1WithdrawalState::Completed);
    }

    #[tokio::test]
    async fn test_bitcoin_client_publish_epoch() {
        let client = BitcoinL1Client::new(test_config());
        let receipt = client.publish_epoch(1, [0xAB; 32]).await.unwrap();
        assert_ne!(receipt.tx_id, [0u8; 32]);
    }

    #[tokio::test]
    async fn test_bitcoin_client_chain_type() {
        let client = BitcoinL1Client::new(test_config());
        assert_eq!(client.chain_type(), ChainType::Bitcoin);
        assert_eq!(client.unit_name(), "sat");
    }

    #[tokio::test]
    async fn test_op_return_size_limit() {
        let _client = BitcoinL1Client::new(test_config());
        // Epoch OP_RETURN is 44 bytes, well within 80-byte limit
        let payload = encode_epoch_op_return(u64::MAX, &[0xFF; 32]);
        assert!(payload.len() <= 80);

        // Deposit OP_RETURN is 36 bytes
        let payload = encode_deposit_op_return(&[0xFF; 32]);
        assert!(payload.len() <= 80);
    }
}
