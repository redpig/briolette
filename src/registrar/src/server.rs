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

use crate::attestation;
use briolette_crypto::v0;
use briolette_proto::briolette::registrar::{
    Algorithm, CredentialReply, RegisterReply, RegisterRequest, SecurityLevel,
};
use briolette_proto::briolette::Version;
use briolette_proto::briolette::{Error as BrioletteError, ErrorCode as BrioletteErrorCode};

use log::{error, info, trace, warn};
use std::path::Path;

#[derive(Debug, Default)]
pub struct BrioletteRegistrar {
    // In the future, an authorized hardware vendor may issue
    // the network credential to its hardware in the field.
    //
    // The currency operator would then issue the token credential
    // which is authenticated bu the network credential.
    network_secret_key: Vec<u8>, // TODO: Add more of these to reflect different hw groups.
    network_group_public_key: Vec<u8>,
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

    /// Verify the hardware attestation based on the algorithm type.
    /// Returns the hardware nonce to use for credential issuance.
    fn verify_attestation(
        &self,
        hwid: &briolette_proto::briolette::registrar::HardwareId,
        sig: &briolette_proto::briolette::registrar::Signature,
    ) -> Result<Vec<u8>, BrioletteError> {
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
                Ok(hwid.hw_id.clone())
            }
            Algorithm::AndroidKmAttestation => {
                info!("verifying Android Key Attestation");
                match attestation::verify_android_attestation(
                    hwid,
                    sig,
                    &self.android_trusted_roots,
                ) {
                    Ok(result) => {
                        info!(
                            "Android attestation verified: security_level={:?}",
                            result.security_level
                        );
                        Ok(result.hw_nonce)
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
                ) {
                    Ok(result) => {
                        info!(
                            "iOS App Attest verified: security_level={:?}",
                            result.security_level
                        );
                        Ok(result.hw_nonce)
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
        let hw_nonce = self.verify_attestation(&hwid, &hwid_signature)?;

        // 3. Issue a network credential with the nonce of the token public key.
        let mut network_credential = vec![];
        let mut network_credential_signature = vec![];
        if v0::issue_credential(
            &network_request.public_key,
            &self.network_secret_key,
            &transfer_request.public_key,
            &mut network_credential,
            &mut network_credential_signature,
        ) == false
        {
            return Err(BrioletteError {
                code: BrioletteErrorCode::InvalidNetworkCredentialRequest.into(),
            });
        }

        // 4. Issue a token credential with the attestation-derived hardware nonce.
        let mut transfer_credential = vec![];
        let mut transfer_credential_signature = vec![];
        if v0::issue_credential(
            &transfer_request.public_key,
            &self.transfer_secret_key,
            &hw_nonce,
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
                group_public_key: self.network_group_public_key.clone(),
            }),
            transfer_credential: Some(CredentialReply {
                credential: transfer_credential,
                credential_signature: transfer_credential_signature,
                group_public_key: self.transfer_group_public_key.clone(),
            }),
        };

        // 5. Return the new credentials.
        return Ok(reply);
    }
}
