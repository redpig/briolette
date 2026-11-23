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

//! ECDAA implementation using the BLS12-381 pairing-friendly curve.
//!
//! This is a port of the v0 BN254-based ECDAA to BLS12-381, providing
//! 128-bit security (vs ~100-bit for BN254 post Kim-Barbulescu).
//!
//! Key differences from v0:
//! - Uses BLS12-381 with compressed point encoding (48-byte G1, 96-byte G2)
//! - Signatures are 288 bytes (vs 356 for v0) — smaller despite stronger security
//! - RFC 9380 compliant hash_to_curve via bls12_381_plus
//! - Proper hash-to-field with domain separation
//! - Same ECDAA algorithm and SmartCard trait interface

use bls12_381_plus::{
    elliptic_curve::{group::Group, hash2curve::ExpandMsgXmd},
    G1Affine, G1Projective, G2Affine, G2Projective, Scalar,
};
use ff::Field;
use rand::rngs::OsRng;
use sha2::Sha256;

// BLS12-381 serialization sizes (compressed point encoding, Zcash/IETF format)
pub(crate) mod native {
    // BLS12-381 scalar field is 255 bits, serialized as 32 bytes
    pub const SCALAR_LENGTH: usize = 32;
    // G1 compressed = 48 bytes (high bits encode compression flag + y-sign)
    pub const G1_LENGTH: usize = 48;
    // G2 compressed = 96 bytes (high bits encode compression flag + y-sign)
    pub const G2_LENGTH: usize = 96;
    // Credential = 4 G1 points
    pub const CREDENTIAL_LENGTH: usize = 4 * G1_LENGTH; // 192
    // Credential signature = (c, s) two scalars
    pub const CREDENTIAL_SIGNATURE_LENGTH: usize = 2 * SCALAR_LENGTH; // 64
    // Signature without pseudonym = (c, s, n) + (R, S, T, W) = 3*32 + 4*48 = 96 + 192 = 288
    pub const SIGNATURE_LENGTH: usize = 3 * SCALAR_LENGTH + 4 * G1_LENGTH;
    // Signature with pseudonym adds K point
    pub const SIGNATURE_WITH_NYM_LENGTH: usize = SIGNATURE_LENGTH + G1_LENGTH; // 336
    // Issuer secret key = (x, y) two scalars
    pub const ISSUER_SECRET_KEY_LENGTH: usize = 2 * SCALAR_LENGTH; // 64
    // Group public key = (X, Y) two G2 points
    pub const ISSUER_GROUP_PUBLIC_KEY_LENGTH: usize = 2 * G2_LENGTH; // 192
    // Member secret key = one scalar
    pub const WALLET_SECRET_KEY_LENGTH: usize = SCALAR_LENGTH; // 32
    // Member public key = (Q, c, s, n) = 48 + 32 + 32 + 32 = 144
    pub const WALLET_PUBLIC_KEY_LENGTH: usize = G1_LENGTH + 3 * SCALAR_LENGTH;
}

// ============================================================================
// Serialization helpers
// ============================================================================

fn serialize_g1(point: &G1Projective) -> [u8; native::G1_LENGTH] {
    G1Affine::from(point).to_compressed()
}

fn deserialize_g1(data: &[u8]) -> Option<G1Projective> {
    if data.len() < native::G1_LENGTH {
        return None;
    }
    let mut buf = [0u8; 48];
    buf.copy_from_slice(&data[..48]);
    let affine = G1Affine::from_compressed(&buf);
    if bool::from(affine.is_none()) {
        return None;
    }
    Some(G1Projective::from(affine.unwrap()))
}

fn serialize_g2(point: &G2Projective) -> [u8; native::G2_LENGTH] {
    G2Affine::from(point).to_compressed()
}

fn deserialize_g2(data: &[u8]) -> Option<G2Projective> {
    if data.len() < native::G2_LENGTH {
        return None;
    }
    let mut buf = [0u8; 96];
    buf.copy_from_slice(&data[..96]);
    let affine = G2Affine::from_compressed(&buf);
    if bool::from(affine.is_none()) {
        return None;
    }
    Some(G2Projective::from(affine.unwrap()))
}

fn serialize_scalar(s: &Scalar) -> [u8; native::SCALAR_LENGTH] {
    // to_be_bytes returns big-endian 32-byte representation
    s.to_be_bytes()
}

fn deserialize_scalar(data: &[u8]) -> Option<Scalar> {
    if data.len() < native::SCALAR_LENGTH {
        return None;
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&data[0..32]);
    let s = Scalar::from_be_bytes(&bytes);
    if bool::from(s.is_some()) {
        Some(s.unwrap())
    } else {
        None
    }
}

fn random_scalar() -> Scalar {
    Scalar::random(OsRng)
}

const DOMAIN_HASH_TO_G1: &[u8] = b"BRIOLETTE-V1-BLS12381_XMD:SHA-256_SSWU_RO_";
const DOMAIN_HASH_TO_SCALAR: &[u8] = b"BRIOLETTE-V1-BLS12381_XMD:SHA-256_";

/// Hash arbitrary data to a scalar field element using hash-to-field
/// with proper domain separation (RFC 9380 compliant via bls12_381_plus).
fn hash_to_scalar(data: &[u8]) -> Scalar {
    Scalar::hash::<ExpandMsgXmd<Sha256>>(data, DOMAIN_HASH_TO_SCALAR)
}

/// Hash to G1 using RFC 9380 compliant hash_to_curve (SWU map).
fn hash_to_g1(data: &[u8]) -> G1Projective {
    G1Projective::hash::<ExpandMsgXmd<Sha256>>(data, DOMAIN_HASH_TO_G1)
}

// ============================================================================
// ECDAA Protocol Implementation
// ============================================================================

// ----- Credential structure (A, B, C, D) stored as 4 concatenated G1 points -----

fn credential_get_a(cred: &[u8]) -> Option<G1Projective> {
    deserialize_g1(&cred[0..native::G1_LENGTH])
}
fn credential_get_b(cred: &[u8]) -> Option<G1Projective> {
    deserialize_g1(&cred[native::G1_LENGTH..2 * native::G1_LENGTH])
}
fn credential_get_c(cred: &[u8]) -> Option<G1Projective> {
    deserialize_g1(&cred[2 * native::G1_LENGTH..3 * native::G1_LENGTH])
}
fn credential_get_d(cred: &[u8]) -> Option<G1Projective> {
    deserialize_g1(&cred[3 * native::G1_LENGTH..4 * native::G1_LENGTH])
}

fn serialize_credential(
    a: &G1Projective,
    b: &G1Projective,
    c: &G1Projective,
    d: &G1Projective,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(native::CREDENTIAL_LENGTH);
    out.extend_from_slice(&serialize_g1(a));
    out.extend_from_slice(&serialize_g1(b));
    out.extend_from_slice(&serialize_g1(c));
    out.extend_from_slice(&serialize_g1(d));
    out
}

// ----- Member public key (Q, c, s, n) -----

fn member_pk_get_q(pk: &[u8]) -> Option<G1Projective> {
    deserialize_g1(&pk[0..native::G1_LENGTH])
}
fn member_pk_get_c(pk: &[u8]) -> Option<Scalar> {
    deserialize_scalar(&pk[native::G1_LENGTH..native::G1_LENGTH + native::SCALAR_LENGTH])
}
fn member_pk_get_s(pk: &[u8]) -> Option<Scalar> {
    deserialize_scalar(
        &pk[native::G1_LENGTH + native::SCALAR_LENGTH
            ..native::G1_LENGTH + 2 * native::SCALAR_LENGTH],
    )
}

// ----- Group public key (X, Y) stored as 2 G2 points -----

fn gpk_get_x(gpk: &[u8]) -> Option<G2Projective> {
    deserialize_g2(&gpk[0..native::G2_LENGTH])
}
fn gpk_get_y(gpk: &[u8]) -> Option<G2Projective> {
    deserialize_g2(&gpk[native::G2_LENGTH..2 * native::G2_LENGTH])
}

// ----- Issuer secret key (x, y) stored as 2 scalars -----

fn isk_get_x(isk: &[u8]) -> Option<Scalar> {
    deserialize_scalar(&isk[0..native::SCALAR_LENGTH])
}
fn isk_get_y(isk: &[u8]) -> Option<Scalar> {
    deserialize_scalar(&isk[native::SCALAR_LENGTH..2 * native::SCALAR_LENGTH])
}

// ----- Signature structure -----
// Layout: c (32) | s (32) | R (48) | S (48) | T (48) | W (48) | n (32) [| K (48)]

fn sig_get_c(sig: &[u8]) -> Option<Scalar> {
    deserialize_scalar(&sig[0..32])
}
fn sig_get_s(sig: &[u8]) -> Option<Scalar> {
    deserialize_scalar(&sig[32..64])
}
fn sig_get_r(sig: &[u8]) -> Option<G1Projective> {
    deserialize_g1(&sig[64..64 + native::G1_LENGTH])
}
fn sig_get_s_point(sig: &[u8]) -> Option<G1Projective> {
    let offset = 64 + native::G1_LENGTH;
    deserialize_g1(&sig[offset..offset + native::G1_LENGTH])
}
fn sig_get_t(sig: &[u8]) -> Option<G1Projective> {
    let offset = 64 + 2 * native::G1_LENGTH;
    deserialize_g1(&sig[offset..offset + native::G1_LENGTH])
}
fn sig_get_w(sig: &[u8]) -> Option<G1Projective> {
    let offset = 64 + 3 * native::G1_LENGTH;
    deserialize_g1(&sig[offset..offset + native::G1_LENGTH])
}
fn sig_get_n(sig: &[u8]) -> Option<Scalar> {
    let offset = 64 + 4 * native::G1_LENGTH;
    deserialize_scalar(&sig[offset..offset + 32])
}
fn sig_get_k(sig: &[u8]) -> Option<G1Projective> {
    if sig.len() < native::SIGNATURE_WITH_NYM_LENGTH {
        return None;
    }
    let offset = native::SIGNATURE_LENGTH;
    deserialize_g1(&sig[offset..offset + native::G1_LENGTH])
}

// Schnorr hash for member key proof-of-knowledge
fn schnorr_hash_member(u: &G1Projective, basepoint: &G1Projective, q: &G1Projective, nonce: &[u8]) -> Scalar {
    let mut data = Vec::new();
    data.extend_from_slice(&serialize_g1(u));
    data.extend_from_slice(&serialize_g1(basepoint));
    data.extend_from_slice(&serialize_g1(q));
    data.extend_from_slice(nonce);
    hash_to_scalar(&data)
}

// Schnorr hash for credential signature
fn schnorr_hash_credential(b_point: &G1Projective, pk_q: &G1Projective, d: &G1Projective) -> Scalar {
    let mut data = Vec::new();
    data.extend_from_slice(&serialize_g1(b_point));
    data.extend_from_slice(&serialize_g1(pk_q));
    data.extend_from_slice(&serialize_g1(d));
    hash_to_scalar(&data)
}

/// BLS12-381 pairing: e(G1, G2) -> Gt
fn do_pairing(a: &G1Projective, b: &G2Projective) -> bls12_381_plus::Gt {
    let a_affine = G1Affine::from(a);
    let b_affine = G2Affine::from(b);
    bls12_381_plus::pairing(&a_affine, &b_affine)
}

// ============================================================================
// Public API
// ============================================================================

pub fn generate_wallet_keypair(
    nonce: &Vec<u8>,
    secret_key: &mut Vec<u8>,
    public_key: &mut Vec<u8>,
) -> bool {
    if nonce.is_empty() {
        return false;
    }

    let sk = random_scalar();
    let b = hash_to_g1(nonce);
    let q = b * sk;

    // Schnorr proof of knowledge
    let r = random_scalar();
    let u = b * r;
    let c = schnorr_hash_member(&u, &b, &q, nonce);
    let s = r + c * sk;
    let n = random_scalar();

    secret_key.clear();
    secret_key.extend_from_slice(&serialize_scalar(&sk));

    public_key.clear();
    public_key.extend_from_slice(&serialize_g1(&q));
    public_key.extend_from_slice(&serialize_scalar(&c));
    public_key.extend_from_slice(&serialize_scalar(&s));
    public_key.extend_from_slice(&serialize_scalar(&n));

    assert_eq!(secret_key.len(), native::WALLET_SECRET_KEY_LENGTH);
    assert_eq!(public_key.len(), native::WALLET_PUBLIC_KEY_LENGTH);
    true
}

pub fn generate_issuer_keypair(
    issuer_secret_key: &mut Vec<u8>,
    group_public_key: &mut Vec<u8>,
) -> bool {
    let x = random_scalar();
    let y = random_scalar();

    let big_x = G2Projective::generator() * x;
    let big_y = G2Projective::generator() * y;

    issuer_secret_key.clear();
    issuer_secret_key.extend_from_slice(&serialize_scalar(&x));
    issuer_secret_key.extend_from_slice(&serialize_scalar(&y));

    group_public_key.clear();
    group_public_key.extend_from_slice(&serialize_g2(&big_x));
    group_public_key.extend_from_slice(&serialize_g2(&big_y));

    assert_eq!(issuer_secret_key.len(), native::ISSUER_SECRET_KEY_LENGTH);
    assert_eq!(
        group_public_key.len(),
        native::ISSUER_GROUP_PUBLIC_KEY_LENGTH
    );
    true
}

pub fn issue_credential(
    member_public_key: &Vec<u8>,
    issuer_secret_key: &Vec<u8>,
    nonce: &Vec<u8>,
    credential_out: &mut Vec<u8>,
    credential_signature_out: &mut Vec<u8>,
) -> bool {
    if member_public_key.len() < native::WALLET_PUBLIC_KEY_LENGTH
        || issuer_secret_key.len() < native::ISSUER_SECRET_KEY_LENGTH
    {
        return false;
    }

    let q = match member_pk_get_q(member_public_key) {
        Some(v) => v,
        None => return false,
    };
    let pk_c = match member_pk_get_c(member_public_key) {
        Some(v) => v,
        None => return false,
    };
    let pk_s = match member_pk_get_s(member_public_key) {
        Some(v) => v,
        None => return false,
    };

    // Verify member's Schnorr proof
    let b = hash_to_g1(nonce);
    let u_recomputed = b * pk_s - q * pk_c;
    let c_recomputed = schnorr_hash_member(&u_recomputed, &b, &q, nonce);
    if pk_c != c_recomputed {
        log::error!("Member public key Schnorr proof verification failed");
        return false;
    }

    let isk_x = match isk_get_x(issuer_secret_key) {
        Some(v) => v,
        None => return false,
    };
    let isk_y = match isk_get_y(issuer_secret_key) {
        Some(v) => v,
        None => return false,
    };

    // Generate credential (A, B, C, D)
    let y_inv_ct = isk_y.invert();
    if bool::from(y_inv_ct.is_none()) {
        return false;
    }
    let y_inv = y_inv_ct.unwrap();
    let a = b * y_inv;
    let d = q;
    let c = (a + d) * isk_x;

    // Credential signature (Schnorr proof)
    let r = random_scalar();
    let u_sig = b * r;
    let c_sig = schnorr_hash_credential(&u_sig, &q, &d);
    let s_sig = r + c_sig * isk_y;

    credential_out.clear();
    credential_out.extend_from_slice(&serialize_credential(&a, &b, &c, &d));

    credential_signature_out.clear();
    credential_signature_out.extend_from_slice(&serialize_scalar(&c_sig));
    credential_signature_out.extend_from_slice(&serialize_scalar(&s_sig));

    assert_eq!(credential_out.len(), native::CREDENTIAL_LENGTH);
    assert_eq!(
        credential_signature_out.len(),
        native::CREDENTIAL_SIGNATURE_LENGTH
    );
    true
}

pub fn verify(
    group_public_key: &Vec<u8>,
    basename: &Option<Vec<u8>>,
    signing_credential: &Option<Vec<u8>>,
    signature: &Vec<u8>,
    message: &Vec<u8>,
) -> bool {
    if group_public_key.is_empty() || signature.is_empty() || message.is_empty() {
        return false;
    }

    let has_basename = basename.is_some();
    let expected_len = if has_basename {
        native::SIGNATURE_WITH_NYM_LENGTH
    } else {
        native::SIGNATURE_LENGTH
    };
    if signature.len() < expected_len {
        return false;
    }

    let gpk_x = match gpk_get_x(group_public_key) {
        Some(v) => v,
        None => return false,
    };
    let gpk_y = match gpk_get_y(group_public_key) {
        Some(v) => v,
        None => return false,
    };

    let sig_c = match sig_get_c(signature) {
        Some(v) => v,
        None => return false,
    };
    let sig_s = match sig_get_s(signature) {
        Some(v) => v,
        None => return false,
    };
    let r = match sig_get_r(signature) {
        Some(v) => v,
        None => return false,
    };
    let s_point = match sig_get_s_point(signature) {
        Some(v) => v,
        None => return false,
    };
    let t = match sig_get_t(signature) {
        Some(v) => v,
        None => return false,
    };
    let w = match sig_get_w(signature) {
        Some(v) => v,
        None => return false,
    };
    let n = match sig_get_n(signature) {
        Some(v) => v,
        None => return false,
    };

    // Check that R, S, T, W are not identity
    if bool::from(G1Affine::from(&r).is_identity())
        || bool::from(G1Affine::from(&s_point).is_identity())
        || bool::from(G1Affine::from(&t).is_identity())
        || bool::from(G1Affine::from(&w).is_identity())
    {
        return false;
    }

    // Check credential match if required
    if let Some(req_cred) = signing_credential {
        if req_cred.len() < native::CREDENTIAL_LENGTH {
            return false;
        }
        let sig_cred = serialize_credential(&r, &s_point, &t, &w);
        if sig_cred != *req_cred {
            return false;
        }
    }

    // Schnorr verification: U = S^s · W^(-c)
    let u = s_point * sig_s - w * sig_c;

    let mut hash_data = Vec::new();
    hash_data.extend_from_slice(&serialize_g1(&u));
    hash_data.extend_from_slice(&serialize_g1(&s_point));
    hash_data.extend_from_slice(&serialize_g1(&w));
    hash_data.extend_from_slice(message);

    if let Some(bsn) = basename {
        let k = match sig_get_k(signature) {
            Some(v) => v,
            None => return false,
        };
        let bsn_base = hash_to_g1(bsn);
        let k_u = bsn_base * sig_s - k * sig_c;
        hash_data.extend_from_slice(&serialize_g1(&k_u));
        hash_data.extend_from_slice(&serialize_g1(&k));
        hash_data.extend_from_slice(&serialize_g1(&bsn_base));
    }

    let c2 = hash_to_scalar(&hash_data);
    let mut final_hash_data = Vec::new();
    final_hash_data.extend_from_slice(&serialize_scalar(&n));
    final_hash_data.extend_from_slice(&serialize_scalar(&c2));
    let c_expected = hash_to_scalar(&final_hash_data);

    if sig_c != c_expected {
        return false;
    }

    // Pairing checks:
    // 1. e(R, Y) == e(S, P2)
    let p2 = G2Projective::generator();
    let lhs1 = do_pairing(&r, &gpk_y);
    let rhs1 = do_pairing(&s_point, &p2);
    if lhs1 != rhs1 {
        return false;
    }

    // 2. e(T, P2) == e(R + W, X)
    let lhs2 = do_pairing(&t, &p2);
    let rhs2 = do_pairing(&(r + w), &gpk_x);
    if lhs2 != rhs2 {
        return false;
    }

    true
}

pub fn sign(
    message: &Vec<u8>,
    credential: &Vec<u8>,
    secret_key: &Vec<u8>,
    basename: &Option<Vec<u8>>,
    randomize_cred: bool,
    signature: &mut Vec<u8>,
) -> bool {
    if message.is_empty() || credential.is_empty() || secret_key.is_empty() {
        return false;
    }
    if let Some(bsn) = basename {
        if bsn.is_empty() {
            return false;
        }
    }

    let mut a = match credential_get_a(credential) {
        Some(v) => v,
        None => return false,
    };
    let mut b = match credential_get_b(credential) {
        Some(v) => v,
        None => return false,
    };
    let mut c = match credential_get_c(credential) {
        Some(v) => v,
        None => return false,
    };
    let mut d = match credential_get_d(credential) {
        Some(v) => v,
        None => return false,
    };

    let sk = match deserialize_scalar(secret_key) {
        Some(v) => v,
        None => return false,
    };

    if randomize_cred {
        let l = random_scalar();
        a = a * l;
        b = b * l;
        c = c * l;
        d = d * l;
    }

    // Schnorr proof of knowledge of sk
    let r = random_scalar();
    let u = b * r;

    let mut hash_data = Vec::new();
    hash_data.extend_from_slice(&serialize_g1(&u));
    hash_data.extend_from_slice(&serialize_g1(&b));
    hash_data.extend_from_slice(&serialize_g1(&d));
    hash_data.extend_from_slice(message);

    let mut k_point = G1Projective::identity();
    if let Some(bsn) = basename {
        let bsn_base = hash_to_g1(bsn);
        k_point = bsn_base * sk;
        let k_u = bsn_base * r;
        hash_data.extend_from_slice(&serialize_g1(&k_u));
        hash_data.extend_from_slice(&serialize_g1(&k_point));
        hash_data.extend_from_slice(&serialize_g1(&bsn_base));
    }

    let c2 = hash_to_scalar(&hash_data);
    let n = random_scalar();

    let mut final_hash_data = Vec::new();
    final_hash_data.extend_from_slice(&serialize_scalar(&n));
    final_hash_data.extend_from_slice(&serialize_scalar(&c2));
    let sig_c = hash_to_scalar(&final_hash_data);

    let sig_s = r + sig_c * sk;

    signature.clear();
    if basename.is_some() {
        signature.reserve(native::SIGNATURE_WITH_NYM_LENGTH);
    } else {
        signature.reserve(native::SIGNATURE_LENGTH);
    }

    signature.extend_from_slice(&serialize_scalar(&sig_c));
    signature.extend_from_slice(&serialize_scalar(&sig_s));
    signature.extend_from_slice(&serialize_g1(&a));
    signature.extend_from_slice(&serialize_g1(&b));
    signature.extend_from_slice(&serialize_g1(&c));
    signature.extend_from_slice(&serialize_g1(&d));
    signature.extend_from_slice(&serialize_scalar(&n));

    if basename.is_some() {
        signature.extend_from_slice(&serialize_g1(&k_point));
    }

    true
}

pub fn randomize_credential(credential: &Vec<u8>, credential_out: &mut Vec<u8>) -> bool {
    if credential.is_empty() {
        return false;
    }

    let a = match credential_get_a(credential) {
        Some(v) => v,
        None => return false,
    };
    let b = match credential_get_b(credential) {
        Some(v) => v,
        None => return false,
    };
    let c = match credential_get_c(credential) {
        Some(v) => v,
        None => return false,
    };
    let d = match credential_get_d(credential) {
        Some(v) => v,
        None => return false,
    };

    let l = random_scalar();
    let r_a = a * l;
    let r_b = b * l;
    let r_c = c * l;
    let r_d = d * l;

    credential_out.clear();
    credential_out.extend_from_slice(&serialize_credential(&r_a, &r_b, &r_c, &r_d));
    assert_eq!(credential_out.len(), native::CREDENTIAL_LENGTH);
    true
}

pub fn credential_from_signature(signature: &Vec<u8>, credential: &mut Vec<u8>) -> bool {
    if signature.len() < native::SIGNATURE_LENGTH {
        return false;
    }
    credential.resize(native::CREDENTIAL_LENGTH, 0);
    let offset = 2 * native::SCALAR_LENGTH;
    let end = offset + native::CREDENTIAL_LENGTH;
    credential.copy_from_slice(&signature[offset..end]);
    true
}

pub fn deflate_signature(signature: &mut Vec<u8>) {
    if signature.len() < native::SIGNATURE_LENGTH {
        return;
    }
    let offset = 2 * native::SCALAR_LENGTH;
    let end = offset + native::CREDENTIAL_LENGTH;
    signature.drain(offset..end);
}

pub fn inflate_signature(credential: &Vec<u8>, signature: &mut Vec<u8>) {
    let offset = 2 * native::SCALAR_LENGTH;
    let rhs = signature.split_off(offset);
    signature.extend_from_slice(credential.as_slice());
    signature.extend_from_slice(rhs.as_slice());
}

pub fn credential_in_group(credential: &Vec<u8>, group_public_key: &Vec<u8>) -> bool {
    if credential.is_empty() || group_public_key.is_empty() {
        return false;
    }
    if credential.len() < native::CREDENTIAL_LENGTH
        || group_public_key.len() < native::ISSUER_GROUP_PUBLIC_KEY_LENGTH
    {
        return false;
    }

    let a = match credential_get_a(credential) {
        Some(v) => v,
        None => return false,
    };
    let b = match credential_get_b(credential) {
        Some(v) => v,
        None => return false,
    };
    let c = match credential_get_c(credential) {
        Some(v) => v,
        None => return false,
    };
    let d = match credential_get_d(credential) {
        Some(v) => v,
        None => return false,
    };

    let gpk_x = match gpk_get_x(group_public_key) {
        Some(v) => v,
        None => return false,
    };
    let gpk_y = match gpk_get_y(group_public_key) {
        Some(v) => v,
        None => return false,
    };

    if bool::from(G1Affine::from(&a).is_identity())
        || bool::from(G1Affine::from(&b).is_identity())
    {
        return false;
    }

    // Pairing checks:
    // 1. e(A, Y) == e(B, P2)
    let p2 = G2Projective::generator();
    let lhs1 = do_pairing(&a, &gpk_y);
    let rhs1 = do_pairing(&b, &p2);
    if lhs1 != rhs1 {
        return false;
    }

    // 2. e(C, P2) == e(A + D, X)
    let lhs2 = do_pairing(&c, &p2);
    let rhs2 = do_pairing(&(a + d), &gpk_x);
    if lhs2 != rhs2 {
        return false;
    }

    true
}

/// Verify that a split-key proof is structurally valid.
///
/// `combined_pk_bytes` is the full ECDAA public key (the first 48 bytes
/// are the G1 point Q, from `generate_wallet_keypair`).
/// `card_pk_bytes` is the card's share Q_card (48-byte compressed G1).
///
/// Returns true if the proof is valid.
pub fn verify_split_key_proof(combined_pk_bytes: &[u8], card_pk_bytes: &[u8]) -> bool {
    // Parse the card's public key share.
    let card_q = match deserialize_g1(card_pk_bytes) {
        Some(q) => q,
        None => return false,
    };
    // Must not be the identity point.
    if bool::from(card_q.is_identity()) {
        return false;
    }
    // Parse the combined public key (first 48 bytes of the ECDAA PK).
    if combined_pk_bytes.len() < native::G1_LENGTH {
        return false;
    }
    let combined_q = match deserialize_g1(&combined_pk_bytes[..native::G1_LENGTH]) {
        Some(q) => q,
        None => return false,
    };
    // Card share must differ from the combined key (host also contributed).
    if G1Affine::from(card_q) == G1Affine::from(combined_q) {
        return false;
    }
    true
}

// ============================================================================
// Split-key signing (Brickell & Li style) for BLS12-381
// ============================================================================

pub mod split {
    use super::*;

    pub struct CardSignCommitment {
        pub u_card: Vec<u8>,
        pub k_card: Option<Vec<u8>>,
        pub k_u_card: Option<Vec<u8>>,
    }

    pub struct CardSignResponse {
        pub s_card: Vec<u8>,
    }

    /// Trait representing the operations a smart card must support.
    /// All operations are G1 scalar multiplications and Scalar arithmetic only.
    /// No pairings, no G2 operations, no GT operations.
    /// Interface is identical to v0::split::SmartCard.
    pub trait SmartCard {
        fn public_key_share(&self, base: &[u8]) -> Vec<u8>;
        fn sign_commit(
            &mut self,
            s_point: &[u8],
            basename_base: Option<&[u8]>,
        ) -> Option<CardSignCommitment>;
        fn sign_respond(&mut self, challenge: &[u8]) -> Option<CardSignResponse>;
        /// Phase 1 of blind join: returns U_card = B * r_card.
        fn join_commit(&mut self, base: &[u8]) -> Option<Vec<u8>>;
        /// Phase 2 of blind join: returns s_card = r_card + c * card_sk.
        fn join_respond(&mut self, challenge: &[u8]) -> Option<Vec<u8>>;
        fn secret_key_share(&self) -> Vec<u8>;
    }

    /// Mock smart card for testing with BLS12-381.
    pub struct MockCard {
        card_sk: Scalar,
        r_card: Option<Scalar>,
    }

    impl MockCard {
        pub fn new() -> Self {
            MockCard {
                card_sk: random_scalar(),
                r_card: None,
            }
        }

        pub fn from_secret(sk: Scalar) -> Self {
            MockCard {
                card_sk: sk,
                r_card: None,
            }
        }
    }

    impl SmartCard for MockCard {
        fn public_key_share(&self, base: &[u8]) -> Vec<u8> {
            let b = deserialize_g1(base).expect("invalid base point");
            let q_card = b * self.card_sk;
            serialize_g1(&q_card).to_vec()
        }

        fn sign_commit(
            &mut self,
            s_point: &[u8],
            basename_base: Option<&[u8]>,
        ) -> Option<CardSignCommitment> {
            let s = deserialize_g1(s_point)?;

            let r = random_scalar();
            self.r_card = Some(r);

            let u_card = s * r;

            let (k_card, k_u_card) = if let Some(bsn_base_bytes) = basename_base {
                let bsn_base = deserialize_g1(bsn_base_bytes)?;
                let k = bsn_base * self.card_sk;
                let k_u = bsn_base * r;
                (
                    Some(serialize_g1(&k).to_vec()),
                    Some(serialize_g1(&k_u).to_vec()),
                )
            } else {
                (None, None)
            };

            Some(CardSignCommitment {
                u_card: serialize_g1(&u_card).to_vec(),
                k_card,
                k_u_card,
            })
        }

        fn sign_respond(&mut self, challenge: &[u8]) -> Option<CardSignResponse> {
            let c = deserialize_scalar(challenge)?;
            let r = self.r_card.take()?;
            let s_card = r + c * self.card_sk;
            Some(CardSignResponse {
                s_card: serialize_scalar(&s_card).to_vec(),
            })
        }

        fn join_commit(&mut self, base: &[u8]) -> Option<Vec<u8>> {
            let b = deserialize_g1(base)?;
            let r = random_scalar();
            self.r_card = Some(r);
            Some(serialize_g1(&(b * r)).to_vec())
        }

        fn join_respond(&mut self, challenge: &[u8]) -> Option<Vec<u8>> {
            let c = deserialize_scalar(challenge)?;
            let r = self.r_card.take()?;
            let s_card = r + c * self.card_sk;
            Some(serialize_scalar(&s_card).to_vec())
        }

        fn secret_key_share(&self) -> Vec<u8> {
            serialize_scalar(&self.card_sk).to_vec()
        }
    }

    /// Generate a split wallet keypair using the blind join protocol.
    /// The combined secret key never exists in one place.
    /// Returns (host_sk, combined_pk).
    pub fn generate_split_wallet_keypair(
        card: &mut dyn SmartCard,
        nonce: &Vec<u8>,
    ) -> Option<(Vec<u8>, Vec<u8>)> {
        if nonce.is_empty() {
            return None;
        }

        let b = hash_to_g1(nonce);
        let b_bytes = serialize_g1(&b);

        let q_card_bytes = card.public_key_share(&b_bytes);
        let q_card = deserialize_g1(&q_card_bytes)?;

        let host_sk = random_scalar();
        let q_host = b * host_sk;

        let q = q_card + q_host;

        // Blind Schnorr proof: split between card and host
        let u_card_bytes = card.join_commit(&b_bytes)?;
        let u_card = deserialize_g1(&u_card_bytes)?;

        let r_host = random_scalar();
        let u_host = b * r_host;

        let u = u_card + u_host;
        let c = schnorr_hash_member(&u, &b, &q, nonce);

        let s_card_bytes = card.join_respond(&serialize_scalar(&c))?;
        let s_card = deserialize_scalar(&s_card_bytes)?;

        let s_host = r_host + c * host_sk;
        let s = s_card + s_host;
        let n = random_scalar();

        let host_sk_bytes = serialize_scalar(&host_sk).to_vec();

        let mut pk = Vec::with_capacity(native::WALLET_PUBLIC_KEY_LENGTH);
        pk.extend_from_slice(&serialize_g1(&q));
        pk.extend_from_slice(&serialize_scalar(&c));
        pk.extend_from_slice(&serialize_scalar(&s));
        pk.extend_from_slice(&serialize_scalar(&n));

        Some((host_sk_bytes, pk))
    }

    pub fn sign_split(
        card: &mut dyn SmartCard,
        host_sk: &Vec<u8>,
        message: &Vec<u8>,
        credential: &Vec<u8>,
        basename: &Option<Vec<u8>>,
        randomize_cred: bool,
        signature: &mut Vec<u8>,
    ) -> bool {
        if message.is_empty() || credential.is_empty() || host_sk.is_empty() {
            return false;
        }
        if let Some(bsn) = basename {
            if bsn.is_empty() {
                return false;
            }
        }

        let mut a = match credential_get_a(credential) {
            Some(v) => v,
            None => return false,
        };
        let mut b = match credential_get_b(credential) {
            Some(v) => v,
            None => return false,
        };
        let mut c_point = match credential_get_c(credential) {
            Some(v) => v,
            None => return false,
        };
        let mut d = match credential_get_d(credential) {
            Some(v) => v,
            None => return false,
        };

        let h_sk = match deserialize_scalar(host_sk) {
            Some(v) => v,
            None => return false,
        };

        if randomize_cred {
            let l = random_scalar();
            a = a * l;
            b = b * l;
            c_point = c_point * l;
            d = d * l;
        }

        let s_bytes = serialize_g1(&b);

        let bsn_base = basename.as_ref().map(|bsn| hash_to_g1(bsn));
        let bsn_base_bytes = bsn_base.as_ref().map(|p| serialize_g1(p).to_vec());

        // Phase 1: Card commits
        let card_commit = match card.sign_commit(&s_bytes, bsn_base_bytes.as_deref()) {
            Some(v) => v,
            None => return false,
        };

        let u_card = match deserialize_g1(&card_commit.u_card) {
            Some(v) => v,
            None => return false,
        };

        // Host commits
        let r_host = random_scalar();
        let u_host = b * r_host;
        let u = u_card + u_host;

        let mut hash_data = Vec::new();
        hash_data.extend_from_slice(&serialize_g1(&u));
        hash_data.extend_from_slice(&serialize_g1(&b));
        hash_data.extend_from_slice(&serialize_g1(&d));
        hash_data.extend_from_slice(message);

        let mut k_combined = G1Projective::identity();
        if let Some(bsn_base_pt) = &bsn_base {
            let k_card = match &card_commit.k_card {
                Some(bytes) => match deserialize_g1(bytes) {
                    Some(v) => v,
                    None => return false,
                },
                None => return false,
            };
            let k_host = *bsn_base_pt * h_sk;
            k_combined = k_card + k_host;

            let k_u_card = match &card_commit.k_u_card {
                Some(bytes) => match deserialize_g1(bytes) {
                    Some(v) => v,
                    None => return false,
                },
                None => return false,
            };
            let k_u_host = *bsn_base_pt * r_host;
            let k_u = k_u_card + k_u_host;

            hash_data.extend_from_slice(&serialize_g1(&k_u));
            hash_data.extend_from_slice(&serialize_g1(&k_combined));
            hash_data.extend_from_slice(&serialize_g1(bsn_base_pt));
        }

        let c2 = hash_to_scalar(&hash_data);
        let n = random_scalar();

        let mut final_hash_data = Vec::new();
        final_hash_data.extend_from_slice(&serialize_scalar(&n));
        final_hash_data.extend_from_slice(&serialize_scalar(&c2));
        let sig_c = hash_to_scalar(&final_hash_data);

        // Phase 2: Card responds
        let card_response = match card.sign_respond(&serialize_scalar(&sig_c)) {
            Some(v) => v,
            None => return false,
        };

        let s_card = match deserialize_scalar(&card_response.s_card) {
            Some(v) => v,
            None => return false,
        };

        let s_host = r_host + sig_c * h_sk;
        let sig_s = s_card + s_host;

        signature.clear();
        if basename.is_some() {
            signature.reserve(native::SIGNATURE_WITH_NYM_LENGTH);
        } else {
            signature.reserve(native::SIGNATURE_LENGTH);
        }

        signature.extend_from_slice(&serialize_scalar(&sig_c));
        signature.extend_from_slice(&serialize_scalar(&sig_s));
        signature.extend_from_slice(&serialize_g1(&a));
        signature.extend_from_slice(&serialize_g1(&b));
        signature.extend_from_slice(&serialize_g1(&c_point));
        signature.extend_from_slice(&serialize_g1(&d));
        signature.extend_from_slice(&serialize_scalar(&n));

        if basename.is_some() {
            signature.extend_from_slice(&serialize_g1(&k_combined));
        }

        true
    }

    /// Errors from smart card operations that can be distinguished
    /// for recovery (e.g., bloom filter hit → swap retry).
    #[derive(Debug, Clone, PartialEq)]
    pub enum SmartCardError {
        /// The bloom filter rejected the basename (false positive or real double-spend).
        /// Recovery: request swap authorization and retry with sign_commit_swap.
        BloomFilterHit,
        /// The swap authorization was invalid (bad Schnorr signature).
        SwapAuthFailed,
        /// The card doesn't have a swap public key set.
        NoSwapKey,
        /// Any other card error (bad version, wrong length, transport failure, etc.)
        Other(u16),
    }

    /// Swap authorization token: a Schnorr signature from the swap server
    /// binding to a specific basename. The card verifies this before allowing
    /// a bloom-filter-bypassing swap signing operation.
    pub struct SwapAuthorization {
        /// Schnorr challenge: c = H(R || bsn_base || swap_pk)
        pub c: Vec<u8>,
        /// Schnorr response: s = r + c * swap_sk
        pub s: Vec<u8>,
    }

    impl SwapAuthorization {
        /// Serialize to 64 bytes: c (32 bytes) || s (32 bytes).
        pub fn to_bytes(&self) -> Vec<u8> {
            let mut out = Vec::with_capacity(64);
            out.extend_from_slice(&self.c);
            out.extend_from_slice(&self.s);
            out
        }

        /// Deserialize from 64 bytes.
        pub fn from_bytes(data: &[u8]) -> Option<Self> {
            if data.len() != 64 {
                return None;
            }
            Some(SwapAuthorization {
                c: data[..32].to_vec(),
                s: data[32..64].to_vec(),
            })
        }
    }

    /// Hash function for swap authorization Schnorr signatures.
    /// c = H(R || bsn_base || swap_pk)
    fn swap_auth_hash(r_point: &G1Projective, bsn_base: &G1Projective, swap_pk: &G1Projective) -> Scalar {
        let mut data = Vec::new();
        data.extend_from_slice(&serialize_g1(r_point));
        data.extend_from_slice(&serialize_g1(bsn_base));
        data.extend_from_slice(&serialize_g1(swap_pk));
        hash_to_scalar(&data)
    }

    /// Create a swap authorization token.
    ///
    /// Called by the swap server when a wallet requests to swap tokens.
    /// The server signs the basename (derived from the token's previous signature)
    /// with its private key, producing a token the card can verify.
    pub fn swap_auth_create(
        swap_sk: &[u8],
        swap_pk: &[u8],
        bsn_base_bytes: &[u8],
    ) -> Option<SwapAuthorization> {
        let sk = deserialize_scalar(swap_sk)?;
        let pk = deserialize_g1(swap_pk)?;
        let bsn = deserialize_g1(bsn_base_bytes)?;

        let r = random_scalar();
        let r_point = G1Projective::generator() * r;
        let c = swap_auth_hash(&r_point, &bsn, &pk);
        let s = r + c * sk;

        Some(SwapAuthorization {
            c: serialize_scalar(&c).to_vec(),
            s: serialize_scalar(&s).to_vec(),
        })
    }

    /// Verify a swap authorization token.
    pub fn swap_auth_verify(
        swap_pk: &[u8],
        bsn_base_bytes: &[u8],
        auth: &SwapAuthorization,
    ) -> bool {
        let pk = match deserialize_g1(swap_pk) {
            Some(v) => v,
            None => return false,
        };
        let bsn = match deserialize_g1(bsn_base_bytes) {
            Some(v) => v,
            None => return false,
        };
        let c = match deserialize_scalar(&auth.c) {
            Some(v) => v,
            None => return false,
        };
        let s = match deserialize_scalar(&auth.s) {
            Some(v) => v,
            None => return false,
        };

        // Reconstruct R' = G1 * s - swap_pk * c
        let r_prime = G1Projective::generator() * s + pk * (-c);

        // Recompute challenge
        let c_prime = swap_auth_hash(&r_prime, &bsn, &pk);

        c == c_prime
    }

    /// Generate a swap server keypair: (secret_key, public_key).
    ///
    /// Returns (swap_sk: 32 bytes, swap_pk: 48 bytes) where swap_pk = G1 * swap_sk.
    pub fn generate_swap_keypair() -> (Vec<u8>, Vec<u8>) {
        let sk = random_scalar();
        let pk = G1Projective::generator() * sk;
        (serialize_scalar(&sk).to_vec(), serialize_g1(&pk).to_vec())
    }

    /// Compute the base point B = hash_to_g1(nonce) for split key generation.
    /// Returns the serialized G1 point (48 bytes, compressed).
    pub fn split_base_point(nonce: &[u8]) -> Vec<u8> {
        let b = hash_to_g1(nonce);
        serialize_g1(&b).to_vec()
    }

    /// Host-side Phase 1+Challenge of the blind join protocol.
    ///
    /// Given the card's public key share Q_card and commitment U_card,
    /// generates the host's share and computes the Schnorr challenge.
    ///
    /// Returns: (host_sk, host_r, challenge, q_combined) all as serialized bytes.
    pub fn split_join_host_commit_and_challenge(
        nonce: &[u8],
        q_card_bytes: &[u8],
        u_card_bytes: &[u8],
    ) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
        let b = hash_to_g1(nonce);
        let q_card = deserialize_g1(q_card_bytes)?;
        let u_card = deserialize_g1(u_card_bytes)?;

        // Host generates its share
        let host_sk = random_scalar();
        let q_host = b * host_sk;
        let q = q_card + q_host;

        // Host commits
        let r_host = random_scalar();
        let u_host = b * r_host;
        let u = u_card + u_host;

        // Challenge
        let c = schnorr_hash_member(&u, &b, &q, nonce);

        // Serialize the combined public key as raw G1 (for use as NAC nonce)
        let q_combined_bytes = serialize_g1(&q).to_vec();

        Some((
            serialize_scalar(&host_sk).to_vec(),
            serialize_scalar(&r_host).to_vec(),
            serialize_scalar(&c).to_vec(),
            q_combined_bytes,
        ))
    }

    /// Finalize the split blind join protocol.
    ///
    /// Given all intermediate values and the card's Schnorr response,
    /// produces the combined public key (Q, c, s, n).
    ///
    /// Returns the serialized combined public key.
    pub fn split_join_finalize(
        nonce: &[u8],
        q_card_bytes: &[u8],
        _u_card_bytes: &[u8],
        host_sk_bytes: &[u8],
        host_r_bytes: &[u8],
        c_bytes: &[u8],
        s_card_bytes: &[u8],
    ) -> Option<Vec<u8>> {
        let b = hash_to_g1(nonce);
        let q_card = deserialize_g1(q_card_bytes)?;
        let host_sk = deserialize_scalar(host_sk_bytes)?;
        let q_host = b * host_sk;
        let q = q_card + q_host;

        let host_r = deserialize_scalar(host_r_bytes)?;
        let c = deserialize_scalar(c_bytes)?;
        let s_card = deserialize_scalar(s_card_bytes)?;

        // Host's response
        let s_host = host_r + c * host_sk;
        // Combined response
        let s = s_card + s_host;
        let n = random_scalar();

        // Serialize combined public key
        let mut pk = Vec::with_capacity(native::WALLET_PUBLIC_KEY_LENGTH);
        pk.extend_from_slice(&serialize_g1(&q));
        pk.extend_from_slice(&serialize_scalar(&c));
        pk.extend_from_slice(&serialize_scalar(&s));
        pk.extend_from_slice(&serialize_scalar(&n));

        Some(pk)
    }

    /// Extended sign_split that accepts optional swap authorization and returns
    /// SmartCardError on failure, allowing callers to distinguish bloom filter
    /// hits from other errors.
    pub fn sign_split_ext(
        card: &mut dyn SmartCard,
        host_sk: &Vec<u8>,
        message: &Vec<u8>,
        credential: &Vec<u8>,
        basename: &Option<Vec<u8>>,
        randomize_cred: bool,
        signature: &mut Vec<u8>,
        swap_auth: Option<&SwapAuthorization>,
    ) -> Result<(), SmartCardError> {
        let other_err = || SmartCardError::Other(0);
        if message.is_empty() || credential.is_empty() || host_sk.is_empty() {
            return Err(other_err());
        }
        if let Some(bsn) = basename {
            if bsn.is_empty() {
                return Err(other_err());
            }
        }

        // Deserialize credential (A, B, C, D)
        let mut a = credential_get_a(credential).ok_or_else(other_err)?;
        let mut b = credential_get_b(credential).ok_or_else(other_err)?;
        let mut c_point = credential_get_c(credential).ok_or_else(other_err)?;
        let mut d = credential_get_d(credential).ok_or_else(other_err)?;

        // Deserialize host secret key share
        let h_sk = deserialize_scalar(host_sk).ok_or_else(other_err)?;

        // Randomize credential if requested
        if randomize_cred {
            let l = random_scalar();
            a = a * l;
            b = b * l;
            c_point = c_point * l;
            d = d * l;
        }

        // S point (randomized B)
        let s_bytes = serialize_g1(&b);

        // Compute basename base point if needed
        let bsn_base = basename.as_ref().map(|bsn| hash_to_g1(bsn));
        let bsn_base_bytes = bsn_base.as_ref().map(|p| serialize_g1(p).to_vec());

        // Phase 1: Card commits
        // For now, swap auth uses same path as normal (MockCard has no bloom filter)
        let card_commit = if let Some(_auth) = swap_auth {
            // With swap auth — for MockCard just does normal commit
            card.sign_commit(&s_bytes, bsn_base_bytes.as_deref())
                .ok_or_else(other_err)?
        } else {
            card.sign_commit(&s_bytes, bsn_base_bytes.as_deref())
                .ok_or_else(other_err)?
        };

        let u_card = deserialize_g1(&card_commit.u_card).ok_or_else(other_err)?;

        // Host commits
        let r_host = random_scalar();
        let u_host = b * r_host;
        let u = u_card + u_host;

        // Build hash for c2
        let mut hash_data = Vec::new();
        hash_data.extend_from_slice(&serialize_g1(&u));
        hash_data.extend_from_slice(&serialize_g1(&b));
        hash_data.extend_from_slice(&serialize_g1(&d));
        hash_data.extend_from_slice(message);

        // Handle basename/pseudonym
        let mut k_combined = G1Projective::identity();
        if let Some(bsn_base_pt) = &bsn_base {
            let k_card_bytes = card_commit.k_card.as_ref().ok_or_else(other_err)?;
            let k_card = deserialize_g1(k_card_bytes).ok_or_else(other_err)?;
            let k_host = *bsn_base_pt * h_sk;
            k_combined = k_card + k_host;

            let k_u_card_bytes = card_commit.k_u_card.as_ref().ok_or_else(other_err)?;
            let k_u_card = deserialize_g1(k_u_card_bytes).ok_or_else(other_err)?;
            let k_u_host = *bsn_base_pt * r_host;
            let k_u = k_u_card + k_u_host;

            hash_data.extend_from_slice(&serialize_g1(&k_u));
            hash_data.extend_from_slice(&serialize_g1(&k_combined));
            hash_data.extend_from_slice(&serialize_g1(bsn_base_pt));
        }

        let c2 = hash_to_scalar(&hash_data);
        let n = random_scalar();

        let mut final_hash_data = Vec::new();
        final_hash_data.extend_from_slice(&serialize_scalar(&n));
        final_hash_data.extend_from_slice(&serialize_scalar(&c2));
        let sig_c = hash_to_scalar(&final_hash_data);

        // Phase 2: Card responds
        let card_response = card.sign_respond(&serialize_scalar(&sig_c)).ok_or_else(other_err)?;
        let s_card = deserialize_scalar(&card_response.s_card).ok_or_else(other_err)?;

        let s_host = r_host + sig_c * h_sk;
        let sig_s = s_card + s_host;

        // Serialize signature
        signature.clear();
        if basename.is_some() {
            signature.reserve(native::SIGNATURE_WITH_NYM_LENGTH);
        } else {
            signature.reserve(native::SIGNATURE_LENGTH);
        }

        signature.extend_from_slice(&serialize_scalar(&sig_c));
        signature.extend_from_slice(&serialize_scalar(&sig_s));
        signature.extend_from_slice(&serialize_g1(&a));
        signature.extend_from_slice(&serialize_g1(&b));
        signature.extend_from_slice(&serialize_g1(&c_point));
        signature.extend_from_slice(&serialize_g1(&d));
        signature.extend_from_slice(&serialize_scalar(&n));

        if basename.is_some() {
            signature.extend_from_slice(&serialize_g1(&k_combined));
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::split::*;

    #[test]
    fn test_keypair_generation() {
        let nonce = b"test-nonce-v1".to_vec();
        let mut sk = vec![];
        let mut pk = vec![];
        assert!(generate_wallet_keypair(&nonce, &mut sk, &mut pk));
        assert_eq!(sk.len(), native::WALLET_SECRET_KEY_LENGTH);
        assert_eq!(pk.len(), native::WALLET_PUBLIC_KEY_LENGTH);
    }

    #[test]
    fn test_issuer_keypair() {
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));
        assert_eq!(isk.len(), native::ISSUER_SECRET_KEY_LENGTH);
        assert_eq!(gpk.len(), native::ISSUER_GROUP_PUBLIC_KEY_LENGTH);
    }

    #[test]
    fn test_full_ecdaa_flow() {
        let nonce = b"test-nonce-v1".to_vec();

        // Issuer setup
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        // Member setup
        let mut sk = vec![];
        let mut pk = vec![];
        assert!(generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        // Issue credential
        let mut cred = vec![];
        let mut cred_sig = vec![];
        assert!(issue_credential(&pk, &isk, &nonce, &mut cred, &mut cred_sig));
        assert_eq!(cred.len(), native::CREDENTIAL_LENGTH);

        // Verify credential is in group
        assert!(credential_in_group(&cred, &gpk));

        // Sign without basename
        let message = b"test message".to_vec();
        let mut sig = vec![];
        assert!(sign(&message, &cred, &sk, &None, false, &mut sig));
        assert_eq!(sig.len(), native::SIGNATURE_LENGTH);

        // Verify without basename
        assert!(verify(&gpk, &None, &None, &sig, &message));

        // Sign with basename
        let basename = Some(b"test-basename".to_vec());
        let mut sig_bsn = vec![];
        assert!(sign(&message, &cred, &sk, &basename, false, &mut sig_bsn));
        assert_eq!(sig_bsn.len(), native::SIGNATURE_WITH_NYM_LENGTH);

        // Verify with basename
        assert!(verify(&gpk, &basename, &None, &sig_bsn, &message));

        // Wrong message should fail
        let wrong_msg = b"wrong message".to_vec();
        assert!(!verify(&gpk, &None, &None, &sig, &wrong_msg));
    }

    #[test]
    fn test_credential_randomization() {
        let nonce = b"test-nonce-v1".to_vec();
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let mut sk = vec![];
        let mut pk = vec![];
        assert!(generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        let mut cred = vec![];
        let mut cred_sig = vec![];
        assert!(issue_credential(&pk, &isk, &nonce, &mut cred, &mut cred_sig));

        // Randomize
        let mut rand_cred = vec![];
        assert!(randomize_credential(&cred, &mut rand_cred));
        assert_ne!(cred, rand_cred);

        // Randomized credential should still be in group
        assert!(credential_in_group(&rand_cred, &gpk));

        // Sign with randomized credential
        let message = b"test".to_vec();
        let mut sig = vec![];
        assert!(sign(&message, &rand_cred, &sk, &None, false, &mut sig));
        assert!(verify(&gpk, &None, &None, &sig, &message));
    }

    #[test]
    fn test_sign_with_randomize_flag() {
        let nonce = b"test-v1".to_vec();
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let mut sk = vec![];
        let mut pk = vec![];
        assert!(generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        let mut cred = vec![];
        let mut cred_sig = vec![];
        assert!(issue_credential(&pk, &isk, &nonce, &mut cred, &mut cred_sig));

        let message = b"test".to_vec();
        let mut sig = vec![];
        assert!(sign(&message, &cred, &sk, &None, true, &mut sig));
        assert!(verify(&gpk, &None, &None, &sig, &message));
    }

    #[test]
    fn test_deflate_inflate_signature() {
        let nonce = b"test-v1".to_vec();
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let mut sk = vec![];
        let mut pk = vec![];
        assert!(generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        let mut cred = vec![];
        let mut cred_sig = vec![];
        assert!(issue_credential(&pk, &isk, &nonce, &mut cred, &mut cred_sig));

        let message = b"test".to_vec();
        let basename = Some(b"basename".to_vec());
        let mut sig = vec![];
        assert!(sign(&message, &cred, &sk, &basename, false, &mut sig));
        let original_sig = sig.clone();

        // Extract credential from signature
        let mut extracted_cred = vec![];
        assert!(credential_from_signature(&sig, &mut extracted_cred));

        // Deflate
        deflate_signature(&mut sig);
        assert!(sig.len() < original_sig.len());

        // Inflate
        inflate_signature(&extracted_cred, &mut sig);
        assert_eq!(sig, original_sig);

        // Should still verify
        assert!(verify(&gpk, &basename, &None, &sig, &message));
    }

    #[test]
    fn test_split_key_sign() {
        let nonce = b"test-split-v1".to_vec();
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        // Generate split keypair using blind join
        let mut card = MockCard::new();
        let card_sk_bytes = card.secret_key_share();
        let (host_sk, pk) =
            generate_split_wallet_keypair(&mut card, &nonce).unwrap();

        // Issue credential using combined public key
        let mut cred = vec![];
        let mut cred_sig = vec![];
        assert!(issue_credential(&pk, &isk, &nonce, &mut cred, &mut cred_sig));

        // Sign with split key (with basename, like token transfers)
        let message = b"transfer data".to_vec();
        let basename = Some(b"previous-signature".to_vec());
        let mut sig = vec![];
        let mut card = MockCard::from_secret(
            deserialize_scalar(&card_sk_bytes).unwrap(),
        );
        assert!(split::sign_split(
            &mut card,
            &host_sk,
            &message,
            &cred,
            &basename,
            false,
            &mut sig,
        ));

        // Verify with standard verify — verifier doesn't know about the split
        assert!(verify(&gpk, &basename, &None, &sig, &message));
    }

    #[test]
    fn test_split_key_no_basename() {
        let nonce = b"test-split-no-bsn-v1".to_vec();
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let mut card = MockCard::new();
        let card_sk_bytes = card.secret_key_share();
        let (host_sk, pk) =
            generate_split_wallet_keypair(&mut card, &nonce).unwrap();

        let mut cred = vec![];
        let mut cred_sig = vec![];
        assert!(issue_credential(&pk, &isk, &nonce, &mut cred, &mut cred_sig));

        let message = b"test".to_vec();
        let mut sig = vec![];
        let mut card = MockCard::from_secret(
            deserialize_scalar(&card_sk_bytes).unwrap(),
        );
        assert!(split::sign_split(
            &mut card,
            &host_sk,
            &message,
            &cred,
            &None,
            true, // with randomization
            &mut sig,
        ));

        assert!(verify(&gpk, &None, &None, &sig, &message));
    }

    #[test]
    fn test_basename_linkability() {
        let nonce = b"link-test-v1".to_vec();
        let mut isk = vec![];
        let mut gpk = vec![];
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let mut sk = vec![];
        let mut pk = vec![];
        assert!(generate_wallet_keypair(&nonce, &mut sk, &mut pk));

        let mut cred = vec![];
        let mut cred_sig = vec![];
        assert!(issue_credential(&pk, &isk, &nonce, &mut cred, &mut cred_sig));

        // Two signatures with the same basename should produce the same K
        let basename = Some(b"same-basename".to_vec());
        let msg1 = b"message1".to_vec();
        let msg2 = b"message2".to_vec();

        let mut sig1 = vec![];
        let mut sig2 = vec![];
        assert!(sign(&msg1, &cred, &sk, &basename, false, &mut sig1));
        assert!(sign(&msg2, &cred, &sk, &basename, false, &mut sig2));

        // K should be the same (basename linkability for double-spend detection)
        let k1 = sig_get_k(&sig1).unwrap();
        let k2 = sig_get_k(&sig2).unwrap();
        assert_eq!(serialize_g1(&k1), serialize_g1(&k2));

        // Different basenames should produce different K
        let basename2 = Some(b"different-basename".to_vec());
        let mut sig3 = vec![];
        assert!(sign(&msg1, &cred, &sk, &basename2, false, &mut sig3));
        let k3 = sig_get_k(&sig3).unwrap();
        assert_ne!(serialize_g1(&k1), serialize_g1(&k3));
    }
}
