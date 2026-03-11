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

// ============================================================================
// Split-key signing (Brickell & Li style)
//
// The member secret key is additively split: sk = card_sk + host_sk
// The smart card performs only G1 scalar multiplications and Fr arithmetic.
// No pairings are ever needed on the card side.
// The host completes the signature by combining its share with the card's.
// Verification is unchanged — the verifier never knows about the split.
// ============================================================================

pub mod split {
    use super::*;

    // Card-side partial outputs from signing
    pub struct CardSignCommitment {
        /// U_card = S * r_card  (G1 point)
        pub u_card: Vec<u8>,
        /// K_card = bsn_base * card_sk  (G1 point, only if basename used)
        pub k_card: Option<Vec<u8>>,
        /// K_u_card = bsn_base * r_card  (G1 point, only if basename used)
        pub k_u_card: Option<Vec<u8>>,
    }

    pub struct CardSignResponse {
        /// s_card = r_card + c * card_sk  (scalar)
        pub s_card: Vec<u8>,
    }

    /// Trait representing the operations a smart card must support.
    /// All operations are G1 scalar multiplications and Fr arithmetic only.
    /// No pairings, no G2 operations, no GT operations.
    pub trait SmartCard {
        /// Returns the card's share of the public key: Q_card = base * card_sk
        fn public_key_share(&self, base: &[u8]) -> Vec<u8>;

        /// Phase 1 of signing: card generates randomness and commits.
        /// Returns U_card = S * r_card (and K_card, K_u_card if basename provided).
        /// The card internally stores r_card for phase 2.
        fn sign_commit(
            &mut self,
            s_point: &[u8],
            basename_base: Option<&[u8]>,
        ) -> Option<CardSignCommitment>;

        /// Phase 2 of signing: card produces its share of the Schnorr response.
        /// Given the challenge c, returns s_card = r_card + c * card_sk.
        fn sign_respond(&mut self, challenge: &[u8]) -> Option<CardSignResponse>;

        /// Phase 1 of blind join: card generates randomness and commits.
        /// Given base point B, returns U_card = B * r_card (serialized G1 point).
        /// The card stores r_card internally for join_respond.
        fn join_commit(&mut self, base: &[u8]) -> Option<Vec<u8>>;

        /// Phase 2 of blind join: card produces its Schnorr response share.
        /// Given challenge c, returns s_card = r_card + c * card_sk (serialized scalar).
        fn join_respond(&mut self, challenge: &[u8]) -> Option<Vec<u8>>;

        /// Returns the card's secret key share serialized (for testing/debugging only).
        /// A real smart card would NEVER expose this.
        fn secret_key_share(&self) -> Vec<u8>;
    }

    /// Mock smart card for testing. Simulates a Javacard/SIM performing
    /// only G1 scalar multiplications and Fr scalar arithmetic.
    pub struct MockCard {
        card_sk: Fr,
        /// Ephemeral randomness for the current signing session
        r_card: Option<Fr>,
    }

    impl MockCard {
        /// Create a new mock card with a random secret key share.
        pub fn new() -> Self {
            MockCard {
                card_sk: random_fr(),
                r_card: None,
            }
        }

        /// Create a mock card with a specific secret key share (for testing).
        pub fn from_secret(sk: Fr) -> Self {
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

            // Generate ephemeral randomness
            let r = random_fr();
            self.r_card = Some(r);

            // U_card = S * r_card
            let u_card = s * r;

            let (k_card, k_u_card) = if let Some(bsn_base_bytes) = basename_base {
                let bsn_base = deserialize_g1(bsn_base_bytes)?;
                // K_card = bsn_base * card_sk
                let k = bsn_base * self.card_sk;
                // K_u_card = bsn_base * r_card
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
            let c = deserialize_fr(challenge)?;
            let r = self.r_card.take()?;

            // s_card = r_card + c * card_sk
            let s_card = r + c * self.card_sk;

            Some(CardSignResponse {
                s_card: serialize_fr(&s_card).to_vec(),
            })
        }

        fn join_commit(&mut self, base: &[u8]) -> Option<Vec<u8>> {
            let b = deserialize_g1(base)?;
            let r = random_fr();
            self.r_card = Some(r);
            Some(serialize_g1(&(b * r)).to_vec())
        }

        fn join_respond(&mut self, challenge: &[u8]) -> Option<Vec<u8>> {
            let c = deserialize_fr(challenge)?;
            let r = self.r_card.take()?;
            let s_card = r + c * self.card_sk;
            Some(serialize_fr(&s_card).to_vec())
        }

        fn secret_key_share(&self) -> Vec<u8> {
            serialize_fr(&self.card_sk).to_vec()
        }
    }

    /// Generate a split wallet keypair using the blind join protocol.
    /// The card generates its share, the host generates its share,
    /// and the combined public key is produced.
    ///
    /// Uses a split Schnorr proof: s = s_card + s_host where
    /// s_card = r_card + c * card_sk and s_host = r_host + c * host_sk.
    /// The combined secret key never exists in one place.
    ///
    /// Returns (host_sk, combined_pk). The combined_sk is never produced.
    pub fn generate_split_wallet_keypair(
        card: &mut dyn SmartCard,
        nonce: &Vec<u8>,
    ) -> Option<(Vec<u8>, Vec<u8>)> {
        if nonce.is_empty() {
            return None;
        }

        // Compute base point B = hash_to_g1(nonce)
        let b = hash_to_g1(nonce);
        let b_bytes = serialize_g1(&b);

        // Card computes Q_card = B * card_sk
        let q_card_bytes = card.public_key_share(&b_bytes);
        let q_card = deserialize_g1(&q_card_bytes)?;

        // Host generates its share
        let host_sk = random_fr();
        let q_host = b * host_sk;

        // Combined public key point: Q = Q_card + Q_host
        let q = q_card + q_host;

        // === Blind Schnorr proof (split between card and host) ===

        // Phase 1: Both commit
        let u_card_bytes = card.join_commit(&b_bytes)?;
        let u_card = deserialize_g1(&u_card_bytes)?;

        let r_host = random_fr();
        let u_host = b * r_host;

        // Combined commitment
        let u = u_card + u_host;

        // Challenge (same hash as non-split version)
        let c = schnorr_hash_member(&u, &b, &q, nonce);

        // Phase 2: Both respond
        let s_card_bytes = card.join_respond(&serialize_fr(&c))?;
        let s_card = deserialize_fr(&s_card_bytes)?;

        let s_host = r_host + c * host_sk;

        // Combined response
        let s = s_card + s_host;
        let n = random_fr();

        // Serialize host secret key share
        let host_sk_bytes = serialize_fr(&host_sk).to_vec();

        // Serialize combined public key (Q, c, s, n)
        let mut pk = Vec::with_capacity(native::WALLET_PUBLIC_KEY_LENGTH);
        pk.extend_from_slice(&serialize_g1(&q));
        pk.extend_from_slice(&serialize_fr(&c));
        pk.extend_from_slice(&serialize_fr(&s));
        pk.extend_from_slice(&serialize_fr(&n));

        Some((host_sk_bytes, pk))
    }

    /// Sign a message using the split-key protocol.
    /// The card performs G1 scalar multiplications; the host combines shares.
    /// The resulting signature is identical in format to `sign()` and
    /// verifiable with the standard `verify()`.
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

        // Deserialize credential (A, B, C, D)
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

        // Deserialize host secret key share
        let h_sk = match deserialize_fr(host_sk) {
            Some(v) => v,
            None => return false,
        };

        // Randomize credential if requested (host can do this — no sk needed)
        if randomize_cred {
            let l = random_fr();
            a = a * l;
            b = b * l;
            c_point = c_point * l;
            d = d * l;
        }

        // S point (randomized B) is what both card and host use for commitments
        let s_bytes = serialize_g1(&b);

        // Compute basename base point if needed
        let bsn_base = basename.as_ref().map(|bsn| hash_to_g1(bsn));
        let bsn_base_bytes = bsn_base.as_ref().map(|p| serialize_g1(p).to_vec());

        // === Phase 1: Card commits ===
        let card_commit = match card.sign_commit(
            &s_bytes,
            bsn_base_bytes.as_deref(),
        ) {
            Some(v) => v,
            None => return false,
        };

        let u_card = match deserialize_g1(&card_commit.u_card) {
            Some(v) => v,
            None => return false,
        };

        // === Host commits ===
        let r_host = random_fr();
        let u_host = b * r_host;

        // Combined commitment: U = U_card + U_host
        let u = u_card + u_host;

        // Build hash for c2 (same as standard sign)
        let mut hash_data = Vec::new();
        hash_data.extend_from_slice(&serialize_g1(&u));
        hash_data.extend_from_slice(&serialize_g1(&b)); // S point
        hash_data.extend_from_slice(&serialize_g1(&d)); // W point
        hash_data.extend_from_slice(message);

        // Handle basename/pseudonym
        let mut k_combined = G1::zero();
        if let Some(bsn_base_pt) = &bsn_base {
            // Combine K shares
            let k_card = match &card_commit.k_card {
                Some(bytes) => match deserialize_g1(bytes) {
                    Some(v) => v,
                    None => return false,
                },
                None => return false,
            };
            let k_host = *bsn_base_pt * h_sk;
            k_combined = k_card + k_host;

            // Combine K_u shares
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

        let c2 = hash_to_fr(&hash_data);

        // n = random nonce
        let n = random_fr();

        // c = H(n || c2)
        let mut final_hash_data = Vec::new();
        final_hash_data.extend_from_slice(&serialize_fr(&n));
        final_hash_data.extend_from_slice(&serialize_fr(&c2));
        let sig_c = hash_to_fr(&final_hash_data);

        // === Phase 2: Card responds to challenge ===
        let card_response = match card.sign_respond(&serialize_fr(&sig_c)) {
            Some(v) => v,
            None => return false,
        };

        let s_card = match deserialize_fr(&card_response.s_card) {
            Some(v) => v,
            None => return false,
        };

        // Host response: s_host = r_host + c * host_sk
        let s_host = r_host + sig_c * h_sk;

        // Combined response: s = s_card + s_host
        let sig_s = s_card + s_host;

        // Serialize signature (identical format to standard sign)
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
        signature.extend_from_slice(&serialize_g1(&c_point)); // T
        signature.extend_from_slice(&serialize_g1(&d)); // W
        signature.extend_from_slice(&serialize_fr(&n));

        if basename.is_some() {
            signature.extend_from_slice(&serialize_g1(&k_combined));
        }

        true
    }

    /// NFC SmartCard transport layer.
    ///
    /// Implements the SmartCard trait by sending ISO 7816 APDUs to a JavaCard
    /// applet over PC/SC (NFC reader). Feature-gated behind "nfc".
    #[cfg(feature = "nfc")]
    pub mod nfc {
        use super::*;

        /// APDU CLA byte for all Briolette commands.
        const CLA: u8 = 0x80;

        /// Curve version P1 byte.
        const P1_BN254: u8 = 0x00;

        /// INS codes matching the JavaCard applet.
        const INS_GENERATE_KEY: u8 = 0x01;
        const INS_PUBLIC_KEY_SHARE: u8 = 0x02;
        const INS_SIGN_COMMIT: u8 = 0x10;
        const INS_SIGN_COMMIT_BSN: u8 = 0x11;
        const INS_SIGN_RESPOND: u8 = 0x12;
        const INS_SIGN_COMMIT_SWAP: u8 = 0x13;
        const INS_JOIN_COMMIT: u8 = 0x20;
        const INS_JOIN_RESPOND: u8 = 0x21;
        const INS_RESET_BLOOM: u8 = 0x30;
        const INS_GET_STATUS: u8 = 0x40;

        /// G1 point size for BN254 (0x04 || x(32) || y(32)).
        const G1_BYTES: usize = 65;
        /// Scalar field element size.
        const FR_BYTES: usize = 32;

        /// SW bytes indicating success.
        const SW_SUCCESS: [u8; 2] = [0x90, 0x00];

        /// SmartCard implementation that communicates with a JavaCard applet
        /// via ISO 7816 APDUs over PC/SC (e.g., ACR122U USB NFC reader).
        pub struct NfcSmartCard {
            card: pcsc::Card,
        }

        impl NfcSmartCard {
            /// Connect to the first available NFC reader and select the
            /// Briolette applet.
            pub fn connect() -> Result<Self, pcsc::Error> {
                let ctx = pcsc::Context::establish(pcsc::Scope::System)?;
                let mut readers_buf = vec![0u8; 2048];
                let readers: Vec<&std::ffi::CStr> =
                    ctx.list_readers(&mut readers_buf)?.collect();

                if readers.is_empty() {
                    return Err(pcsc::Error::NoReadersAvailable);
                }

                // Connect to first reader
                let card = ctx.connect(
                    readers[0],
                    pcsc::ShareMode::Shared,
                    pcsc::Protocols::ANY,
                )?;

                Ok(NfcSmartCard { card })
            }

            /// Connect using a specific PC/SC reader name.
            pub fn connect_reader(reader_name: &std::ffi::CStr) -> Result<Self, pcsc::Error> {
                let ctx = pcsc::Context::establish(pcsc::Scope::System)?;
                let card = ctx.connect(
                    reader_name,
                    pcsc::ShareMode::Shared,
                    pcsc::Protocols::ANY,
                )?;
                Ok(NfcSmartCard { card })
            }

            /// Send an APDU and return the response data (excluding SW bytes).
            /// Returns None if the card returns a non-success status word.
            fn send_apdu(&self, ins: u8, p1: u8, data: &[u8]) -> Option<Vec<u8>> {
                let mut apdu = Vec::with_capacity(5 + data.len() + 1);
                apdu.push(CLA);
                apdu.push(ins);
                apdu.push(p1);
                apdu.push(0x00); // P2
                apdu.push(data.len() as u8); // Lc
                apdu.extend_from_slice(data);
                apdu.push(0x00); // Le (expect maximum response)

                let mut recv_buf = vec![0u8; 258]; // Max APDU response
                match self.card.transmit(&apdu, &mut recv_buf) {
                    Ok(response) => {
                        if response.len() < 2 {
                            return None;
                        }
                        let sw = &response[response.len() - 2..];
                        if sw != SW_SUCCESS {
                            log::warn!(
                                "APDU INS={:02x} returned SW={:02x}{:02x}",
                                ins, sw[0], sw[1]
                            );
                            return None;
                        }
                        Some(response[..response.len() - 2].to_vec())
                    }
                    Err(e) => {
                        log::error!("PC/SC transmit error: {:?}", e);
                        None
                    }
                }
            }

            /// Send GENERATE_KEY command to initialize the card's secret key.
            pub fn generate_key(&self) -> bool {
                self.send_apdu(INS_GENERATE_KEY, P1_BN254, &[]).is_some()
            }

            /// Reset the bloom filter for a new epoch.
            pub fn reset_bloom(&self, epoch: u32) -> bool {
                let data = epoch.to_be_bytes();
                self.send_apdu(INS_RESET_BLOOM, 0x00, &data).is_some()
            }

            /// Sign commit with swap mode (skips bloom filter check).
            pub fn sign_commit_swap(
                &mut self,
                s_point: &[u8],
                basename_base: &[u8],
            ) -> Option<CardSignCommitment> {
                let mut data = Vec::with_capacity(s_point.len() + basename_base.len());
                data.extend_from_slice(s_point);
                data.extend_from_slice(basename_base);
                let resp = self.send_apdu(INS_SIGN_COMMIT_SWAP, P1_BN254, &data)?;
                if resp.len() != G1_BYTES * 3 {
                    return None;
                }
                Some(CardSignCommitment {
                    u_card: resp[..G1_BYTES].to_vec(),
                    k_card: Some(resp[G1_BYTES..G1_BYTES * 2].to_vec()),
                    k_u_card: Some(resp[G1_BYTES * 2..G1_BYTES * 3].to_vec()),
                })
            }
        }

        impl SmartCard for NfcSmartCard {
            fn public_key_share(&self, base: &[u8]) -> Vec<u8> {
                self.send_apdu(INS_PUBLIC_KEY_SHARE, P1_BN254, base)
                    .expect("PUBLIC_KEY_SHARE APDU failed")
            }

            fn sign_commit(
                &mut self,
                s_point: &[u8],
                basename_base: Option<&[u8]>,
            ) -> Option<CardSignCommitment> {
                match basename_base {
                    None => {
                        let resp = self.send_apdu(INS_SIGN_COMMIT, P1_BN254, s_point)?;
                        if resp.len() != G1_BYTES {
                            return None;
                        }
                        Some(CardSignCommitment {
                            u_card: resp,
                            k_card: None,
                            k_u_card: None,
                        })
                    }
                    Some(bsn) => {
                        let mut data = Vec::with_capacity(s_point.len() + bsn.len());
                        data.extend_from_slice(s_point);
                        data.extend_from_slice(bsn);
                        let resp = self.send_apdu(INS_SIGN_COMMIT_BSN, P1_BN254, &data)?;
                        if resp.len() != G1_BYTES * 3 {
                            return None;
                        }
                        Some(CardSignCommitment {
                            u_card: resp[..G1_BYTES].to_vec(),
                            k_card: Some(resp[G1_BYTES..G1_BYTES * 2].to_vec()),
                            k_u_card: Some(resp[G1_BYTES * 2..G1_BYTES * 3].to_vec()),
                        })
                    }
                }
            }

            fn sign_respond(&mut self, challenge: &[u8]) -> Option<CardSignResponse> {
                let resp = self.send_apdu(INS_SIGN_RESPOND, P1_BN254, challenge)?;
                if resp.len() != FR_BYTES {
                    return None;
                }
                Some(CardSignResponse { s_card: resp })
            }

            fn join_commit(&mut self, base: &[u8]) -> Option<Vec<u8>> {
                let resp = self.send_apdu(INS_JOIN_COMMIT, P1_BN254, base)?;
                if resp.len() != G1_BYTES {
                    return None;
                }
                Some(resp)
            }

            fn join_respond(&mut self, challenge: &[u8]) -> Option<Vec<u8>> {
                let resp = self.send_apdu(INS_JOIN_RESPOND, P1_BN254, challenge)?;
                if resp.len() != FR_BYTES {
                    return None;
                }
                Some(resp)
            }

            fn secret_key_share(&self) -> Vec<u8> {
                unimplemented!(
                    "secret_key_share is not available on real hardware — \
                     the card never exports its secret key"
                )
            }
        }
    }
}

use thiserror::Error;
#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Low level {0:?}() failed {1:?})")]
    LowLevelError(String, u64),
    #[error("I/O Error: {0}")]
    IOError(String),
}

/* TODO(redpig) encapsulate key interactions idiomatically
pub trait Keypair {
  fn public_key(&self) -> &Vec<u8>;
  fn secret_key(&self) -> &Vec<u8>;

  fn generate(&mut self) -> Result<(), CryptoError>;
  // sign
  // credentials should be diff than keypairs so we can deal with basename sep.
  // ...

  fn serialize(&self) -> Result<Vec<u8>, CryptoError>;
  fn deserialize(&mut self, bytes: Vec<u8>) -> Result<(), CryptoError>;

  fn load(&mut self, sk: &Path, pk: &Path) -> Result<(), CryptoError>;
  fn store(&self, sk: &Path, pk: &Path) -> Result<(), CryptoError>;
}


#[derive(Debug)]
pub struct IssuerKeypair {
  secret: Vec<u8>,
  group_public: Vec<u8>,
}

impl Keypair for IssuerKeypair {
  fn public_key(&self) -> &Vec<u8> {
    self.group_public
  }

  fn secret(&self) -> &Vec<u8> {
    self.secret
  }

  pub fn generate(&mut self) -> Result<(), CryptoError> {
    let result = generate_issuer_keypair(self.secret, self.group_public);
    // TOOD(redpig) pass through Err
    if result == false {
      return Err(CryptoError::LowLevelError("generate_issuer_keypair", 1));
    }
    Ok(())
  }
}

*/

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Helper: set up a full issuer + member keypair and credential
    // ========================================================================

    struct TestContext {
        issuer_sk: Vec<u8>,
        group_pk: Vec<u8>,
        member_sk: Vec<u8>,
        member_pk: Vec<u8>,
        nonce: Vec<u8>,
        credential: Vec<u8>,
        credential_sig: Vec<u8>,
    }

    fn setup() -> TestContext {
        setup_with_nonce(b"test-wallet-1")
    }

    fn setup_with_nonce(nonce: &[u8]) -> TestContext {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        assert!(generate_issuer_keypair(&mut issuer_sk, &mut group_pk));

        let mut member_sk = Vec::new();
        let mut member_pk = Vec::new();
        let nonce_vec = nonce.to_vec();
        assert!(generate_wallet_keypair(&nonce_vec, &mut member_sk, &mut member_pk));

        let mut credential = Vec::new();
        let mut credential_sig = Vec::new();
        assert!(issue_credential(
            &member_pk,
            &issuer_sk,
            &nonce_vec,
            &mut credential,
            &mut credential_sig,
        ));

        TestContext {
            issuer_sk,
            group_pk,
            member_sk,
            member_pk,
            nonce: nonce_vec,
            credential,
            credential_sig,
        }
    }

    // ========================================================================
    // Serialization round-trip tests
    // ========================================================================

    #[test]
    fn fr_serialize_roundtrip() {
        for _ in 0..10 {
            let original = random_fr();
            let bytes = serialize_fr(&original);
            let recovered = deserialize_fr(&bytes).unwrap();
            assert_eq!(serialize_fr(&original), serialize_fr(&recovered));
        }
    }

    #[test]
    fn g1_serialize_roundtrip() {
        for _ in 0..5 {
            let point = G1::one() * random_fr();
            let bytes = serialize_g1(&point);
            assert_eq!(bytes.len(), 65);
            assert_eq!(bytes[0], 0x04);
            let recovered = deserialize_g1(&bytes).unwrap();
            // Re-serialize to compare (G1 Eq may not work directly due to Jacobian coords)
            assert_eq!(serialize_g1(&point), serialize_g1(&recovered));
        }
    }

    #[test]
    fn g1_zero_serialize_roundtrip() {
        let zero = G1::zero();
        let bytes = serialize_g1(&zero);
        assert_eq!(bytes[0], 0x04);
        assert!(bytes[1..].iter().all(|&b| b == 0));
        let recovered = deserialize_g1(&bytes).unwrap();
        assert!(recovered.is_zero());
    }

    #[test]
    fn g2_serialize_roundtrip() {
        for _ in 0..5 {
            let point = G2::one() * random_fr();
            let bytes = serialize_g2(&point);
            assert_eq!(bytes.len(), 129);
            assert_eq!(bytes[0], 0x04);
            let recovered = deserialize_g2(&bytes).unwrap();
            assert_eq!(serialize_g2(&point), serialize_g2(&recovered));
        }
    }

    #[test]
    fn g2_zero_serialize_roundtrip() {
        let zero = G2::zero();
        let bytes = serialize_g2(&zero);
        assert_eq!(bytes[0], 0x04);
        assert!(bytes[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn fr_zero_and_one_serialize() {
        let zero = Fr::zero();
        let one = Fr::one();
        let zero_bytes = serialize_fr(&zero);
        let one_bytes = serialize_fr(&one);
        assert!(zero_bytes.iter().all(|&b| b == 0));
        assert_ne!(zero_bytes, one_bytes);
        // one should be 0x00..01
        assert_eq!(one_bytes[31], 1);
        assert!(one_bytes[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn deserialize_g1_rejects_bad_prefix() {
        let mut bytes = serialize_g1(&(G1::one() * random_fr()));
        bytes[0] = 0x02; // wrong prefix
        assert!(deserialize_g1(&bytes).is_none());
    }

    #[test]
    fn deserialize_g1_rejects_short_input() {
        assert!(deserialize_g1(&[0x04; 10]).is_none());
        assert!(deserialize_g1(&[]).is_none());
    }

    #[test]
    fn deserialize_g2_rejects_short_input() {
        assert!(deserialize_g2(&[0x04; 10]).is_none());
        assert!(deserialize_g2(&[]).is_none());
    }

    #[test]
    fn deserialize_fr_rejects_short_input() {
        assert!(deserialize_fr(&[0u8; 16]).is_none());
        assert!(deserialize_fr(&[]).is_none());
    }

    // ========================================================================
    // Hash function tests
    // ========================================================================

    #[test]
    fn hash_to_fr_deterministic() {
        let data = b"deterministic input";
        let h1 = hash_to_fr(data);
        let h2 = hash_to_fr(data);
        assert_eq!(serialize_fr(&h1), serialize_fr(&h2));
    }

    #[test]
    fn hash_to_fr_different_inputs_differ() {
        let h1 = hash_to_fr(b"input A");
        let h2 = hash_to_fr(b"input B");
        assert_ne!(serialize_fr(&h1), serialize_fr(&h2));
    }

    #[test]
    fn hash_to_g1_deterministic() {
        let data = b"deterministic point";
        let p1 = hash_to_g1(data);
        let p2 = hash_to_g1(data);
        assert_eq!(serialize_g1(&p1), serialize_g1(&p2));
    }

    #[test]
    fn hash_to_g1_different_inputs_differ() {
        let p1 = hash_to_g1(b"point A");
        let p2 = hash_to_g1(b"point B");
        assert_ne!(serialize_g1(&p1), serialize_g1(&p2));
    }

    #[test]
    fn hash_to_g1_result_on_curve() {
        // Verify the point satisfies y^2 = x^3 + b by re-serializing and deserializing
        let point = hash_to_g1(b"curve check");
        let bytes = serialize_g1(&point);
        // If AffineG1::new succeeds during deserialization, the point is on the curve
        assert!(deserialize_g1(&bytes).is_some());
    }

    // ========================================================================
    // Key generation tests
    // ========================================================================

    #[test]
    fn issuer_generate_keypair_basic() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        let ret = generate_issuer_keypair(&mut issuer_sk, &mut group_pk);
        assert_eq!(issuer_sk.len(), native::ISSUER_SECRET_KEY_LENGTH);
        assert_eq!(group_pk.len(), native::ISSUER_GROUP_PUBLIC_KEY_LENGTH);
        assert!(ret);
    }

    #[test]
    fn issuer_keypair_unique_each_time() {
        let mut sk1 = Vec::new();
        let mut pk1 = Vec::new();
        let mut sk2 = Vec::new();
        let mut pk2 = Vec::new();
        assert!(generate_issuer_keypair(&mut sk1, &mut pk1));
        assert!(generate_issuer_keypair(&mut sk2, &mut pk2));
        assert_ne!(sk1, sk2);
        assert_ne!(pk1, pk2);
    }

    #[test]
    fn issuer_keypair_gpk_has_valid_g2_points() {
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        assert!(generate_issuer_keypair(&mut sk, &mut pk));
        assert!(gpk_get_x(&pk).is_some());
        assert!(gpk_get_y(&pk).is_some());
    }

    #[test]
    fn wallet_generate_keypair_basic() {
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = b"test-wallet-1".to_vec();
        let ret = generate_wallet_keypair(&id, &mut sk, &mut pk);
        assert_eq!(sk.len(), native::WALLET_SECRET_KEY_LENGTH);
        assert_eq!(pk.len(), native::WALLET_PUBLIC_KEY_LENGTH);
        assert!(ret);
    }

    #[test]
    fn wallet_keypair_empty_nonce_fails() {
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = Vec::new();
        assert!(!generate_wallet_keypair(&id, &mut sk, &mut pk));
    }

    #[test]
    fn wallet_keypair_unique_per_generation() {
        let id = b"same-nonce".to_vec();
        let mut sk1 = Vec::new();
        let mut pk1 = Vec::new();
        let mut sk2 = Vec::new();
        let mut pk2 = Vec::new();
        assert!(generate_wallet_keypair(&id, &mut sk1, &mut pk1));
        assert!(generate_wallet_keypair(&id, &mut sk2, &mut pk2));
        // Even with same nonce, random sk means different outputs
        assert_ne!(sk1, sk2);
        assert_ne!(pk1, pk2);
    }

    #[test]
    fn wallet_pk_contains_valid_g1_point() {
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let id = b"wallet-pk-test".to_vec();
        assert!(generate_wallet_keypair(&id, &mut sk, &mut pk));
        // First 65 bytes is Q (G1 point)
        assert!(member_pk_get_q(&pk).is_some());
        assert!(member_pk_get_c(&pk).is_some());
        assert!(member_pk_get_s(&pk).is_some());
    }

    // ========================================================================
    // Credential issuance tests
    // ========================================================================

    #[test]
    fn new_credential_from_generated_keys() {
        let ctx = setup();
        assert_eq!(ctx.credential.len(), native::CREDENTIAL_LENGTH);
        assert_eq!(ctx.credential_sig.len(), native::CREDENTIAL_SIGNATURE_LENGTH);
    }

    #[test]
    fn credential_contains_valid_g1_points() {
        let ctx = setup();
        assert!(credential_get_a(&ctx.credential).is_some());
        assert!(credential_get_b(&ctx.credential).is_some());
        assert!(credential_get_c(&ctx.credential).is_some());
        assert!(credential_get_d(&ctx.credential).is_some());
        // None should be the identity
        assert!(!credential_get_a(&ctx.credential).unwrap().is_zero());
        assert!(!credential_get_b(&ctx.credential).unwrap().is_zero());
    }

    #[test]
    fn issue_credential_rejects_empty_pk() {
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        assert!(generate_issuer_keypair(&mut issuer_sk, &mut group_pk));
        let empty_pk = Vec::new();
        let nonce = b"test".to_vec();
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        assert!(!issue_credential(&empty_pk, &issuer_sk, &nonce, &mut cred, &mut cred_sig));
    }

    #[test]
    fn issue_credential_rejects_empty_isk() {
        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let nonce = b"test".to_vec();
        assert!(generate_wallet_keypair(&nonce, &mut sk, &mut pk));
        let empty_isk = Vec::new();
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        assert!(!issue_credential(&pk, &empty_isk, &nonce, &mut cred, &mut cred_sig));
    }

    #[test]
    fn issue_credential_rejects_wrong_nonce() {
        // Generate wallet keys with one nonce, issue with another
        let mut issuer_sk = Vec::new();
        let mut group_pk = Vec::new();
        assert!(generate_issuer_keypair(&mut issuer_sk, &mut group_pk));

        let mut sk = Vec::new();
        let mut pk = Vec::new();
        let nonce1 = b"nonce-1".to_vec();
        assert!(generate_wallet_keypair(&nonce1, &mut sk, &mut pk));

        let nonce2 = b"nonce-2".to_vec();
        let mut cred = Vec::new();
        let mut cred_sig = Vec::new();
        // The Schnorr proof in pk is bound to nonce1, so issuing with nonce2 should fail
        assert!(!issue_credential(&pk, &issuer_sk, &nonce2, &mut cred, &mut cred_sig));
    }

    #[test]
    fn credential_pairing_equations_hold() {
        // Directly verify the pairing equations that credential_in_group checks
        let ctx = setup();
        let a = credential_get_a(&ctx.credential).unwrap();
        let b = credential_get_b(&ctx.credential).unwrap();
        let c = credential_get_c(&ctx.credential).unwrap();
        let d = credential_get_d(&ctx.credential).unwrap();
        let gpk_x = gpk_get_x(&ctx.group_pk).unwrap();
        let gpk_y = gpk_get_y(&ctx.group_pk).unwrap();
        let p2 = G2::one();

        // e(A, Y) == e(B, P2)
        assert!(pairing(a, gpk_y) == pairing(b, p2));
        // e(C, P2) == e(A + D, X)
        assert!(pairing(c, p2) == pairing(a + d, gpk_x));
    }

    // ========================================================================
    // Sign and verify tests
    // ========================================================================

    #[test]
    fn sign_and_verify_test() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));
        assert_eq!(sig.len(), native::SIGNATURE_LENGTH);
        assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    #[test]
    fn sign_produces_different_signatures_each_time() {
        let ctx = setup();
        let message = b"same message".to_vec();
        let mut sig1 = Vec::new();
        let mut sig2 = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig1));
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig2));
        // Randomization and random nonce means signatures differ
        assert_ne!(sig1, sig2);
        // But both verify
        assert!(verify(&ctx.group_pk, &None, &None, &sig1, &message));
        assert!(verify(&ctx.group_pk, &None, &None, &sig2, &message));
    }

    #[test]
    fn verify_fails_wrong_message() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let wrong_message = b"goodbye".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));
        assert!(!verify(&ctx.group_pk, &None, &None, &sig, &wrong_message));
    }

    #[test]
    fn verify_fails_wrong_group_key() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));

        let mut other_isk = Vec::new();
        let mut other_gpk = Vec::new();
        assert!(generate_issuer_keypair(&mut other_isk, &mut other_gpk));
        assert!(!verify(&other_gpk, &None, &None, &sig, &message));
    }

    #[test]
    fn verify_fails_tampered_signature() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));

        // Flip a byte in the middle of the signature
        sig[100] ^= 0xff;
        assert!(!verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    #[test]
    fn verify_fails_truncated_signature() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));
        sig.truncate(100);
        assert!(!verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    #[test]
    fn sign_fails_empty_message() {
        let ctx = setup();
        let empty = Vec::new();
        let mut sig = Vec::new();
        assert!(!sign(&empty, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));
    }

    #[test]
    fn sign_fails_empty_credential() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let empty_cred = Vec::new();
        let mut sig = Vec::new();
        assert!(!sign(&message, &empty_cred, &ctx.member_sk, &None, true, &mut sig));
    }

    #[test]
    fn sign_fails_empty_secret_key() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let empty_sk = Vec::new();
        let mut sig = Vec::new();
        assert!(!sign(&message, &ctx.credential, &empty_sk, &None, true, &mut sig));
    }

    #[test]
    fn verify_fails_empty_inputs() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));

        let empty = Vec::new();
        assert!(!verify(&empty, &None, &None, &sig, &message));
        assert!(!verify(&ctx.group_pk, &None, &None, &empty, &message));
        assert!(!verify(&ctx.group_pk, &None, &None, &sig, &empty));
    }

    #[test]
    fn sign_and_verify_large_message() {
        let ctx = setup();
        let message = vec![0xABu8; 10000];
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));
        assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    // ========================================================================
    // Required credential (pre-randomized commitment) tests
    // ========================================================================

    #[test]
    fn sign_and_verify_with_required_cred_test() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let some_cred = Some(ctx.credential.clone());
        let mut sig = Vec::new();

        // Non-randomized: credential in sig matches required
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, false, &mut sig));
        assert!(verify(&ctx.group_pk, &None, &some_cred, &sig, &message));

        // Randomized: credential in sig won't match required
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));
        assert!(!verify(&ctx.group_pk, &None, &some_cred, &sig, &message));
    }

    #[test]
    fn prerandomized_credential_verify_with_required() {
        let ctx = setup();
        let mut r_cred = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r_cred));

        let message = b"pre-randomized test".to_vec();
        let required_cred = Some(r_cred.clone());
        let mut sig = Vec::new();

        // Sign with pre-randomized cred, no further randomization
        assert!(sign(&message, &r_cred, &ctx.member_sk, &None, false, &mut sig));
        // Verify requiring that exact pre-randomized credential
        assert!(verify(&ctx.group_pk, &None, &required_cred, &sig, &message));
    }

    #[test]
    fn prerandomized_credential_fails_wrong_required() {
        let ctx = setup();
        let mut r_cred1 = Vec::new();
        let mut r_cred2 = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r_cred1));
        assert!(randomize_credential(&ctx.credential, &mut r_cred2));
        // Two randomizations produce different credentials
        assert_ne!(r_cred1, r_cred2);

        let message = b"mismatch test".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &r_cred1, &ctx.member_sk, &None, false, &mut sig));
        // Verify with r_cred2 should fail
        assert!(!verify(&ctx.group_pk, &None, &Some(r_cred2), &sig, &message));
    }

    // ========================================================================
    // Basename / pseudonym tests
    // ========================================================================

    #[test]
    fn sign_and_verify_with_basename_test() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let basename = Some(b"5pm on Friday".to_vec());
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &basename, true, &mut sig));
        assert_eq!(sig.len(), native::SIGNATURE_WITH_NYM_LENGTH);
        assert!(verify(&ctx.group_pk, &basename, &None, &sig, &message));
        // Without basename should fail
        assert!(!verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    #[test]
    fn verify_fails_wrong_basename() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let basename1 = Some(b"basename-1".to_vec());
        let basename2 = Some(b"basename-2".to_vec());
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &basename1, true, &mut sig));
        assert!(!verify(&ctx.group_pk, &basename2, &None, &sig, &message));
    }

    #[test]
    fn basename_pseudonym_linkable() {
        // Same member, same basename -> same K point (pseudonym)
        let ctx = setup();
        let basename = Some(b"epoch-42".to_vec());

        let mut sig1 = Vec::new();
        let mut sig2 = Vec::new();
        let msg1 = b"message 1".to_vec();
        let msg2 = b"message 2".to_vec();
        assert!(sign(&msg1, &ctx.credential, &ctx.member_sk, &basename, true, &mut sig1));
        assert!(sign(&msg2, &ctx.credential, &ctx.member_sk, &basename, true, &mut sig2));

        // Extract K from both signatures (bytes 356..421)
        let k1 = &sig1[356..421];
        let k2 = &sig2[356..421];
        assert_eq!(k1, k2, "Same member + same basename should produce same pseudonym");
    }

    #[test]
    fn basename_pseudonym_unlinkable_different_basenames() {
        let ctx = setup();
        let bn1 = Some(b"epoch-1".to_vec());
        let bn2 = Some(b"epoch-2".to_vec());
        let message = b"msg".to_vec();

        let mut sig1 = Vec::new();
        let mut sig2 = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &bn1, true, &mut sig1));
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &bn2, true, &mut sig2));

        let k1 = &sig1[356..421];
        let k2 = &sig2[356..421];
        assert_ne!(k1, k2, "Different basenames should produce different pseudonyms");
    }

    #[test]
    fn basename_pseudonym_differs_per_member() {
        // Issue credentials from the same issuer for both members
        let mut isk = Vec::new();
        let mut gpk = Vec::new();
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let mut cred1 = Vec::new();
        let mut cred1_sig = Vec::new();
        let mut sk1 = Vec::new();
        let mut pk1 = Vec::new();
        let n1 = b"member-1".to_vec();
        assert!(generate_wallet_keypair(&n1, &mut sk1, &mut pk1));
        assert!(issue_credential(&pk1, &isk, &n1, &mut cred1, &mut cred1_sig));

        let mut cred2 = Vec::new();
        let mut cred2_sig = Vec::new();
        let mut sk2 = Vec::new();
        let mut pk2 = Vec::new();
        let n2 = b"member-2".to_vec();
        assert!(generate_wallet_keypair(&n2, &mut sk2, &mut pk2));
        assert!(issue_credential(&pk2, &isk, &n2, &mut cred2, &mut cred2_sig));

        let basename = Some(b"same-basename".to_vec());
        let message = b"msg".to_vec();

        let mut sig1 = Vec::new();
        let mut sig2 = Vec::new();
        assert!(sign(&message, &cred1, &sk1, &basename, true, &mut sig1));
        assert!(sign(&message, &cred2, &sk2, &basename, true, &mut sig2));

        let k1 = &sig1[356..421];
        let k2 = &sig2[356..421];
        assert_ne!(k1, k2, "Different members should have different pseudonyms");
    }

    #[test]
    fn sign_and_verify_with_required_cred_with_basename_test() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let basename = Some(b"5pm on Friday".to_vec());
        let some_cred = Some(ctx.credential.clone());
        let mut sig = Vec::new();

        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &basename, false, &mut sig));
        assert!(verify(&ctx.group_pk, &basename, &some_cred, &sig, &message));
        // Without basename should fail
        assert!(!verify(&ctx.group_pk, &None, &some_cred, &sig, &message));

        // Randomize and required cred should fail
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &basename, true, &mut sig));
        assert!(!verify(&ctx.group_pk, &basename, &some_cred, &sig, &message));
    }

    #[test]
    fn sign_fails_empty_basename() {
        let ctx = setup();
        let message = b"hello".to_vec();
        let empty_bn = Some(Vec::new());
        let mut sig = Vec::new();
        assert!(!sign(&message, &ctx.credential, &ctx.member_sk, &empty_bn, true, &mut sig));
    }

    // ========================================================================
    // Randomize credential tests
    // ========================================================================

    #[test]
    fn randomize_credential_sign_and_verify() {
        let ctx = setup();
        let mut r_cred = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r_cred));

        let message = b"hello".to_vec();
        let some_cred = Some(r_cred.clone());
        let mut sig = Vec::new();
        assert!(sign(&message, &r_cred, &ctx.member_sk, &None, false, &mut sig));
        assert!(verify(&ctx.group_pk, &None, &some_cred, &sig, &message));
    }

    #[test]
    fn randomize_credential_produces_different_output() {
        let ctx = setup();
        let mut r1 = Vec::new();
        let mut r2 = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r1));
        assert!(randomize_credential(&ctx.credential, &mut r2));
        assert_ne!(r1, r2);
        assert_eq!(r1.len(), native::CREDENTIAL_LENGTH);
        assert_eq!(r2.len(), native::CREDENTIAL_LENGTH);
    }

    #[test]
    fn randomize_credential_preserves_length() {
        let ctx = setup();
        let mut r = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r));
        assert_eq!(r.len(), native::CREDENTIAL_LENGTH);
    }

    #[test]
    fn randomize_credential_empty_fails() {
        let empty = Vec::new();
        let mut out = Vec::new();
        assert!(!randomize_credential(&empty, &mut out));
    }

    #[test]
    fn randomize_credential_with_basename_sign_and_verify() {
        let ctx = setup();
        let mut r_cred = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r_cred));

        let message = b"basename with prerandomized".to_vec();
        let basename = Some(b"my-basename".to_vec());
        let mut sig = Vec::new();
        assert!(sign(&message, &r_cred, &ctx.member_sk, &basename, false, &mut sig));
        assert!(verify(&ctx.group_pk, &basename, &None, &sig, &message));
    }

    #[test]
    fn double_randomize_still_valid() {
        let ctx = setup();
        let mut r1 = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r1));
        let mut r2 = Vec::new();
        assert!(randomize_credential(&r1, &mut r2));

        // Should still be in the group
        assert!(credential_in_group(&r2, &ctx.group_pk));

        // Should still sign/verify
        let message = b"double randomized".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &r2, &ctx.member_sk, &None, false, &mut sig));
        assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    // ========================================================================
    // Credential in group tests
    // ========================================================================

    #[test]
    fn issued_credential_in_group() {
        let ctx = setup();
        assert!(credential_in_group(&ctx.credential, &ctx.group_pk));

        let mut other_isk = Vec::new();
        let mut other_gpk = Vec::new();
        assert!(generate_issuer_keypair(&mut other_isk, &mut other_gpk));
        assert!(!credential_in_group(&ctx.credential, &other_gpk));
    }

    #[test]
    fn randomize_credential_in_group() {
        let ctx = setup();
        let mut r_cred = Vec::new();
        assert!(randomize_credential(&ctx.credential, &mut r_cred));
        assert!(credential_in_group(&r_cred, &ctx.group_pk));

        let mut other_isk = Vec::new();
        let mut other_gpk = Vec::new();
        assert!(generate_issuer_keypair(&mut other_isk, &mut other_gpk));
        assert!(!credential_in_group(&r_cred, &other_gpk));
    }

    #[test]
    fn credential_in_group_rejects_empty() {
        let ctx = setup();
        let empty = Vec::new();
        assert!(!credential_in_group(&empty, &ctx.group_pk));
        assert!(!credential_in_group(&ctx.credential, &empty));
        assert!(!credential_in_group(&empty, &empty));
    }

    #[test]
    fn credential_in_group_rejects_short() {
        let ctx = setup();
        let short = vec![0u8; 100];
        assert!(!credential_in_group(&short, &ctx.group_pk));
        assert!(!credential_in_group(&ctx.credential, &short));
    }

    // ========================================================================
    // Deflate / inflate signature tests
    // ========================================================================

    #[test]
    fn deflate_then_inflate_roundtrip() {
        let ctx = setup();
        let message = b"deflate test".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, false, &mut sig));
        let original_sig = sig.clone();
        assert_eq!(sig.len(), native::SIGNATURE_LENGTH);

        // Extract credential before deflation
        let mut cred_from_sig = Vec::new();
        assert!(credential_from_signature(&sig, &mut cred_from_sig));

        // Deflate removes 260 bytes (the credential)
        deflate_signature(&mut sig);
        assert_eq!(sig.len(), native::SIGNATURE_LENGTH - native::CREDENTIAL_LENGTH);

        // Inflate restores them
        inflate_signature(&cred_from_sig, &mut sig);
        assert_eq!(sig.len(), native::SIGNATURE_LENGTH);
        assert_eq!(sig, original_sig);

        // And it still verifies
        assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    #[test]
    fn deflate_then_inflate_with_basename() {
        let ctx = setup();
        let message = b"deflate basename test".to_vec();
        let basename = Some(b"my-basename".to_vec());
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &basename, false, &mut sig));
        let original_sig = sig.clone();
        assert_eq!(sig.len(), native::SIGNATURE_WITH_NYM_LENGTH);

        let mut cred_from_sig = Vec::new();
        assert!(credential_from_signature(&sig, &mut cred_from_sig));

        deflate_signature(&mut sig);
        assert_eq!(sig.len(), native::SIGNATURE_WITH_NYM_LENGTH - native::CREDENTIAL_LENGTH);

        inflate_signature(&cred_from_sig, &mut sig);
        assert_eq!(sig, original_sig);
        assert!(verify(&ctx.group_pk, &basename, &None, &sig, &message));
    }

    #[test]
    fn deflate_short_signature_is_noop() {
        let mut short = vec![0u8; 10];
        let original = short.clone();
        deflate_signature(&mut short);
        assert_eq!(short, original);
    }

    #[test]
    fn credential_from_signature_extracts_correctly() {
        let ctx = setup();
        let message = b"cred extract test".to_vec();
        let mut sig = Vec::new();
        // Sign without randomization so credential in sig == original
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, false, &mut sig));

        let mut extracted = Vec::new();
        assert!(credential_from_signature(&sig, &mut extracted));
        assert_eq!(extracted.len(), native::CREDENTIAL_LENGTH);
        assert_eq!(extracted, ctx.credential);
    }

    #[test]
    fn credential_from_signature_fails_short() {
        let short = vec![0u8; 10];
        let mut cred = Vec::new();
        assert!(!credential_from_signature(&short, &mut cred));
    }

    // ========================================================================
    // Multi-member / cross-group tests
    // ========================================================================

    #[test]
    fn two_members_same_group_both_verify() {
        let mut isk = Vec::new();
        let mut gpk = Vec::new();
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let n1 = b"member-A".to_vec();
        let n2 = b"member-B".to_vec();

        let mut sk1 = Vec::new();
        let mut pk1 = Vec::new();
        assert!(generate_wallet_keypair(&n1, &mut sk1, &mut pk1));
        let mut cred1 = Vec::new();
        let mut cred1_sig = Vec::new();
        assert!(issue_credential(&pk1, &isk, &n1, &mut cred1, &mut cred1_sig));

        let mut sk2 = Vec::new();
        let mut pk2 = Vec::new();
        assert!(generate_wallet_keypair(&n2, &mut sk2, &mut pk2));
        let mut cred2 = Vec::new();
        let mut cred2_sig = Vec::new();
        assert!(issue_credential(&pk2, &isk, &n2, &mut cred2, &mut cred2_sig));

        let message = b"shared message".to_vec();
        let mut sig1 = Vec::new();
        let mut sig2 = Vec::new();
        assert!(sign(&message, &cred1, &sk1, &None, true, &mut sig1));
        assert!(sign(&message, &cred2, &sk2, &None, true, &mut sig2));

        assert!(verify(&gpk, &None, &None, &sig1, &message));
        assert!(verify(&gpk, &None, &None, &sig2, &message));
    }

    #[test]
    fn member_from_one_group_fails_in_another() {
        let ctx1 = setup_with_nonce(b"group1-member");

        let mut isk2 = Vec::new();
        let mut gpk2 = Vec::new();
        assert!(generate_issuer_keypair(&mut isk2, &mut gpk2));

        let message = b"cross group".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx1.credential, &ctx1.member_sk, &None, true, &mut sig));

        // Should verify under group1 but not group2
        assert!(verify(&ctx1.group_pk, &None, &None, &sig, &message));
        assert!(!verify(&gpk2, &None, &None, &sig, &message));
    }

    #[test]
    fn cannot_sign_with_wrong_secret_key() {
        let ctx = setup();
        let message = b"wrong sk".to_vec();

        // Generate a different member's secret key
        let mut wrong_sk = Vec::new();
        let mut wrong_pk = Vec::new();
        let n = b"other-member".to_vec();
        assert!(generate_wallet_keypair(&n, &mut wrong_sk, &mut wrong_pk));

        let mut sig = Vec::new();
        // sign() will succeed (it doesn't check sk against credential)
        assert!(sign(&message, &ctx.credential, &wrong_sk, &None, true, &mut sig));
        // But verify should fail because the Schnorr proof won't match
        assert!(!verify(&ctx.group_pk, &None, &None, &sig, &message));
    }

    // ========================================================================
    // Signature structure tests
    // ========================================================================

    #[test]
    fn signature_length_without_basename() {
        let ctx = setup();
        let message = b"len test".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));
        assert_eq!(sig.len(), native::SIGNATURE_LENGTH);
        // c(32) + s(32) + R(65) + S(65) + T(65) + W(65) + n(32) = 356
        assert_eq!(native::SIGNATURE_LENGTH, 32 + 32 + 65 + 65 + 65 + 65 + 32);
    }

    #[test]
    fn signature_length_with_basename() {
        let ctx = setup();
        let message = b"len test".to_vec();
        let basename = Some(b"bn".to_vec());
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &basename, true, &mut sig));
        assert_eq!(sig.len(), native::SIGNATURE_WITH_NYM_LENGTH);
        // 356 + K(65) = 421
        assert_eq!(native::SIGNATURE_WITH_NYM_LENGTH, native::SIGNATURE_LENGTH + 65);
    }

    #[test]
    fn signature_components_deserialize() {
        let ctx = setup();
        let message = b"components test".to_vec();
        let mut sig = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig));

        assert!(sig_get_c(&sig).is_some());
        assert!(sig_get_s(&sig).is_some());
        assert!(sig_get_r(&sig).is_some());
        assert!(sig_get_s_point(&sig).is_some());
        assert!(sig_get_t(&sig).is_some());
        assert!(sig_get_w(&sig).is_some());
        assert!(sig_get_n(&sig).is_some());

        // R, S, T, W should not be identity
        assert!(!sig_get_r(&sig).unwrap().is_zero());
        assert!(!sig_get_s_point(&sig).unwrap().is_zero());
        assert!(!sig_get_t(&sig).unwrap().is_zero());
        assert!(!sig_get_w(&sig).unwrap().is_zero());
    }

    #[test]
    fn signature_k_only_present_with_basename() {
        let ctx = setup();
        let message = b"k test".to_vec();

        let mut sig_no_bn = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &None, true, &mut sig_no_bn));
        assert!(sig_get_k(&sig_no_bn).is_none());

        let basename = Some(b"present".to_vec());
        let mut sig_bn = Vec::new();
        assert!(sign(&message, &ctx.credential, &ctx.member_sk, &basename, true, &mut sig_bn));
        assert!(sig_get_k(&sig_bn).is_some());
    }

    // ========================================================================
    // Anonymity property tests
    // ========================================================================

    #[test]
    fn signatures_unlinkable_without_basename() {
        // Two signatures from the same member on different messages should have
        // different (R,S,T,W) due to randomization, making them unlinkable
        let ctx = setup();
        let mut sig1 = Vec::new();
        let mut sig2 = Vec::new();
        assert!(sign(&b"msg1".to_vec(), &ctx.credential, &ctx.member_sk, &None, true, &mut sig1));
        assert!(sign(&b"msg2".to_vec(), &ctx.credential, &ctx.member_sk, &None, true, &mut sig2));

        // The credential portion (R,S,T,W) should differ
        let cred_region1 = &sig1[64..324];
        let cred_region2 = &sig2[64..324];
        assert_ne!(cred_region1, cred_region2);
    }

    #[test]
    fn different_members_signatures_both_anonymous() {
        // Two different members' signatures should be indistinguishable structurally
        let mut isk = Vec::new();
        let mut gpk = Vec::new();
        assert!(generate_issuer_keypair(&mut isk, &mut gpk));

        let n1 = b"anon-1".to_vec();
        let n2 = b"anon-2".to_vec();
        let mut sk1 = Vec::new();
        let mut pk1 = Vec::new();
        let mut sk2 = Vec::new();
        let mut pk2 = Vec::new();
        assert!(generate_wallet_keypair(&n1, &mut sk1, &mut pk1));
        assert!(generate_wallet_keypair(&n2, &mut sk2, &mut pk2));

        let mut cred1 = Vec::new();
        let mut cs1 = Vec::new();
        let mut cred2 = Vec::new();
        let mut cs2 = Vec::new();
        assert!(issue_credential(&pk1, &isk, &n1, &mut cred1, &mut cs1));
        assert!(issue_credential(&pk2, &isk, &n2, &mut cred2, &mut cs2));

        let message = b"same message".to_vec();
        let mut sig1 = Vec::new();
        let mut sig2 = Vec::new();
        assert!(sign(&message, &cred1, &sk1, &None, true, &mut sig1));
        assert!(sign(&message, &cred2, &sk2, &None, true, &mut sig2));

        // Both verify under the same group key
        assert!(verify(&gpk, &None, &None, &sig1, &message));
        assert!(verify(&gpk, &None, &None, &sig2, &message));

        // Same length
        assert_eq!(sig1.len(), sig2.len());
    }

    // ========================================================================
    // Constants consistency tests
    // ========================================================================

    #[test]
    fn constants_are_consistent() {
        assert_eq!(native::CREDENTIAL_LENGTH, 4 * 65);
        assert_eq!(native::CREDENTIAL_SIGNATURE_LENGTH, 2 * native::MODBYTES_256_56);
        assert_eq!(native::ISSUER_SECRET_KEY_LENGTH, 2 * native::MODBYTES_256_56);
        assert_eq!(native::ISSUER_GROUP_PUBLIC_KEY_LENGTH, 2 * 129);
        assert_eq!(native::WALLET_SECRET_KEY_LENGTH, native::MODBYTES_256_56);
        assert_eq!(
            native::WALLET_PUBLIC_KEY_LENGTH,
            65 + 3 * native::MODBYTES_256_56
        );
        assert_eq!(
            native::SIGNATURE_LENGTH,
            3 * native::MODBYTES_256_56 + 4 * 65
        );
        assert_eq!(
            native::SIGNATURE_WITH_NYM_LENGTH,
            native::SIGNATURE_LENGTH + 65
        );
    }

    // ========================================================================
    // Split-key signing tests (Brickell & Li style)
    // ========================================================================

    mod split_tests {
        use super::*;
        use crate::v0::split::{MockCard, SmartCard};

        struct SplitTestContext {
            issuer_sk: Vec<u8>,
            group_pk: Vec<u8>,
            host_sk: Vec<u8>,
            member_pk: Vec<u8>,
            nonce: Vec<u8>,
            credential: Vec<u8>,
            #[allow(dead_code)]
            credential_sig: Vec<u8>,
            card: MockCard,
        }

        fn split_setup() -> SplitTestContext {
            split_setup_with_nonce(b"test-split-wallet-1")
        }

        fn split_setup_with_nonce(nonce: &[u8]) -> SplitTestContext {
            let mut issuer_sk = Vec::new();
            let mut group_pk = Vec::new();
            assert!(generate_issuer_keypair(&mut issuer_sk, &mut group_pk));

            let nonce_vec = nonce.to_vec();
            let mut card = MockCard::new();

            let (host_sk, member_pk) =
                split::generate_split_wallet_keypair(&mut card, &nonce_vec)
                    .expect("split keygen failed");

            let mut credential = Vec::new();
            let mut credential_sig = Vec::new();
            assert!(issue_credential(
                &member_pk,
                &issuer_sk,
                &nonce_vec,
                &mut credential,
                &mut credential_sig,
            ));

            SplitTestContext {
                issuer_sk,
                group_pk,
                host_sk,
                member_pk,
                nonce: nonce_vec,
                credential,
                credential_sig,
                card,
            }
        }

        #[test]
        fn split_keygen_produces_valid_public_key() {
            let ctx = split_setup();
            assert_eq!(ctx.member_pk.len(), native::WALLET_PUBLIC_KEY_LENGTH);
            assert_eq!(ctx.host_sk.len(), native::WALLET_SECRET_KEY_LENGTH);
        }

        #[test]
        fn split_keygen_credential_issuance_succeeds() {
            let ctx = split_setup();
            assert_eq!(ctx.credential.len(), native::CREDENTIAL_LENGTH);
            assert_eq!(
                ctx.credential_sig.len(),
                native::CREDENTIAL_SIGNATURE_LENGTH
            );
        }

        #[test]
        fn split_keygen_credential_in_group() {
            let ctx = split_setup();
            assert!(credential_in_group(&ctx.credential, &ctx.group_pk));
        }

        #[test]
        fn split_sign_verify_no_basename() {
            let mut ctx = split_setup();
            let message = b"test transaction".to_vec();
            let mut sig = Vec::new();

            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &message,
                &ctx.credential,
                &None,
                true,
                &mut sig,
            ));

            assert_eq!(sig.len(), native::SIGNATURE_LENGTH);
            assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));
        }

        #[test]
        fn split_sign_verify_with_basename() {
            let mut ctx = split_setup();
            let message = b"test transaction".to_vec();
            let basename = Some(b"test-basename".to_vec());
            let mut sig = Vec::new();

            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &message,
                &ctx.credential,
                &basename,
                true,
                &mut sig,
            ));

            assert_eq!(sig.len(), native::SIGNATURE_WITH_NYM_LENGTH);
            assert!(verify(&ctx.group_pk, &basename, &None, &sig, &message));
        }

        #[test]
        fn split_sign_matches_standard_verify() {
            let mut ctx = split_setup();
            let message = b"payment of 5 tokens".to_vec();
            let mut sig = Vec::new();

            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &message,
                &ctx.credential,
                &None,
                true,
                &mut sig,
            ));

            // Standard verify accepts it
            assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));

            // Wrong message fails
            let wrong = b"payment of 50 tokens".to_vec();
            assert!(!verify(&ctx.group_pk, &None, &None, &sig, &wrong));
        }

        #[test]
        fn split_sign_equivalent_to_combined_sign() {
            let mut ctx = split_setup();
            let message = b"equivalence test".to_vec();

            // Reconstruct combined_sk for equivalence comparison (test-only;
            // in production the combined key never exists in one place).
            let card_sk = deserialize_fr(&ctx.card.secret_key_share()).unwrap();
            let host_sk = deserialize_fr(&ctx.host_sk).unwrap();
            let combined_sk = serialize_fr(&(card_sk + host_sk)).to_vec();

            // Standard sign with combined sk
            let mut std_sig = Vec::new();
            assert!(sign(
                &message,
                &ctx.credential,
                &combined_sk,
                &None,
                true,
                &mut std_sig,
            ));
            assert!(verify(&ctx.group_pk, &None, &None, &std_sig, &message));

            // Split sign
            let mut split_sig = Vec::new();
            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &message,
                &ctx.credential,
                &None,
                true,
                &mut split_sig,
            ));
            assert!(verify(&ctx.group_pk, &None, &None, &split_sig, &message));
        }

        #[test]
        fn split_sign_without_randomization() {
            let mut ctx = split_setup();
            let message = b"no randomize".to_vec();
            let mut sig = Vec::new();

            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &message,
                &ctx.credential,
                &None,
                false,
                &mut sig,
            ));

            assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));
        }

        #[test]
        fn split_sign_multiple_messages() {
            let mut ctx = split_setup();

            for i in 0..5 {
                let message = format!("transaction {}", i).into_bytes();
                let mut sig = Vec::new();

                assert!(split::sign_split(
                    &mut ctx.card,
                    &ctx.host_sk,
                    &message,
                    &ctx.credential,
                    &None,
                    true,
                    &mut sig,
                ));

                assert!(verify(&ctx.group_pk, &None, &None, &sig, &message));
            }
        }

        #[test]
        fn split_sign_host_alone_cannot_sign() {
            let ctx = split_setup();
            let message = b"host only attempt".to_vec();

            let mut sig = Vec::new();
            assert!(sign(
                &message,
                &ctx.credential,
                &ctx.host_sk,
                &None,
                true,
                &mut sig,
            ));

            // host_sk alone != combined_sk, so verify must fail
            assert!(!verify(&ctx.group_pk, &None, &None, &sig, &message));
        }

        #[test]
        fn split_sign_card_alone_cannot_sign() {
            let ctx = split_setup();
            let message = b"card only attempt".to_vec();
            let card_sk = ctx.card.secret_key_share();

            let mut sig = Vec::new();
            assert!(sign(
                &message,
                &ctx.credential,
                &card_sk,
                &None,
                true,
                &mut sig,
            ));

            // card_sk alone != combined_sk, so verify must fail
            assert!(!verify(&ctx.group_pk, &None, &None, &sig, &message));
        }

        #[test]
        fn split_sign_basename_linkability() {
            let mut ctx = split_setup();
            let basename = Some(b"station-A".to_vec());

            let msg1 = b"tap in".to_vec();
            let mut sig1 = Vec::new();
            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &msg1,
                &ctx.credential,
                &basename,
                true,
                &mut sig1,
            ));

            let msg2 = b"tap out".to_vec();
            let mut sig2 = Vec::new();
            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &msg2,
                &ctx.credential,
                &basename,
                true,
                &mut sig2,
            ));

            assert!(verify(&ctx.group_pk, &basename, &None, &sig1, &msg1));
            assert!(verify(&ctx.group_pk, &basename, &None, &sig2, &msg2));

            // K points should be the same (same key, same basename)
            let k1 = sig_get_k(&sig1).unwrap();
            let k2 = sig_get_k(&sig2).unwrap();
            assert_eq!(serialize_g1(&k1), serialize_g1(&k2));
        }

        #[test]
        fn split_sign_different_basenames_unlinkable() {
            let mut ctx = split_setup();

            let bsn1 = Some(b"station-A".to_vec());
            let msg1 = b"tap".to_vec();
            let mut sig1 = Vec::new();
            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &msg1,
                &ctx.credential,
                &bsn1,
                true,
                &mut sig1,
            ));

            let bsn2 = Some(b"station-B".to_vec());
            let msg2 = b"tap".to_vec();
            let mut sig2 = Vec::new();
            assert!(split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &msg2,
                &ctx.credential,
                &bsn2,
                true,
                &mut sig2,
            ));

            // K points should differ (different basenames)
            let k1 = sig_get_k(&sig1).unwrap();
            let k2 = sig_get_k(&sig2).unwrap();
            assert_ne!(serialize_g1(&k1), serialize_g1(&k2));
        }

        #[test]
        fn split_sign_cross_group_rejection() {
            let mut ctx1 = split_setup_with_nonce(b"wallet-group-1");
            let ctx2 = split_setup_with_nonce(b"wallet-group-2");

            let message = b"cross group test".to_vec();
            let mut sig = Vec::new();
            assert!(split::sign_split(
                &mut ctx1.card,
                &ctx1.host_sk,
                &message,
                &ctx1.credential,
                &None,
                true,
                &mut sig,
            ));

            assert!(verify(&ctx1.group_pk, &None, &None, &sig, &message));
            assert!(!verify(&ctx2.group_pk, &None, &None, &sig, &message));
        }

        #[test]
        fn split_sign_empty_message_rejected() {
            let mut ctx = split_setup();
            let mut sig = Vec::new();
            assert!(!split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &Vec::new(),
                &ctx.credential,
                &None,
                true,
                &mut sig,
            ));
        }

        #[test]
        fn split_sign_empty_credential_rejected() {
            let mut ctx = split_setup();
            let message = b"test".to_vec();
            let mut sig = Vec::new();
            assert!(!split::sign_split(
                &mut ctx.card,
                &ctx.host_sk,
                &message,
                &Vec::new(),
                &None,
                true,
                &mut sig,
            ));
        }

        #[test]
        fn mock_card_public_key_share_deterministic() {
            let card = MockCard::from_secret(
                deserialize_fr(&[
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 42,
                ])
                .unwrap(),
            );
            let base = serialize_g1(&G1::one());

            let pk1 = card.public_key_share(&base);
            let pk2 = card.public_key_share(&base);
            assert_eq!(pk1, pk2);
        }

        #[test]
        fn mock_card_sign_commit_consumes_randomness() {
            let mut card = MockCard::new();
            let s = serialize_g1(&(G1::one() * random_fr()));

            // First commit succeeds
            let commit = card.sign_commit(&s, None);
            assert!(commit.is_some());

            // Respond consumes the randomness
            let challenge = serialize_fr(&random_fr());
            let response = card.sign_respond(&challenge);
            assert!(response.is_some());

            // Second respond without new commit fails
            let response2 = card.sign_respond(&challenge);
            assert!(response2.is_none());
        }

        #[test]
        fn split_two_members_same_group() {
            let mut ctx1 = split_setup_with_nonce(b"member-A");

            // Create second member under same issuer
            let nonce2 = b"member-B".to_vec();
            let mut card2 = MockCard::new();
            let (host_sk2, member_pk2) =
                split::generate_split_wallet_keypair(&mut card2, &nonce2).unwrap();

            let mut cred2 = Vec::new();
            let mut cred_sig2 = Vec::new();
            assert!(issue_credential(
                &member_pk2,
                &ctx1.issuer_sk,
                &nonce2,
                &mut cred2,
                &mut cred_sig2,
            ));

            let message = b"same message".to_vec();

            let mut sig1 = Vec::new();
            assert!(split::sign_split(
                &mut ctx1.card,
                &ctx1.host_sk,
                &message,
                &ctx1.credential,
                &None,
                true,
                &mut sig1,
            ));

            let mut card2 = card2;
            let mut sig2 = Vec::new();
            assert!(split::sign_split(
                &mut card2,
                &host_sk2,
                &message,
                &cred2,
                &None,
                true,
                &mut sig2,
            ));

            // Both verify under same group
            assert!(verify(&ctx1.group_pk, &None, &None, &sig1, &message));
            assert!(verify(&ctx1.group_pk, &None, &None, &sig2, &message));

            // Signatures differ (different keys, different randomization)
            assert_ne!(sig1, sig2);
        }
    }
}
