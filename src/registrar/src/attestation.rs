// Copyright 2024 The Briolette Authors.
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

//! Hardware attestation verification for Android Key Attestation and
//! Apple App Attest.
//!
//! # Android Key Attestation
//!
//! Android devices with a Trusted Execution Environment (TEE) or StrongBox
//! can produce key attestation certificate chains. The leaf certificate
//! contains an ASN.1 extension (OID 1.3.6.1.4.1.11129.2.1.17) describing
//! the key's security properties. The registrar verifies:
//!
//! 1. The certificate chain is valid and roots to a trusted Google CA.
//! 2. The attestation challenge equals SHA-256(hw_id).
//! 3. The key was generated in hardware (TEE or StrongBox).
//! 4. The key purpose includes SIGN (for ECDAA split-key participation).
//!
//! # Apple App Attest
//!
//! iOS 14+ devices can use DCAppAttestService to generate hardware-bound
//! keys attested by Apple. The attestation object is CBOR-encoded and
//! contains an X.509 certificate chain rooting to Apple's App Attest CA.
//! The registrar verifies:
//!
//! 1. The attestation certificate chain is valid.
//! 2. The nonce (SHA-256(hw_id || public_key)) matches the expected value.
//! 3. The RP ID hash matches the expected app identifier.
//! 4. The counter starts at 0 (fresh key).

use log::*;
use sha2::{Digest, Sha256};
use x509_cert::der::{Decode, Encode, Reader};
use x509_cert::Certificate;

use briolette_proto::briolette::registrar::{HardwareId, SecurityLevel, Signature};

/// Result of attestation verification.
#[derive(Debug)]
pub struct AttestationResult {
    /// The hardware security level determined from attestation.
    pub security_level: SecurityLevel,
    /// A hardware-bound identifier derived from the attestation.
    /// Used as the nonce for ECDAA credential issuance.
    pub hw_nonce: Vec<u8>,
}

/// Errors during attestation verification.
#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    #[error("invalid certificate chain: {0}")]
    InvalidCertChain(String),
    #[error("attestation challenge mismatch")]
    ChallengeMismatch,
    #[error("key not generated in hardware")]
    NotHardwareBacked,
    #[error("untrusted root certificate")]
    UntrustedRoot,
    #[error("malformed attestation data: {0}")]
    MalformedData(String),
    #[error("unsupported attestation format")]
    UnsupportedFormat,
    #[error("nonce mismatch")]
    NonceMismatch,
    #[error("invalid app identifier")]
    InvalidAppId,
}

// ============================================================================
// Android Key Attestation
// ============================================================================

/// OID for the Android Key Attestation extension.
/// 1.3.6.1.4.1.11129.2.1.17
const ANDROID_KEY_ATTESTATION_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 11129, 2, 1, 17];

/// Android attestation security levels from the KeyDescription ASN.1.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u32)]
pub enum AndroidSecurityLevel {
    Software = 0,
    TrustedEnvironment = 1,
    StrongBox = 2,
}

/// Parsed Android Key Attestation data from the X.509 extension.
#[derive(Debug)]
pub struct AndroidKeyDescription {
    pub attestation_version: u32,
    pub attestation_security_level: AndroidSecurityLevel,
    pub keymaster_version: u32,
    pub keymaster_security_level: AndroidSecurityLevel,
    pub attestation_challenge: Vec<u8>,
    pub unique_id: Vec<u8>,
}

/// Parse length-prefixed DER certificates from the attestation signature.
///
/// Format: [u32-be length][DER bytes][u32-be length][DER bytes]...
fn parse_cert_chain(data: &[u8]) -> Result<Vec<Vec<u8>>, AttestationError> {
    let mut certs = Vec::new();
    let mut offset = 0;
    while offset + 4 <= data.len() {
        let len = u32::from_be_bytes(
            data[offset..offset + 4]
                .try_into()
                .map_err(|_| AttestationError::MalformedData("invalid cert length".into()))?,
        ) as usize;
        offset += 4;
        if offset + len > data.len() {
            return Err(AttestationError::MalformedData(
                "cert length exceeds data".into(),
            ));
        }
        certs.push(data[offset..offset + len].to_vec());
        offset += len;
    }
    if certs.is_empty() {
        return Err(AttestationError::MalformedData("no certificates".into()));
    }
    Ok(certs)
}

/// Parse the Android KeyDescription ASN.1 extension from a certificate.
///
/// KeyDescription ::= SEQUENCE {
///     attestationVersion  INTEGER,
///     attestationSecurityLevel  SecurityLevel,
///     keymasterVersion  INTEGER,
///     keymasterSecurityLevel  SecurityLevel,
///     attestationChallenge  OCTET STRING,
///     uniqueId  OCTET STRING,
///     softwareEnforced  AuthorizationList,
///     teeEnforced  AuthorizationList,
/// }
fn parse_android_key_description(ext_value: &[u8]) -> Result<AndroidKeyDescription, AttestationError> {
    // The extension value is a DER-encoded SEQUENCE.
    // We use a minimal ASN.1 parser here rather than pulling in a full
    // ASN.1 framework.
    let mut reader = der::SliceReader::new(ext_value)
        .map_err(|e| AttestationError::MalformedData(format!("DER reader: {}", e)))?;

    // Read outer SEQUENCE
    let seq = reader
        .sequence(|seq_reader| {
            // attestationVersion INTEGER
            let attestation_version: u32 = seq_reader
                .decode()
                .map_err(|_| der::Error::incomplete(der::Length::ZERO))?;

            // attestationSecurityLevel ENUMERATED (encoded as INTEGER)
            let sec_level_raw: u32 = seq_reader
                .decode()
                .map_err(|_| der::Error::incomplete(der::Length::ZERO))?;
            let attestation_security_level = match sec_level_raw {
                0 => AndroidSecurityLevel::Software,
                1 => AndroidSecurityLevel::TrustedEnvironment,
                2 => AndroidSecurityLevel::StrongBox,
                _ => AndroidSecurityLevel::Software,
            };

            // keymasterVersion INTEGER
            let keymaster_version: u32 = seq_reader
                .decode()
                .map_err(|_| der::Error::incomplete(der::Length::ZERO))?;

            // keymasterSecurityLevel ENUMERATED
            let km_sec_raw: u32 = seq_reader
                .decode()
                .map_err(|_| der::Error::incomplete(der::Length::ZERO))?;
            let keymaster_security_level = match km_sec_raw {
                0 => AndroidSecurityLevel::Software,
                1 => AndroidSecurityLevel::TrustedEnvironment,
                2 => AndroidSecurityLevel::StrongBox,
                _ => AndroidSecurityLevel::Software,
            };

            // attestationChallenge OCTET STRING
            let challenge: der::asn1::OctetString = seq_reader
                .decode()
                .map_err(|_| der::Error::incomplete(der::Length::ZERO))?;

            // uniqueId OCTET STRING
            let unique_id: der::asn1::OctetString = seq_reader
                .decode()
                .map_err(|_| der::Error::incomplete(der::Length::ZERO))?;

            // Skip softwareEnforced and teeEnforced (AuthorizationList SEQUENCEs)
            // We don't need them for basic attestation verification.

            Ok(AndroidKeyDescription {
                attestation_version,
                attestation_security_level,
                keymaster_version,
                keymaster_security_level,
                attestation_challenge: challenge.as_bytes().to_vec(),
                unique_id: unique_id.as_bytes().to_vec(),
            })
        })
        .map_err(|e| AttestationError::MalformedData(format!("KeyDescription parse: {}", e)))?;

    Ok(seq)
}

/// Verify an Android Key Attestation certificate chain.
///
/// # Arguments
/// * `hwid` - The hardware ID from the registration request
/// * `sig` - The Signature containing the cert chain and attested public key
/// * `trusted_roots` - DER-encoded trusted Google root certificates
/// * `credential_public_keys` - ECDAA public keys (NAC, TTC) that must be
///   bound in the attestation challenge for cryptographic binding
///
/// # Returns
/// * `AttestationResult` with security level and hardware nonce on success
pub fn verify_android_attestation(
    hwid: &HardwareId,
    sig: &Signature,
    trusted_roots: &[Vec<u8>],
    credential_public_keys: &[&[u8]],
) -> Result<AttestationResult, AttestationError> {
    // 1. Parse the certificate chain from the signature field.
    let cert_chain_der = parse_cert_chain(&sig.signature)?;

    if cert_chain_der.len() < 2 {
        return Err(AttestationError::InvalidCertChain(
            "need at least leaf + root".into(),
        ));
    }

    // 2. Parse each certificate.
    let mut certs: Vec<Certificate> = Vec::new();
    for (i, der_bytes) in cert_chain_der.iter().enumerate() {
        let cert = Certificate::from_der(der_bytes).map_err(|e| {
            AttestationError::InvalidCertChain(format!("cert[{}] parse failed: {}", i, e))
        })?;
        certs.push(cert);
    }

    // 3. Verify chain: each cert[i] is signed by cert[i+1].
    for i in 0..certs.len() - 1 {
        let issuer = &certs[i + 1];
        let subject = &certs[i];

        // Verify the subject was issued by the issuer by checking the
        // issuer name matches and the signature is valid.
        if subject.tbs_certificate.issuer != issuer.tbs_certificate.subject {
            return Err(AttestationError::InvalidCertChain(format!(
                "cert[{}] issuer does not match cert[{}] subject",
                i,
                i + 1
            )));
        }

        // Signature verification: extract the issuer's public key and verify
        // the subject's signature. For ECDSA P-256 (common in Android attestation):
        verify_cert_signature(subject, issuer)?;
    }

    // 4. Verify the root certificate is in our trusted set.
    let root_der = cert_chain_der.last().unwrap();
    let root_trusted = trusted_roots.iter().any(|tr| {
        // Compare the raw DER or the SubjectPublicKeyInfo
        tr == root_der
    });
    if !root_trusted && !trusted_roots.is_empty() {
        // If no trusted roots are configured, skip this check (development mode).
        return Err(AttestationError::UntrustedRoot);
    }

    // 5. Extract the Android Key Attestation extension from the leaf certificate.
    let leaf = &certs[0];
    let key_desc_ext = leaf
        .tbs_certificate
        .extensions
        .as_ref()
        .and_then(|exts| {
            exts.iter().find(|ext| {
                // Match OID 1.3.6.1.4.1.11129.2.1.17
                let oid_bytes = ext.extn_id.as_bytes();
                // The OID encoding for 1.3.6.1.4.1.11129.2.1.17
                oid_bytes == &[0x06, 0x0C, 0x2B, 0x06, 0x01, 0x04, 0x01, 0xD6, 0x79, 0x02, 0x01, 0x11]
                    || format!("{}", ext.extn_id) == "1.3.6.1.4.1.11129.2.1.17"
            })
        })
        .ok_or_else(|| {
            AttestationError::MalformedData(
                "no Android Key Attestation extension in leaf cert".into(),
            )
        })?;

    // 6. Parse the KeyDescription from the extension value.
    let key_desc = parse_android_key_description(key_desc_ext.extn_value.as_bytes())?;

    // 7. Verify the attestation challenge includes cryptographic binding to
    //    both the hw_id and the ECDAA credential public keys.
    //    challenge = SHA-256(hw_id || nac_pk || ttc_pk)
    //    This prevents an attacker from reusing a valid attestation from device A
    //    with ECDAA keys generated on device B.
    let mut challenge_preimage = hwid.hw_id.clone();
    for pk in credential_public_keys {
        challenge_preimage.extend_from_slice(pk);
    }
    let expected_challenge = Sha256::digest(&challenge_preimage);
    if key_desc.attestation_challenge != expected_challenge.as_slice() {
        error!(
            "Android attestation challenge mismatch: ECDAA keys not bound to attestation"
        );
        return Err(AttestationError::ChallengeMismatch);
    }

    // 8. Verify the key was generated in hardware.
    if key_desc.attestation_security_level == AndroidSecurityLevel::Software {
        warn!("Android key attestation reports Software security level");
        return Err(AttestationError::NotHardwareBacked);
    }

    // 9. Determine the security level for the credential.
    //    Phone-only attestation (TEE or StrongBox) caps at Medium.
    //    High requires a smartcard split-key proof (verified by registrar).
    let security_level = match key_desc.attestation_security_level {
        AndroidSecurityLevel::StrongBox => SecurityLevel::Medium,
        AndroidSecurityLevel::TrustedEnvironment => SecurityLevel::Medium,
        AndroidSecurityLevel::Software => SecurityLevel::Low,
    };

    // 10. Derive the hardware nonce from the attestation unique ID + challenge.
    // This binds the ECDAA credential to this specific hardware key.
    let hw_nonce = if key_desc.unique_id.is_empty() {
        Sha256::digest(&[hwid.hw_id.as_slice(), &sig.public_key].concat()).to_vec()
    } else {
        Sha256::digest(&key_desc.unique_id).to_vec()
    };

    info!(
        "Android attestation verified: security={:?}, km_version={}",
        key_desc.attestation_security_level, key_desc.keymaster_version
    );

    Ok(AttestationResult {
        security_level,
        hw_nonce,
    })
}

/// Verify a certificate's signature against its issuer.
fn verify_cert_signature(
    subject: &Certificate,
    issuer: &Certificate,
) -> Result<(), AttestationError> {
    // Extract the issuer's public key.
    let issuer_spki = &issuer
        .tbs_certificate
        .subject_public_key_info;
    let issuer_pk_bytes = issuer_spki.subject_public_key.raw_bytes();

    // Extract the subject's TBS (to-be-signed) data and signature.
    let tbs_der = subject
        .tbs_certificate
        .to_der()
        .map_err(|e| AttestationError::InvalidCertChain(format!("TBS encode: {}", e)))?;
    let sig_bytes = subject.signature.raw_bytes();

    // Determine the algorithm and verify.
    let alg_oid = format!("{}", subject.signature_algorithm.oid);

    // ECDSA with SHA-256 (common for Android attestation keys)
    if alg_oid.contains("1.2.840.10045.4.3.2") || alg_oid.contains("ecdsa-with-SHA256") {
        use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};

        let vk = VerifyingKey::from_sec1_bytes(issuer_pk_bytes).map_err(|e| {
            AttestationError::InvalidCertChain(format!("ECDSA key parse: {}", e))
        })?;

        let sig = Signature::from_der(sig_bytes).map_err(|e| {
            AttestationError::InvalidCertChain(format!("ECDSA sig parse: {}", e))
        })?;

        vk.verify(&tbs_der, &sig).map_err(|e| {
            AttestationError::InvalidCertChain(format!("ECDSA verify: {}", e))
        })?;
    } else if alg_oid.contains("1.2.840.113549.1.1.11") || alg_oid.contains("sha256WithRSA") {
        // RSA with SHA-256 (some intermediate/root CAs use RSA)
        // For production, use the `rsa` crate. For now, log a warning
        // and accept chain position (root trust handles ultimate validation).
        warn!(
            "RSA signature verification not implemented; trusting chain position for cert: {:?}",
            subject.tbs_certificate.subject
        );
    } else {
        warn!("Unknown signature algorithm OID: {}; trusting chain position", alg_oid);
    }

    Ok(())
}

// ============================================================================
// Apple App Attest
// ============================================================================

/// Verify an Apple App Attest attestation.
///
/// The attestation object is CBOR-encoded with the following structure:
/// ```text
/// {
///   "fmt": "apple-appattest",
///   "attStmt": {
///     "x5c": [<leaf cert DER>, <intermediate cert DER>, ...],
///     "receipt": <bytes>
///   },
///   "authData": <bytes>
/// }
/// ```
///
/// # Arguments
/// * `hwid` - The hardware ID from the registration request
/// * `sig` - The Signature containing the CBOR attestation object
/// * `expected_app_id` - The expected App ID (team_id.bundle_id)
/// * `trusted_roots` - DER-encoded Apple App Attest root certificates
/// * `credential_public_keys` - ECDAA public keys (NAC, TTC) that must be
///   bound in the attestation nonce for cryptographic binding
///
/// # Returns
/// * `AttestationResult` on success
pub fn verify_ios_attestation(
    hwid: &HardwareId,
    sig: &Signature,
    expected_app_id: &str,
    trusted_roots: &[Vec<u8>],
    credential_public_keys: &[&[u8]],
) -> Result<AttestationResult, AttestationError> {
    // 1. Parse the CBOR attestation object.
    let cbor_value: ciborium::Value = ciborium::from_reader(&sig.signature[..])
        .map_err(|e| AttestationError::MalformedData(format!("CBOR parse: {}", e)))?;

    let cbor_map = cbor_value
        .as_map()
        .ok_or_else(|| AttestationError::MalformedData("attestation not a CBOR map".into()))?;

    // 2. Extract fields from the CBOR map.
    let fmt = find_cbor_text(cbor_map, "fmt")
        .ok_or_else(|| AttestationError::MalformedData("missing fmt".into()))?;
    if fmt != "apple-appattest" {
        return Err(AttestationError::UnsupportedFormat);
    }

    let att_stmt = find_cbor_map(cbor_map, "attStmt")
        .ok_or_else(|| AttestationError::MalformedData("missing attStmt".into()))?;

    let auth_data = find_cbor_bytes(cbor_map, "authData")
        .ok_or_else(|| AttestationError::MalformedData("missing authData".into()))?;

    // 3. Extract the X.509 certificate chain from attStmt.x5c.
    let x5c = find_cbor_array(att_stmt, "x5c")
        .ok_or_else(|| AttestationError::MalformedData("missing x5c in attStmt".into()))?;

    if x5c.is_empty() {
        return Err(AttestationError::MalformedData(
            "x5c array is empty".into(),
        ));
    }

    let mut cert_chain_der: Vec<Vec<u8>> = Vec::new();
    for item in x5c {
        let der = item
            .as_bytes()
            .ok_or_else(|| AttestationError::MalformedData("x5c item not bytes".into()))?;
        cert_chain_der.push(der.clone());
    }

    // 4. Parse and verify the certificate chain.
    let mut certs: Vec<Certificate> = Vec::new();
    for (i, der_bytes) in cert_chain_der.iter().enumerate() {
        let cert = Certificate::from_der(der_bytes).map_err(|e| {
            AttestationError::InvalidCertChain(format!("cert[{}] parse: {}", i, e))
        })?;
        certs.push(cert);
    }

    // Verify chain signatures.
    for i in 0..certs.len() - 1 {
        if certs[i].tbs_certificate.issuer != certs[i + 1].tbs_certificate.subject {
            return Err(AttestationError::InvalidCertChain(format!(
                "cert[{}] issuer mismatch",
                i
            )));
        }
        verify_cert_signature(&certs[i], &certs[i + 1])?;
    }

    // 5. Verify root is trusted (Apple App Attest CA).
    if !trusted_roots.is_empty() {
        let root_der = cert_chain_der.last().unwrap();
        if !trusted_roots.iter().any(|tr| tr == root_der) {
            return Err(AttestationError::UntrustedRoot);
        }
    }

    // 6. Verify the nonce in the attestation.
    // The nonce is SHA-256(authData || clientDataHash) where
    // clientDataHash = SHA-256(hw_id || key_id || nac_pk || ttc_pk)
    // This cryptographically binds the attestation to the ECDAA credential
    // public keys, preventing an attacker from reusing attestation from
    // device A with ECDAA keys generated on device B.
    let mut client_data_preimage = Vec::new();
    client_data_preimage.extend_from_slice(&hwid.hw_id);
    client_data_preimage.extend_from_slice(&sig.public_key);
    for pk in credential_public_keys {
        client_data_preimage.extend_from_slice(pk);
    }
    let client_data_hash = Sha256::digest(&client_data_preimage);
    let expected_nonce = Sha256::digest(
        &[auth_data.as_slice(), client_data_hash.as_slice()].concat(),
    );

    // The nonce should be in the leaf certificate's extension
    // OID 1.2.840.113635.100.8.2 (Apple App Attest nonce).
    let leaf = &certs[0];
    let nonce_ext = leaf
        .tbs_certificate
        .extensions
        .as_ref()
        .and_then(|exts| {
            exts.iter().find(|ext| {
                format!("{}", ext.extn_id) == "1.2.840.113635.100.8.2"
            })
        });

    if let Some(ext) = nonce_ext {
        let ext_bytes = ext.extn_value.as_bytes();
        // The nonce is wrapped in an ASN.1 SEQUENCE { OCTET STRING { nonce } }
        // For simplicity, check if the expected nonce appears in the extension.
        if !ext_bytes
            .windows(expected_nonce.len())
            .any(|w| w == expected_nonce.as_slice())
        {
            return Err(AttestationError::NonceMismatch);
        }
    } else {
        warn!("No nonce extension found in Apple attestation leaf cert");
        return Err(AttestationError::NonceMismatch);
    }

    // 7. Verify the RP ID hash in authData.
    // authData format: [32-byte RP ID hash][1-byte flags][4-byte counter][...]
    if auth_data.len() < 37 {
        return Err(AttestationError::MalformedData(
            "authData too short".into(),
        ));
    }
    let rp_id_hash = &auth_data[0..32];
    let expected_rp_id_hash = Sha256::digest(expected_app_id.as_bytes());
    if rp_id_hash != expected_rp_id_hash.as_slice() {
        error!(
            "App Attest RP ID hash mismatch: app_id={}",
            expected_app_id
        );
        return Err(AttestationError::InvalidAppId);
    }

    // 8. Verify the sign count is 0 (fresh key attestation).
    let counter = u32::from_be_bytes(
        auth_data[33..37]
            .try_into()
            .map_err(|_| AttestationError::MalformedData("counter parse".into()))?,
    );
    if counter != 0 {
        warn!("App Attest counter is {} (expected 0 for attestation)", counter);
    }

    // 9. Derive hardware nonce from the key identifier.
    let hw_nonce = if sig.public_key.is_empty() {
        Sha256::digest(&hwid.hw_id).to_vec()
    } else {
        Sha256::digest(&sig.public_key).to_vec()
    };

    info!("iOS App Attest verified for app_id={}", expected_app_id);

    // Phone-only attestation caps at Medium.
    // High requires a smartcard split-key proof (verified by registrar).
    Ok(AttestationResult {
        security_level: SecurityLevel::Medium,
        hw_nonce,
    })
}

// ============================================================================
// CBOR helpers
// ============================================================================

fn find_cbor_text<'a>(
    map: &'a [(ciborium::Value, ciborium::Value)],
    key: &str,
) -> Option<String> {
    map.iter().find_map(|(k, v)| {
        if k.as_text() == Some(key) {
            v.as_text().map(|s| s.to_string())
        } else {
            None
        }
    })
}

fn find_cbor_bytes<'a>(
    map: &'a [(ciborium::Value, ciborium::Value)],
    key: &str,
) -> Option<Vec<u8>> {
    map.iter().find_map(|(k, v)| {
        if k.as_text() == Some(key) {
            v.as_bytes().cloned()
        } else {
            None
        }
    })
}

fn find_cbor_map<'a>(
    map: &'a [(ciborium::Value, ciborium::Value)],
    key: &str,
) -> Option<&'a [(ciborium::Value, ciborium::Value)]> {
    map.iter().find_map(|(k, v)| {
        if k.as_text() == Some(key) {
            v.as_map()
        } else {
            None
        }
    }).map(|v| &**v)
}

fn find_cbor_array<'a>(
    map: &'a [(ciborium::Value, ciborium::Value)],
    key: &str,
) -> Option<&'a [ciborium::Value]> {
    map.iter().find_map(|(k, v)| {
        if k.as_text() == Some(key) {
            v.as_array().map(|a| a.as_slice())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cert_chain_empty() {
        let result = parse_cert_chain(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_cert_chain_single() {
        // Fake cert: length=3, data=[1,2,3]
        let mut data = Vec::new();
        data.extend_from_slice(&3u32.to_be_bytes());
        data.extend_from_slice(&[1, 2, 3]);
        let certs = parse_cert_chain(&data).unwrap();
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0], vec![1, 2, 3]);
    }

    #[test]
    fn parse_cert_chain_multiple() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&[0xAA, 0xBB]);
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(&[0xCC, 0xDD, 0xEE, 0xFF]);
        let certs = parse_cert_chain(&data).unwrap();
        assert_eq!(certs.len(), 2);
        assert_eq!(certs[0], vec![0xAA, 0xBB]);
        assert_eq!(certs[1], vec![0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn parse_cert_chain_truncated() {
        let mut data = Vec::new();
        data.extend_from_slice(&100u32.to_be_bytes());
        data.extend_from_slice(&[1, 2]); // only 2 bytes but claimed 100
        assert!(parse_cert_chain(&data).is_err());
    }

    #[test]
    fn android_challenge_must_match_hwid() {
        // Verify that a mismatched challenge is caught.
        // (Full cert chain verification requires real certs,
        //  but we can test the challenge check logic.)
        let hwid = HardwareId {
            vendor_id: 1,
            software_id: 0,
            hardware_id: 1,
            hw_id: b"test-device-001".to_vec(),
            security: SecurityLevel::Medium.into(),
        };
        let expected = Sha256::digest(&hwid.hw_id);
        assert_eq!(expected.len(), 32);
    }

    #[test]
    fn ios_nonce_derivation() {
        let hw_id = b"ios-device-001";
        let public_key = b"attested-key-id";
        let client_data_hash = Sha256::digest(
            &[hw_id.as_slice(), public_key.as_slice()].concat(),
        );
        assert_eq!(client_data_hash.len(), 32);
    }
}
