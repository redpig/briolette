# NAC Attestation Architecture

The NAC (Network Access Credential) registrar's primary purpose is to assure
hardware binding of wallet key material. This document describes the current
attestation model, its limitations, and the target architecture.

## Background

ECDAA credentials use BLS12-381 (pairing-friendly) keys. No mainstream
platform keystore supports pairing-friendly curve operations:

- **Android Keystore**: P-256, RSA only (TEE/StrongBox)
- **iOS Secure Enclave**: P-256, Curve25519 only
- **JavaCard / GlobalPlatform**: Vendor-specific; no standard attestation

If platform keystores supported BLS12-381 key generation and signing, the
ECDAA private keys could be generated and used entirely within the secure
hardware, and the platform's built-in attestation would prove it. Until then,
we must use indirect mechanisms.

## Current Model: P-256 Attestation Gates NAC Issuance

The phone generates an attested P-256 key in the platform keystore
(Android Key Attestation / iOS App Attest) and uses it to prove the device
is genuine hardware. The registrar verifies this attestation and selects the
NAC credential group based on the result:

```
Phone generates ECDAA keys (BLS12-381, in software)
Phone generates attested P-256 key (in TEE/StrongBox/Secure Enclave)
  attestation_challenge = SHA-256(hw_id || nac_pk || ttc_pk)

Registrar verifies P-256 attestation:
  No attestation       → LOW tier  → NAC issued from LOW group
  P-256 hw attestation → MEDIUM    → NAC issued from MEDIUM group
  P-256 + split-key    → HIGH      → NAC issued from HIGH group
```

The P-256 attestation cryptographically binds the ECDAA public keys to the
attested hardware via the challenge preimage. This proves:

1. The ECDAA keys were generated on a genuine device (not an emulator)
2. The specific NAC and TTC public keys are bound to this device identity
3. The device had access to TEE/StrongBox/Secure Enclave at key generation time

It does NOT prove the ECDAA keys remain hardware-protected after generation.
The wallet JSON (containing private keys) is encrypted at rest using platform
keystore AES-256-GCM (Android) or iOS Keychain, but this is defense-in-depth,
not cryptographically attested to the registrar.

## Split-Key Enhancement

For HIGH tier, a smartcard (NFC JavaCard) contributes half of each ECDAA key
via a blind Schnorr join protocol. The split-key proof sent to the registrar
contains the card's G1 public key shares (`Q_card_nac`, `Q_card_ttc`).

The registrar verifies:
- Each card share is a valid, non-identity G1 point
- Each card share differs from the combined public key (both parties contributed)

This ensures the ECDAA secret key is split between phone and card, so
compromising the phone alone is insufficient. However, the registrar cannot
verify that the card is genuine secure hardware — a software NFC emulator
could produce the same proof.

## Target Architecture: Manufacturer DAA Credentials

The ideal solution uses ECDAA itself for smartcard attestation. This is
exactly what ECDAA was designed for (TCG Direct Anonymous Attestation for
TPMs), and briolette already has the full ECDAA infrastructure.

### Card Personalization (Manufacturing)

```
Card Manufacturer:
  1. Generate ECDAA issuer keypair: (mfr_sk, mfr_gpk)
  2. Publish mfr_gpk (registrar trusts this)
  3. During card personalization:
     a. Card generates card_sk internally
     b. Manufacturer issues DAA credential to card:
        mfr_credential = Issue(mfr_sk, card_pk)
     c. Card stores: (card_sk, mfr_credential)
```

### Registration with Card Attestation

```
Wallet Registration:
  1. Phone generates ECDAA keys (existing flow)
  2. Phone requests card attestation:
     a. Card signs challenge with mfr_credential (ECDAA signature)
     b. This proves: "I am a genuine [manufacturer] card" anonymously
  3. Phone sends to registrar:
     - ECDAA credential requests (existing)
     - P-256 platform attestation (existing)
     - Card manufacturer attestation (new)
     - Split-key proof (existing)
  4. Registrar verifies card attestation against mfr_gpk
     → Card is genuine hardware from trusted manufacturer
     → Combined with P-256 attestation: both device AND card are genuine
```

### NAC Issuance with Card Attestation

With manufacturer DAA credentials, the registrar can offer two paths for
HIGH-tier NAC issuance:

1. **Card-attested split-key** (recommended): Phone P-256 attestation +
   card manufacturer attestation + split-key proof. The registrar knows
   both the device and card are genuine, and the key is split between them.

2. **Card-primary NAC**: The card's manufacturer credential IS the NAC
   equivalent. The card can directly request NAC issuance using its
   manufacturer credential, without needing a phone attestation at all.
   The phone provides the TTC credential (for offline token transfers),
   protected by platform keystore encryption.

Path 2 is the strongest binding: NAC operations require the card to be
physically present (the card signs with its own key), and the manufacturer
credential proves the card is genuine hardware. The phone is only needed
for TTC (offline payments) and UI.

### Transition Path

```
Today:    P-256 attestation → NAC group selection
          Split-key proof → HIGH tier (structural only)

Near:     P-256 attestation → NAC issuance gate
          Platform keystore encryption → key protection at rest
          Split-key → operational hardware binding

Future:   Card manufacturer DAA credential → card attestation
          Card-attested split-key → strongest hardware binding
          Or: card-primary NAC → card IS the hardware binding
```

### Requirements for Manufacturer DAA

- Card manufacturer generates ECDAA issuer keypair (BLS12-381)
- Manufacturer personalizes each card with a DAA credential
- Registrar is configured with trusted manufacturer GPKs
- Card applet implements ECDAA Sign with manufacturer credential
  (separate from the split-key ECDAA operations)
- The JavaCard BLS12-381 implementation already exists in the
  briolette applet; adding a second credential slot is straightforward

### Alternative: Platform Keystore Pairing-Friendly Curves

If Android Keystore or iOS Secure Enclave were to add support for
BLS12-381 (or any pairing-friendly curve), the ECDAA private keys could
be generated and used entirely within the secure hardware. The platform's
built-in attestation would then directly prove hardware binding of the
ECDAA keys, eliminating the need for indirect P-256 attestation.

This would be the simplest possible architecture:
- No split-key protocol needed for hardware binding
- No manufacturer DAA credentials needed
- Platform attestation directly covers the ECDAA keys
- The registrar's verification is the same as today, just with
  BLS12-381 keys instead of P-256

Until platform vendors add pairing-friendly curve support, the P-256
attestation and split-key/manufacturer-DAA approaches remain necessary.

## Security Level Summary

| Tier | Phone Attestation | Card Proof | Guarantee |
|------|-------------------|------------|-----------|
| LOW | None | None | Keys in software, no hw binding |
| MEDIUM | P-256 (TEE/SE) | None | Device is genuine; keys encrypted at rest |
| HIGH | P-256 (TEE/SE) | Split-key structural | Key split across phone + card |
| HIGH+ | P-256 (TEE/SE) | Manufacturer DAA | Key split + card is genuine hardware |
| Ideal | BLS12-381 in TEE | N/A | Keys never leave hardware |
