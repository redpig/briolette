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
    ArchiveReply, ArchiveRequest, RevocationDataReply, RevocationDataRequest, StoreTicketsReply,
    StoreTicketsRequest, UpdateReply, UpdateRequest,
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
        let candidate = request.token.clone().unwrap();
        if candidate.effective_amount().is_none() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidAmountType.into(),
            });
        }
        trace!("Looking up token: {:?}", request.id);
        let maybe_entry: Option<tokenmap::Entry> =
            self.get_tokenmap_entry(request.id.clone()).await?;
        trace!("Entry found: {:?}", maybe_entry);
        if maybe_entry.is_none() {
            self.create_tokenmap_entry(request.id.clone(), candidate)
                .await?;
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
            entry.tokens[idx] = candidate;
            entry.last_update = now;
            let abuse_detected = entry.abuses.len() != 0;
            self.update_tokenmap_entry(&request.id, entry).await?;
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
            entry.tokens.push(candidate);
            entry.last_update = now;
            let abuse_detected = entry.abuses.len() != 0;
            self.update_tokenmap_entry(&request.id, entry).await?;

            return Ok(UpdateReply {
                created: false,
                abuse_detected: abuse_detected,
            });
        }
        // We're in double spend territory now.
        trace!("double spending detected");
        let (token_index, history_index) =
            token_get_fork(&candidate, &entry.tokens).expect("a fork must exist to reach this far");
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
                if *sg != tokenmap::SelectGroup::All.into() {
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
        let total = known_amount + unknown_amount;
        if total == parent_total {
            return true;
        }
        trace!("splits do not add up!");
    }
    return false;
}

#[cfg(test)]
mod tests {
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
                    fractional: 0.0,
                    code: AmountType::TestToken as i32,
                }),
            }),
            base: None,
            history,
        }
    }

    fn amt(whole: i32, fractional: f32) -> Amount {
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
        assert_eq!(base.effective_amount(), Some(amt(1, 0.0)));

        // Valid split (1, 0) followed by a regular transfer keeps effective value 0 on the 0-branch.
        let split_zero_then_tx =
            make_token(1, vec![(b"sig-b1", Some(amt(0, 0.0))), (b"sig-b2", None)]);
        assert_eq!(split_zero_then_tx.effective_amount(), Some(amt(0, 0.0)));
        assert_eq!(
            token_is_extension(&split_zero_then_tx, &vec![base.clone()]),
            Some(0)
        );

        // Over-split (2 > 1) is rejected by effective_amount and token_is_extension.
        let over_split = make_token(1, vec![(b"sig-over", Some(amt(2, 0.0)))]);
        assert_eq!(over_split.effective_amount(), None);
        assert_eq!(token_is_extension(&over_split, &vec![base.clone()]), None);

        // Negative split is rejected.
        let neg_split = make_token(1, vec![(b"sig-neg", Some(amt(-1, 0.0)))]);
        assert_eq!(neg_split.effective_amount(), None);
        assert_eq!(token_is_extension(&neg_split, &vec![base.clone()]), None);

        // Increasing value after a smaller split (1 -> 0 -> 1) is rejected.
        let rebound_split = make_token(
            1,
            vec![
                (b"sig-s1", Some(amt(0, 0.0))),
                (b"sig-s2", Some(amt(1, 0.0))),
            ],
        );
        assert_eq!(rebound_split.effective_amount(), None);
    }

    #[test]
    fn second_split_enforces_sum_and_two_way_limit() {
        let split_a = make_token(1, vec![(b"sig-a", Some(amt(1, 0.0)))]);
        let split_b = make_token(1, vec![(b"sig-b", Some(amt(0, 0.0)))]);
        let split_c = make_token(1, vec![(b"sig-c", Some(amt(0, 0.0)))]);
        let split_bad = make_token(1, vec![(b"sig-bad", Some(amt(1, 0.0)))]);

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

        // Fractional complementary split (0.5 + 0.5 == 1.0) is accepted.
        let half_a = make_token(1, vec![(b"sig-ha", Some(amt(0, 0.5)))]);
        let half_b = make_token(1, vec![(b"sig-hb", Some(amt(0, 0.5)))]);
        let half_c = make_token(1, vec![(b"sig-hc", Some(amt(0, 0.5)))]);
        assert_eq!(token_is_second_split(&half_b, &vec![half_a.clone()]), true);
        assert_eq!(token_is_second_split(&half_c, &vec![half_a, half_b]), false);

        // Nested sub-split must sum to the parent split value (6), not descriptor value (10).
        let sub_a1 = make_token(
            10,
            vec![
                (b"sig-root-a", Some(amt(6, 0.0))),
                (b"sig-sub-1", Some(amt(4, 0.0))),
            ],
        );
        let sub_a2_valid = make_token(
            10,
            vec![
                (b"sig-root-a", Some(amt(6, 0.0))),
                (b"sig-sub-2", Some(amt(2, 0.0))),
            ],
        );
        let sub_a2_inflated = make_token(
            10,
            vec![
                (b"sig-root-a", Some(amt(6, 0.0))),
                (b"sig-sub-3", Some(amt(6, 0.0))),
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
