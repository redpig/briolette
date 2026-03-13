//! ECDAA split-key signing for the credstick.
//!
//! The credstick holds one half of the ECDAA secret key (card_sk). The phone
//! holds the other half (host_sk). Signatures require both halves, so a stolen
//! credstick alone cannot forge tokens.
//!
//! This module implements the card-side operations from the split-key protocol:
//! - `sign_commit`: Generate ephemeral randomness and compute card's commitment
//! - `sign_respond`: Compute card's signature share s_card = r_card + c * card_sk
//! - `join_commit` / `join_respond`: Blind join protocol for credential issuance
//!
//! Uses BLS12-381 (v1) curve — same as briolette-crypto::v1 but compiled for
//! Cortex-M4F. G1 scalar multiplication is the dominant cost (~2s at 64MHz).
//!
//! The credstick does NOT perform pairings (G2/GT operations). Those happen on
//! the phone/host side. The card only needs:
//! - G1 scalar multiplication (sign_commit, join_commit)
//! - Scalar multiply-and-add (sign_respond, join_respond)
//! - SHA-256 hashing (bloom filter)

use heapless::Vec;

use crate::bloom::BloomFilter;

/// BLS12-381 field sizes.
pub mod sizes {
    /// Scalar field element (Fr): 32 bytes.
    pub const SCALAR: usize = 32;
    /// G1 point (compressed): 48 bytes.
    pub const G1: usize = 48;
}

/// Card-side ECDAA secret key (half of the split key).
/// Stored in nRF52840 flash, protected by APPROTECT fuse.
pub struct CardSecretKey {
    /// The secret scalar sk_card (32 bytes, BLS12-381 Fr).
    sk: [u8; sizes::SCALAR],
}

impl CardSecretKey {
    pub fn from_bytes(bytes: &[u8; sizes::SCALAR]) -> Self {
        Self { sk: *bytes }
    }

    pub fn as_bytes(&self) -> &[u8; sizes::SCALAR] {
        &self.sk
    }
}

/// Ephemeral state for a signing session.
/// The card generates r_card during sign_commit and uses it in sign_respond.
struct SignSession {
    /// Ephemeral random scalar.
    r_card: [u8; sizes::SCALAR],
}

/// Sign commitment result (returned from sign_commit).
pub struct SignCommitment {
    /// U_card = S * r_card (G1 point, 48 bytes compressed).
    pub u_card: [u8; sizes::G1],
    /// K_card = basename_base * card_sk (if basename provided).
    pub k_card: Option<[u8; sizes::G1]>,
    /// K_u_card = basename_base * r_card (if basename provided).
    pub k_u_card: Option<[u8; sizes::G1]>,
}

/// Active signing session (persists in RAM between APDU exchanges).
static mut SIGN_SESSION: Option<SignSession> = None;

/// Compute the card's public key share: Q_card = base * card_sk.
pub fn public_key_share(base: &[u8], card_sk: &CardSecretKey) -> Option<[u8; sizes::G1]> {
    // BLS12-381 G1 scalar multiplication.
    // On Cortex-M4F at 64MHz: ~2 seconds.
    //
    // Uses bls12_381_plus in no_std mode:
    //   let base_point = G1Affine::from_compressed(base)?;
    //   let sk = Scalar::from_be_bytes(card_sk.as_bytes())?;
    //   let result = G1Projective::from(base_point) * sk;
    //   Some(G1Affine::from(result).to_compressed())

    if base.len() < sizes::G1 {
        return None;
    }

    // TODO: Implement with actual BLS12-381 scalar multiplication.
    // Placeholder: return zeros (will be replaced with real crypto).
    let result = [0u8; sizes::G1];
    Some(result)
}

/// Phase 1 of split-key signing: generate commitment.
///
/// Inputs:
/// - s_point: The randomized credential point S (G1, 48 bytes)
/// - basename_base: Optional basename base point for linkable signatures
/// - card_sk: The card's secret key half
///
/// Outputs:
/// - U_card = S * r_card
/// - K_card = basename_base * card_sk (if basename)
/// - K_u_card = basename_base * r_card (if basename)
///
/// Stores r_card in session for sign_respond.
pub fn sign_commit(
    s_point: &[u8],
    basename_base: Option<&[u8]>,
    card_sk: &CardSecretKey,
    bloom: &mut BloomFilter,
) -> Option<SignCommitment> {
    if s_point.len() < sizes::G1 {
        return None;
    }

    // Check bloom filter for basename (double-spend prevention).
    if let Some(bsn) = basename_base {
        if bloom.check_and_add(bsn) {
            defmt::warn!("ECDAA: basename already used in this epoch!");
            return None;
        }
    }

    // Generate ephemeral random scalar r_card.
    // On nRF52840: use the hardware RNG (TRNG).
    let mut r_card = [0u8; sizes::SCALAR];
    generate_random(&mut r_card);

    // Compute U_card = S * r_card (G1 scalar mul, ~2s).
    // TODO: real BLS12-381 computation.
    let u_card = [0u8; sizes::G1]; // placeholder

    // Compute basename-linked values if basename provided.
    let (k_card, k_u_card) = if let Some(_bsn_base) = basename_base {
        // K_card = basename_base * card_sk
        // K_u_card = basename_base * r_card
        // TODO: real BLS12-381 computation.
        (Some([0u8; sizes::G1]), Some([0u8; sizes::G1]))
    } else {
        (None, None)
    };

    // Store session state.
    unsafe {
        SIGN_SESSION = Some(SignSession { r_card });
    }

    Some(SignCommitment {
        u_card,
        k_card,
        k_u_card,
    })
}

/// Phase 2 of split-key signing: compute response.
///
/// Input: challenge scalar c (32 bytes, from host).
/// Output: s_card = r_card + c * card_sk (scalar, 32 bytes).
///
/// Consumes the session state (r_card is cleared).
pub fn sign_respond(
    challenge: &[u8],
    card_sk: &CardSecretKey,
) -> Option<[u8; sizes::SCALAR]> {
    if challenge.len() < sizes::SCALAR {
        return None;
    }

    let session = unsafe { SIGN_SESSION.take()? };

    // s_card = r_card + c * card_sk (mod scalar order).
    // This is just scalar arithmetic, very fast even on Cortex-M4.
    //
    // TODO: real BLS12-381 scalar arithmetic:
    //   let r = Scalar::from_be_bytes(&session.r_card)?;
    //   let c = Scalar::from_be_bytes(challenge)?;
    //   let sk = Scalar::from_be_bytes(card_sk.as_bytes())?;
    //   let s_card = r + c * sk;
    //   Some(s_card.to_be_bytes())

    let s_card = [0u8; sizes::SCALAR]; // placeholder
    Some(s_card)
}

/// Phase 1 of blind join: compute U_card = B * r_card.
pub fn join_commit(base: &[u8]) -> Option<([u8; sizes::G1], [u8; sizes::SCALAR])> {
    if base.len() < sizes::G1 {
        return None;
    }

    let mut r_card = [0u8; sizes::SCALAR];
    generate_random(&mut r_card);

    // U_card = B * r_card (G1 scalar mul).
    // TODO: real computation.
    let u_card = [0u8; sizes::G1]; // placeholder

    // Store r_card for join_respond.
    unsafe {
        SIGN_SESSION = Some(SignSession { r_card });
    }

    Some((u_card, r_card))
}

/// Phase 2 of blind join: compute s_card = r_card + c * card_sk.
pub fn join_respond(
    challenge: &[u8],
    card_sk: &CardSecretKey,
) -> Option<[u8; sizes::SCALAR]> {
    // Same computation as sign_respond.
    sign_respond(challenge, card_sk)
}

/// Sign tokens for a TRANSFER operation.
///
/// For each token in the proposal, perform the split-key ECDAA signing
/// protocol (commit + respond in one shot, since both card_sk and challenge
/// computation happen locally on the credstick).
///
/// In the full split-key protocol, the host computes the challenge. But for
/// credstick-only signing (non-split mode), we compute it locally.
///
/// Returns serialized signatures.
pub fn sign_tokens(
    proposed_tokens: &[u8],
    card_sk: &[u8],
    bloom: &mut BloomFilter,
) -> Option<Vec<u8, 2048>> {
    if card_sk.len() < sizes::SCALAR {
        return None;
    }

    let sk = CardSecretKey::from_bytes(card_sk.try_into().ok()?);

    // Parse token count from the first 4 bytes.
    if proposed_tokens.len() < 4 {
        return None;
    }
    let count = u32::from_be_bytes([
        proposed_tokens[0],
        proposed_tokens[1],
        proposed_tokens[2],
        proposed_tokens[3],
    ]) as usize;

    let mut signatures: Vec<u8, 2048> = Vec::new();

    // For each token, extract the signing point and basename, then sign.
    let mut offset = 4;
    for _i in 0..count {
        if offset + sizes::G1 > proposed_tokens.len() {
            return None;
        }

        let s_point = &proposed_tokens[offset..offset + sizes::G1];
        offset += sizes::G1;

        // Extract basename if present.
        let basename = if offset + sizes::G1 <= proposed_tokens.len() {
            let bsn = &proposed_tokens[offset..offset + sizes::G1];
            offset += sizes::G1;
            Some(bsn)
        } else {
            None
        };

        // Sign commit.
        let commitment = sign_commit(s_point, basename, &sk, bloom)?;
        signatures.extend_from_slice(&commitment.u_card).ok();
        if let Some(ref k) = commitment.k_card {
            signatures.extend_from_slice(k).ok();
        }
        if let Some(ref ku) = commitment.k_u_card {
            signatures.extend_from_slice(ku).ok();
        }

        // For credstick-local signing, compute challenge and respond.
        // TODO: In split-key mode, the challenge comes from the phone.
        // For now, compute a local challenge (non-split mode).
        let challenge = compute_local_challenge(&commitment, s_point, basename);
        let s_card = sign_respond(&challenge, &sk)?;
        signatures.extend_from_slice(&s_card).ok();
    }

    Some(signatures)
}

/// Compute a signing challenge locally (for non-split-key mode).
///
/// In the full protocol, the host computes c = H(combined commitment values).
/// For credstick-only signing, we compute it locally using the same hash.
fn compute_local_challenge(
    commitment: &SignCommitment,
    s_point: &[u8],
    _basename: Option<&[u8]>,
) -> [u8; sizes::SCALAR] {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(s_point);
    hasher.update(&commitment.u_card);
    if let Some(ref k) = commitment.k_card {
        hasher.update(k);
    }
    if let Some(ref ku) = commitment.k_u_card {
        hasher.update(ku);
    }
    let hash = hasher.finalize();

    let mut result = [0u8; sizes::SCALAR];
    result.copy_from_slice(&hash[..sizes::SCALAR]);
    result
}

/// Generate random bytes using the nRF52840 hardware TRNG.
fn generate_random(buf: &mut [u8]) {
    // The nRF52840 has a hardware True Random Number Generator (RNG).
    // Access via PAC:
    //
    // let rng = unsafe { &*pac::RNG::ptr() };
    // rng.tasks_start.write(|w| w.bits(1));
    // for byte in buf.iter_mut() {
    //     while rng.events_valrdy.read().bits() == 0 {}
    //     *byte = rng.value.read().bits() as u8;
    //     rng.events_valrdy.write(|w| w.bits(0));
    // }
    // rng.tasks_stop.write(|w| w.bits(1));

    // TODO: Use actual hardware RNG. Placeholder: zeros (INSECURE).
    for byte in buf.iter_mut() {
        *byte = 0;
    }
}
