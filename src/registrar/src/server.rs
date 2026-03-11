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

use crate::attestation::{self, AttestationResult};
use briolette_crypto::v0;
use briolette_proto::briolette::registrar::{
    Algorithm, CredentialReply, RegisterReply, RegisterRequest, SecurityLevel,
};
use briolette_proto::briolette::Version;
use briolette_proto::briolette::{Error as BrioletteError, ErrorCode as BrioletteErrorCode};

use log::{error, info, trace, warn};
use std::path::Path;

/// An ECDAA issuer keypair.
#[derive(Debug, Clone, Default)]
pub struct IssuerKeys {
    pub secret_key: Vec<u8>,
    pub group_public_key: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct BrioletteRegistrar {
    // Default NAC issuer keypair (used when no per-level keys are configured).
    network_secret_key: Vec<u8>,
    network_group_public_key: Vec<u8>,
    // Per-security-level NAC issuer keypairs.  The registrar selects
    // which NAC group to issue into based on attestation strength.
    // All wallets share the same TTC group so they can transact — the
    // differentiation is purely on the NAC (policy) side.
    // Index: SecurityLevel as usize (0=Low, 1=Medium, 2=High).
    nac_issuers: Vec<IssuerKeys>,
    // Single TTC issuer keypair — shared by all wallets.
    transfer_secret_key: Vec<u8>,
    transfer_group_public_key: Vec<u8>,
    /// DER-encoded trusted root certificates for Android Key Attestation.
    pub android_trusted_roots: Vec<Vec<u8>>,
    /// DER-encoded trusted root certificates for Apple App Attest.
    pub ios_trusted_roots: Vec<Vec<u8>>,
    /// Expected iOS app identifier (team_id.bundle_id) for App Attest.
    pub ios_app_id: String,
    /// Whether to require hardware attestation (reject Algorithm::NONE).
    pub require_attestation: bool,
}

impl BrioletteRegistrar {
    fn read_or_generate_key(
        generate: bool,
        secret_key_file: &Path,
        group_public_key_file: &Path,
        sk: &mut Vec<u8>,
        gpk: &mut Vec<u8>,
    ) -> bool {
        let mut loaded = false;
        if let Ok(mut secret_key_in) = std::fs::read(secret_key_file) {
            if let Ok(mut group_public_key_in) = std::fs::read(group_public_key_file) {
                info!(
                    "loaded keys from disk: {}, {}",
                    secret_key_file.display(),
                    group_public_key_file.display()
                );
                sk.append(&mut secret_key_in);
                gpk.append(&mut group_public_key_in);
                loaded = true;
            }
        }
        if !loaded {
            if generate {
                // Generate a new secret key public key, and group key.
                info!(
                    "generating new issuer keypair: {}, {}",
                    secret_key_file.display(),
                    group_public_key_file.display()
                );
                let result = v0::generate_issuer_keypair(sk, gpk);
                if result == false {
                    error!("failed to generate issuer keypair");
                    return false;
                }
                // Attempt to update the supplied path with the new keys.
                if !secret_key_file.as_os_str().is_empty() {
                    std::fs::write(secret_key_file, sk).unwrap_or_else(|_| {
                        panic!(
                            "could not write secret key to: {:?}/{:?}",
                            std::env::current_dir().unwrap(),
                            secret_key_file
                        )
                    });
                }
                if !group_public_key_file.as_os_str().is_empty() {
                    std::fs::write(group_public_key_file, gpk).unwrap();
                }
                loaded = true;
            } else {
                error!("no issuer keypairs found and generation disabled!");
            }
        }
        return loaded;
    }

    /// Create a registrar with a single TTC issuer keypair (all security
    /// levels receive the same group).  This is the original API.
    pub fn new(
        generate: bool,
        network_secret_key_file: &Path,
        network_group_public_key_file: &Path,
        transfer_secret_key_file: &Path,
        transfer_group_public_key_file: &Path,
    ) -> Self {
        let mut network_secret_key: Vec<u8> = vec![];
        let mut network_group_public_key: Vec<u8> = vec![];
        let mut transfer_secret_key: Vec<u8> = vec![];
        let mut transfer_group_public_key: Vec<u8> = vec![];
        assert_eq!(
            BrioletteRegistrar::read_or_generate_key(
                generate,
                &network_secret_key_file,
                &network_group_public_key_file,
                &mut network_secret_key,
                &mut network_group_public_key
            ),
            true
        );
        assert_eq!(
            BrioletteRegistrar::read_or_generate_key(
                generate,
                &transfer_secret_key_file,
                &transfer_group_public_key_file,
                &mut transfer_secret_key,
                &mut transfer_group_public_key
            ),
            true
        );
        Self {
            network_secret_key,
            network_group_public_key,
            transfer_secret_key,
            transfer_group_public_key,
            ..Default::default()
        }
    }

    /// Create a registrar with per-security-level NAC issuer keypairs.
    /// Each file pair is (secret_key, group_public_key) for the given level.
    /// Levels without file paths fall back to the default NAC keypair.
    /// All wallets share the same TTC group so they can transact.
    pub fn new_tiered(
        generate: bool,
        default_network_secret_key_file: &Path,
        default_network_group_public_key_file: &Path,
        transfer_secret_key_file: &Path,
        transfer_group_public_key_file: &Path,
        nac_level_keys: &[(SecurityLevel, &Path, &Path)],
    ) -> Self {
        let mut registrar = Self::new(
            generate,
            default_network_secret_key_file,
            default_network_group_public_key_file,
            transfer_secret_key_file,
            transfer_group_public_key_file,
        );
        // Initialize 3 slots (Low=0, Medium=1, High=2), all empty.
        registrar.nac_issuers = vec![IssuerKeys::default(); 3];
        for (level, sk_file, gpk_file) in nac_level_keys {
            let idx = *level as usize;
            let keys = &mut registrar.nac_issuers[idx];
            assert!(
                BrioletteRegistrar::read_or_generate_key(
                    generate,
                    sk_file,
                    gpk_file,
                    &mut keys.secret_key,
                    &mut keys.group_public_key,
                ),
                "failed to load NAC keypair for level {:?}",
                level
            );
            info!(
                "loaded NAC issuer for {:?}: gpk={} bytes",
                level,
                keys.group_public_key.len()
            );
        }
        registrar
    }

    /// Look up the NAC issuer keypair for a given security level.
    /// Falls back to the default keypair if no per-level keys are configured.
    fn nac_keys_for_level(&self, level: SecurityLevel) -> (&[u8], &[u8]) {
        let idx = level as usize;
        if idx < self.nac_issuers.len() && !self.nac_issuers[idx].secret_key.is_empty() {
            (&self.nac_issuers[idx].secret_key, &self.nac_issuers[idx].group_public_key)
        } else {
            (&self.network_secret_key, &self.network_group_public_key)
        }
    }

    /// Verify the hardware attestation based on the algorithm type.
    /// Returns the full attestation result (security level + hardware nonce).
    ///
    /// `credential_public_keys` are the ECDAA NAC and TTC public keys from
    /// the registration request. They must be cryptographically bound in the
    /// attestation challenge to prevent attestation replay attacks.
    fn verify_attestation(
        &self,
        hwid: &briolette_proto::briolette::registrar::HardwareId,
        sig: &briolette_proto::briolette::registrar::Signature,
        credential_public_keys: &[&[u8]],
    ) -> Result<AttestationResult, BrioletteError> {
        let algorithm = match sig.algorithm {
            x if x == Algorithm::None as i32 => Algorithm::None,
            x if x == Algorithm::AndroidKmAttestation as i32 => Algorithm::AndroidKmAttestation,
            x if x == Algorithm::IosAppAttest as i32 => Algorithm::IosAppAttest,
            _ => Algorithm::None,
        };
        match algorithm {
            Algorithm::None => {
                if self.require_attestation {
                    error!("attestation required but Algorithm::NONE received");
                    return Err(BrioletteError {
                        code: BrioletteErrorCode::InvalidHwidSignature.into(),
                    });
                }
                warn!("no hardware attestation provided; using hw_id as nonce");
                Ok(AttestationResult {
                    security_level: SecurityLevel::Low,
                    hw_nonce: hwid.hw_id.clone(),
                })
            }
            Algorithm::AndroidKmAttestation => {
                info!("verifying Android Key Attestation");
                match attestation::verify_android_attestation(
                    hwid,
                    sig,
                    &self.android_trusted_roots,
                    credential_public_keys,
                ) {
                    Ok(result) => {
                        info!(
                            "Android attestation verified: security_level={:?}",
                            result.security_level
                        );
                        Ok(result)
                    }
                    Err(e) => {
                        error!("Android attestation verification failed: {}", e);
                        Err(BrioletteError {
                            code: BrioletteErrorCode::InvalidHwidSignature.into(),
                        })
                    }
                }
            }
            Algorithm::IosAppAttest => {
                info!("verifying iOS App Attest");
                match attestation::verify_ios_attestation(
                    hwid,
                    sig,
                    &self.ios_app_id,
                    &self.ios_trusted_roots,
                    credential_public_keys,
                ) {
                    Ok(result) => {
                        info!(
                            "iOS App Attest verified: security_level={:?}",
                            result.security_level
                        );
                        Ok(result)
                    }
                    Err(e) => {
                        error!("iOS App Attest verification failed: {}", e);
                        Err(BrioletteError {
                            code: BrioletteErrorCode::InvalidHwidSignature.into(),
                        })
                    }
                }
            }
        }
    }

    // We always provide a non-async implementation for cleaner testing.
    // It also allows migration to different wrapping frameworks.
    pub fn register_call_impl(
        &self,
        request: &RegisterRequest,
    ) -> Result<RegisterReply, BrioletteError> {
        trace!("register_call: request = {:?}", &request);
        // 1. Validate the version and required fields.
        if request.version != Version::Current.into() {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidVersion.into(),
            });
        }
        if !request.hwid.is_some()
            || !request.hwid_signature.is_some()
            || !request.network_credential.is_some()
            || !request.transfer_credential.is_some()
        {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidMissingFields.into(),
            });
        }
        let hwid = request.hwid.clone().unwrap();
        let hwid_signature = request.hwid_signature.clone().unwrap();
        let network_request = request.network_credential.clone().unwrap();
        let transfer_request = request.transfer_credential.clone().unwrap();

        // 2. Verify hardware attestation (Android Key Attestation, iOS App Attest, or NONE).
        //    Pass credential public keys for cryptographic binding verification —
        //    the attestation challenge must commit to these keys to prevent replay.
        let credential_pks: Vec<&[u8]> = vec![
            &network_request.public_key,
            &transfer_request.public_key,
        ];
        let attestation_result = self.verify_attestation(&hwid, &hwid_signature, &credential_pks)?;

        // 3. Select the NAC issuer keypair based on attestation strength.
        //    All wallets share the same TTC group so they can transact.
        //    The NAC group determines the wallet's policy tier (ticket
        //    lifetime, etc.) — the clerk looks up policy by NAC GPK.
        let (nac_sk, nac_gpk) = self.nac_keys_for_level(attestation_result.security_level);
        info!(
            "issuing NAC credential for security_level={:?}, nac_gpk_len={}",
            attestation_result.security_level,
            nac_gpk.len()
        );

        // 4. Issue a network credential with the nonce of the token public key,
        //    using the security-level-appropriate NAC issuer.
        let mut network_credential = vec![];
        let mut network_credential_signature = vec![];
        if v0::issue_credential(
            &network_request.public_key,
            &nac_sk.to_vec(),
            &transfer_request.public_key,
            &mut network_credential,
            &mut network_credential_signature,
        ) == false
        {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidNetworkCredentialRequest.into(),
            });
        }

        // 5. Issue a token credential with the attestation-derived hardware nonce.
        //    TTC is the same for all wallets.
        let mut transfer_credential = vec![];
        let mut transfer_credential_signature = vec![];
        if v0::issue_credential(
            &transfer_request.public_key,
            &self.transfer_secret_key,
            &attestation_result.hw_nonce,
            &mut transfer_credential,
            &mut transfer_credential_signature,
        ) == false
        {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidTokenCredentialRequest.into(),
            });
        }

        let reply = RegisterReply {
            network_credential: Some(CredentialReply {
                credential: network_credential,
                credential_signature: network_credential_signature,
                group_public_key: nac_gpk.to_vec(),
            }),
            transfer_credential: Some(CredentialReply {
                credential: transfer_credential,
                credential_signature: transfer_credential_signature,
                group_public_key: self.transfer_group_public_key.clone(),
            }),
        };

        // 6. Return the new credentials.
        return Ok(reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a registrar with tiered NAC keypairs (generated in-memory)
    /// and verify that different security levels yield different NAC GPKs
    /// but the same TTC GPK.
    fn make_tiered_registrar() -> BrioletteRegistrar {
        let mut registrar = BrioletteRegistrar::default();
        // Generate a single TTC keypair (shared by all wallets).
        assert!(v0::generate_issuer_keypair(
            &mut registrar.transfer_secret_key,
            &mut registrar.transfer_group_public_key
        ));
        // Generate separate NAC keypairs for Low, Medium, High.
        registrar.nac_issuers = vec![IssuerKeys::default(); 3];
        for level in [SecurityLevel::Low, SecurityLevel::Medium, SecurityLevel::High] {
            let idx = level as usize;
            let keys = &mut registrar.nac_issuers[idx];
            assert!(v0::generate_issuer_keypair(
                &mut keys.secret_key,
                &mut keys.group_public_key
            ));
        }
        // Use Low as the default fallback too.
        registrar.network_secret_key = registrar.nac_issuers[0].secret_key.clone();
        registrar.network_group_public_key = registrar.nac_issuers[0].group_public_key.clone();
        registrar
    }

    /// Helper: generate a wallet keypair and build a register request.
    /// The TTC key must be generated with the hw_id as nonce (since the
    /// registrar will use hw_id as hw_nonce for Algorithm::None).
    fn make_register_request(hw_id: &[u8]) -> RegisterRequest {
        let mut ttc_pk = vec![];
        let mut ttc_sk = vec![];
        assert!(v0::generate_wallet_keypair(&hw_id.to_vec(), &mut ttc_sk, &mut ttc_pk));
        let mut nac_pk = vec![];
        let mut nac_sk = vec![];
        assert!(v0::generate_wallet_keypair(&ttc_pk, &mut nac_sk, &mut nac_pk));
        RegisterRequest {
            version: Version::Current.into(),
            hwid: Some(briolette_proto::briolette::registrar::HardwareId {
                vendor_id: 1,
                software_id: 0,
                hardware_id: 1,
                hw_id: hw_id.to_vec(),
                security: SecurityLevel::Low.into(),
            }),
            hwid_signature: Some(briolette_proto::briolette::registrar::Signature {
                algorithm: Algorithm::None.into(),
                signature: vec![],
                public_key: vec![],
            }),
            network_credential: Some(briolette_proto::briolette::registrar::CredentialRequest {
                public_key: nac_pk,
            }),
            transfer_credential: Some(briolette_proto::briolette::registrar::CredentialRequest {
                public_key: ttc_pk,
            }),
        }
    }

    #[test]
    fn tiered_nac_groups_share_same_ttc() {
        let registrar = make_tiered_registrar();
        let hw_id = vec![0xAA; 32];
        let request = make_register_request(&hw_id);

        // Register with default (no attestation → Low).
        let reply = registrar.register_call_impl(&request).unwrap();
        let nac_gpk_low = reply.network_credential.as_ref().unwrap().group_public_key.clone();
        let ttc_gpk = reply.transfer_credential.as_ref().unwrap().group_public_key.clone();

        // Verify the NAC GPK matches the Low tier.
        assert_eq!(nac_gpk_low, registrar.nac_issuers[SecurityLevel::Low as usize].group_public_key);
        // Verify TTC GPK is the shared one.
        assert_eq!(ttc_gpk, registrar.transfer_group_public_key);
    }

    #[test]
    fn different_nac_groups_per_security_level() {
        let registrar = make_tiered_registrar();

        // Get the three NAC GPKs directly.
        let low_gpk = &registrar.nac_issuers[SecurityLevel::Low as usize].group_public_key;
        let med_gpk = &registrar.nac_issuers[SecurityLevel::Medium as usize].group_public_key;
        let high_gpk = &registrar.nac_issuers[SecurityLevel::High as usize].group_public_key;

        // All three NAC groups must be distinct.
        assert_ne!(low_gpk, med_gpk, "Low and Medium NAC GPKs must differ");
        assert_ne!(med_gpk, high_gpk, "Medium and High NAC GPKs must differ");
        assert_ne!(low_gpk, high_gpk, "Low and High NAC GPKs must differ");
    }

    #[test]
    fn nac_keys_for_level_falls_back_to_default() {
        // A registrar with no tiered keys should use the default NAC keypair.
        let mut registrar = BrioletteRegistrar::default();
        assert!(v0::generate_issuer_keypair(
            &mut registrar.network_secret_key,
            &mut registrar.network_group_public_key
        ));
        assert!(v0::generate_issuer_keypair(
            &mut registrar.transfer_secret_key,
            &mut registrar.transfer_group_public_key
        ));

        let (_, gpk) = registrar.nac_keys_for_level(SecurityLevel::High);
        assert_eq!(gpk, registrar.network_group_public_key.as_slice());
    }

    #[test]
    fn clerk_lifetime_lookup_by_nac_gpk() {
        use briolette_proto::briolette::clerk::GroupPolicy;

        let registrar = make_tiered_registrar();
        let low_gpk = registrar.nac_issuers[SecurityLevel::Low as usize].group_public_key.clone();
        let high_gpk = registrar.nac_issuers[SecurityLevel::High as usize].group_public_key.clone();

        // Simulate epoch policies: Low gets 3 epochs, High gets 14.
        let policies = vec![
            GroupPolicy {
                nac_group_public_key: low_gpk.clone(),
                ticket_lifetime: 3,
            },
            GroupPolicy {
                nac_group_public_key: high_gpk.clone(),
                ticket_lifetime: 14,
            },
        ];

        // Import the clerk's lookup function.
        use crate::server::tests::ticket_lifetime_for_nac_group_helper;

        // Low security → 3 epoch lifetime.
        assert_eq!(ticket_lifetime_for_nac_group_helper(&policies, &low_gpk), 3);
        // High security → 14 epoch lifetime.
        assert_eq!(ticket_lifetime_for_nac_group_helper(&policies, &high_gpk), 14);
        // Unknown GPK → default (7).
        assert_eq!(ticket_lifetime_for_nac_group_helper(&policies, &[0xFF; 32]), 7);
    }

    // Re-expose the clerk's lookup logic so we can test it here without
    // importing from a different crate's private module.
    fn ticket_lifetime_for_nac_group_helper(
        policies: &[briolette_proto::briolette::clerk::GroupPolicy],
        nac_gpk: &[u8],
    ) -> u32 {
        for p in policies {
            if p.nac_group_public_key == nac_gpk {
                return p.ticket_lifetime;
            }
        }
        7 // default
    }
}
