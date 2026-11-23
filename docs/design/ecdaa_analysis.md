# ECDAA Variant Analysis

## Overview

Briolette uses ECDAA (Elliptic Curve Direct Anonymous Attestation) as
the core cryptographic primitive for token transfer authentication. This
document analyzes the specific ECDAA variant implemented, its relationship
to published DAA schemes, and the security properties it provides.

## Lineage

The implementation follows the FIDO ECDAA specification structure, which
itself derives from the Brickell-Chen-Li (BCL) DAA scheme:

- **Original DAA**: Brickell, Camenisch, Chen (2004) — "Direct Anonymous
  Attestation" (CCS 2004)
- **LRSW-DAA**: Based on LRSW signatures (Lysyanskaya-Rivest-Sahai-Wolf)
- **ECDAA**: Chen (2010) — "A DAA Scheme Using Elliptic Curve Cryptography"
  adapted the DAA construction to elliptic curves with pairings
- **FIDO ECDAA**: Camenisch, Drijvers, Lehmann (2017) — Standardized for
  FIDO Alliance with specific Join/Sign/Verify protocols

Briolette's implementation is closest to the FIDO ECDAA construction with
modifications for the basename-based double-spend detection use case.

## Credential Structure

The credential is a tuple (A, B, C, D) of G1 points where:

```
B = hash_to_g1(nonce)          — base point from join nonce
A = B^(1/y)                    — issuer's partial signature
D = Q = B^sk                   — member's public key point
C = (A + D)^x                  — binding element
```

Here (x, y) is the issuer's secret key and sk is the member's secret key.
The group public key is (X, Y) = (P2^x, P2^y) in G2.

**Verification equations** (pairing checks):
1. `e(A, Y) = e(B, P2)` — verifies the A/B relationship under Y
2. `e(C, P2) = e(A + D, X)` — verifies C binds A and D under X

These are standard LRSW-based credential verification equations.

## Signing Protocol

The sign protocol produces a Schnorr proof of knowledge of sk such that
W = S^sk (where (R, S, T, W) is a randomized credential).

### Standard Sign

1. Randomize credential: `(R, S, T, W) = (A^l, B^l, C^l, D^l)` for random l
2. Generate randomness r
3. Compute U = S^r (Schnorr commitment)
4. If basename b is provided:
   - Compute K = hash_to_g1(b)^sk (pseudonym/linkable tag)
   - Compute K_u = hash_to_g1(b)^r
5. Hash: c2 = H(U || S || W || message [|| K_u || K || bsn_base])
6. Nonce n = random, challenge c = H(n || c2)
7. Response s = r + c * sk

### Nonce-then-Hash Construction

The challenge computation `c = H(n || c2)` where `c2 = H(data)` differs
from the standard Fiat-Shamir approach of `c = H(data)` directly. This
serves two purposes:

1. **Replay protection**: The random nonce n ensures each signature is
   unique even for identical (message, credential, basename) inputs.
2. **Simulation soundness**: The nonce prevents an adversary from choosing
   the hash input to satisfy the verification equation without knowing sk.

This is equivalent to the standard Fiat-Shamir transform with the nonce
treated as part of the public coin — the signature includes n, so the
verifier can reconstruct c = H(n || c2) from the signature components.

## Basename Linkability

When a basename b is provided, the signature includes a pseudonym:

```
K = hash_to_g1(b) ^ sk
```

This is deterministic — the same (sk, b) always produces the same K.
In Briolette, the previous transaction signature serves as the basename.
If a token is signed twice to different recipients (double-spend), both
signatures have the same basename and therefore the same K, enabling
detection.

**Security property**: Given two signatures (sig1, sig2) with the same
basename, if K1 = K2 then they were produced by the same secret key.
This follows from the hardness of the CDH problem in G1 — computing
K = bsn_base^sk requires knowledge of sk.

**Privacy property**: Signatures with different basenames produce
unlinkable pseudonyms. Given K1 = bsn1^sk and K2 = bsn2^sk, an
adversary cannot determine whether K1 and K2 came from the same sk
without solving the DDH problem in G1.

## Split-Key Extension

The split-key (Brickell & Li) extension divides sk = card_sk + host_sk:

- **Card operations** (smart card / secure element):
  - G1 scalar multiplication: Q_card = base * card_sk
  - G1 scalar multiplication: U_card = S * r_card
  - Fr arithmetic: s_card = r_card + c * card_sk

- **Host operations**:
  - Same as card, plus combining shares:
  - U = U_card + U_host
  - s = s_card + s_host
  - K = K_card + K_host (for basename)

The card never needs pairings, G2 operations, or GT operations. This
makes it compatible with constrained environments (JavaCard, SIM cards).
The resulting signature is indistinguishable from a standard signature —
the verifier never knows about the split.

**Curve agnosticism**: The SmartCard trait uses opaque byte vectors for
points and scalars. The same trait interface works for both BN254 (v0)
and BLS12-381 (v1) — only the serialization format changes.

## Security Properties

### Anonymity

Given a signature, an adversary cannot determine which group member
produced it, except through basename linkability. This follows from
credential randomization — each signature uses a fresh random l to
produce (R, S, T, W) = (A^l, B^l, C^l, D^l), which is uniformly
distributed in the space of valid randomizations.

### Unforgeability

An adversary who does not hold a valid credential (A, B, C, D) cannot
produce a signature that passes the pairing checks. This reduces to
the hardness of the q-SDH problem on the pairing curve.

### Non-frameability

The issuer, despite knowing the issuer secret key (x, y), cannot produce
a signature that links to an honest member's pseudonym K = bsn_base^sk,
because the issuer does not know sk. This requires that the Join protocol
does not leak sk to the issuer.

**Note**: In the current implementation, the Join protocol for split-key
mode reconstructs combined_sk on the host side for credential issuance.
In a production deployment, this should be replaced with a blind issuance
protocol where the issuer never sees the combined secret key.

### Revocability

The epoch-based group bitfield mechanism provides group-level revocation
without breaking anonymity of non-revoked members. Individual revocation
(linking a specific pseudonym to a NAC identity) requires the operator's
correlation databases — this is intentional for accountability.

## Known Limitations

1. **No formal proof for this exact variant**: While the component
   primitives (LRSW signatures, Schnorr proofs, basename linkability)
   are individually well-studied, the specific composition in this
   implementation has not been formally verified. A formal security
   proof in the Universal Composability (UC) or game-based framework
   would strengthen confidence.

2. **Hash-to-curve in v0**: The v0 implementation uses try-and-increment
   for hash_to_g1, which is non-constant-time. This is a timing side
   channel, primarily relevant for server-side operations. The v1
   implementation uses RFC 9380 compliant hash_to_curve (SWU map).

3. **Hash-to-field bias in v0**: The v0 hash_to_fr uses SHA-256 output
   directly, introducing a negligible bias (~2^-253) in the scalar
   distribution. The v1 implementation uses proper hash-to-field with
   domain separation.

4. **BN254 security level**: The v0 curve (BN254) provides approximately
   100-bit security post Kim-Barbulescu improvements. The v1 curve
   (BLS12-381) provides approximately 128-bit security.

## References

1. Brickell, Camenisch, Chen. "Direct Anonymous Attestation" (CCS 2004)
2. Chen. "A DAA Scheme Using Elliptic Curve Cryptography" (2010)
3. Camenisch, Drijvers, Lehmann. "Universally Composable Direct Anonymous
   Attestation" (PKC 2016)
4. FIDO Alliance. "ECDAA Algorithm" (FIDO ECDAA Spec)
5. RFC 9380. "Hashing to Elliptic Curves" (2023)
6. Brickell, Li. "Enhanced Privacy ID from Bilinear Pairing for Hardware
   Authentication and Attestation" (2010)
