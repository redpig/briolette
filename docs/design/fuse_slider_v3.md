# V3: Slider-fuse modules with progressive trust

## Summary

This document describes a radical departure from the current briolette
credential model. Instead of binding ECDAA credentials to secure hardware
(v0/v1) or treating fuse modules as simple anti-replay tokens (fuse_modules.md),
v3 **decouples the ECDAA credential from the fuse module** and uses physical
fuse consumption as a **progressive trust signal**. Trust is earned through
verified physical sacrifice, not hardware attestation.

The key insight: a wallet with many blown fuses has proven, through irreversible
physical commitment, that it has participated honestly in many transactions.
A fresh wallet with no history is inherently suspicious.

## Design overview

### Physical form factor: slider-trace-LED

Each fuse module contains a series of physical sliders. Each slider, when
actuated (pushed across), breaks an electrical trace. Breaking the trace:

1. **Irreversibly destroys** the fuse (the trace is physically severed)
2. **Energizes an LED** (or electrochromic indicator) that latches on,
   providing permanent visible evidence that the fuse was consumed
3. **Releases a secret value** stored in the trace (e.g., via a
   sense-on-break circuit that reads out the fuse value during destruction)

The LED serves as persistent memory: you can see at a glance how many
transactions this module has performed. Unlike eFuses that require electrical
probing, the slider-LED design provides **naked-eye attestation**.

```
+--------------------------------------------------------------+
|  Fuse Module v3  (slider array)                              |
|                                                              |
|  [ ]==[LED]  [ ]==[LED]  [ ]==[LED]  [ ]==[LED]  ...        |
|   F1          F2          F3          F4                      |
|  unblown     unblown     unblown     unblown                 |
|                                                              |
|  After 2 transactions:                                       |
|  [X]==[*LED*]  [X]==[*LED*]  [ ]==[LED]  [ ]==[LED]  ...    |
|   F1 blown     F2 blown      F3          F4                  |
|                                                              |
+--------------------------------------------------------------+
```

### Decoupled credentials

In v0/v1, the ECDAA secret key `sk` lives in secure hardware (JavaCard,
ATECC608B, TEE) and the hardware enforces one-signature-per-basename via
a bloom filter. The credential is **hard-bound** to the hardware.

In v3, the ECDAA credential is **not hard-bound** to the fuse module.
The wallet holds `sk` in commodity storage. The fuse module provides:

1. **Per-transaction secrets** (`vi` from each fuse) that are incorporated
   into signatures
2. **Visual proof of consumption** (LEDs)
3. **A module identity** that is cryptographically bound to a credential
   at activation time

The critical question becomes: without hardware enforcement, how do we
prevent credential reuse after a fuse is blown?

### Preventing value reuse

Since the ECDAA key isn't locked in secure hardware, a compromised wallet
could theoretically extract `sk` and sign without consuming a fuse. The
defense is multi-layered:

#### Layer 1: Fuse secret binding

Each fuse `Fi` contains a secret `vi`. The manufacturer publishes commitments
`H(vi)` in the module manifest. A valid transfer signature must incorporate
`vi`:

```
signature = ECDAA.Sign(sk, message, basename, vi)
```

Where `vi` is mixed into the signature computation (e.g., as part of the
nonce derivation or basename salt). Without `vi`, the signature is invalid.
Since reading `vi` destroys the fuse, each `vi` can only be used once by
honest hardware.

**But**: an attacker who intercepts `vi` during readout (before destruction
completes, or via a modified reader) could reuse it. This is bounded by
the physics of the fuse, but not impossible. Hence layer 2.

#### Layer 2: Module tainting on abuse detection

When double-spend is detected (same `vi` used in two different signatures),
the **entire fuse module's credential is revoked**:

- The module's ECDAA credential (bound at activation) is added to the
  revocation bitfield
- **All remaining unblown fuses become worthless** -- peers reject any
  signature from this module's credential
- The attacker loses `(n - k)` remaining transaction slots, where `k`
  fuses were consumed before the abuse

This makes the cost of a single double-spend:

```
cost_of_attack = module_price + (remaining_fuses * per_tx_value)
```

For a 100-fuse module at $3.00, double-spending on fuse #5 wastes 95
remaining fuses ($2.85 in transaction capacity). Double-spending on
fuse #95 wastes only 5 fuses ($0.15) -- but by fuse #95, the module has
accumulated significant trust through proven honest transactions.

#### Layer 3: Progressive trust limits

Peers enforce transaction limits based on the module's proven history:

| Fuses consumed | Trust tier | Max transaction value |
|----------------|------------|-----------------------|
| 0-5            | UNTRUSTED  | $1.00                 |
| 6-20           | LOW        | $10.00                |
| 21-50          | MEDIUM     | $50.00                |
| 51+            | HIGH       | $200.00               |

A fresh wallet with zero blown fuses can only transact in small amounts.
Trust is earned through physical, irreversible, verifiable commitment.

**Fresh wallets are suspicious by design.** This is the inverse of
traditional systems where a new device is "clean." Here, a new module has
no track record. It must build trust transaction by transaction, fuse by
fuse.

### Double-spend bound

An attacker can double-spend **at most once per fuse module**. After the
first detected double-spend:

1. The module's credential is tainted (revoked via epoch update)
2. The revocation propagates through gossip
3. All remaining fuses on the module are worthless
4. The attacker must acquire a new module to try again

Since fresh modules start at UNTRUSTED tier, the attacker can only
double-spend on low-value transactions. To double-spend on high-value
transactions, the attacker must first consume many fuses honestly (building
trust), making each attack expensive in both module cost and forfeited
transaction capacity.

### Credential lifecycle

```
1. Purchase fuse module
   └─ Module contains: n fuses, manufacturer manifest, module_id

2. Activate module in wallet
   └─ Wallet generates ECDAA keypair (sk, Q)
   └─ First fuse (F0) is burned as binding fuse:
      v0 is mixed into credential request to registrar
   └─ Registrar issues NAC + TTC bound to (module_id, Q, H(v0))
   └─ Module is now cryptographically bound to this credential

3. Transact (repeat for each fuse)
   └─ Wallet reads fuse Fi → vi (fuse destroyed, LED lit)
   └─ Sign transfer: sig = ECDAA.Sign(sk, msg, basename, vi)
   └─ Recipient verifies: sig valid ∧ H(vi) in manifest ∧ vi not seen before
   └─ Trust tier determined by number of lit LEDs (blown fuses)

4. Module exhausted (all fuses blown)
   └─ All LEDs lit -- visual confirmation
   └─ Wallet prompts for new module
   └─ Old module credential expires naturally (ticket expiration)
   └─ New module → new credential → trust resets to UNTRUSTED

5. Abuse detected
   └─ Same vi in two signatures → module credential revoked
   └─ All remaining fuses worthless
   └─ Wallet must acquire new module, starts over at UNTRUSTED
```

### Fuse credential for revocation and abuse detection

Since the ECDAA credential is not hard-bound, the **fused credential**
(the combination of module identity + consumed fuse values) serves as the
revocation anchor:

- **Revocation target**: The module_id + ECDAA public key, bound at
  activation, identifies the credential to revoke. This goes into the
  epoch update bitfield.

- **Abuse evidence**: Each transaction includes `H(vi)` as a tag. On
  double-spend detection, the two conflicting signatures both contain the
  same `vi`, proving they came from the same fuse module. The module_id
  is the revocation key.

- **Module tainting**: Revoking the module's credential effectively
  "taints" the entire fuse module. The physical module still has unblown
  fuses, but they are cryptographically useless because no peer will
  accept signatures from the revoked credential. The module becomes
  dead hardware.

- **Taint propagation**: If a manufacturer's modules show a pattern of
  abuse (manufacturing defect allowing non-destructive reads), the
  manufacturer's signing key can be revoked, tainting **all modules from
  that manufacturer**. This is analogous to revoking a certificate
  authority.

### Comparison with v0/v1 and fuse_modules.md

| Property | v0/v1 (secure HW) | Fuse modules (v2) | Slider-fuse v3 |
|----------|-------------------|-------------------|----------------|
| Trust anchor | Secure element | Fuse physics | Fuse physics + history |
| Credential binding | Hard (key in SE) | Hard (key in fuse) | Soft (key in wallet) |
| Double-spend bound | Until revocation | 1 per fuse value | 1 per module |
| Fresh device trust | Attestation-based | Same as v0 | Untrusted (must earn) |
| Visual attestation | None | Fuse count | LED array |
| Attack cost | Compromise SE | Compromise fuse | Module + lost fuses |
| Wallet trust | Trusted (SE) | Untrusted | Untrusted |
| Progressive trust | No | No | Yes |

### Open questions

#### Trust transfer between modules

When a user exhausts a module and installs a new one, trust resets to
UNTRUSTED. Should there be a mechanism to carry forward reputation?
Options:

- **No carry-forward**: Each module starts fresh. Simple, conservative.
- **Credential chain**: New module's activation signature includes old
  module's last fuse value, creating a chain. Verifiable history without
  identity linkage.
- **Registrar vouching**: Registrar issues new credential with a trust
  bonus based on the old module's clean record.

#### Module counterfeiting

A fake module could claim fuses are blown (LEDs lit) without actually
having consumed real fuse secrets. Mitigations:

- Manufacturer signs the commitment list; verifier checks signatures
- Each blown fuse's `vi` is recorded on-chain/in-token-history
- Physical anti-counterfeiting on the module itself (holograms,
  tamper-evident packaging, PUF-based module authentication)

#### Interaction with split-key protocol

In v0/v1, the split-key protocol divides `sk` between a smartcard and
host. In v3, since the wallet is untrusted, split-key may not apply
in the same way. However, the fuse module could hold `card_sk` and
release `partial_signature_i` with each fuse blow, combining the
split-key and fuse concepts. This would make the fuse module an active
participant in signing rather than just a secret store.

#### Minimum fuse count for meaningful trust

What's the minimum number of honest transactions before a module
is "trusted enough" for substantial transactions? This depends on:

- The cost/benefit ratio for an attacker
- The value distribution of typical transactions
- The ecosystem's risk tolerance

The progressive trust table above is illustrative; the actual thresholds
should be determined by economic modeling.

#### Peer-to-peer trust negotiation

In an offline transaction, the recipient inspects the sender's module.
How does the recipient verify:

- That the LEDs are genuine (not faked by the wallet electronics)?
- That the blown fuses correspond to actual committed values?
- That the module hasn't been tampered with?

This may require the module to have a minimal secure element just for
module authentication (signing its own state), even though the ECDAA
key is not hardware-bound.
