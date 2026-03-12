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

//! NAC-based service-to-service authentication for Briolette.
//!
//! Services register with the registrar to obtain NAC credentials, then use
//! those credentials to sign outgoing gRPC requests and verify incoming ones.
//! This keeps the security model self-contained within Briolette's existing
//! credential infrastructure — no secondary PKI (mTLS) needed.

use briolette_crypto::v0;
use briolette_proto::briolette::registrar::registrar_client::RegistrarClient;
use briolette_proto::briolette::registrar::{
    Algorithm, CredentialRequest, HardwareId, RegisterRequest, Signature,
};
use briolette_proto::briolette::service_auth::{ServiceAuthMetadata, ServiceGroup};
use briolette_proto::briolette::Version;
use log::*;
use prost::Message;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::metadata::{MetadataMap, MetadataValue};
use tonic::{Code, Request, Status};

/// Maximum age (in seconds) for a service auth timestamp before it's rejected.
const MAX_TIMESTAMP_AGE_SECS: u64 = 30;

/// Metadata key for the binary-encoded ServiceAuthMetadata.
const SERVICE_AUTH_METADATA_KEY: &str = "x-briolette-service-auth-bin";

/// Holds a service's NAC credential and keys for signing/verifying requests.
#[derive(Debug, Clone)]
pub struct ServiceIdentity {
    pub group: ServiceGroup,
    pub secret_key: Vec<u8>,
    pub public_key: Vec<u8>,
    pub credential: Vec<u8>,
    pub credential_signature: Vec<u8>,
    pub group_public_key: Vec<u8>,
}

/// On-disk state for a service identity, persisted so that the service
/// keeps the same hw_id and keypair across restarts.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedServiceState {
    hw_id: Vec<u8>,
    nonce: Vec<u8>,
}

impl ServiceIdentity {
    /// Compute the state file path for a given service group.
    fn state_path(state_dir: &Path, group: ServiceGroup) -> PathBuf {
        state_dir.join(format!("service_{}.json", group.as_str_name()))
    }

    /// Load or create persisted state (random hw_id + nonce).
    fn load_or_create_state(
        state_dir: &Path,
        group: ServiceGroup,
    ) -> Result<PersistedServiceState, Box<dyn std::error::Error>> {
        let path = Self::state_path(state_dir, group);
        if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            let state: PersistedServiceState = serde_json::from_str(&data)?;
            info!("loaded service identity state from {}", path.display());
            return Ok(state);
        }

        // Generate random hw_id and nonce.
        let mut hw_id = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut hw_id);
        let mut nonce = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce);

        let state = PersistedServiceState { hw_id, nonce };

        // Persist to disk.
        std::fs::create_dir_all(state_dir)?;
        let json = serde_json::to_string_pretty(&state)?;
        std::fs::write(&path, &json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        info!("created new service identity state at {}", path.display());

        Ok(state)
    }

    /// Register this service with the registrar to obtain a NAC credential.
    ///
    /// Uses a randomly-generated hardware ID persisted to `state_dir` so
    /// that the identity is stable across restarts but not predictable.
    pub async fn register(
        registrar_uri: &str,
        group: ServiceGroup,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Default state directory next to typical data dirs.
        let state_dir = PathBuf::from("data/service_auth");
        Self::register_with_state_dir(registrar_uri, group, &state_dir).await
    }

    /// Register with an explicit state directory for persisting the identity.
    pub async fn register_with_state_dir(
        registrar_uri: &str,
        group: ServiceGroup,
        state_dir: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let state = Self::load_or_create_state(state_dir, group)?;

        // Generate keypair using the persisted random nonce.
        let mut secret_key = vec![];
        let mut public_key = vec![];
        if !v0::generate_wallet_keypair(&state.nonce, &mut secret_key, &mut public_key) {
            return Err("failed to generate service keypair".into());
        }

        let request = RegisterRequest {
            version: Version::Current.into(),
            hwid: Some(HardwareId {
                vendor_id: 0,
                software_id: 0,
                hardware_id: group as u64,
                hw_id: state.hw_id.clone(),
                security: 0, // LOW
            }),
            hwid_signature: Some(Signature {
                algorithm: Algorithm::None.into(),
                signature: vec![],
                public_key: vec![],
            }),
            network_credential: Some(CredentialRequest {
                public_key: public_key.clone(),
            }),
            transfer_credential: Some(CredentialRequest {
                public_key: public_key.clone(),
            }),
            split_key_proof: None,
        };

        let mut client = RegistrarClient::connect(registrar_uri.to_string()).await?;
        let response = client.register_call(request).await?;
        let reply = response.into_inner();

        let network_cred = reply
            .network_credential
            .ok_or("no network credential in reply")?;

        info!(
            "service {:?} registered with registrar, credential len={}",
            group,
            network_cred.credential.len()
        );

        Ok(Self {
            group,
            secret_key,
            public_key,
            credential: network_cred.credential,
            credential_signature: network_cred.credential_signature,
            group_public_key: network_cred.group_public_key,
        })
    }

    /// Sign a request payload, producing ServiceAuthMetadata to attach as gRPC metadata.
    pub fn sign_request(&self, request_bytes: &[u8]) -> Result<ServiceAuthMetadata, String> {
        let timestamp_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("time error: {}", e))?
            .as_nanos() as u64;

        // Message = SHA-256(request_bytes) || timestamp_nanos (little-endian)
        let request_hash = Sha256::digest(request_bytes);
        let mut message = request_hash.to_vec();
        message.extend_from_slice(&timestamp_nanos.to_le_bytes());

        let mut signature = vec![];
        if !v0::sign(
            &message.to_vec(),
            &self.credential,
            &self.secret_key,
            &None, // no basename for service auth
            true,  // randomize credential
            &mut signature,
        ) {
            return Err("failed to sign request".to_string());
        }

        Ok(ServiceAuthMetadata {
            signature,
            timestamp_nanos,
            caller_group: self.group.into(),
            credential: self.credential.clone(),
        })
    }

    /// Attach service auth metadata to an outgoing gRPC request.
    pub fn sign_grpc_request<T: Message>(&self, request: &mut Request<T>) -> Result<(), String> {
        // Serialize the inner message to get the request bytes.
        let msg = request.get_ref();
        let mut request_bytes = vec![];
        msg.encode(&mut request_bytes)
            .map_err(|e| format!("encode error: {}", e))?;

        let auth_metadata = self.sign_request(&request_bytes)?;
        let mut auth_bytes = vec![];
        auth_metadata
            .encode(&mut auth_bytes)
            .map_err(|e| format!("encode auth error: {}", e))?;

        request.metadata_mut().insert_bin(
            SERVICE_AUTH_METADATA_KEY,
            MetadataValue::from_bytes(&auth_bytes),
        );
        Ok(())
    }
}

/// Verify service auth metadata from an incoming gRPC request.
///
/// Returns the caller's ServiceGroup on success, or a tonic::Status error.
pub fn verify_service_auth(
    metadata: &MetadataMap,
    request_bytes: &[u8],
    nac_group_public_key: &[u8],
) -> Result<ServiceGroup, Status> {
    let auth_value = metadata
        .get_bin(SERVICE_AUTH_METADATA_KEY)
        .ok_or_else(|| Status::new(Code::Unauthenticated, "missing service auth metadata"))?;

    let auth_bytes = auth_value
        .to_bytes()
        .map_err(|_| Status::new(Code::InvalidArgument, "invalid service auth metadata encoding"))?;

    let auth_metadata = ServiceAuthMetadata::decode(auth_bytes.as_ref())
        .map_err(|_| Status::new(Code::InvalidArgument, "failed to decode service auth metadata"))?;

    // Check timestamp freshness (replay prevention).
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let age_nanos = now_nanos.saturating_sub(auth_metadata.timestamp_nanos);
    if age_nanos > MAX_TIMESTAMP_AGE_SECS * 1_000_000_000 {
        return Err(Status::new(
            Code::Unauthenticated,
            "service auth timestamp too old",
        ));
    }

    // Reject timestamps in the future (with small tolerance for clock skew).
    if auth_metadata.timestamp_nanos > now_nanos + 5_000_000_000 {
        return Err(Status::new(
            Code::Unauthenticated,
            "service auth timestamp in the future",
        ));
    }

    // Reconstruct the signed message: SHA-256(request_bytes) || timestamp_nanos
    let request_hash = Sha256::digest(request_bytes);
    let mut message = request_hash.to_vec();
    message.extend_from_slice(&auth_metadata.timestamp_nanos.to_le_bytes());

    // Verify the ECDAA signature.
    if !v0::verify(
        &nac_group_public_key.to_vec(),
        &None, // no basename
        &None, // credential not needed for verification (embedded in signature)
        &auth_metadata.signature,
        &message.to_vec(),
    ) {
        return Err(Status::new(
            Code::Unauthenticated,
            "invalid service auth signature",
        ));
    }

    let caller_group = ServiceGroup::from_i32(auth_metadata.caller_group)
        .unwrap_or(ServiceGroup::ServiceUnknown);

    if caller_group == ServiceGroup::ServiceUnknown {
        return Err(Status::new(
            Code::Unauthenticated,
            "unknown service group",
        ));
    }

    trace!("service auth verified for {:?}", caller_group);
    Ok(caller_group)
}

/// Extract the binary request bytes from a tonic request for verification.
/// This must be called before the request body is consumed.
pub fn extract_request_bytes<T: Message>(request: &Request<T>) -> Vec<u8> {
    let mut buf = vec![];
    request.get_ref().encode(&mut buf).unwrap_or_default();
    buf
}

/// Check if the caller's service group is authorized for the requested operation.
pub fn authorize_caller(
    caller: ServiceGroup,
    allowed_groups: &[ServiceGroup],
) -> Result<(), Status> {
    if allowed_groups.contains(&caller) {
        Ok(())
    } else {
        Err(Status::new(
            Code::PermissionDenied,
            format!(
                "service group {:?} not authorized for this operation",
                caller
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        // Generate issuer keypair for NAC.
        let mut issuer_sk = vec![];
        let mut group_pk = vec![];
        assert!(v0::generate_issuer_keypair(&mut issuer_sk, &mut group_pk));

        // Generate service keypair.
        let nonce = b"test-service-nonce".to_vec();
        let mut sk = vec![];
        let mut pk = vec![];
        assert!(v0::generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        // Issue credential.
        let mut credential = vec![];
        let mut credential_sig = vec![];
        assert!(v0::issue_credential(
            &pk,
            &issuer_sk,
            &nonce,
            &mut credential,
            &mut credential_sig,
        ));

        let identity = ServiceIdentity {
            group: ServiceGroup::ServiceValidate,
            secret_key: sk,
            public_key: pk,
            credential,
            credential_signature: credential_sig,
            group_public_key: group_pk.clone(),
        };

        // Sign a request.
        let request_bytes = b"test request payload";
        let auth = identity.sign_request(request_bytes).unwrap();

        assert_eq!(auth.caller_group, ServiceGroup::ServiceValidate as i32);
        assert!(auth.signature.len() > 0);
        assert!(auth.timestamp_nanos > 0);

        // Verify the signature.
        let request_hash = Sha256::digest(request_bytes);
        let mut message = request_hash.to_vec();
        message.extend_from_slice(&auth.timestamp_nanos.to_le_bytes());

        assert!(v0::verify(
            &group_pk,
            &None,
            &None,
            &auth.signature,
            &message.to_vec(),
        ));
    }

    #[test]
    fn test_verify_rejects_wrong_key() {
        // Generate two separate issuer keypairs.
        let mut issuer_sk = vec![];
        let mut group_pk = vec![];
        assert!(v0::generate_issuer_keypair(&mut issuer_sk, &mut group_pk));

        let mut wrong_issuer_sk = vec![];
        let mut wrong_group_pk = vec![];
        assert!(v0::generate_issuer_keypair(
            &mut wrong_issuer_sk,
            &mut wrong_group_pk
        ));

        // Generate service keypair and credential under first issuer.
        let nonce = b"test-nonce".to_vec();
        let mut sk = vec![];
        let mut pk = vec![];
        assert!(v0::generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        let mut credential = vec![];
        let mut credential_sig = vec![];
        assert!(v0::issue_credential(
            &pk,
            &issuer_sk,
            &nonce,
            &mut credential,
            &mut credential_sig,
        ));

        let identity = ServiceIdentity {
            group: ServiceGroup::ServiceMint,
            secret_key: sk,
            public_key: pk,
            credential,
            credential_signature: credential_sig,
            group_public_key: group_pk,
        };

        let request_bytes = b"test request";
        let auth = identity.sign_request(request_bytes).unwrap();

        // Verify with wrong group public key should fail.
        let request_hash = Sha256::digest(request_bytes);
        let mut message = request_hash.to_vec();
        message.extend_from_slice(&auth.timestamp_nanos.to_le_bytes());

        assert!(!v0::verify(
            &wrong_group_pk,
            &None,
            &None,
            &auth.signature,
            &message.to_vec(),
        ));
    }

    #[test]
    fn test_verify_rejects_tampered_message() {
        let mut issuer_sk = vec![];
        let mut group_pk = vec![];
        assert!(v0::generate_issuer_keypair(&mut issuer_sk, &mut group_pk));

        let nonce = b"tamper-test".to_vec();
        let mut sk = vec![];
        let mut pk = vec![];
        assert!(v0::generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        let mut credential = vec![];
        let mut credential_sig = vec![];
        assert!(v0::issue_credential(
            &pk,
            &issuer_sk,
            &nonce,
            &mut credential,
            &mut credential_sig,
        ));

        let identity = ServiceIdentity {
            group: ServiceGroup::ServiceClerk,
            secret_key: sk,
            public_key: pk,
            credential,
            credential_signature: credential_sig,
            group_public_key: group_pk.clone(),
        };

        let request_bytes = b"original request";
        let auth = identity.sign_request(request_bytes).unwrap();

        // Verify with different request bytes should fail.
        let tampered_bytes = b"tampered request";
        let request_hash = Sha256::digest(tampered_bytes);
        let mut message = request_hash.to_vec();
        message.extend_from_slice(&auth.timestamp_nanos.to_le_bytes());

        assert!(!v0::verify(
            &group_pk,
            &None,
            &None,
            &auth.signature,
            &message.to_vec(),
        ));
    }

    #[test]
    fn test_authorize_caller() {
        assert!(authorize_caller(
            ServiceGroup::ServiceValidate,
            &[ServiceGroup::ServiceValidate, ServiceGroup::ServiceMint],
        )
        .is_ok());

        assert!(authorize_caller(
            ServiceGroup::ServiceClerk,
            &[ServiceGroup::ServiceValidate, ServiceGroup::ServiceMint],
        )
        .is_err());
    }
}
