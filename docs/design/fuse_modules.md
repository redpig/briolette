# Fuse modules: consumable secure hardware for double-spend prevention

## Summary

This document explores a design where the secure hardware requirement is
shifted from the wallet to a cheap, consumable fuse module. Each fuse contains
a cryptographically verifiable value that is destroyed on read. The wallet
itself becomes commodity, untrusted electronics. Double-spend prevention is
achieved through physics (irreversible fuse destruction) rather than trusted
computation.

## Motivation

The current briolette design relies on secure hardware in the wallet (JavaCard,
ATECC608B, or platform TEE) to enforce that a private key is never used to
sign the same basename twice. This creates several challenges:

- **Vendor lock-in**: Wallet hardware must be trusted, limiting vendors and
  increasing device cost.
- **Attestation complexity**: The registrar must verify hardware attestation
  chains (Android KeyMaster, iOS App Attest, card P-256) to establish trust
  tiers.
- **Attack surface**: A compromised wallet SE allows unlimited double-spending
  until online detection and revocation.
- **Cost**: Secure elements add per-unit cost to every wallet device.

If the security property (one-time use) could be embodied in a cheap,
physically verifiable consumable rather than the wallet itself, wallets become
commodity hardware and the trust model simplifies dramatically.

## Design

### Core concept

A **fuse module** is a small, snap-in hardware module containing an array of
**read-once fuses**. Each fuse stores a cryptographically verifiable value
(e.g., a one-time secret, nonce, or blinding factor). The physical act of
reading a fuse destroys it -- the fuse is blown during readout. This is
analogous to a scratch-off ticket: you cannot read the value without consuming
it.

The wallet reads one fuse per transaction. The cryptographic value from the
fuse is incorporated into the transfer signature, binding the signature to a
physical, non-repeatable event. Since the fuse is destroyed on read, the same
value cannot be used twice. Double-spend prevention becomes a physical property,
not a computational one.

### Fuse module anatomy

```
+--------------------------------------------------+
|  Fuse Module  (snap-in, disposable)               |
|                                                   |
|  [F1] [F2] [F3] [F4] ... [Fn]   (read-once fuses)|
|                                                   |
|  Module ID: unique per module                     |
|  Manufacturer signature over:                     |
|    (module_id, fuse_count, fuse_commitments[])    |
|                                                   |
|  Interface: electrical contact to wallet          |
+--------------------------------------------------+
```

Each fuse `Fi` contains a secret value `vi`. The manufacturer publishes (or
signs) a commitment to each fuse value -- for example, `H(vi)` or a signature
over `vi` -- so that when the wallet reads `vi`, the value is independently
verifiable without contacting the manufacturer.

### Trust model

```
Fuse Manufacturer
  1. Generate fuse values: v1, v2, ..., vn
  2. Compute commitments: c1 = H(v1), c2 = H(v2), ..., cn = H(vn)
  3. Sign module manifest: sig = Sign(mfr_sk, (module_id, n, [c1..cn]))
  4. Provision fuses with values, publish manifest
  5. Physical fuse construction ensures read = destroy

Wallet (untrusted)
  1. Snap in fuse module
  2. Read and verify module manifest (check mfr signature)
  3. Per transaction:
     a. Read next unblown fuse Fi -> vi (fuse is destroyed)
     b. Incorporate vi into transfer signature
     c. Recipient/verifier checks H(vi) against published commitment
  4. When all fuses are blown, snap in a new module
```

The wallet does not need to be trusted. Even a fully compromised wallet cannot
double-spend because the fuse values are physically consumed. The trust anchor
is the fuse manufacturer's guarantee that:

- Each fuse can only be read once (physical property)
- Each fuse value is unique and unpredictable
- The commitments are correctly published

### Failure mode: fuse compromise does not mint money

An important property of this design is that fuse values have no intrinsic
monetary value. They are anti-replay tokens, not currency. If an attacker
could somehow extract a fuse value without destroying it (defeating the
physical guarantee), the result is the ability to sign the same token to
multiple recipients -- a double-spend. It does **not** create new tokens.

Token value is conferred exclusively by the mint's signing authority. The fuse
module gates *transfer count*, not *token value*. The worst case for a
compromised fuse is the same as the worst case for compromised secure hardware
in the current design: double-spending that is detectable online and results
in revocation. The fuse module simply makes that compromise require a physical
attack on the fuse hardware rather than a software/side-channel attack on a
wallet's secure element.

### Visual attestation

A key advantage of physical fuses is that their state can be **visually
inspected**. A counterparty can look at the fuse module and see:

- How many fuses remain (transaction capacity)
- That fuses have been consumed (evidence of prior use)
- That the module is genuine (physical anti-counterfeiting)

This eliminates the need for remote attestation protocols entirely for the
one-time-use property. You can see it.

### Integration with NAC/TTC credential model

The fuse module design complements the NAC/TTC separation:

- **NAC (Network Access Credential)**: Issued by the fuse module vendor at
  manufacture. Allows the vendor to manage devices, push firmware updates to
  non-revoked wallets, and issue new TTCs. Most users never interact with it
  directly.
- **TTC (Token Transfer Credential)**: Issued during personalization or when a
  new fuse module is snapped in. The fuse module purchase is the natural point
  for TTC refresh.
- **Fuse module vendor = NAC relationship**: The entity selling fuse modules
  is the vendor. The NAC gives them a management channel. Different vendors
  can sell compatible fuse modules as long as they are registered with the
  system's fuse manufacturer trust anchors.

Since the wallet is no longer the security boundary, the same secret key
could back both NAC and TTC public keys without the attestation complexity
of proving secure key storage. The fuse module provides the one-time-use
guarantee instead.

### Fuse economics

The fuse module embodies transaction fees physically:

| Module type     | Fuse count | Price   | Per-tx cost |
|-----------------|------------|---------|-------------|
| Micro (casual)  | 20         | $1.00   | $0.050      |
| Standard        | 100        | $3.00   | $0.030      |
| Bulk (merchant) | 500        | $10.00  | $0.020      |

The per-transaction cost is transparent and paid upfront. No hidden fees,
no per-transaction network charges. The module price can also encode the
security tier -- higher-assurance fuse manufacturing processes cost more
but provide stronger read-once guarantees.

## Open questions

### Binding fuse state to cryptographic state

How exactly is the fuse value incorporated into the ECDAA signature? Options:

1. **Fuse value as signing nonce**: The fuse value `vi` replaces the random
   nonce `r` in the ECDAA sign protocol. This binds the signature to the
   physical fuse event but requires the fuse value to have appropriate
   algebraic properties (e.g., uniformly random in the scalar field).

2. **Fuse value as basename salt**: The fuse value is mixed into the basename
   computation: `basename' = H(basename || vi)`. This ensures each signature
   uses a unique basename even for the same token, preventing linkability
   without the fuse value.

3. **Fuse value as one-time signing key**: Each fuse contains a full signing
   key that is used once and destroyed. The corresponding public key is in
   the published commitment. This is the simplest model but requires larger
   fuses.

### Module-to-wallet binding

If fuse modules are freely swappable, an attacker could:
- Read a fuse value, remove the module, insert it in another wallet
- Use the same fuse value with a different wallet identity

Mitigations:
- Module snaps in permanently (tamper-evident, physically hard to remove
  without destroying remaining fuses)
- Module commits to wallet identity on first insertion (burns a binding
  fuse)
- Module ID is cryptographically bound to wallet's NAC at TTC issuance time

### Fuse technology

What physical mechanism provides read-once semantics?

- **Electrical fuses (eFuse)**: Common in silicon. Reading at high current
  blows the fuse. Well-understood, cheap at scale. Not visually attestable
  at fine pitch.
- **Optical fuses**: Fuse state visible under magnification or UV light.
  Combines electrical read-once with visual attestation.
- **Chemical fuses**: Irreversible chemical reaction on read (e.g.,
  electrochromic). State change is visible to naked eye.
- **MEMS fuses**: Micro-mechanical structures that physically break on read.

The ideal fuse technology is cheap to manufacture, impossible to read without
destroying, and visually distinguishable between blown and unblown states.

### Fuse value provisioning security

The fuse manufacturer knows all fuse values at provisioning time. This is
analogous to the mint knowing token signing keys. Mitigations:

- Manufacturer provisions values in a secure facility and destroys records
  after module manufacture (similar to SIM card key provisioning)
- Values are generated by the fuse hardware itself during manufacture using
  a PUF (physically unclonable function), with only commitments extracted
- Split provisioning: two independent manufacturers each contribute half
  the fuse value, neither knows the full value

### Transaction limits

The finite fuse count creates a real transaction limit per module. This is a
feature, not a bug:

- Prevents unbounded offline spending (natural rate limiting)
- Creates a physical reason to periodically interact with a vendor (fuse
  module purchase), enabling TTC refresh and epoch gossip
- Module replacement is the natural point for online validation of
  accumulated tokens

However, the UX must handle the "out of fuses" state gracefully. The wallet
should track remaining fuses and warn before exhaustion. The e-ink display
on a credstick is well suited for showing remaining transaction capacity.
