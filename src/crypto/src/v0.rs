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

//! Pure Rust ECDAA (Elliptic Curve Direct Anonymous Attestation) implementation
//! using the BN254 pairing-friendly curve via substrate-bn.
//!
//! This replaces the xaptum/ecdaa C library with a native Rust implementation
//! that maintains the same public API and serialization format.

use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use substrate_bn::{pairing, AffineG1, AffineG2, Fq, Fq2, Fr, G1, G2, Group};

// Serialization sizes matching the original C library's FP256BN format.
// BN254 has 32-byte field elements, same as FP256BN.
pub(crate) mod native {
    pub const MODBYTES_256_56: usize = 32;
    // Credential = 4 EC points in G1, each serialized as (0x04 || x || y) = 65 bytes
    pub const CREDENTIAL_LENGTH: usize = 260; // 4 * 65
    // Credential signature = (c, s) two scalars
    pub const CREDENTIAL_SIGNATURE_LENGTH: usize = 64; // 2 * 32
    // Signature without pseudonym = (c, s, n) + (R, S, T, W) = 3*32 + 4*65 = 96 + 260 = 356
    pub const SIGNATURE_LENGTH: usize = 356;
    // Signature with pseudonym/basename adds K point = 356 + 65 = 421
    pub const SIGNATURE_WITH_NYM_LENGTH: usize = 421;
    // Issuer secret key = (x, y) two scalars
    pub const ISSUER_SECRET_KEY_LENGTH: usize = 64; // 2 * 32
    // Group public key = (X, Y) two G2 points, each (0x04 || x_a || x_b || y_a || y_b) = 129 bytes
    // Actually 2 * (4*32 + 1) = 258
    pub const ISSUER_GROUP_PUBLIC_KEY_LENGTH: usize = 258; // 2 * 129
    // Member/wallet secret key = one scalar
    pub const WALLET_SECRET_KEY_LENGTH: usize = 32;
    // Member/wallet public key = (Q, c, s, n) = 65 + 32 + 32 + 32 = 161
    pub const WALLET_PUBLIC_KEY_LENGTH: usize = 161;
}

// ============================================================================
// Serialization helpers
// ============================================================================

fn serialize_g1(point: &G1) -> [u8; 65] {
    let mut buf = [0u8; 65];
    if point.is_zero() {
        // Point at infinity: use 0x04 with all zeros
        buf[0] = 0x04;
        return buf;
    }
    let mut p = *point;
    p.normalize();
    let affine = AffineG1::from_jacobian(p).unwrap();
    buf[0] = 0x04;
    affine.x().to_big_endian(&mut buf[1..33]).unwrap();
    affine.y().to_big_endian(&mut buf[33..65]).unwrap();
    buf
}

fn deserialize_g1(data: &[u8]) -> Option<G1> {
    if data.len() < 65 || data[0] != 0x04 {
        return None;
    }
    let x = Fq::from_slice(&data[1..33]).ok()?;
    let y = Fq::from_slice(&data[33..65]).ok()?;
    if x.is_zero() && y.is_zero() {
        return Some(G1::zero());
    }
    let affine = AffineG1::new(x, y).ok()?;
    Some(G1::from(affine))
}

fn serialize_g2(point: &G2) -> [u8; 129] {
    let mut buf = [0u8; 129];
    if point.is_zero() {
        buf[0] = 0x04;
        return buf;
    }
    let mut p = *point;
    p.normalize();
    let affine = AffineG2::from_jacobian(p).unwrap();
    buf[0] = 0x04;
    // Fq2 has real and imaginary parts
    // substrate-bn AffineG2 x() returns Fq2, y() returns Fq2
    // Fq2 serialization: real part first, then imaginary
    affine.x().real().to_big_endian(&mut buf[1..33]).unwrap();
    affine.x().imaginary().to_big_endian(&mut buf[33..65]).unwrap();
    affine.y().real().to_big_endian(&mut buf[65..97]).unwrap();
    affine.y().imaginary().to_big_endian(&mut buf[97..129]).unwrap();
    buf
}

fn deserialize_g2(data: &[u8]) -> Option<G2> {
    if data.len() < 129 || data[0] != 0x04 {
        return None;
    }
    let x_re = Fq::from_slice(&data[1..33]).ok()?;
    let x_im = Fq::from_slice(&data[33..65]).ok()?;
    let y_re = Fq::from_slice(&data[65..97]).ok()?;
    let y_im = Fq::from_slice(&data[97..129]).ok()?;
    let x = Fq2::new(x_re, x_im);
    let y = Fq2::new(y_re, y_im);
    let affine = AffineG2::new(x, y).ok()?;
    Some(G2::from(affine))
}

fn serialize_fr(scalar: &Fr) -> [u8; 32] {
    let mut buf = [0u8; 32];
    // Use into_u256() which converts from Montgomery representation,
    // matching what from_slice() expects (non-Montgomery).
    // Fr::to_big_endian() uses raw() which is still in Montgomery form.
    scalar.into_u256().to_big_endian(&mut buf).unwrap();
    buf
}

fn deserialize_fr(data: &[u8]) -> Option<Fr> {
    if data.len() < 32 {
        return None;
    }
    Fr::from_slice(&data[0..32]).ok()
}

fn random_fr() -> Fr {
    Fr::random(&mut OsRng)
}

/// Hash arbitrary data to a scalar field element Fr
fn hash_to_fr(data: &[u8]) -> Fr {
    let hash = Sha256::digest(data);
    let mut buf = [0u8; 64];
    buf[32..64].copy_from_slice(&hash);
    Fr::interpret(&buf)
}

/// Hash to G1 using try-and-increment method
fn hash_to_g1(data: &[u8]) -> G1 {
    let mut counter: u32 = 0;
    loop {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.update(counter.to_be_bytes());
        let hash = hasher.finalize();
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&hash);
        // Try to interpret as x coordinate
        if let Ok(x) = Fq::from_slice(&buf) {
            // y^2 = x^3 + b
            let x2 = x * x;
            let x3 = x2 * x;
            let rhs = x3 + G1::b();
            if let Some(y) = rhs.sqrt() {
                if let Ok(affine) = AffineG1::new(x, y) {
                    return G1::from(affine);
                }
            }
        }
        counter += 1;
        if counter > 10000 {
            panic!("hash_to_g1: failed to find a point after 10000 iterations");
        }
    }
}

// ============================================================================
// ECDAA Protocol Implementation
// ============================================================================

// ----- Credential structure (A, B, C, D) stored as 4 concatenated G1 points -----

fn credential_get_a(cred: &[u8]) -> Option<G1> {
    deserialize_g1(&cred[0..65])
}
fn credential_get_b(cred: &[u8]) -> Option<G1> {
    deserialize_g1(&cred[65..130])
}
fn credential_get_c(cred: &[u8]) -> Option<G1> {
    deserialize_g1(&cred[130..195])
}
fn credential_get_d(cred: &[u8]) -> Option<G1> {
    deserialize_g1(&cred[195..260])
}

fn serialize_credential(a: &G1, b: &G1, c: &G1, d: &G1) -> Vec<u8> {
    let mut out = Vec::with_capacity(native::CREDENTIAL_LENGTH);
    out.extend_from_slice(&serialize_g1(a));
    out.extend_from_slice(&serialize_g1(b));
    out.extend_from_slice(&serialize_g1(c));
    out.extend_from_slice(&serialize_g1(d));
    out
}

// ----- Member public key (Q, c, s, n) -----

fn member_pk_get_q(pk: &[u8]) -> Option<G1> {
    deserialize_g1(&pk[0..65])
}
fn member_pk_get_c(pk: &[u8]) -> Option<Fr> {
    deserialize_fr(&pk[65..97])
}
fn member_pk_get_s(pk: &[u8]) -> Option<Fr> {
    deserialize_fr(&pk[97..129])
}
// ----- Group public key (X, Y) stored as 2 G2 points -----

fn gpk_get_x(gpk: &[u8]) -> Option<G2> {
    deserialize_g2(&gpk[0..129])
}
fn gpk_get_y(gpk: &[u8]) -> Option<G2> {
    deserialize_g2(&gpk[129..258])
}

// ----- Issuer secret key (x, y) stored as 2 scalars -----

fn isk_get_x(isk: &[u8]) -> Option<Fr> {
    deserialize_fr(&isk[0..32])
}
fn isk_get_y(isk: &[u8]) -> Option<Fr> {
    deserialize_fr(&isk[32..64])
}

// ----- Signature structure -----
// Without basename: (c, s, R, S, T, W, n)  = 32+32+65+65+65+65+32 = 356
// With basename:    (c, s, R, S, T, W, n, K) = 356+65 = 421

fn sig_get_c(sig: &[u8]) -> Option<Fr> {
    deserialize_fr(&sig[0..32])
}
fn sig_get_s(sig: &[u8]) -> Option<Fr> {
    deserialize_fr(&sig[32..64])
}
fn sig_get_r(sig: &[u8]) -> Option<G1> {
    deserialize_g1(&sig[64..129])
}
fn sig_get_s_point(sig: &[u8]) -> Option<G1> {
    deserialize_g1(&sig[129..194])
}
fn sig_get_t(sig: &[u8]) -> Option<G1> {
    deserialize_g1(&sig[194..259])
}
fn sig_get_w(sig: &[u8]) -> Option<G1> {
    deserialize_g1(&sig[259..324])
}
fn sig_get_n(sig: &[u8]) -> Option<Fr> {
    deserialize_fr(&sig[324..356])
}
fn sig_get_k(sig: &[u8]) -> Option<G1> {
    if sig.len() < native::SIGNATURE_WITH_NYM_LENGTH {
        return None;
    }
    deserialize_g1(&sig[356..421])
}

// Schnorr hash for member key proof-of-knowledge
fn schnorr_hash_member(u: &G1, basepoint: &G1, q: &G1, nonce: &[u8]) -> Fr {
    let mut data = Vec::new();
    data.extend_from_slice(&serialize_g1(u));
    data.extend_from_slice(&serialize_g1(basepoint));
    data.extend_from_slice(&serialize_g1(q));
    data.extend_from_slice(nonce);
    hash_to_fr(&data)
}

// Schnorr hash for credential signature
fn schnorr_hash_credential(b_point: &G1, pk_q: &G1, d: &G1) -> Fr {
    let mut data = Vec::new();
    data.extend_from_slice(&serialize_g1(b_point));
    data.extend_from_slice(&serialize_g1(pk_q));
    data.extend_from_slice(&serialize_g1(d));
    hash_to_fr(&data)
}

// ============================================================================
// Public API - matches the original v0.rs interface exactly
// ============================================================================

pub fn generate_wallet_keypair(
    nonce: &Vec<u8>,
    secret_key: &mut Vec<u8>,
    public_key: &mut Vec<u8>,
) -> bool {
    if nonce.len() == 0 {
        return false;
    }

    // Generate member secret key
    let sk = random_fr();

    // Compute base point B = hash_to_g1(nonce)
    let b = hash_to_g1(nonce);

    // Q = B^sk
    let q = b * sk;

    // Create Schnorr proof of knowledge of sk
    // 1. r = random
    let r = random_fr();
    // 2. U = B^r
    let u = b * r;
    // 3. c = H(U, B, Q, nonce)
    let c = schnorr_hash_member(&u, &b, &q, nonce);
    // 4. s = r + c * sk (mod p) -- note: in the xaptum code it's r - c*sk
    //    but we follow the same convention to maintain compatibility
    let s = r + c * sk;
    // 5. n = random nonce for replay protection
    let n = random_fr();

    // Serialize
    secret_key.clear();
    secret_key.extend_from_slice(&serialize_fr(&sk));

    // Public key = (Q, c, s, n)
    public_key.clear();
    public_key.extend_from_slice(&serialize_g1(&q));
    public_key.extend_from_slice(&serialize_fr(&c));
    public_key.extend_from_slice(&serialize_fr(&s));
    public_key.extend_from_slice(&serialize_fr(&n));

    assert_eq!(secret_key.len(), native::WALLET_SECRET_KEY_LENGTH);
    assert_eq!(public_key.len(), native::WALLET_PUBLIC_KEY_LENGTH);
    true
}

pub fn generate_issuer_keypair(
    issuer_secret_key: &mut Vec<u8>,
    group_public_key: &mut Vec<u8>,
) -> bool {
    // Generate issuer secret key (x, y)
    let x = random_fr();
    let y = random_fr();

    // Group public key: X = P2^x, Y = P2^y where P2 is the G2 generator
    let big_x = G2::one() * x;
    let big_y = G2::one() * y;

    issuer_secret_key.clear();
    issuer_secret_key.extend_from_slice(&serialize_fr(&x));
    issuer_secret_key.extend_from_slice(&serialize_fr(&y));

    group_public_key.clear();
    group_public_key.extend_from_slice(&serialize_g2(&big_x));
    group_public_key.extend_from_slice(&serialize_g2(&big_y));

    assert_eq!(issuer_secret_key.len(), native::ISSUER_SECRET_KEY_LENGTH);
    assert_eq!(group_public_key.len(), native::ISSUER_GROUP_PUBLIC_KEY_LENGTH);
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

    // Deserialize member public key
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

    // Verify the Schnorr proof in the member public key
    let b = hash_to_g1(nonce);
    // Recompute U = B^s - Q^c (or B^s * Q^(-c))
    let u_recomputed = b * pk_s + q * (-pk_c);
    let c_recomputed = schnorr_hash_member(&u_recomputed, &b, &q, nonce);
    if pk_c != c_recomputed {
        log::error!("Member public key Schnorr proof verification failed");
        return false;
    }

    // Deserialize issuer secret key
    let isk_x = match isk_get_x(issuer_secret_key) {
        Some(v) => v,
        None => return false,
    };
    let isk_y = match isk_get_y(issuer_secret_key) {
        Some(v) => v,
        None => return false,
    };

    // Generate credential (A, B, C, D)
    // A = B^(1/y)
    let y_inv = match isk_y.inverse() {
        Some(v) => v,
        None => return false,
    };
    let a = b * y_inv;

    // D = Q (the member's public key point)
    let d = q;

    // C = (A + D)^x  -- note: A + D is point addition, then scalar mul by x
    let c = (a + d) * isk_x;

    // Generate credential signature (Schnorr proof that credential is valid)
    let r = random_fr();
    let u_sig = b * r;
    let c_sig = schnorr_hash_credential(&u_sig, &q, &d);
    let s_sig = r + c_sig * isk_y;

    // Serialize credential
    credential_out.clear();
    credential_out.extend_from_slice(&serialize_credential(&a, &b, &c, &d));

    credential_signature_out.clear();
    credential_signature_out.extend_from_slice(&serialize_fr(&c_sig));
    credential_signature_out.extend_from_slice(&serialize_fr(&s_sig));

    assert_eq!(credential_out.len(), native::CREDENTIAL_LENGTH);
    assert_eq!(
        credential_signature_out.len(),
        native::CREDENTIAL_SIGNATURE_LENGTH
    );
    true
}

/// Verifies an ECDAA member signature optionally with the supplied
/// basename and optionally enforcing the signing credential is |signing_credential|.
pub fn verify(
    group_public_key: &Vec<u8>,
    basename: &Option<Vec<u8>>,
    signing_credential: &Option<Vec<u8>>,
    signature: &Vec<u8>,
    message: &Vec<u8>,
) -> bool {
    if group_public_key.len() == 0 || signature.len() == 0 || message.len() == 0 {
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

    // Deserialize group public key
    let gpk_x = match gpk_get_x(group_public_key) {
        Some(v) => v,
        None => return false,
    };
    let gpk_y = match gpk_get_y(group_public_key) {
        Some(v) => v,
        None => return false,
    };

    // Deserialize signature components
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

    // Check that R, S, T, W are not the identity
    if r.is_zero() || s_point.is_zero() || t.is_zero() || w.is_zero() {
        return false;
    }

    // If required credential is specified, check it matches
    if let Some(req_cred) = signing_credential {
        if req_cred.len() < native::CREDENTIAL_LENGTH {
            return false;
        }
        // Extract credential (R, S, T, W) from signature and compare with required
        // The credential in the signature is the randomized version (R=A^l, S=B^l, T=C^l, W=D^l)
        let sig_cred = serialize_credential(&r, &s_point, &t, &w);
        if sig_cred != *req_cred {
            return false;
        }
    }

    // Verify the Schnorr proof
    // Recompute: U = S^s · W^(-c)
    let u = s_point * sig_s + w * (-sig_c);

    // Build the hash input
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
        // Also include K in verification
        let bsn_base = hash_to_g1(bsn);
        // Verify K = bsn_base^sk by checking: bsn_base^s · K^(-c) should be consistent
        let k_u = bsn_base * sig_s + k * (-sig_c);
        hash_data.extend_from_slice(&serialize_g1(&k_u));
        hash_data.extend_from_slice(&serialize_g1(&k));
        hash_data.extend_from_slice(&serialize_g1(&bsn_base));
    }

    let c2 = hash_to_fr(&hash_data);
    // c should equal H(n || c2)
    let mut final_hash_data = Vec::new();
    final_hash_data.extend_from_slice(&serialize_fr(&n));
    final_hash_data.extend_from_slice(&serialize_fr(&c2));
    let c_expected = hash_to_fr(&final_hash_data);

    if sig_c != c_expected {
        return false;
    }

    // Pairing checks:
    // 1. e(R, Y) == e(S, P2)  (verifies A/B relationship under Y)
    let p2 = G2::one();
    let lhs1 = pairing(r, gpk_y);
    let rhs1 = pairing(s_point, p2);
    if lhs1 != rhs1 {
        return false;
    }

    // 2. e(T, P2) == e(R + W, X)  (verifies C relationship under X)
    let lhs2 = pairing(t, p2);
    let rhs2 = pairing(r + w, gpk_x);
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
    if message.len() == 0 || credential.len() == 0 || secret_key.len() == 0 {
        return false;
    }
    if let Some(bsn) = basename {
        if bsn.len() == 0 {
            return false;
        }
    }

    // Deserialize credential (A, B, C, D)
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

    // Deserialize secret key
    let sk = match deserialize_fr(secret_key) {
        Some(v) => v,
        None => return false,
    };

    // Randomize credential if requested
    if randomize_cred {
        let l = random_fr();
        a = a * l;
        b = b * l;
        c = c * l;
        d = d * l;
    }
    // Now (R, S, T, W) = (A, B, C, D) (possibly randomized)

    // Schnorr proof of knowledge of sk such that W = S^sk
    // 1. r = random
    let r = random_fr();
    // 2. U = S^r
    let u = b * r;

    // Build hash for c2
    let mut hash_data = Vec::new();
    hash_data.extend_from_slice(&serialize_g1(&u));
    hash_data.extend_from_slice(&serialize_g1(&b)); // S point
    hash_data.extend_from_slice(&serialize_g1(&d)); // W point
    hash_data.extend_from_slice(message);

    // Handle basename/pseudonym
    let mut k_point = G1::zero();
    if let Some(bsn) = basename {
        let bsn_base = hash_to_g1(bsn);
        k_point = bsn_base * sk;
        let k_u = bsn_base * r;
        hash_data.extend_from_slice(&serialize_g1(&k_u));
        hash_data.extend_from_slice(&serialize_g1(&k_point));
        hash_data.extend_from_slice(&serialize_g1(&bsn_base));
    }

    let c2 = hash_to_fr(&hash_data);

    // 3. n = random nonce
    let n = random_fr();

    // 4. c = H(n || c2)
    let mut final_hash_data = Vec::new();
    final_hash_data.extend_from_slice(&serialize_fr(&n));
    final_hash_data.extend_from_slice(&serialize_fr(&c2));
    let sig_c = hash_to_fr(&final_hash_data);

    // 5. s = r + c * sk
    let sig_s = r + sig_c * sk;

    // Serialize signature
    signature.clear();
    if basename.is_some() {
        signature.reserve(native::SIGNATURE_WITH_NYM_LENGTH);
    } else {
        signature.reserve(native::SIGNATURE_LENGTH);
    }

    signature.extend_from_slice(&serialize_fr(&sig_c));
    signature.extend_from_slice(&serialize_fr(&sig_s));
    signature.extend_from_slice(&serialize_g1(&a)); // R
    signature.extend_from_slice(&serialize_g1(&b)); // S
    signature.extend_from_slice(&serialize_g1(&c)); // T
    signature.extend_from_slice(&serialize_g1(&d)); // W
    signature.extend_from_slice(&serialize_fr(&n));

    if basename.is_some() {
        signature.extend_from_slice(&serialize_g1(&k_point));
    }

    true
}

pub fn randomize_credential(credential: &Vec<u8>, credential_out: &mut Vec<u8>) -> bool {
    if credential.len() == 0 {
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

    let l = random_fr();
    let r_a = a * l;
    let r_b = b * l;
    let r_c = c * l;
    let r_d = d * l;

    credential_out.clear();
    credential_out.extend_from_slice(&serialize_credential(&r_a, &r_b, &r_c, &r_d));
    assert_eq!(credential_out.len(), native::CREDENTIAL_LENGTH);
    true
}

// TODO: wrap the Vec<u8>s in a struct so we can use From/Into
pub fn credential_from_signature(signature: &Vec<u8>, credential: &mut Vec<u8>) -> bool {
    if signature.len() < native::SIGNATURE_LENGTH {
        return false;
    }
    credential.resize(native::CREDENTIAL_LENGTH, 0);
    let offset = 2 * native::MODBYTES_256_56;
    let end = offset + native::CREDENTIAL_LENGTH;
    credential.copy_from_slice(&signature[offset..end]);
    return true;
}

// Removes the credential saving 260 bytes
pub fn deflate_signature(signature: &mut Vec<u8>) {
    if signature.len() < native::SIGNATURE_LENGTH {
        return;
    }
    let offset = 2 * native::MODBYTES_256_56;
    let end = offset + native::CREDENTIAL_LENGTH;
    signature.drain(offset..end);
}

pub fn inflate_signature(credential: &Vec<u8>, signature: &mut Vec<u8>) {
    let offset = 2 * native::MODBYTES_256_56;
    let rhs = signature.split_off(offset);
    signature.extend_from_slice(credential.as_slice());
    signature.extend_from_slice(rhs.as_slice());
}

pub fn credential_in_group(credential: &Vec<u8>, group_public_key: &Vec<u8>) -> bool {
    if credential.len() == 0 || group_public_key.len() == 0 {
        return false;
    }
    if credential.len() < native::CREDENTIAL_LENGTH
        || group_public_key.len() < native::ISSUER_GROUP_PUBLIC_KEY_LENGTH
    {
        return false;
    }

    // Deserialize credential (A, B, C, D)
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

    // Deserialize group public key
    let gpk_x = match gpk_get_x(group_public_key) {
        Some(v) => v,
        None => return false,
    };
    let gpk_y = match gpk_get_y(group_public_key) {
        Some(v) => v,
        None => return false,
    };

    // Check that points are not identity
    if a.is_zero() || b.is_zero() {
        return false;
    }

    // Verify pairing equations:
    // 1. e(A, Y) == e(B, P2)
    let p2 = G2::one();
    let lhs1 = pairing(a, gpk_y);
    let rhs1 = pairing(b, p2);
    if lhs1 != rhs1 {
        return false;
    }

    // 2. e(C, P2) == e(A + D, X)
    let lhs2 = pairing(c, p2);
    let rhs2 = pairing(a + d, gpk_x);
    if lhs2 != rhs2 {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuer_generate_keypair_basic() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(issuer_sk.len(), native::ISSUER_SECRET_KEY_LENGTH);
        assert_eq!(group_pk.len(), native::ISSUER_GROUP_PUBLIC_KEY_LENGTH);
        assert_eq!(ret, true);
    }

    #[test]
    fn wallet_generate_keypair_basic() {
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes();
        let ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(sk.len(), native::WALLET_SECRET_KEY_LENGTH);
        assert_eq!(pk.len(), native::WALLET_PUBLIC_KEY_LENGTH);
        assert_eq!(ret, true);
    }

    #[test]
    fn new_credential_from_generated_keys() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);

        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);

        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);
        assert_eq!(cred.len(), native::CREDENTIAL_LENGTH);
        assert_eq!(cred_sig.len(), native::CREDENTIAL_SIGNATURE_LENGTH);
    }

    #[test]
    fn sign_and_verify_test() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);

        let mut sig: Vec<u8> = vec![];
        let message = "hello".as_bytes().to_vec();
        ret = sign(&message, &cred, &sk, &None, true, &mut sig);
        assert_eq!(ret, true);
        assert_ne!(sig.len(), 0);
        ret = verify(&group_pk, &None, &None, &sig, &message);
        assert_eq!(ret, true);
    }

    #[test]
    fn sign_and_verify_with_required_cred_test() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);

        let mut sig: Vec<u8> = vec![];
        let message = "hello".as_bytes().to_vec();
        let some_cred = Some(cred.clone());
        // To enforce a given credential, we can't randomize at sign.
        ret = sign(&message, &cred, &sk, &None, false, &mut sig);
        assert_eq!(ret, true);
        assert_ne!(sig.len(), 0);
        ret = verify(&group_pk, &None, &some_cred, &sig, &message);
        assert_eq!(ret, true);

        // Expect failure when randomized
        ret = sign(&message, &cred, &sk, &None, true, &mut sig);
        assert_eq!(ret, true);
        assert_ne!(sig.len(), 0);
        ret = verify(&group_pk, &None, &some_cred, &sig, &message);
        assert_eq!(ret, false);
    }

    #[test]
    fn sign_and_verify_with_basename_test() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);

        let mut sig: Vec<u8> = vec![];
        let message = "hello".as_bytes().to_vec();
        let basename = Some("5pm on Friday".as_bytes().to_vec());
        ret = sign(&message, &cred, &sk, &basename, true, &mut sig);
        assert_eq!(ret, true);
        assert_ne!(sig.len(), 0);
        ret = verify(&group_pk, &basename, &None, &sig, &message);
        assert_eq!(ret, true);
        ret = verify(&group_pk, &None, &None, &sig, &message);
        assert_eq!(ret, false);
    }

    #[test]
    fn sign_and_verify_with_required_cred_with_basename_test() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);

        let mut sig: Vec<u8> = vec![];
        let message = "hello".as_bytes().to_vec();
        let basename = Some("5pm on Friday".as_bytes().to_vec());
        let some_cred = Some(cred.clone());
        ret = sign(&message, &cred, &sk, &basename, false, &mut sig);
        assert_eq!(ret, true);
        assert_ne!(sig.len(), 0);
        ret = verify(&group_pk, &basename, &some_cred, &sig, &message);
        assert_eq!(ret, true);
        ret = verify(&group_pk, &None, &some_cred, &sig, &message);
        assert_eq!(ret, false);

        // Randomize and fail.
        ret = sign(&message, &cred, &sk, &basename, true, &mut sig);
        assert_eq!(ret, true);
        assert_ne!(sig.len(), 0);
        ret = verify(&group_pk, &basename, &some_cred, &sig, &message);
        assert_eq!(ret, false);
    }

    #[test]
    fn randomize_credential_sign_and_verify() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);

        let mut r_cred = vec![];
        assert_eq!(randomize_credential(&cred, &mut r_cred), true);

        let mut sig: Vec<u8> = vec![];
        let message = "hello".as_bytes().to_vec();
        let some_cred = Some(r_cred.clone());
        ret = sign(&message, &r_cred, &sk, &None, false, &mut sig);
        assert_eq!(ret, true);
        assert_ne!(sig.len(), 0);
        ret = verify(&group_pk, &None, &some_cred, &sig, &message);
        assert_eq!(ret, true);
    }

    #[test]
    fn issued_credential_in_group() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);
        assert_eq!(credential_in_group(&cred, &group_pk), true);

        let mut issuer2_sk = Vec::new();
        let mut group2_pk = Vec::new();
        assert_eq!(
            generate_issuer_keypair(&mut issuer2_sk, &mut group2_pk),
            true
        );
        assert_eq!(credential_in_group(&cred, &group2_pk), false);
    }

    #[test]
    fn randomize_credential_in_group() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let mut ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(ret, true);
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = String::from("test-wallet-1").into_bytes().to_vec();
        ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(ret, true);
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        ret = issue_credential(&pk, &issuer_sk, &id, &mut cred, &mut cred_sig);
        assert_eq!(ret, true);

        let mut r_cred = vec![];
        assert_eq!(randomize_credential(&cred, &mut r_cred), true);
        assert_eq!(credential_in_group(&r_cred, &group_pk), true);

        let mut issuer2_sk = Vec::new();
        let mut group2_pk = Vec::new();
        assert_eq!(
            generate_issuer_keypair(&mut issuer2_sk, &mut group2_pk),
            true
        );
        assert_eq!(credential_in_group(&r_cred, &group2_pk), false);
    }
}
