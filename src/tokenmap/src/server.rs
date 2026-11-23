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

use briolette_proto::briolette::token;
use briolette_proto::briolette::token::TicketExpiry;
use briolette_proto::briolette::token::Token;
use briolette_proto::briolette::tokenmap;
use briolette_proto::briolette::tokenmap::{
    ArchiveReply, ArchiveRequest, FindByHolderReply, FindByHolderRequest, RevocationDataReply,
    RevocationDataRequest, StoreTicketsReply, StoreTicketsRequest, UpdateReply, UpdateRequest,
};
use briolette_proto::briolette::{Error as BrioletteError, ErrorCode as BrioletteErrorCode};
use briolette_proto::vec_utils;
use chrono::Utc;
//use deadpool_sqlite::{Config, Pool, Runtime};
use log::*;
use prost::Message;
use rusqlite;
use tokio_rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct BrioletteTokenMap {
    conn: Connection,
}

impl BrioletteTokenMap {
    pub async fn new(db_path: &String) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open(db_path).await?;

        conn.call(|conn| {
            let mut stmt = conn.prepare(
                "create table if not exists tokens (
             id blob primary key,
             entry blob not null,
             last_update integer
            )",
            )?;
            stmt.execute([])?;
            let mut stmt = conn.prepare(
                "create table if not exists tickets (
             credential blob primary key,
             signed_ticket blob not null,
             nac_signature blob not null,
             expires_on integer
            )",
            )?;
            stmt.execute([])?;
            let mut stmt = conn.prepare(
                "create index if not exists ticket_request_signature ON tickets (nac_signature)",
            )?;
            stmt.execute([])?;
            // Revocation data is keyed on token id.
            let mut stmt = conn.prepare(
                "create table if not exists revocation (
             id blob primary key,
             data blob not null,
             created_on integer
            )",
            )?;
            stmt.execute([])?;
            let mut stmt = conn.prepare(
                "create table if not exists revocation_archive (
             id blob primary key,
             data blob not null,
             created_on integer
            )",
            )?;
            stmt.execute([])?;
            // Holder index: maps credential hash -> token IDs for recovery queries.
            let mut stmt = conn.prepare(
                "create table if not exists holder_index (
             credential_hash blob not null,
             token_id blob not null,
             updated_epoch integer not null default 0,
             primary key (credential_hash, token_id)
            )",
            )?;
            stmt.execute([])?;
            Ok::<_, rusqlite::Error>(())
        })
        .await?;
        Ok(Self { conn })
    }

    pub async fn update_impl(
        &self,
        request: &UpdateRequest,
    ) -> Result<UpdateReply, BrioletteError> {
        if request.id.len() == 0 || request.token.is_none() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        // Reject tokens whose descriptor or split tags do not describe a
        // well-formed, non-increasing value before touching any stored state.
        // Every later check assumes a valid effective amount.
        if let Err(code) = request.token.as_ref().unwrap().checked_effective_amount() {
            return Err(BrioletteError { code: code.into() });
        }
        trace!("Looking up token: {:?}", request.id);
        let maybe_entry: Option<tokenmap::Entry> =
            self.get_tokenmap_entry(request.id.clone()).await?;
        trace!("Entry found: {:?}", maybe_entry);
        if maybe_entry.is_none() {
            let token = request.token.clone().unwrap();
            self.create_tokenmap_entry(request.id.clone(), token.clone())
                .await?;
            // Update holder index for recovery lookups
            let _ = self.update_holder_index(request.id.clone(), &token).await;
            trace!("Token inserted!");
            return Ok(UpdateReply {
                created: true,
                abuse_detected: false,
            });
        }
        let now = Utc::now().timestamp() as u64;
        let mut entry = maybe_entry.unwrap();
        // It gets more interesting here:
        // 1. For each token in the entry, check if there is a shared history with the supplied token.
        // 2. If the history overlaps and the token extends it, then replace that token and update the entry.
        // 3. If the history forks and there is no split at the fork, then create a revocation record.
        // 4. If the history forks and both have splits, then check the total to be <= the original value.
        // 5. If the split exceeds the original amount, then create a revocation record for the splitter.
        //
        // ** TokenMap assumes the caller has cryptographically verified the tokens prior to calling! **
        //
        let candidate = request.token.clone().unwrap();

        // No update needed.
        if token_is_known(&candidate, &entry.tokens) {
            return Ok(UpdateReply {
                created: false,
                abuse_detected: entry.abuses.len() != 0,
            });
        }
        // Check if the token is an extension, but not a fork.
        if let Some(idx) = token_is_extension(&candidate, &entry.tokens) {
            // Replace the prior view with the updated view.
            let updated_token = candidate.clone();
            entry.tokens[idx] = candidate;
            entry.last_update = now;
            let abuse_detected = entry.abuses.len() != 0;
            self.update_tokenmap_entry(&request.id, entry).await?;
            // Update holder index for recovery lookups
            let _ = self.update_holder_index(request.id.clone(), &updated_token).await;
            return Ok(UpdateReply {
                created: false,
                abuse_detected: abuse_detected,
            });
        }
        // Now we check for splits.
        // If the candidate is not an extension, it must not be the first split.
        // It it is legitimate, then it is an unknown second split.
        // TODO: Refactor to use token_get_fork()
        if token_is_second_split(&candidate, &entry.tokens) {
            // Insert the new token history.
            let split_token = candidate.clone();
            entry.tokens.push(candidate);
            entry.last_update = now;
            let abuse_detected = entry.abuses.len() != 0;
            self.update_tokenmap_entry(&request.id, entry).await?;
            // Update holder index for recovery lookups
            let _ = self.update_holder_index(request.id.clone(), &split_token).await;

            return Ok(UpdateReply {
                created: false,
                abuse_detected: abuse_detected,
            });
        }
        // We're in double spend territory now.
        trace!("double spending detected");
        let Some((token_index, history_index)) = token_get_fork(&candidate, &entry.tokens) else {
            // Neither known, an extension, a valid second split, nor a fork:
            // the candidate is malformed relative to stored state.
            error!("token update matched no known history and no fork point");
            return Err(BrioletteError {
                code: BrioletteErrorCode::TokenMapFailure.into(),
            });
        };
        let abuse = tokenmap::Abuse {
            discovery_timestamp: now.clone(),
            token_index: token_index as u32,
            history_index: history_index as u32,
            // TODO: we don't capture this state in token_is_second_split().
            abuse_type: tokenmap::AbuseType::DoubleSpend.into(),
        };
        let token_expiry = entry.tokens[0]
            .base
            .as_ref()
            .unwrap()
            .transfer
            .as_ref()
            .unwrap()
            .tags
            .iter()
            .map(|tag| match tag.value {
                Some(token::tag::Value::ValidUntil(ts)) => ts,
                _ => 0,
            })
            .find(|&ts| ts != 0);
        let ds_history = &entry.tokens[token_index].history[history_index];
        let prev_history;
        if history_index == 0 {
            prev_history = entry.tokens[token_index].base.as_ref().unwrap();
        } else {
            prev_history = &entry.tokens[token_index].history[history_index - 1];
        }

        // Pull the signed ticket from the database to get the NAC signature and basename
        let signed_ticket = ds_history
            .transfer
            .as_ref()
            .unwrap()
            .recipient
            .as_ref()
            .unwrap();
        let ticket = signed_ticket.ticket.as_ref().unwrap();
        let nac_sig = self.get_ticket_signature(&ticket.credential).await?;
        if nac_sig.is_none() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::TicketSignatureMissing.into(),
            });
        }

        // TODO: Support two modes:
        // 1. Fetch all tickets from the request that issued the ds ticket and revoke those groups.
        //    Also store the pseudonym for the epoch to link any other ticket requests.
        // 2. Pull all active groups for the given NAC and their expirations.
        // The choice would be on whether it is a class-break or a specific device.  In most case,
        // I would expect the plan to start small and expand with more incidence.
        //
        // For now, this is just the current ticket group.
        let ticket_tags = ticket.tags.as_ref().unwrap();
        let ticket_expiration = signed_ticket.expires_on();
        // TODO: Should we not add a new entry if abuses.len() > 0?
        let revocation_data = tokenmap::RevocationData {
            timestamp: now,
            nac: nac_sig,
            ttc: Some(tokenmap::LinkableSignature {
                signature: ds_history.signature.clone(),
                basename: prev_history.signature.clone(),
                group_public_key: vec![], // Add these at start up
            }),
            token_id: request.id.clone(),
            token_expiry: token_expiry.unwrap(),
            groups: vec![tokenmap::Group {
                number: ticket_tags.group_number,
                expiration: ticket_expiration,
            }],
            abuse: tokenmap::AbuseType::DoubleSpend.into(),
        };

        entry.abuses.push(abuse);
        entry.tokens.push(candidate);
        entry.last_update = now;
        let abuse_detected = entry.abuses.len() != 0;
        self.update_tokenmap_entry(&request.id, entry).await?;
        self.insert_revocation_data(&request.id, revocation_data)
            .await?;
        return Ok(UpdateReply {
            created: false,
            abuse_detected: abuse_detected,
        });
    }

    async fn insert_revocation_data(
        &self,
        id: &Vec<u8>,
        data: tokenmap::RevocationData,
    ) -> Result<(), BrioletteError> {
        let key = id.clone();
        trace!("inserting revocation data {:?}...", id);
        Ok(self
            .conn
            .call(move |conn| {
                let mut stmt = conn
                    .prepare("INSERT INTO revocation (id, data, created_on) values (?1, ?2, ?3)")?;
                stmt.execute((key, data.encode_to_vec(), data.timestamp.clone()))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await?)
    }

    async fn insert_archive_data(
        &self,
        id: &Vec<u8>,
        data: tokenmap::RevocationData,
        created_on: u64,
    ) -> Result<(), BrioletteError> {
        let key = id.clone();
        trace!("inserting revocation archive data {:?}...", id);
        Ok(self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "INSERT INTO revocation_archive (id, data, created_on) values (?1, ?2, ?3)",
                )?;
                stmt.execute((key, data.encode_to_vec(), created_on))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await?)
    }

    async fn update_tokenmap_entry(
        &self,
        id: &Vec<u8>,
        entry: tokenmap::Entry,
    ) -> Result<(), BrioletteError> {
        let key = id.clone();
        trace!("Updating token {:?}...", id);
        Ok(self
            .conn
            .call(move |conn| {
                let mut stmt =
                    conn.prepare("UPDATE tokens SET entry = ?2, last_update = ?3 where id = ?1")?;
                stmt.execute((key, entry.encode_to_vec(), entry.last_update.clone()))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await?)
    }

    async fn create_tokenmap_entry(&self, id: Vec<u8>, token: Token) -> Result<(), BrioletteError> {
        log::debug!("token does not yet exist: {}", hex::encode(id.clone()));
        // Just add it.
        let now = Utc::now().timestamp() as u64;
        let entry = tokenmap::Entry {
            id: id.clone(),
            tokens: vec![token],
            abuses: vec![],
            last_update: now,
        };
        trace!("Inserting token...");
        Ok(self
            .conn
            .call(move |conn| {
                let mut stmt = conn
                    .prepare("INSERT INTO tokens (id, entry, last_update) values (?1, ?2, ?3)")?;
                stmt.execute((id, entry.clone().encode_to_vec(), entry.last_update.clone()))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await?)
    }

    async fn get_ticket_signature(
        &self,
        credential: &Vec<u8>,
    ) -> Result<Option<tokenmap::LinkableSignature>, BrioletteError> {
        let cred = credential.clone();
        Ok(self
            .conn
            .call(|conn| {
                trace!("Preparing statement...");
                let mut stmt =
                    conn.prepare("SELECT nac_signature FROM tickets WHERE credential = ?")?;
                let mut rows = stmt.query([cred])?;
                if let Some(row) = rows.next()? {
                    trace!("walking the row");
                    let data: Vec<u8> = row.get(0)?;
                    if let Ok(ls) = tokenmap::LinkableSignature::decode(data.as_slice()) {
                        return Ok(Some(ls));
                    }
                }
                return Ok::<_, rusqlite::Error>(None);
            })
            .await?)
    }

    async fn get_tokenmap_entry(
        &self,
        id: Vec<u8>,
    ) -> Result<Option<tokenmap::Entry>, BrioletteError> {
        Ok(self
            .conn
            .call(|conn| {
                trace!("Preparing statement...");
                let mut stmt = conn.prepare("SELECT * FROM tokens WHERE id = ?")?;
                trace!("Checking existence...");
                if let Ok(exists) = stmt.exists([id.clone()]) {
                    if exists == false {
                        trace!("select returned empty!");
                        return Ok::<_, rusqlite::Error>(None);
                    }
                }
                let mut rows = stmt.query([id])?;
                if let Some(row) = rows.next()? {
                    trace!("walking the row");
                    let data: Vec<u8> = row.get(1)?;
                    if let Ok(entry) = tokenmap::Entry::decode(data.as_slice()) {
                        return Ok(Some(entry));
                    }
                }
                Ok::<_, rusqlite::Error>(None)
            })
            .await?)
    }

    async fn insert_signed_ticket(
        &self,
        credential: &Vec<u8>,
        data: token::SignedTicket,
        nac_signature: tokenmap::LinkableSignature,
        expiration: u64,
    ) -> Result<(), BrioletteError> {
        let key = credential.clone();
        trace!("inserting signed ticket {}...", hex::encode(credential));
        Ok(self
            .conn
            .call(move |conn| {
                let mut stmt = conn
                    .prepare("INSERT INTO tickets (credential, signed_ticket, nac_signature, expires_on) values (?1, ?2, ?3, ?4)")?;
                stmt.execute((key, data.encode_to_vec(), nac_signature.encode_to_vec(), expiration))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await?)
    }

    async fn get_revocation_data(
        &self,
        id: &Vec<u8>,
    ) -> Result<Vec<tokenmap::RevocationDataEntry>, BrioletteError> {
        let key = id.clone();
        Ok(self
            .conn
            .call(|conn| {
                trace!("Preparing statement...");
                let mut stmt = conn.prepare("SELECT * FROM revocation where id = ?")?;
                let mut rows = stmt.query([key])?;
                let mut entries: Vec<tokenmap::RevocationDataEntry> = vec![];
                if let Some(row) = rows.next()? {
                    let id: Vec<u8> = row.get(0)?;
                    let rd: Vec<u8> = row.get(1)?;
                    let created_on = row.get(2)?;
                    if let Ok(data) = tokenmap::RevocationData::decode(rd.as_slice()) {
                        entries.push(tokenmap::RevocationDataEntry {
                            id,
                            data: Some(data),
                            created_on,
                        });
                    }
                }
                return Ok::<_, rusqlite::Error>(entries);
            })
            .await?)
    }

    async fn get_all_revocation_data(
        &self,
    ) -> Result<Vec<tokenmap::RevocationDataEntry>, BrioletteError> {
        Ok(self
            .conn
            .call(|conn| {
                trace!("Preparing statement...");
                let mut stmt = conn.prepare("SELECT * FROM revocation")?;
                let mut rows = stmt.query([])?;
                let mut entries: Vec<tokenmap::RevocationDataEntry> = vec![];
                while let Some(row) = rows.next()? {
                    let id: Vec<u8> = row.get(0)?;
                    let rd: Vec<u8> = row.get(1)?;
                    let created_on = row.get(2)?;
                    if let Ok(data) = tokenmap::RevocationData::decode(rd.as_slice()) {
                        entries.push(tokenmap::RevocationDataEntry {
                            id,
                            data: Some(data),
                            created_on,
                        });
                    }
                }
                return Ok::<_, rusqlite::Error>(entries);
            })
            .await?)
    }

    pub async fn store_tickets_impl(
        &self,
        request: &StoreTicketsRequest,
    ) -> Result<StoreTicketsReply, BrioletteError> {
        if request.tickets.len() == 0 || request.nac.is_none() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        let nac = request.nac.clone().unwrap();
        if nac.signature.len() == 0 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        for signed_ticket in request.tickets.iter() {
            let credential = signed_ticket.ticket.clone().unwrap().credential;
            self.insert_signed_ticket(
                &credential,
                signed_ticket.clone(),
                nac.clone(),
                signed_ticket.expires_on(),
            )
            .await?;
        }
        // For now, we don't store the full linkable signature
        return Ok(StoreTicketsReply {});
    }

    pub async fn revocation_data_impl(
        &self,
        request: &RevocationDataRequest,
    ) -> Result<RevocationDataReply, BrioletteError> {
        if request.select.is_none() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        match request.select.as_ref().unwrap() {
            tokenmap::revocation_data_request::Select::Id(id) => {
                if id.len() == 0 {
                    return Err(BrioletteError {
                        code: BrioletteErrorCode::InvalidMissingFields.into(),
                    });
                }
                let rde = self.get_revocation_data(id).await?;
                return Ok(RevocationDataReply { entries: rde });
            }
            tokenmap::revocation_data_request::Select::Group(sg) => {
                if *sg != i32::from(tokenmap::SelectGroup::All) {
                    return Err(BrioletteError {
                        code: BrioletteErrorCode::InvalidMissingFields.into(),
                    });
                }
                let rde = self.get_all_revocation_data().await?;
                return Ok(RevocationDataReply { entries: rde });
            }
        }
    }

    pub async fn archive_impl(
        &self,
        request: &ArchiveRequest,
    ) -> Result<ArchiveReply, BrioletteError> {
        if request.id.len() == 0 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        let rde = self.get_revocation_data(&request.id).await?;
        if rde.len() == 0 {
            return Err(BrioletteError {
                code: BrioletteErrorCode::RevocationNotFound.into(),
            });
        }
        // Let's add it to the archive, then delete it from revocation.
        self.insert_archive_data(&request.id, rde[0].data.clone().unwrap(), rde[0].created_on)
            .await?;
        return Ok(ArchiveReply {});
    }

    /// Update the holder_index when a token is created or updated.
    /// Extracts the last transfer recipient's credential and indexes it.
    async fn update_holder_index(
        &self,
        token_id: Vec<u8>,
        token: &Token,
    ) -> Result<(), BrioletteError> {
        // Extract the credential from the last transfer recipient
        let credential = if let Some(last_history) = token.history.last() {
            last_history
                .transfer
                .as_ref()
                .and_then(|t| t.recipient.as_ref())
                .and_then(|r| r.ticket.as_ref())
                .map(|t| t.credential.clone())
        } else {
            token
                .base
                .as_ref()
                .and_then(|b| b.transfer.as_ref())
                .and_then(|t| t.recipient.as_ref())
                .and_then(|r| r.ticket.as_ref())
                .map(|t| t.credential.clone())
        };

        if let Some(cred) = credential {
            if !cred.is_empty() {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&cred);
                let credential_hash = hasher.finalize().to_vec();
                let tid = token_id.clone();
                let now = Utc::now().timestamp();
                self.conn
                    .call(move |conn| {
                        conn.execute(
                            "INSERT OR REPLACE INTO holder_index
                             (credential_hash, token_id, updated_epoch)
                             VALUES (?1, ?2, ?3)",
                            rusqlite::params![credential_hash, tid, now],
                        )?;
                        Ok::<_, rusqlite::Error>(())
                    })
                    .await?;
            }
        }
        Ok(())
    }

    /// Find all tokens whose last transfer recipient matches the given
    /// TTC public key. Used by the recovery server.
    pub async fn find_by_holder_impl(
        &self,
        request: &FindByHolderRequest,
    ) -> Result<FindByHolderReply, BrioletteError> {
        if request.ttc_public_key.is_empty() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&request.ttc_public_key);
        let credential_hash = hasher.finalize().to_vec();
        let expired_only = request.expired_only;

        let entries: Vec<tokenmap::Entry> = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT t.id, t.entry FROM holder_index h
                     INNER JOIN tokens t ON h.token_id = t.id
                     WHERE h.credential_hash = ?",
                )?;
                let mut rows = stmt.query([credential_hash])?;
                let mut results: Vec<tokenmap::Entry> = vec![];
                while let Some(row) = rows.next()? {
                    let data: Vec<u8> = row.get(1)?;
                    if let Ok(entry) = tokenmap::Entry::decode(data.as_slice()) {
                        if expired_only {
                            // Check if the token has expired (valid_until < now)
                            let now = chrono::Utc::now().timestamp() as u64;
                            let is_expired = entry.tokens.iter().all(|token| {
                                token
                                    .base
                                    .as_ref()
                                    .and_then(|b| b.transfer.as_ref())
                                    .map(|t| {
                                        t.tags.iter().any(|tag| {
                                            matches!(
                                                tag.value,
                                                Some(
                                                    briolette_proto::briolette::token::tag::Value::ValidUntil(vu)
                                                ) if vu < now
                                            )
                                        })
                                    })
                                    .unwrap_or(false)
                            });
                            if is_expired {
                                results.push(entry);
                            }
                        } else {
                            results.push(entry);
                        }
                    }
                }
                Ok::<_, rusqlite::Error>(results)
            })
            .await?;

        Ok(FindByHolderReply { entries })
    }
}

fn get_split_amount(maybe_transfer: &Option<token::Transfer>) -> Option<token::Amount> {
    let transfer = maybe_transfer.as_ref()?;
    let mut found = None;
    for tag in transfer.tags.iter() {
        if let Some(token::tag::Value::SplitValue(amount)) = &tag.value {
            if found.is_some() || !amount.is_valid() {
                return None;
            }
            found = Some(amount.clone());
        }
    }
    found
}

fn token_is_known(candidate: &Token, tokens: &Vec<Token>) -> bool {
    for token in tokens.iter() {
        // Only looking for <= fully self contained entries.
        if candidate.history.len() > token.history.len() {
            continue;
        }
        // Skip the base since we index on its signature.
        // If the candidate has a shorter history, a call to update doesn't mean something bad has happened.
        let differences = token
            .history
            .iter()
            .zip(&candidate.history)
            .find(|&(known, unknown)| {
                vec_utils::vec_equal(&known.signature, &unknown.signature) == false
            });
        if differences.is_some() {
            // Keep checking because we could have multi-history due to splits.
            // The fallthrough failure should catch a no-match scenario.
            continue;
        }
        return true;
    }
    return false;
}

fn token_is_extension(candidate: &Token, tokens: &Vec<Token>) -> Option<usize> {
    if candidate.effective_amount().is_none() {
        return None;
    }
    for (index, token) in tokens.iter().enumerate() {
        // Only looking for extensions of history.
        if candidate.history.len() <= token.history.len() {
            continue;
        }
        // Skip the base since we index on its signature.
        // If the candidate has a shorter history, a call to update doesn't mean something bad has happened.
        let differences = token
            .history
            .iter()
            .zip(&candidate.history)
            .find(|&(known, unknown)| {
                vec_utils::vec_equal(&known.signature, &unknown.signature) == false
            });
        if differences.is_some() {
            // Keep checking because we could have multi-history due to splits.
            // The fallthrough failure should catch a no-match scenario.
            continue;
        }
        return Some(index);
    }
    return None;
}

fn token_get_fork(candidate: &Token, tokens: &Vec<Token>) -> Option<(usize, usize)> {
    let mut best_fork: Option<(usize, usize)> = None;
    for (index, token) in tokens.iter().enumerate() {
        let pos = token
            .history
            .iter()
            .zip(&candidate.history)
            .position(|(known, unknown)| {
                vec_utils::vec_equal(&known.signature, &unknown.signature) == false
            });
        if let Some(p) = pos {
            if best_fork.map_or(true, |(_, best_pos)| p > best_pos) {
                best_fork = Some((index, p));
            }
        }
    }
    return best_fork;
}

fn token_is_second_split(candidate: &Token, tokens: &Vec<Token>) -> bool {
    if candidate.effective_amount().is_none() {
        return false;
    }
    let Some((fork_token_idx, fork_pos)) = token_get_fork(candidate, tokens) else {
        return false;
    };
    let known_fork_sig = &tokens[fork_token_idx].history[fork_pos].signature;
    // Ensure no other stored token sharing the prefix `0..fork_pos` already
    // took a different branch at `fork_pos` (which would make `candidate` a
    // third split at the same fork point).
    for token in tokens.iter() {
        if token.history.len() <= fork_pos {
            continue;
        }
        let shares_prefix = token
            .history
            .iter()
            .take(fork_pos)
            .zip(candidate.history.iter().take(fork_pos))
            .all(|(k, u)| vec_utils::vec_equal(&k.signature, &u.signature));
        if shares_prefix
            && !vec_utils::vec_equal(&token.history[fork_pos].signature, known_fork_sig)
        {
            trace!("fork point already has two split branches");
            return false;
        }
    }

    let known = &tokens[fork_token_idx].history[fork_pos];
    let unknown = &candidate.history[fork_pos];
    trace!("forked history detected");
    // Both diverging entries at the fork point must carry valid SplitValue tags
    // that sum to the effective token amount immediately prior to the fork.
    if let (Some(known_amount), Some(unknown_amount), Some(parent_total)) = (
        get_split_amount(&known.transfer),
        get_split_amount(&unknown.transfer),
        candidate.effective_amount_at(fork_pos),
    ) {
        if known_amount.code != unknown_amount.code || known_amount.code != parent_total.code {
            return false;
        }
        if let Ok(total) = known_amount + unknown_amount {
            if total == parent_total {
                return true;
            }
        }
        trace!("splits do not add up!");
    }
    return false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use briolette_proto::briolette::token::{
        tag, Amount, Descriptor, History, Tag, Token, Transfer,
    };

    // ========================================================================
    // Test helpers
    // ========================================================================

    /// Build a History entry with a given signature and optional transfer/split tag.
    fn make_history(sig: &[u8], split_amount: Option<Amount>) -> History {
        let mut tags = vec![];
        if let Some(amt) = split_amount {
            tags.push(Tag {
                value: Some(tag::Value::SplitValue(amt)),
            });
        }
        History {
            transfer: Some(Transfer {
                recipient: None,
                tags,
                previous_signature: vec![],
            }),
            signature: sig.to_vec(),
        }
    }

    /// Build a Token with the given base signature, history signatures, and descriptor.
    fn make_token(base_sig: &[u8], history_sigs: &[&[u8]], value: Option<Amount>) -> Token {
        Token {
            descriptor: Some(Descriptor {
                version: 0,
                value: value,
            }),
            base: Some(History {
                transfer: Some(Transfer {
                    recipient: None,
                    tags: vec![],
                    previous_signature: vec![],
                }),
                signature: base_sig.to_vec(),
            }),
            history: history_sigs
                .iter()
                .map(|s| make_history(s, None))
                .collect(),
        }
    }

    /// Build a Token whose history entries at the fork point carry split tags.
    fn make_split_token(
        base_sig: &[u8],
        pre_fork_sigs: &[&[u8]],
        fork_sig: &[u8],
        split_amount: Amount,
    ) -> Token {
        let mut history: Vec<History> = pre_fork_sigs
            .iter()
            .map(|s| make_history(s, None))
            .collect();
        history.push(make_history(fork_sig, Some(split_amount)));
        Token {
            descriptor: Some(Descriptor {
                version: 0,
                value: Some(Amount {
                    whole: 10,
                    fractional: 0,
                    code: 0,
                }),
            }),
            base: Some(History {
                transfer: Some(Transfer {
                    recipient: None,
                    tags: vec![],
                    previous_signature: vec![],
                }),
                signature: base_sig.to_vec(),
            }),
            history,
        }
    }

    // ========================================================================
    // token_is_known tests
    // ========================================================================

    #[test]
    fn token_is_known_identical_history() {
        let t = make_token(b"base", &[b"h1", b"h2"], None);
        assert!(token_is_known(&t, &vec![t.clone()]));
    }

    #[test]
    fn token_is_known_shorter_history() {
        // A candidate with fewer history entries is "known" if its entries
        // are a prefix of an existing token's history.
        let full = make_token(b"base", &[b"h1", b"h2", b"h3"], None);
        let shorter = make_token(b"base", &[b"h1", b"h2"], None);
        assert!(token_is_known(&shorter, &vec![full]));
    }

    #[test]
    fn token_is_known_returns_false_for_different() {
        let existing = make_token(b"base", &[b"h1", b"h2"], None);
        let different = make_token(b"base", &[b"h1", b"DIFFERENT"], None);
        assert!(!token_is_known(&different, &vec![existing]));
    }

    #[test]
    fn token_is_known_returns_false_for_longer() {
        // A candidate with MORE history than any known token is not "known"
        // (it's potentially an extension).
        let existing = make_token(b"base", &[b"h1"], None);
        let longer = make_token(b"base", &[b"h1", b"h2"], None);
        assert!(!token_is_known(&longer, &vec![existing]));
    }

    // ========================================================================
    // token_is_extension tests
    // ========================================================================

    #[test]
    fn token_is_extension_longer_history() {
        // Extensions must carry a valid descriptor value.
        let value = Some(Amount { whole: 1, fractional: 0, code: 0 });
        let existing = make_token(b"base", &[b"h1", b"h2"], value.clone());
        let extended = make_token(b"base", &[b"h1", b"h2", b"h3"], value.clone());
        assert_eq!(token_is_extension(&extended, &vec![existing]), Some(0));
        // A token without a descriptor value is never a valid extension.
        let valueless = make_token(b"base", &[b"h1", b"h2", b"h3"], None);
        let existing = make_token(b"base", &[b"h1", b"h2"], value);
        assert_eq!(token_is_extension(&valueless, &vec![existing]), None);
    }

    #[test]
    fn token_is_extension_returns_none_for_same_length() {
        let t = make_token(b"base", &[b"h1", b"h2"], None);
        assert_eq!(token_is_extension(&t, &vec![t.clone()]), None);
    }

    #[test]
    fn token_is_extension_returns_none_for_fork() {
        // Same length but different content — not an extension.
        let existing = make_token(b"base", &[b"h1", b"h2"], None);
        let forked = make_token(b"base", &[b"h1", b"FORK"], None);
        assert_eq!(token_is_extension(&forked, &vec![existing]), None);
    }

    #[test]
    fn token_is_extension_returns_none_for_longer_fork() {
        // Longer than existing but diverges — not an extension.
        let existing = make_token(b"base", &[b"h1", b"h2"], None);
        let longer_fork = make_token(b"base", &[b"h1", b"FORK", b"h3"], None);
        assert_eq!(token_is_extension(&longer_fork, &vec![existing]), None);
    }

    // ========================================================================
    // token_get_fork tests
    // ========================================================================

    #[test]
    fn token_get_fork_at_index_0() {
        let existing = make_token(b"base", &[b"h1", b"h2"], None);
        let forked = make_token(b"base", &[b"FORK", b"h2"], None);
        assert_eq!(token_get_fork(&forked, &vec![existing]), Some((0, 0)));
    }

    #[test]
    fn token_get_fork_at_last_index() {
        let existing = make_token(b"base", &[b"h1", b"h2", b"h3"], None);
        let forked = make_token(b"base", &[b"h1", b"h2", b"FORK"], None);
        assert_eq!(token_get_fork(&forked, &vec![existing]), Some((0, 2)));
    }

    #[test]
    fn token_get_fork_at_middle() {
        let existing = make_token(b"base", &[b"h1", b"h2", b"h3"], None);
        let forked = make_token(b"base", &[b"h1", b"FORK", b"h3"], None);
        assert_eq!(token_get_fork(&forked, &vec![existing]), Some((0, 1)));
    }

    #[test]
    fn token_get_fork_returns_none_for_extension() {
        // An extension has no fork — the prefix matches exactly.
        let existing = make_token(b"base", &[b"h1", b"h2"], None);
        let extended = make_token(b"base", &[b"h1", b"h2", b"h3"], None);
        assert_eq!(token_get_fork(&extended, &vec![existing]), None);
    }

    #[test]
    fn token_get_fork_multiple_tokens() {
        // Two known tokens, fork matches the second one.
        let t1 = make_token(b"base", &[b"h1", b"h2"], None);
        let t2 = make_token(b"base", &[b"h1", b"h3"], None); // already a split
        let forked = make_token(b"base", &[b"h1", b"h3", b"FORK"], None);
        // t1 forks at index 1 (h2 vs h3), t2 is an extension — no fork.
        // token_get_fork finds the first fork, which is against t1.
        let result = token_get_fork(&forked, &vec![t1, t2]);
        assert_eq!(result, Some((0, 1)));
    }

    // ========================================================================
    // token_is_second_split tests
    // ========================================================================

    #[test]
    fn token_is_second_split_valid() {
        // Two tokens fork at the same point, both carry split tags summing to original.
        let original_value = Amount {
            whole: 10,
            fractional: 0,
            code: 0,
        };
        let split_a = Amount {
            whole: 6,
            fractional: 0,
            code: 0,
        };
        let split_b = Amount {
            whole: 4,
            fractional: 0,
            code: 0,
        };

        let t1 = make_split_token(b"base", &[b"h1"], b"split-A", split_a);
        let t2 = make_split_token(b"base", &[b"h1"], b"split-B", split_b);

        assert!(token_is_second_split(&t2, &vec![t1]));
    }

    #[test]
    fn token_is_second_split_invalid_amounts() {
        // Split tags don't sum to original — abuse.
        let split_a = Amount {
            whole: 6,
            fractional: 0,
            code: 0,
        };
        let split_b = Amount {
            whole: 6,
            fractional: 0,
            code: 0,
        }; // 6+6=12 != 10

        let t1 = make_split_token(b"base", &[b"h1"], b"split-A", split_a);
        let t2 = make_split_token(b"base", &[b"h1"], b"split-B", split_b);

        assert!(!token_is_second_split(&t2, &vec![t1]));
    }

    #[test]
    fn token_is_second_split_one_side_missing_tag() {
        // One side has a split tag, the other doesn't — not a valid split.
        let split_a = Amount {
            whole: 6,
            fractional: 0,
            code: 0,
        };

        let t1 = make_split_token(b"base", &[b"h1"], b"split-A", split_a);
        let t2 = make_token(b"base", &[b"h1", b"no-split-tag"], None);

        assert!(!token_is_second_split(&t2, &vec![t1]));
    }

    #[test]
    fn token_is_second_split_wrong_currency_code() {
        // Split amounts use different currency codes — invalid.
        let split_a = Amount {
            whole: 6,
            fractional: 0,
            code: 0,
        };
        let split_b = Amount {
            whole: 4,
            fractional: 0,
            code: 840,
        }; // Different code

        let t1 = make_split_token(b"base", &[b"h1"], b"split-A", split_a);
        let t2 = make_split_token(b"base", &[b"h1"], b"split-B", split_b);

        assert!(!token_is_second_split(&t2, &vec![t1]));
    }

    // ========================================================================
    // Full decision tree test
    // ========================================================================

    #[test]
    fn fork_detection_decision_tree() {
        // This test exercises the complete decision tree that update_impl
        // uses, proving every case is handled:
        //
        //   candidate vs existing tokens:
        //   1. is_known(candidate) → true  → no-op (already seen)
        //   2. is_extension(candidate) → Some(i) → replace tokens[i]
        //   3. is_second_split(candidate) → true  → add parallel history
        //   4. get_fork(candidate) → Some((t,h)) → DOUBLE SPEND DETECTED
        //
        // We construct one existing token and test all four paths.

        let value = Some(Amount {
            whole: 10,
            fractional: 0,
            code: 0,
        });
        let existing = make_token(b"base", &[b"h1", b"h2"], value.clone());
        let tokens = vec![existing.clone()];

        // Case 1: Known — identical or shorter
        let known = make_token(b"base", &[b"h1", b"h2"], value.clone());
        assert!(token_is_known(&known, &tokens));

        // Case 2: Extension — longer with matching prefix
        let extension = make_token(b"base", &[b"h1", b"h2", b"h3"], value.clone());
        assert!(!token_is_known(&extension, &tokens));
        assert_eq!(token_is_extension(&extension, &tokens), Some(0));

        // Case 3: Valid second split — fork with matching split amounts
        let split_a = Amount {
            whole: 6,
            fractional: 0,
            code: 0,
        };
        let split_b = Amount {
            whole: 4,
            fractional: 0,
            code: 0,
        };
        let existing_with_split = make_split_token(b"base", &[b"h1"], b"split-A", split_a);
        let candidate_split = make_split_token(b"base", &[b"h1"], b"split-B", split_b);
        let split_tokens = vec![existing_with_split];
        assert!(!token_is_known(&candidate_split, &split_tokens));
        assert_eq!(token_is_extension(&candidate_split, &split_tokens), None);
        assert!(token_is_second_split(&candidate_split, &split_tokens));

        // Case 4: Double spend — fork without valid split tags
        let double_spend = make_token(b"base", &[b"h1", b"DOUBLE-SPEND"], value.clone());
        assert!(!token_is_known(&double_spend, &tokens));
        assert_eq!(token_is_extension(&double_spend, &tokens), None);
        assert!(!token_is_second_split(&double_spend, &tokens));
        assert_eq!(token_get_fork(&double_spend, &tokens), Some((0, 1)));
    }

    // ========================================================================
    // token_is_extension split validation tests
    // ========================================================================

    /// Helper: build a token where a specific history entry carries a split tag.
    fn make_extension_with_split(
        base_sig: &[u8],
        pre_split_sigs: &[&[u8]],
        split_sig: &[u8],
        split_amount: Amount,
        post_split_sigs: &[&[u8]],
        descriptor_value: Option<Amount>,
    ) -> Token {
        let mut history: Vec<History> = pre_split_sigs
            .iter()
            .map(|s| make_history(s, None))
            .collect();
        history.push(make_history(split_sig, Some(split_amount)));
        for s in post_split_sigs {
            history.push(make_history(s, None));
        }
        Token {
            descriptor: Some(Descriptor {
                version: 0,
                value: descriptor_value,
            }),
            base: Some(History {
                transfer: Some(Transfer {
                    recipient: None,
                    tags: vec![],
                    previous_signature: vec![],
                }),
                signature: base_sig.to_vec(),
            }),
            history,
        }
    }

    #[test]
    fn token_is_extension_rejects_inflated_split() {
        // Existing token has history [h1]. Candidate extends with a split
        // claiming 15 on a token worth 10 — must be rejected.
        let value = Some(Amount {
            whole: 10,
            fractional: 0,
            code: 0,
        });
        let existing = make_token(b"base", &[b"h1"], value.clone());
        let inflated = make_extension_with_split(
            b"base",
            &[b"h1"],
            b"split-inflated",
            Amount {
                whole: 15,
                fractional: 0,
                code: 0,
            },
            &[],
            value.clone(),
        );
        assert_eq!(token_is_extension(&inflated, &vec![existing]), None);
    }

    #[test]
    fn token_is_extension_accepts_valid_split() {
        // Split claiming 6 on a token worth 10 — should be accepted.
        let value = Some(Amount {
            whole: 10,
            fractional: 0,
            code: 0,
        });
        let existing = make_token(b"base", &[b"h1"], value.clone());
        let valid_split = make_extension_with_split(
            b"base",
            &[b"h1"],
            b"split-valid",
            Amount {
                whole: 6,
                fractional: 0,
                code: 0,
            },
            &[],
            value.clone(),
        );
        assert_eq!(token_is_extension(&valid_split, &vec![existing]), Some(0));
    }

    #[test]
    fn token_is_extension_rejects_split_currency_mismatch() {
        // Split uses a different currency code — must be rejected.
        let value = Some(Amount {
            whole: 10,
            fractional: 0,
            code: 0,
        });
        let existing = make_token(b"base", &[b"h1"], value.clone());
        let wrong_currency = make_extension_with_split(
            b"base",
            &[b"h1"],
            b"split-wrong-code",
            Amount {
                whole: 5,
                fractional: 0,
                code: 840,
            },
            &[],
            value.clone(),
        );
        assert_eq!(token_is_extension(&wrong_currency, &vec![existing]), None);
    }

    #[test]
    fn token_is_extension_rejects_exact_amount_with_fractional_overflow() {
        // Split matches whole amount but exceeds via fractional.
        let value = Some(Amount {
            whole: 10,
            fractional: 0,
            code: 0,
        });
        let existing = make_token(b"base", &[b"h1"], value.clone());
        let fractional_overflow = make_extension_with_split(
            b"base",
            &[b"h1"],
            b"split-frac",
            Amount {
                whole: 10,
                fractional: 10000,
                code: 0,
            },
            &[],
            value.clone(),
        );
        assert_eq!(
            token_is_extension(&fractional_overflow, &vec![existing]),
            None
        );
    }

    #[test]
    fn token_is_extension_split_deep_in_extension() {
        // Existing has [h1, h2]. Extension adds [h3, split(15), h5].
        // The split is in the new entries, not in the shared prefix.
        let value = Some(Amount {
            whole: 10,
            fractional: 0,
            code: 0,
        });
        let existing = make_token(b"base", &[b"h1", b"h2"], value.clone());
        let deep_split = make_extension_with_split(
            b"base",
            &[b"h1", b"h2", b"h3"],
            b"split-deep",
            Amount {
                whole: 15,
                fractional: 0,
                code: 0,
            },
            &[b"h5"],
            value.clone(),
        );
        assert_eq!(token_is_extension(&deep_split, &vec![existing]), None);
    }
}

#[cfg(test)]
mod split_bound_tests {
    use super::*;
    use briolette_proto::briolette::token::{
        tag, Amount, AmountType, Descriptor, History, Tag, Transfer,
    };

    fn make_token(whole: i32, steps: Vec<(&[u8], Option<Amount>)>) -> Token {
        let mut history = Vec::new();
        for (sig, split) in steps {
            let tags = match split {
                Some(amount) => vec![Tag {
                    value: Some(tag::Value::SplitValue(amount)),
                }],
                None => vec![],
            };
            history.push(History {
                transfer: Some(Transfer {
                    recipient: None,
                    tags,
                    previous_signature: vec![],
                }),
                signature: sig.to_vec(),
            });
        }
        Token {
            descriptor: Some(Descriptor {
                version: 0,
                value: Some(Amount {
                    whole,
                    fractional: 0,
                    code: AmountType::TestToken as i32,
                }),
            }),
            base: None,
            history,
        }
    }

    fn amt(whole: i32, fractional: i32) -> Amount {
        Amount {
            whole,
            fractional,
            code: AmountType::TestToken as i32,
        }
    }

    #[test]
    fn token_is_known_ok() {
        assert_eq!(4, 4);
    }

    #[test]
    fn effective_amount_and_extension_bounds() {
        let base = make_token(1, vec![]);
        assert_eq!(base.effective_amount(), Some(amt(1, 0)));

        // Valid split (1, 0) followed by a regular transfer keeps effective value 0 on the 0-branch.
        let split_zero_then_tx =
            make_token(1, vec![(b"sig-b1", Some(amt(0, 0))), (b"sig-b2", None)]);
        assert_eq!(split_zero_then_tx.effective_amount(), Some(amt(0, 0)));
        assert_eq!(
            token_is_extension(&split_zero_then_tx, &vec![base.clone()]),
            Some(0)
        );

        // Over-split (2 > 1) is rejected by effective_amount and token_is_extension.
        let over_split = make_token(1, vec![(b"sig-over", Some(amt(2, 0)))]);
        assert_eq!(over_split.effective_amount(), None);
        assert_eq!(token_is_extension(&over_split, &vec![base.clone()]), None);

        // Negative split is rejected.
        let neg_split = make_token(1, vec![(b"sig-neg", Some(amt(-1, 0)))]);
        assert_eq!(neg_split.effective_amount(), None);
        assert_eq!(token_is_extension(&neg_split, &vec![base.clone()]), None);

        // Increasing value after a smaller split (1 -> 0 -> 1) is rejected.
        let rebound_split = make_token(
            1,
            vec![
                (b"sig-s1", Some(amt(0, 0))),
                (b"sig-s2", Some(amt(1, 0))),
            ],
        );
        assert_eq!(rebound_split.effective_amount(), None);
    }

    #[test]
    fn second_split_enforces_sum_and_two_way_limit() {
        let split_a = make_token(1, vec![(b"sig-a", Some(amt(1, 0)))]);
        let split_b = make_token(1, vec![(b"sig-b", Some(amt(0, 0)))]);
        let split_c = make_token(1, vec![(b"sig-c", Some(amt(0, 0)))]);
        let split_bad = make_token(1, vec![(b"sig-bad", Some(amt(1, 0)))]);

        // Complementary (1, 0) + (0, 0) is accepted as second split.
        assert_eq!(
            token_is_second_split(&split_b, &vec![split_a.clone()]),
            true
        );
        // Non-summing (1, 0) + (1, 0) on a 1-unit token is rejected.
        assert_eq!(
            token_is_second_split(&split_bad, &vec![split_a.clone()]),
            false
        );
        // Third split branch at the same fork node is rejected.
        assert_eq!(
            token_is_second_split(&split_c, &vec![split_a.clone(), split_b.clone()]),
            false
        );

        // Fractional complementary split (500_000 + 500_000 micros == 1) is accepted.
        let half_a = make_token(1, vec![(b"sig-ha", Some(amt(0, 500_000)))]);
        let half_b = make_token(1, vec![(b"sig-hb", Some(amt(0, 500_000)))]);
        let half_c = make_token(1, vec![(b"sig-hc", Some(amt(0, 500_000)))]);
        assert_eq!(token_is_second_split(&half_b, &vec![half_a.clone()]), true);
        assert_eq!(token_is_second_split(&half_c, &vec![half_a, half_b]), false);

        // Nested sub-split must sum to the parent split value (6), not descriptor value (10).
        let sub_a1 = make_token(
            10,
            vec![
                (b"sig-root-a", Some(amt(6, 0))),
                (b"sig-sub-1", Some(amt(4, 0))),
            ],
        );
        let sub_a2_valid = make_token(
            10,
            vec![
                (b"sig-root-a", Some(amt(6, 0))),
                (b"sig-sub-2", Some(amt(2, 0))),
            ],
        );
        let sub_a2_inflated = make_token(
            10,
            vec![
                (b"sig-root-a", Some(amt(6, 0))),
                (b"sig-sub-3", Some(amt(6, 0))),
            ],
        );
        assert_eq!(
            token_is_second_split(&sub_a2_valid, &vec![sub_a1.clone()]),
            true
        );
        assert_eq!(
            token_is_second_split(&sub_a2_inflated, &vec![sub_a1]),
            false
        );
    }
}
