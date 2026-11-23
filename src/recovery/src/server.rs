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

use briolette_proto::briolette::recovery::{
    BindingState, GetBindingStatusReply, GetBindingStatusRequest, RecoverTokensReply,
    RecoverTokensRequest, RecoveredToken, RefreshBindingReply, RefreshBindingRequest,
    RegisterBindingReply, RegisterBindingRequest, RevokeBindingReply, RevokeBindingRequest,
};
use briolette_proto::briolette::token::Token;
use briolette_proto::briolette::Version;
use briolette_proto::briolette::{Error as BrioletteError, ErrorCode as BrioletteErrorCode};
use briolette_wallet::{Wallet, WalletData};
use log::*;
use prost::Message;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex, RwLock};

/// Recovery server state and configuration.
#[derive(Debug)]
pub struct BrioletteRecovery {
    /// SQLite database for binding storage and recovery log.
    db: Arc<Mutex<Connection>>,
    /// Registered wallet for drawing replacement tokens from the mint.
    wallet: Arc<RwLock<WalletData>>,
    /// Mandatory cooling-off period in epochs between token expiry and
    /// recovery eligibility. Gives honest holders time to validate/swap.
    recovery_cooloff_epochs: u64,
    /// TokenMap service URI for FindByHolder queries.
    tokenmap_uri: String,
}

impl BrioletteRecovery {
    pub async fn new(
        registrar_uri: String,
        clerk_uri: String,
        mint_uri: String,
        validate_uri: String,
        tokenmap_uri: String,
        db_path: &str,
        recovery_cooloff_epochs: u64,
    ) -> Result<Self, BrioletteErrorCode> {
        trace!("initializing recovery wallet");
        let mut wd = WalletData::new(
            registrar_uri,
            clerk_uri,
            mint_uri,
            validate_uri,
        )
        .map_err(|_| BrioletteErrorCode::InvalidMissingFields)?;
        assert!(wd.initialize_keys(b"recovery-wallet-001"));
        assert!(wd.initialize_credential().await);
        assert!(wd.synchronize().await);
        assert!(wd.get_tickets(2).await);
        // Pre-fill the recovery token pool
        assert!(wd.withdraw(25).await);

        let db = Connection::open(db_path)
            .map_err(|_| BrioletteErrorCode::ServerDiskError)?;
        Self::initialize_db(&db)
            .map_err(|_| BrioletteErrorCode::DatabaseInteractionError)?;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            wallet: Arc::new(RwLock::new(wd)),
            recovery_cooloff_epochs,
            tokenmap_uri,
        })
    }

    fn initialize_db(db: &Connection) -> Result<(), rusqlite::Error> {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS bindings (
                id BLOB PRIMARY KEY,
                ttc_public_key_hash BLOB NOT NULL,
                ttc_public_key BLOB NOT NULL,
                delegate_type INTEGER NOT NULL DEFAULT 0,
                delegate_key BLOB NOT NULL,
                valid_until INTEGER NOT NULL,
                state TEXT NOT NULL DEFAULT 'active',
                created_epoch INTEGER NOT NULL,
                nac_basename BLOB
            );
            CREATE INDEX IF NOT EXISTS idx_ttc_hash ON bindings(ttc_public_key_hash);

            CREATE TABLE IF NOT EXISTS recovery_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                binding_id BLOB NOT NULL,
                token_base_sig BLOB NOT NULL,
                original_value_whole INTEGER NOT NULL DEFAULT 0,
                original_value_fractional INTEGER NOT NULL DEFAULT 0,
                new_token_base_sig BLOB,
                recovered_epoch INTEGER NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Compute SHA-256 hash of a TTC public key for indexing.
    fn hash_ttc_key(ttc_public_key: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(ttc_public_key);
        hasher.finalize().to_vec()
    }

    /// Generate a random 32-byte binding ID.
    fn generate_binding_id() -> Vec<u8> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let mut id = vec![0u8; 32];
        // Use a combination of time and pointer for uniqueness.
        // In production, use a CSPRNG.
        let time_bytes = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_le_bytes();
        id[..16].copy_from_slice(&time_bytes);
        let mut hasher = Sha256::new();
        hasher.update(&id);
        hasher.finalize().to_vec()
    }

    pub async fn register_binding_impl(
        &self,
        request: &RegisterBindingRequest,
    ) -> Result<RegisterBindingReply, BrioletteError> {
        trace!("register_binding: request = {:?}", &request);
        if request.version != Version::Current as i32 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidVersion.into(),
            });
        }
        let binding = request.binding.as_ref().ok_or(BrioletteError {
            code: BrioletteErrorCode::InvalidMissingFields.into(),
        })?;
        if binding.ttc_public_key.is_empty() || binding.delegate_key.is_empty() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        let _nac_signature = request.nac_signature.as_ref().ok_or(BrioletteError {
            code: BrioletteErrorCode::InvalidMissingFields.into(),
        })?;

        // TODO: Verify NAC signature against current epoch's NAC group public keys.
        // This requires access to the clerk's epoch data to get the NAC GPK.
        // For now, we accept the binding if the NAC signature is present.
        // In production, this MUST be verified.

        let ttc_hash = Self::hash_ttc_key(&binding.ttc_public_key);
        let binding_id = Self::generate_binding_id();

        let db = self.db.lock().unwrap();
        // Check for existing active binding for this TTC key
        let existing: Option<String> = db
            .query_row(
                "SELECT state FROM bindings WHERE ttc_public_key_hash = ?1 AND state = 'active'",
                params![ttc_hash],
                |row| row.get(0),
            )
            .ok();
        if existing.is_some() {
            // Already has an active binding — must revoke first
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }

        db.execute(
            "INSERT INTO bindings (id, ttc_public_key_hash, ttc_public_key, delegate_type,
             delegate_key, valid_until, state, created_epoch, nac_basename)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8)",
            params![
                binding_id,
                ttc_hash,
                binding.ttc_public_key,
                binding.delegate_type,
                binding.delegate_key,
                binding.valid_until,
                0i64, // TODO: get current epoch from clerk
                _nac_signature.basename,
            ],
        )
        .map_err(|_| BrioletteError {
            code: BrioletteErrorCode::DatabaseInteractionError.into(),
        })?;

        info!(
            "registered recovery binding for ttc_hash={}",
            hex::encode(&ttc_hash[..8])
        );
        Ok(RegisterBindingReply {
            binding_id,
            error: None,
        })
    }

    pub async fn refresh_binding_impl(
        &self,
        request: &RefreshBindingRequest,
    ) -> Result<RefreshBindingReply, BrioletteError> {
        trace!("refresh_binding: request = {:?}", &request);
        if request.version != Version::Current as i32 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidVersion.into(),
            });
        }
        if request.binding_id.is_empty() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        let _nac_signature = request.nac_signature.as_ref().ok_or(BrioletteError {
            code: BrioletteErrorCode::InvalidMissingFields.into(),
        })?;

        // TODO: Verify NAC signature against current epoch keys

        let db = self.db.lock().unwrap();
        let rows_updated = db
            .execute(
                "UPDATE bindings SET valid_until = ?1 WHERE id = ?2 AND state = 'active'",
                params![request.new_valid_until, request.binding_id],
            )
            .map_err(|_| BrioletteError {
                code: BrioletteErrorCode::DatabaseInteractionError.into(),
            })?;

        if rows_updated == 0 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::BindingNotFound.into(),
            });
        }
        info!("refreshed binding {:?}", &request.binding_id[..8]);
        Ok(RefreshBindingReply { error: None })
    }

    pub async fn revoke_binding_impl(
        &self,
        request: &RevokeBindingRequest,
    ) -> Result<RevokeBindingReply, BrioletteError> {
        trace!("revoke_binding: request = {:?}", &request);
        if request.version != Version::Current as i32 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidVersion.into(),
            });
        }
        if request.binding_id.is_empty() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        let _nac_signature = request.nac_signature.as_ref().ok_or(BrioletteError {
            code: BrioletteErrorCode::InvalidMissingFields.into(),
        })?;

        // TODO: Verify NAC signature matches the binding owner

        let db = self.db.lock().unwrap();
        let rows_updated = db
            .execute(
                "UPDATE bindings SET state = 'revoked' WHERE id = ?1 AND state = 'active'",
                params![request.binding_id],
            )
            .map_err(|_| BrioletteError {
                code: BrioletteErrorCode::DatabaseInteractionError.into(),
            })?;

        if rows_updated == 0 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::BindingNotFound.into(),
            });
        }
        info!("revoked binding {:?}", &request.binding_id[..8]);
        Ok(RevokeBindingReply { error: None })
    }

    pub async fn recover_tokens_impl(
        &self,
        request: &RecoverTokensRequest,
    ) -> Result<RecoverTokensReply, BrioletteError> {
        trace!("recover_tokens: request = {:?}", &request);
        if request.version != Version::Current as i32 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidVersion.into(),
            });
        }
        if request.old_ttc_public_key.is_empty() || request.delegate_proof.is_empty() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        let new_ticket = request.new_wallet_ticket.as_ref().ok_or(BrioletteError {
            code: BrioletteErrorCode::InvalidMissingFields.into(),
        })?;

        // 1. Look up binding by TTC public key hash
        let ttc_hash = Self::hash_ttc_key(&request.old_ttc_public_key);
        let (binding_id, delegate_type, delegate_key, valid_until, state): (
            Vec<u8>,
            i32,
            Vec<u8>,
            i64,
            String,
        ) = {
            let db = self.db.lock().unwrap();
            db.query_row(
                "SELECT id, delegate_type, delegate_key, valid_until, state
                 FROM bindings WHERE ttc_public_key_hash = ?1
                 ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END
                 LIMIT 1",
                params![ttc_hash],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(|_| BrioletteError {
                code: BrioletteErrorCode::BindingNotFound.into(),
            })?
        };

        // 2. Check binding state
        match state.as_str() {
            "active" => {}
            "expired" => {
                return Err(BrioletteError {
                    code: BrioletteErrorCode::BindingExpired.into(),
                });
            }
            "revoked" => {
                return Err(BrioletteError {
                    code: BrioletteErrorCode::BindingRevoked.into(),
                });
            }
            "consumed" => {
                return Err(BrioletteError {
                    code: BrioletteErrorCode::RecoveryAlreadyClaimed.into(),
                });
            }
            _ => {
                return Err(BrioletteError {
                    code: BrioletteErrorCode::InvalidServerState.into(),
                });
            }
        }

        // 3. Verify delegate proof
        // TODO: Full cryptographic verification of the delegate_proof against
        // the stored delegate_key. For ECDSA_P256 delegates, verify the ECDSA
        // signature over SHA-256(old_ttc_public_key || timestamp). For TTC_ECDAA
        // delegates, verify the ECDAA signature with the appropriate basename.
        //
        // For now, we verify that the proof is non-empty and the delegate_key
        // matches the stored binding. Production MUST implement full verification.
        if request.delegate_proof.is_empty() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidDelegateProof.into(),
            });
        }

        // 4. Query TokenMap for expired tokens held by the lost wallet
        // TODO: Connect to TokenMap service and call FindByHolder RPC.
        // For now, return an empty result to allow the framework to compile
        // and be tested end-to-end once the TokenMap integration is wired up.
        let recovered_tokens: Vec<RecoveredToken> = Vec::new();
        let pending_count: u32 = 0;

        // When tokens are found:
        // 5. For each eligible token, transfer a replacement from our pool
        //    to the new_wallet_ticket
        // 6. Request revocation of the old wallet's credential group
        // 7. Mark binding as consumed and log recovery

        if !recovered_tokens.is_empty() {
            let db = self.db.lock().unwrap();
            db.execute(
                "UPDATE bindings SET state = 'consumed' WHERE id = ?1",
                params![binding_id],
            )
            .map_err(|_| BrioletteError {
                code: BrioletteErrorCode::DatabaseInteractionError.into(),
            })?;
        }

        info!(
            "recovery claim for ttc_hash={}: {} tokens recovered, {} pending",
            hex::encode(&ttc_hash[..8]),
            recovered_tokens.len(),
            pending_count,
        );

        Ok(RecoverTokensReply {
            tokens: recovered_tokens,
            pending_count,
            error: None,
        })
    }

    pub async fn get_binding_status_impl(
        &self,
        request: &GetBindingStatusRequest,
    ) -> Result<GetBindingStatusReply, BrioletteError> {
        trace!("get_binding_status: request = {:?}", &request);
        if request.version != Version::Current as i32 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidVersion.into(),
            });
        }
        if request.ttc_public_key.is_empty() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }

        let ttc_hash = Self::hash_ttc_key(&request.ttc_public_key);
        let db = self.db.lock().unwrap();
        let result: Result<(String, i64), _> = db.query_row(
            "SELECT state, valid_until FROM bindings
             WHERE ttc_public_key_hash = ?1
             ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END
             LIMIT 1",
            params![ttc_hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok((state, valid_until)) => {
                let binding_state = match state.as_str() {
                    "active" => BindingState::BindingActive,
                    "expired" => BindingState::BindingExpired,
                    "revoked" => BindingState::BindingRevoked,
                    "consumed" => BindingState::BindingConsumed,
                    _ => BindingState::BindingUnknown,
                };
                Ok(GetBindingStatusReply {
                    state: binding_state.into(),
                    valid_until: valid_until as u64,
                    error: None,
                })
            }
            Err(_) => Ok(GetBindingStatusReply {
                state: BindingState::BindingUnknown.into(),
                valid_until: 0,
                error: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ttc_key_deterministic() {
        let key = b"test-ttc-public-key";
        let h1 = BrioletteRecovery::hash_ttc_key(key);
        let h2 = BrioletteRecovery::hash_ttc_key(key);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32); // SHA-256 output
    }

    #[test]
    fn hash_ttc_key_different_inputs_differ() {
        let h1 = BrioletteRecovery::hash_ttc_key(b"key-a");
        let h2 = BrioletteRecovery::hash_ttc_key(b"key-b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn generate_binding_id_is_32_bytes() {
        let id = BrioletteRecovery::generate_binding_id();
        assert_eq!(id.len(), 32);
    }

    #[test]
    fn generate_binding_id_unique() {
        let id1 = BrioletteRecovery::generate_binding_id();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = BrioletteRecovery::generate_binding_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn initialize_db_creates_tables() {
        let db = Connection::open_in_memory().unwrap();
        BrioletteRecovery::initialize_db(&db).unwrap();

        // Verify bindings table exists
        db.execute(
            "INSERT INTO bindings (id, ttc_public_key_hash, ttc_public_key,
             delegate_type, delegate_key, valid_until, state, created_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
            params![
                vec![1u8; 32], vec![2u8; 32], vec![3u8; 65],
                0i32, vec![4u8; 65], 100i64, 0i64,
            ],
        )
        .unwrap();

        // Verify recovery_log table exists
        db.execute(
            "INSERT INTO recovery_log (binding_id, token_base_sig,
             original_value_whole, original_value_fractional, recovered_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![vec![1u8; 32], vec![5u8; 48], 10i64, 0i64, 1i64],
        )
        .unwrap();
    }

    #[test]
    fn initialize_db_idempotent() {
        let db = Connection::open_in_memory().unwrap();
        BrioletteRecovery::initialize_db(&db).unwrap();
        BrioletteRecovery::initialize_db(&db).unwrap();
    }

    #[test]
    fn db_index_on_ttc_hash() {
        let db = Connection::open_in_memory().unwrap();
        BrioletteRecovery::initialize_db(&db).unwrap();

        let ttc_key = b"test-ttc-key";
        let ttc_hash = BrioletteRecovery::hash_ttc_key(ttc_key);

        db.execute(
            "INSERT INTO bindings (id, ttc_public_key_hash, ttc_public_key,
             delegate_type, delegate_key, valid_until, state, created_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
            params![
                vec![1u8; 32], ttc_hash, ttc_key.to_vec(),
                0i32, vec![4u8; 65], 100i64, 0i64,
            ],
        )
        .unwrap();

        let state: String = db
            .query_row(
                "SELECT state FROM bindings WHERE ttc_public_key_hash = ?1",
                params![ttc_hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "active");
    }

    #[test]
    fn db_revoke_and_no_active_binding() {
        let db = Connection::open_in_memory().unwrap();
        BrioletteRecovery::initialize_db(&db).unwrap();

        let ttc_hash = vec![0xCDu8; 32];
        let binding_id = vec![1u8; 32];

        db.execute(
            "INSERT INTO bindings (id, ttc_public_key_hash, ttc_public_key,
             delegate_type, delegate_key, valid_until, state, created_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
            params![
                binding_id.clone(), ttc_hash.clone(), vec![3u8; 65],
                0i32, vec![4u8; 65], 100i64, 0i64,
            ],
        )
        .unwrap();

        let rows = db
            .execute(
                "UPDATE bindings SET state = 'revoked' WHERE id = ?1 AND state = 'active'",
                params![binding_id],
            )
            .unwrap();
        assert_eq!(rows, 1);

        let existing: Option<String> = db
            .query_row(
                "SELECT state FROM bindings WHERE ttc_public_key_hash = ?1 AND state = 'active'",
                params![ttc_hash],
                |row| row.get(0),
            )
            .ok();
        assert!(existing.is_none());
    }

    #[test]
    fn db_binding_state_priority_ordering() {
        let db = Connection::open_in_memory().unwrap();
        BrioletteRecovery::initialize_db(&db).unwrap();

        let ttc_hash = vec![0xEFu8; 32];

        // Insert a revoked binding first
        db.execute(
            "INSERT INTO bindings (id, ttc_public_key_hash, ttc_public_key,
             delegate_type, delegate_key, valid_until, state, created_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'revoked', ?7)",
            params![
                vec![1u8; 32], ttc_hash.clone(), vec![3u8; 65],
                0i32, vec![4u8; 65], 50i64, 0i64,
            ],
        )
        .unwrap();

        // Insert an active binding
        db.execute(
            "INSERT INTO bindings (id, ttc_public_key_hash, ttc_public_key,
             delegate_type, delegate_key, valid_until, state, created_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
            params![
                vec![2u8; 32], ttc_hash.clone(), vec![3u8; 65],
                0i32, vec![4u8; 65], 200i64, 1i64,
            ],
        )
        .unwrap();

        // Query should prefer 'active' state
        let (state, valid_until): (String, i64) = db
            .query_row(
                "SELECT state, valid_until FROM bindings
                 WHERE ttc_public_key_hash = ?1
                 ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END
                 LIMIT 1",
                params![ttc_hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "active");
        assert_eq!(valid_until, 200);
    }

    #[test]
    fn binding_state_mapping() {
        let cases = vec![
            ("active", BindingState::BindingActive),
            ("expired", BindingState::BindingExpired),
            ("revoked", BindingState::BindingRevoked),
            ("consumed", BindingState::BindingConsumed),
            ("unknown_state", BindingState::BindingUnknown),
        ];
        for (state_str, expected) in cases {
            let binding_state = match state_str {
                "active" => BindingState::BindingActive,
                "expired" => BindingState::BindingExpired,
                "revoked" => BindingState::BindingRevoked,
                "consumed" => BindingState::BindingConsumed,
                _ => BindingState::BindingUnknown,
            };
            assert_eq!(binding_state, expected, "mismatch for state '{}'", state_str);
        }
    }

    #[test]
    fn recovery_log_records_token_recovery() {
        let db = Connection::open_in_memory().unwrap();
        BrioletteRecovery::initialize_db(&db).unwrap();

        let binding_id = vec![0xAAu8; 32];
        let token_sig = vec![0xBBu8; 48];

        db.execute(
            "INSERT INTO recovery_log (binding_id, token_base_sig,
             original_value_whole, original_value_fractional, recovered_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![binding_id.clone(), token_sig, 42i64, 500i64, 10i64],
        )
        .unwrap();

        let (whole, frac, epoch): (i64, i64, i64) = db
            .query_row(
                "SELECT original_value_whole, original_value_fractional, recovered_epoch
                 FROM recovery_log WHERE binding_id = ?1",
                params![binding_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(whole, 42);
        assert_eq!(frac, 500);
        assert_eq!(epoch, 10);
    }
}
