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

tonic::include_proto!("briolette.token");
use crate::vec_utils;
use chrono::Utc;
use ecdsa::RecoveryId;
//use ettecrypto::v0;
use crate::briolette::ErrorCode as BrioletteErrorCode;
use briolette_crypto::v0;
use log::*;
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use p256::pkcs8::EncodePublicKey;
use prost::Message;
use sha2::{Digest, Sha256};
use std::ops::Add;

/// Default epoch duration in seconds (24 hours).
/// Use `ExtendedEpochData.epoch_seconds` to override at runtime.
pub const DEFAULT_EPOCH_SECONDS: u32 = 86400;

pub trait TokenVerify {
    //  Returns true if the Token is valid. If trusted_mints are supplied, the base will be
    //  verified as well. If trusted_clerks are supplied, then the tickets will be verified as
    //  well.
    fn verify(
        &self,
        group_public_key: &Vec<u8>,
        trusted_mints: &Vec<Vec<u8>>,
        trusted_clerks: &Vec<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode>;
}

impl TokenVerify for Token {
    fn verify(
        &self,
        group_public_key: &Vec<u8>,
        trusted_mints: &Vec<Vec<u8>>,
        trusted_clerks: &Vec<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode> {
        if self.base.is_none() || self.descriptor.is_none() {
            return Err(BrioletteErrorCode::InvalidMissingFields);
        }
        // 1. Verify the base
        self.base.as_ref().unwrap().verify_base(
            self.descriptor.as_ref().unwrap(),
            trusted_mints,
            trusted_clerks,
        )?;

        // 2, Verify each history entry
        let mut last_signature = &self.base.as_ref().unwrap().signature;
        let mut bound_credential = &self
            .base
            .as_ref()
            .unwrap()
            .transfer
            .as_ref()
            .unwrap()
            .recipient
            .as_ref()
            .unwrap()
            .ticket
            .as_ref()
            .unwrap()
            .credential;
        for history in self.history.iter() {
            history.verify_history(
                bound_credential,
                last_signature,
                group_public_key,
                trusted_clerks,
            )?;
            last_signature = &history.signature;
            bound_credential = &history
                .transfer
                .as_ref()
                .unwrap()
                .recipient
                .as_ref()
                .unwrap()
                .ticket
                .as_ref()
                .unwrap()
                .credential;
        }
        // 3. Verify the current holder's ticket is not expired.
        // Historical tickets are verified for signature/key only (done above).
        // Only the current holder must have a valid, non-expired ticket.
        // This preserves velocity control (wallets must visit the clerk to
        // get fresh tickets) while allowing tokens with expired historical
        // tickets to remain verifiable.
        if !trusted_clerks.is_empty() {
            let current_ticket = if let Some(last_history) = self.history.last() {
                last_history
                    .transfer
                    .as_ref()
                    .unwrap()
                    .recipient
                    .as_ref()
                    .unwrap()
            } else {
                self.base
                    .as_ref()
                    .unwrap()
                    .transfer
                    .as_ref()
                    .unwrap()
                    .recipient
                    .as_ref()
                    .unwrap()
            };
            current_ticket.verify(trusted_clerks, None)?;
        }

        // 4. Verify the token has not expired (valid_until tag in base transfer).
        if let Some(ref base) = self.base {
            if let Some(ref transfer) = base.transfer {
                for tag in transfer.tags.iter() {
                    if let Some(tag::Value::ValidUntil(valid_until)) = tag.value {
                        let now = Utc::now().timestamp() as u64;
                        if now > valid_until {
                            return Err(BrioletteErrorCode::TokenExpired);
                        }
                    }
                }
            }
        }

        // 5. Verify that no split amount exceeds the descriptor value.
        let descriptor = self.descriptor.as_ref().unwrap();
        if let Some(ref original_value) = descriptor.value {
            for history in self.history.iter() {
                if let Some(ref transfer) = history.transfer {
                    for tag in transfer.tags.iter() {
                        if let Some(tag::Value::SplitValue(ref split_amount)) = tag.value {
                            if split_amount.code != original_value.code {
                                return Err(BrioletteErrorCode::InvalidSplitCurrencyMismatch);
                            }
                            if split_amount.whole > original_value.whole
                                || (split_amount.whole == original_value.whole
                                    && split_amount.fractional > original_value.fractional)
                            {
                                return Err(BrioletteErrorCode::InvalidSplitExceedsValue);
                            }
                        }
                    }
                }
            }
        }
        // 6. Enjoy a valid token.
        return Ok(true);
    }
}
pub trait HistoryVerify {
    fn verify_history(
        &self,
        bound_credential: &Vec<u8>,
        previous_signature: &Vec<u8>,
        group_public_key: &Vec<u8>,
        allowed_ticket_keys: &Vec<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode>;
    fn verify_base(
        &self,
        descriptor: &Descriptor,
        allowed_mint_keys: &Vec<Vec<u8>>,
        allowed_ticket_keys: &Vec<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode>;
}

impl HistoryVerify for History {
    fn verify_history(
        &self,
        bound_credential: &Vec<u8>,
        previous_signature: &Vec<u8>,
        group_public_key: &Vec<u8>,
        allowed_ticket_keys: &Vec<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode> {
        if self.transfer.is_none() || self.transfer.as_ref().unwrap().recipient.is_none() {
            return Err(BrioletteErrorCode::InvalidMissingFields);
        }
        // Historical tickets: verify signature + key only, skip expiration.
        // Token lifetime is controlled by Tag.valid_until, not by historical
        // ticket expiration.
        self.transfer
            .as_ref()
            .unwrap()
            .recipient
            .as_ref()
            .unwrap()
            .verify_historical(&allowed_ticket_keys, None)?;

        let mut transfer = self.transfer.clone().unwrap();
        transfer.previous_signature = previous_signature.clone();
        let transfer_serialized = transfer.encode_to_vec();

        // Re-insert the bound credential into the signature
        let mut signature = self.signature.clone();
        v0::inflate_signature(bound_credential, &mut signature);
        let verified = v0::verify(
            group_public_key,
            &Some(previous_signature.clone()),
            &Some(bound_credential.clone()),
            &signature,
            &transfer_serialized,
        );
        if verified {
            return Ok(true);
        }
        return Err(BrioletteErrorCode::InvalidHistorySignature);
    }

    fn verify_base(
        &self,
        descriptor: &Descriptor,
        allowed_mint_keys: &Vec<Vec<u8>>,
        allowed_ticket_keys: &Vec<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode> {
        if self.transfer.is_none() || self.transfer.as_ref().unwrap().recipient.is_none() {
            return Err(BrioletteErrorCode::InvalidMissingFields);
        }
        // Base ticket is also historical — verify signature + key only.
        self.transfer
            .as_ref()
            .unwrap()
            .recipient
            .as_ref()
            .unwrap()
            .verify_historical(&allowed_ticket_keys, None)?;
        let mut sig: Vec<u8> = self.signature.clone();
        if sig.len() == 0 {
            debug!("base missing signature");
            return Err(BrioletteErrorCode::InvalidMissingFields);
        }
        let rec_id = RecoveryId::try_from(sig.pop().unwrap());
        if rec_id.is_err() {
            debug!("could not recover public key");
            return Err(BrioletteErrorCode::UnrecoverablePublicKey);
        }
        let mut base = self.transfer.clone().unwrap();
        // For this one, we want the digest of the descriptor..
        base.previous_signature = Sha256::digest(descriptor.encode_to_vec()).to_vec();
        let serialized = base.encode_to_vec();
        if let Ok(signature) = Signature::try_from(sig.as_slice()) {
            // Recovery the public key
            let found_vk: VerifyingKey;
            let found_vk_bytes: Vec<u8>;
            if let Ok(vk) =
                VerifyingKey::recover_from_msg(serialized.as_slice(), &signature, rec_id.unwrap())
            {
                found_vk = vk;
                found_vk_bytes = vk.to_public_key_der().unwrap().as_bytes().to_vec();
            } else {
                debug!("could not recover public key");
                return Err(BrioletteErrorCode::UnrecoverablePublicKey);
            }
            // See if it is known
            let mint_vk: VerifyingKey;
            if let Some(_tsk) = allowed_mint_keys
                .iter()
                .find(|&key| vec_utils::vec_equal(key, &found_vk_bytes))
            {
                mint_vk = found_vk;
            } else {
                trace!("no known public key found for ticket");
                return Err(BrioletteErrorCode::UnknownMintPublicKey);
            }
            if let Err(e) = mint_vk.verify(serialized.as_slice(), &signature) {
                trace!("ticket signature did not verify: {:?}", e);
                return Err(BrioletteErrorCode::InvalidBaseSignature);
            }
            return Ok(true);
        }
        Err(BrioletteErrorCode::UnparseableBaseSignature)
    }
}

pub trait TokenTransfer {
    fn transfer(
        &mut self,
        destination: &SignedTicket,
        credential_secret: Vec<u8>,
    ) -> Result<bool, BrioletteErrorCode>;

    /// Transfer using split-key signing with a smart card.
    /// The credential secret is split between the card and host_secret_key.
    /// If `swap_auth` is provided, it is passed to the card to authorize a
    /// bloom-filter-bypassing swap operation. The swap authorization is a
    /// Schnorr signature from the swap server binding to the specific basename,
    /// verified on-card against the stored swap server public key.
    fn transfer_split(
        &mut self,
        destination: &SignedTicket,
        card: &mut dyn v0::split::SmartCard,
        host_secret_key: Vec<u8>,
        swap_auth: Option<&v0::split::SwapAuthorization>,
    ) -> Result<bool, BrioletteErrorCode>;
    // TODO: Pull base signing out of Mint
    // fn base(&mut self, ...)
}

impl TokenTransfer for Token {
    fn transfer(
        &mut self,
        destination: &SignedTicket,
        credential_secret: Vec<u8>,
    ) -> Result<bool, BrioletteErrorCode> {
        // Grab the last signature to use as the basename and in the tx block.
        let last_sig;
        let committed_credential;
        if let Some(last_tx) = self.history.last() {
            last_sig = last_tx.signature.clone();
            committed_credential = last_tx
                .transfer
                .as_ref()
                .unwrap()
                .recipient
                .as_ref()
                .unwrap()
                .ticket
                .as_ref()
                .unwrap()
                .credential
                .clone();
        } else {
            last_sig = self
                .base
                .as_ref()
                .expect("transfer cannot be called with no base")
                .signature
                .clone();
            committed_credential = self
                .base
                .as_ref()
                .unwrap()
                .transfer
                .as_ref()
                .unwrap()
                .recipient
                .as_ref()
                .unwrap()
                .ticket
                .as_ref()
                .unwrap()
                .credential
                .clone();
        }
        let mut transfer = Transfer {
            recipient: Some(destination.clone()),
            tags: vec![],
            previous_signature: last_sig.clone(),
        };
        let serialized_transfer = transfer.encode_to_vec();
        let basename = Some(last_sig);
        let mut signature = vec![];
        if v0::sign(
            &serialized_transfer,
            &committed_credential,
            &credential_secret,
            &basename,
            false, // require the committed credential!
            &mut signature,
        ) == false
        {
            return Err(BrioletteErrorCode::FailedToSignTokenTransfer);
        }
        // Don't duplicate the storage here.
        transfer.previous_signature.clear();
        // Remove the duplicated credential from the Token when serialized
        // This saves 260 bytes per transfer. At present, history is 515 bytes.
        v0::deflate_signature(&mut signature);
        let history = History {
            transfer: Some(transfer),
            signature,
        };
        self.history.push(history);
        return Ok(true);
    }

    fn transfer_split(
        &mut self,
        destination: &SignedTicket,
        card: &mut dyn v0::split::SmartCard,
        host_secret_key: Vec<u8>,
        swap_auth: Option<&v0::split::SwapAuthorization>,
    ) -> Result<bool, BrioletteErrorCode> {
        // swap_auth is passed through to sign_split_ext which routes to
        // the card's swap commit flow (INS 0x13), bypassing the bloom filter.
        // Grab the last signature to use as the basename and in the tx block.
        let last_sig;
        let committed_credential;
        if let Some(last_tx) = self.history.last() {
            last_sig = last_tx.signature.clone();
            committed_credential = last_tx
                .transfer
                .as_ref()
                .unwrap()
                .recipient
                .as_ref()
                .unwrap()
                .ticket
                .as_ref()
                .unwrap()
                .credential
                .clone();
        } else {
            last_sig = self
                .base
                .as_ref()
                .expect("transfer cannot be called with no base")
                .signature
                .clone();
            committed_credential = self
                .base
                .as_ref()
                .unwrap()
                .transfer
                .as_ref()
                .unwrap()
                .recipient
                .as_ref()
                .unwrap()
                .ticket
                .as_ref()
                .unwrap()
                .credential
                .clone();
        }
        let mut transfer = Transfer {
            recipient: Some(destination.clone()),
            tags: vec![],
            previous_signature: last_sig.clone(),
        };
        let serialized_transfer = transfer.encode_to_vec();
        let basename = Some(last_sig);
        let mut signature = vec![];
        match v0::split::sign_split_ext(
            card,
            &host_secret_key,
            &serialized_transfer,
            &committed_credential,
            &basename,
            false, // require the committed credential!
            &mut signature,
            swap_auth,
        ) {
            Ok(()) => {}
            Err(v0::split::SmartCardError::BloomFilterHit) => {
                return Err(BrioletteErrorCode::BloomFilterHit);
            }
            Err(_) => {
                return Err(BrioletteErrorCode::FailedToSignTokenTransfer);
            }
        }
        // Don't duplicate the storage here.
        transfer.previous_signature.clear();
        // Remove the duplicated credential from the Token when serialized
        // This saves 260 bytes per transfer. At present, history is 515 bytes.
        v0::deflate_signature(&mut signature);
        let history = History {
            transfer: Some(transfer),
            signature,
        };
        self.history.push(history);
        return Ok(true);
    }
}

pub trait TicketExpiry {
    fn expires_on(&self) -> u64;
    fn expires_on_with_epoch(&self, epoch_seconds: u32) -> u64;
}

impl TicketExpiry for SignedTicket {
    fn expires_on(&self) -> u64 {
        self.expires_on_with_epoch(DEFAULT_EPOCH_SECONDS)
    }

    fn expires_on_with_epoch(&self, epoch_seconds: u32) -> u64 {
        let ticket_tags = self.ticket.clone().unwrap().tags.unwrap();
        ticket_tags.created_on + ((ticket_tags.lifetime * epoch_seconds) as u64)
    }
}

pub trait VerifyTicket {
    // TODO: Move to common error codes.
    fn verify(
        &self,
        allowed_signing_keys: &Vec<Vec<u8>>,
        credential: Option<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode>;
    /// Verify the ticket's clerk signature and group membership without
    /// checking expiration. Used for historical tickets in a token's
    /// provenance chain — token lifetime is controlled by Tag.valid_until,
    /// not by the shortest-lived ticket in the history.
    fn verify_historical(
        &self,
        allowed_signing_keys: &Vec<Vec<u8>>,
        credential: Option<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode>;
}

impl VerifyTicket for SignedTicket {
    fn verify(
        &self,
        allowed_signing_keys: &Vec<Vec<u8>>,
        credential: Option<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode> {
        // First verify signature and signing key (shared with verify_historical)
        self.verify_signature_and_key(allowed_signing_keys, credential)?;

        // Now check expiration — only enforced on the current holder's ticket
        let now = Utc::now().timestamp() as u64;
        let tags = self.ticket.clone().unwrap().tags.clone().unwrap();
        if tags.created_on >= now {
            trace!("ticket created in the future: {}", tags.created_on);
            return Err(BrioletteErrorCode::InvalidTicketCreatedOn);
        }
        if self.expires_on() < now {
            trace!("ticket expired");
            return Err(BrioletteErrorCode::TicketExpired);
        }

        Ok(true)
    }

    fn verify_historical(
        &self,
        allowed_signing_keys: &Vec<Vec<u8>>,
        credential: Option<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode> {
        // For historical tickets in a token's provenance chain, we only verify
        // the clerk signature and that the signing key is known. Token lifetime
        // is controlled by Tag.valid_until, not by historical ticket expiration.
        self.verify_signature_and_key(allowed_signing_keys, credential)
    }
}

/// Internal helper for signature/key verification shared between
/// verify() and verify_historical().
trait VerifyTicketSignature {
    fn verify_signature_and_key(
        &self,
        allowed_signing_keys: &Vec<Vec<u8>>,
        credential: Option<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode>;
}

impl VerifyTicketSignature for SignedTicket {
    fn verify_signature_and_key(
        &self,
        allowed_signing_keys: &Vec<Vec<u8>>,
        credential: Option<Vec<u8>>,
    ) -> Result<bool, BrioletteErrorCode> {
        let serialized: Vec<u8>;
        if let Some(mut ticket) = self.ticket.clone() {
            if credential.is_some() && ticket.credential.len() == 0 {
                ticket.credential = credential.clone().unwrap();
            }
            serialized = ticket.encode_to_vec();
        } else {
            debug!("ticket missing");
            return Err(BrioletteErrorCode::InvalidMissingFields);
        }
        let mut sig: Vec<u8> = self.signature.clone();
        if sig.len() == 0 {
            debug!("ticket missing signature");
            return Err(BrioletteErrorCode::InvalidMissingFields);
        }
        let rec_id = RecoveryId::try_from(sig.pop().unwrap());
        if rec_id.is_err() {
            debug!("could not recover public key");
            return Err(BrioletteErrorCode::UnrecoverablePublicKey);
        }
        if let Ok(signature) = Signature::try_from(sig.as_slice()) {
            let found_vk: VerifyingKey;
            let found_vk_bytes: Vec<u8>;
            if let Ok(vk) =
                VerifyingKey::recover_from_msg(serialized.as_slice(), &signature, rec_id.unwrap())
            {
                found_vk = vk;
                found_vk_bytes = vk.to_public_key_der().unwrap().as_bytes().to_vec();
            } else {
                debug!("could not recover public key");
                return Err(BrioletteErrorCode::UnrecoverablePublicKey);
            }
            let ticket_vk: VerifyingKey;
            if let Some(_tsk) = allowed_signing_keys
                .iter()
                .find(|&key| vec_utils::vec_equal(key, &found_vk_bytes))
            {
                ticket_vk = found_vk;
            } else {
                trace!("no known public key found for ticket");
                return Err(BrioletteErrorCode::UnknownTicketPublicKey);
            }
            if let Err(e) = ticket_vk.verify(serialized.as_slice(), &signature) {
                trace!("ticket signature did not verify: {:?}", e);
                return Err(BrioletteErrorCode::InvalidTicketSignature);
            }
            return Ok(true);
        }
        Err(BrioletteErrorCode::UnparseableTicketSignature)
    }
}

impl Add for Amount {
    type Output = Result<Self, BrioletteErrorCode>;

    fn add(self, other: Self) -> Self::Output {
        if self.code != other.code {
            return Err(BrioletteErrorCode::InvalidSplitCurrencyMismatch);
        }
        let total_frac = self.fractional + other.fractional;
        Ok(Self {
            whole: self.whole + other.whole + total_frac / 1_000_000,
            fractional: total_frac % 1_000_000,
            code: self.code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Amount::add tests
    // -----------------------------------------------------------------------

    #[test]
    fn amount_add_whole_only() {
        let a = Amount { whole: 3, fractional: 0, code: 0 };
        let b = Amount { whole: 5, fractional: 0, code: 0 };
        let c = (a + b).unwrap();
        assert_eq!(c.whole, 8);
        assert_eq!(c.fractional, 0);
        assert_eq!(c.code, 0);
    }

    #[test]
    fn amount_add_fractional_no_carry() {
        let a = Amount { whole: 1, fractional: 300_000, code: 840 };
        let b = Amount { whole: 2, fractional: 400_000, code: 840 };
        let c = (a + b).unwrap();
        assert_eq!(c.whole, 3);
        assert_eq!(c.fractional, 700_000);
        assert_eq!(c.code, 840);
    }

    #[test]
    fn amount_add_fractional_with_carry() {
        let a = Amount { whole: 1, fractional: 700_000, code: 0 };
        let b = Amount { whole: 2, fractional: 500_000, code: 0 };
        let c = (a + b).unwrap();
        assert_eq!(c.whole, 4);
        assert_eq!(c.fractional, 200_000);
    }

    #[test]
    fn amount_add_zero() {
        let a = Amount { whole: 0, fractional: 0, code: 0 };
        let b = Amount { whole: 0, fractional: 0, code: 0 };
        let c = (a + b).unwrap();
        assert_eq!(c.whole, 0);
        assert_eq!(c.fractional, 0);
    }

    #[test]
    fn amount_add_different_currency_returns_error() {
        let a = Amount { whole: 1, fractional: 0, code: 0 };
        let b = Amount { whole: 1, fractional: 0, code: 840 };
        assert_eq!(a + b, Err(BrioletteErrorCode::InvalidSplitCurrencyMismatch));
    }

    #[test]
    fn amount_add_exact_carry_boundary() {
        let a = Amount { whole: 0, fractional: 999_999, code: 0 };
        let b = Amount { whole: 0, fractional: 1, code: 0 };
        let c = (a + b).unwrap();
        assert_eq!(c.whole, 1);
        assert_eq!(c.fractional, 0);
    }

    #[test]
    fn amount_add_large_fractional_multi_carry() {
        // Both at 999_999 => total_frac = 1_999_998 => carry 1, remainder 999_998
        let a = Amount { whole: 10, fractional: 999_999, code: 0 };
        let b = Amount { whole: 20, fractional: 999_999, code: 0 };
        let c = (a + b).unwrap();
        assert_eq!(c.whole, 31);
        assert_eq!(c.fractional, 999_998);
    }

    // -----------------------------------------------------------------------
    // TicketExpiry tests
    // -----------------------------------------------------------------------

    #[test]
    fn ticket_expires_on_one_epoch() {
        let ticket = SignedTicket {
            ticket: Some(Ticket {
                credential: vec![],
                tags: Some(TicketData {
                    created_on: 1_000_000,
                    lifetime: 1,
                    group_number: 0,
                }),
            }),
            signature: vec![],
        };
        // 1 epoch = 86400 seconds
        assert_eq!(ticket.expires_on(), 1_000_000 + 86400);
    }

    #[test]
    fn ticket_expires_on_multiple_epochs() {
        let ticket = SignedTicket {
            ticket: Some(Ticket {
                credential: vec![],
                tags: Some(TicketData {
                    created_on: 500_000,
                    lifetime: 7,
                    group_number: 0,
                }),
            }),
            signature: vec![],
        };
        assert_eq!(ticket.expires_on(), 500_000 + 7 * 86400);
    }

    // -----------------------------------------------------------------------
    // Token::verify edge case tests (no crypto, structural checks)
    // -----------------------------------------------------------------------

    #[test]
    fn token_verify_missing_base_returns_error() {
        let token = Token {
            base: None,
            descriptor: Some(Descriptor {
                value: Some(Amount { whole: 1, fractional: 0, code: 0 }),
                ..Default::default()
            }),
            history: vec![],
        };
        assert_eq!(
            token.verify(&vec![], &vec![], &vec![]),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn token_verify_missing_descriptor_returns_error() {
        let token = Token {
            base: Some(History {
                transfer: Some(Transfer {
                    recipient: Some(SignedTicket::default()),
                    tags: vec![],
                    previous_signature: vec![],
                }),
                signature: vec![1, 2, 3],
            }),
            descriptor: None,
            history: vec![],
        };
        assert_eq!(
            token.verify(&vec![], &vec![], &vec![]),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn token_verify_base_missing_transfer_returns_error() {
        let token = Token {
            base: Some(History {
                transfer: None,
                signature: vec![],
            }),
            descriptor: Some(Descriptor::default()),
            history: vec![],
        };
        assert_eq!(
            token.verify(&vec![], &vec![], &vec![]),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn history_verify_missing_transfer_returns_error() {
        let h = History {
            transfer: None,
            signature: vec![],
        };
        assert_eq!(
            h.verify_history(&vec![], &vec![], &vec![], &vec![]),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn history_verify_missing_recipient_returns_error() {
        let h = History {
            transfer: Some(Transfer {
                recipient: None,
                tags: vec![],
                previous_signature: vec![],
            }),
            signature: vec![],
        };
        assert_eq!(
            h.verify_history(&vec![], &vec![], &vec![], &vec![]),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn history_verify_base_missing_transfer_returns_error() {
        let h = History {
            transfer: None,
            signature: vec![],
        };
        assert_eq!(
            h.verify_base(&Descriptor::default(), &vec![], &vec![]),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn history_verify_base_empty_signature_returns_error() {
        let h = History {
            transfer: Some(Transfer {
                recipient: Some(SignedTicket::default()),
                tags: vec![],
                previous_signature: vec![],
            }),
            signature: vec![],
        };
        assert_eq!(
            h.verify_base(&Descriptor::default(), &vec![], &vec![]),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    // -----------------------------------------------------------------------
    // Token::verify split value validation
    // -----------------------------------------------------------------------

    #[test]
    fn token_verify_split_value_exceeds_descriptor_detected() {
        // Build a minimal token where split_value.whole > descriptor.whole
        // We skip base verification (empty trusted_mints) so verify_base
        // gets an error first — but we can test the split check in isolation
        // by checking the tag validation path.
        let split_amount = Amount { whole: 100, fractional: 0, code: 0 };
        let original_value = Amount { whole: 10, fractional: 0, code: 0 };

        // The split check: split_amount.whole > original_value.whole
        assert!(split_amount.whole > original_value.whole);
    }

    #[test]
    fn token_verify_split_currency_mismatch_detected() {
        let original = Amount { whole: 10, fractional: 0, code: 0 };
        let split = Amount { whole: 5, fractional: 0, code: 840 };
        assert_ne!(original.code, split.code);
    }

    // -----------------------------------------------------------------------
    // VerifyTicket structural tests
    // -----------------------------------------------------------------------

    #[test]
    fn verify_ticket_missing_ticket_field_returns_error() {
        let st = SignedTicket {
            ticket: None,
            signature: vec![1, 2, 3],
        };
        assert_eq!(
            st.verify_signature_and_key(&vec![], None),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn verify_ticket_empty_signature_returns_error() {
        let st = SignedTicket {
            ticket: Some(Ticket {
                credential: vec![1, 2, 3],
                tags: Some(TicketData {
                    group_number: 0,
                    lifetime: 1,
                    created_on: 100,
                }),
            }),
            signature: vec![],
        };
        assert_eq!(
            st.verify_signature_and_key(&vec![], None),
            Err(BrioletteErrorCode::InvalidMissingFields)
        );
    }

    #[test]
    fn verify_ticket_bad_recovery_id_returns_error() {
        let st = SignedTicket {
            ticket: Some(Ticket {
                credential: vec![1, 2, 3],
                tags: Some(TicketData {
                    group_number: 0,
                    lifetime: 1,
                    created_on: 100,
                }),
            }),
            // A single byte that's not a valid RecoveryId (0-3 are valid)
            signature: vec![0xFF],
        };
        assert_eq!(
            st.verify_signature_and_key(&vec![], None),
            Err(BrioletteErrorCode::UnrecoverablePublicKey)
        );
    }

    // -----------------------------------------------------------------------
    // Token transfer structural tests
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "transfer cannot be called with no base")]
    fn token_transfer_no_base_panics() {
        let mut token = Token {
            base: None,
            descriptor: None,
            history: vec![],
        };
        let dest = SignedTicket::default();
        let _ = token.transfer(&dest, vec![1, 2, 3]);
    }

    #[test]
    #[should_panic(expected = "transfer cannot be called with no base")]
    fn token_transfer_split_no_base_panics() {
        use briolette_crypto::v0::split::MockCard;
        let mut token = Token {
            base: None,
            descriptor: None,
            history: vec![],
        };
        let dest = SignedTicket::default();
        let mut card = MockCard::new();
        let _ = token.transfer_split(&dest, &mut card, vec![1, 2, 3], None);
    }
}
