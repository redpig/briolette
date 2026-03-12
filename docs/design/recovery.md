# Wallet Recovery Design

## Overview

When a wallet is lost or destroyed, the tokens it held become inaccessible.
Because briolette tokens have soft expirations (`valid_until`) and tickets
have epoch-based lifetimes, a lost wallet's holdings will eventually expire
out of circulation. The recovery system allows the original owner to reclaim
value from expired tokens by proving prior ownership through a pre-established
cryptographic binding.

This document specifies the proof-of-binding protocol, the recovery server
architecture, the end-to-end recovery flow, and the security properties that
prevent abuse.

## Design Principles

1. **Recovery is opt-in.** Wallets must register a binding before loss occurs.
   No binding means no recovery — the value is absorbed by the operator.

2. **Recovery follows expiry.** Tokens and tickets must expire before recovery
   is eligible. This prevents interference with honest holders who may
   currently hold or have recently received the tokens.

3. **Old wallet is revoked.** After recovery completes, the lost wallet's
   credential group is revoked. This prevents the "lost" wallet from
   resurfacing and double-spending recovered tokens.

4. **No new minting authority.** The recovery server does not mint tokens. It
   works like the swap service: it holds a pool of pre-withdrawn tokens and
   draws from the mint or a treasury service as needed.

5. **Conservation of value.** The total token value in circulation does not
   change. Expired tokens are retired; replacement tokens are issued from the
   mint's supply.

## Proof-of-Binding Protocol

### Binding Structure

A wallet pre-registers a **recovery binding** with the recovery server:

```
RecoveryBinding {
    ttc_public_key    : bytes         // Wallet's TTC ECDAA public key (Q point)
    delegate_type     : DelegateType  // ECDSA_P256 or TTC_ECDAA
    delegate_key      : bytes         // Delegate's public key
    valid_until       : uint64        // Binding expiration (epoch timestamp)
}
```

The binding is authenticated by a NAC signature (basename = current epoch)
over the serialized `RecoveryBinding`, proving the wallet holds a valid
network access credential at binding time.

### Delegate Types

Two delegate models are supported, selectable via the `DelegateType` enum:

**ECDSA P-256 (offline backup key):**
- The wallet generates or imports a standard P-256 keypair as delegate.
- The public key is stored in the binding; the private key is kept offline
  (printed QR code, safety deposit box, institutional custody).
- Recovery proof: ECDSA signature over `SHA-256(old_ttc_public_key || timestamp)`.
- Best for: individual users who want a cold backup key.

**TTC ECDAA (peer wallet recovery):**
- Another briolette wallet's TTC public key acts as delegate.
- Enables peer-to-peer recovery: a family member, business partner, or
  institutional custodian can recover on behalf of the lost wallet.
- Recovery proof: ECDAA signature with basename = `SHA-256(old_ttc_public_key || timestamp)`.
- Best for: users who want another wallet to act as recovery agent.

### Why TTC Public Key?

The TTC Q point is the wallet's persistent identity across randomized
tickets. While individual tickets use randomized credentials (unlinkable
across tickets), the TTC Q point is the underlying key material that
generated them. The recovery server stores bindings indexed by a SHA-256
hash of the TTC public key, enabling lookup without exposing the key
directly.

### Why NAC Signature?

The NAC signature proves the binding was created by a wallet in good standing
(valid network credential, not revoked). Using `basename = current_epoch`
ensures:
- Bindings are linkable within the same epoch (rate-limiting: the recovery
  server can detect if the same NAC credential creates multiple bindings
  in one epoch).
- Bindings from different epochs are unlinkable (privacy).
- The recovery server can verify the signature against the epoch's NAC group
  public keys without knowing which specific wallet created it.

### Binding Lifecycle

```
  register_binding()          refresh_binding()
        │                          │
        ▼                          ▼
   ┌─────────┐              ┌─────────┐
   │  Active  │──(refresh)──▶│  Active  │
   └─────────┘              └─────────┘
        │                         │
        ├──(valid_until passes)───┤
        ▼                         ▼
   ┌─────────┐              ┌─────────┐
   │ Expired  │              │ Revoked  │◀──revoke_binding()
   └─────────┘              └─────────┘
        │
        ├──(cannot be used for recovery)
        │
   ┌──────────┐
   │ Consumed  │◀──recover_tokens() succeeds
   └──────────┘
```

- **Active**: binding can be used for recovery claims.
- **Expired**: binding has passed `valid_until`. Cannot be used. Wallet must
  register a new binding.
- **Revoked**: wallet explicitly cancelled the binding. Cannot be used.
- **Consumed**: recovery was completed. Binding is permanently retired.

## Recovery Server Architecture

### Trust Model

The recovery server is an operator-controlled service with the following
capabilities and limitations:

**Has access to:**
- TokenMap (read): find tokens by last holder credential
- Validate service: verify token chain integrity
- Clerk/epoch system: request revocation of old wallet's credential group
- Mint or treasury: draw replacement tokens

**Does NOT have:**
- Mint signing keys (cannot create tokens ex nihilo)
- Wallet private keys (cannot spend anyone's tokens)
- Token transfer authority (uses swap-like pattern via its own wallet)

**Deployment:**
The recovery server runs its own registered wallet (like the swap service).
During initialization, it:
1. Registers with the registrar
2. Obtains tickets from the clerk
3. Withdraws tokens from the mint (pre-filling a pool)
4. Begins serving RPCs

### Service Interface

```
service Recovery {
    rpc RegisterBinding   (RegisterBindingRequest)   returns (RegisterBindingReply);
    rpc RefreshBinding    (RefreshBindingRequest)     returns (RefreshBindingReply);
    rpc RevokeBinding     (RevokeBindingRequest)      returns (RevokeBindingReply);
    rpc RecoverTokens     (RecoverTokensRequest)      returns (RecoverTokensReply);
    rpc GetBindingStatus  (GetBindingStatusRequest)   returns (GetBindingStatusReply);
}
```

### Persistent State

The recovery server maintains a SQLite database with:

**`bindings` table:**
- `id` (random 32 bytes, primary key)
- `ttc_public_key_hash` (SHA-256 of TTC Q point, indexed)
- `ttc_public_key` (full key for verification)
- `delegate_type` (ECDSA_P256 or TTC_ECDAA)
- `delegate_key` (delegate's public key)
- `valid_until` (epoch timestamp)
- `state` (active / expired / revoked / consumed)
- `created_epoch` (epoch when binding was registered)
- `nac_basename` (for rate-limit tracking within epoch)

**`recovery_log` table:**
- `id` (auto-increment)
- `binding_id` (foreign key → bindings)
- `token_base_sig` (original expired token's base signature)
- `original_value` (whole, fractional, currency)
- `new_token_base_sig` (replacement token's base signature)
- `recovered_epoch` (epoch when recovery was executed)

## End-to-End Recovery Flow

### Phase 1: Pre-Loss Binding Registration

```
                    Wallet                Recovery Server
                      │                        │
   1. Generate or     │                        │
      import delegate │                        │
      key             │                        │
                      │                        │
   2. Create          │  RegisterBinding       │
      RecoveryBinding │───────────────────────▶│
      + NAC signature │                        │ 3. Verify NAC sig
                      │                        │    against epoch keys
                      │  RegisterBindingReply  │
                      │◀───────────────────────│ 4. Store binding
                      │  (binding_id)          │    Return ID
                      │                        │
   5. Store binding_id│                        │
      in wallet data  │                        │
```

**Periodic refresh:** Wallet calls `RefreshBinding` before `valid_until`
to extend the binding's lifetime, similar to ticket refresh. The refresh
requires a fresh NAC signature (proving continued good standing).

### Phase 2: Post-Loss Recovery

```
   Delegate (new wallet)     Recovery Server        TokenMap       Mint/Treasury
         │                        │                    │                │
   1. Create                      │                    │                │
      RecoverTokensRequest        │                    │                │
      + delegate proof            │                    │                │
      + new wallet ticket         │                    │                │
         │  RecoverTokens         │                    │                │
         │───────────────────────▶│                    │                │
         │                        │                    │                │
         │                   2. Look up binding        │                │
         │                      by ttc_public_key_hash │                │
         │                                             │                │
         │                   3. Verify delegate        │                │
         │                      proof matches          │                │
         │                      stored delegate_key    │                │
         │                        │                    │                │
         │                   4.   │  FindByHolder      │                │
         │                        │───────────────────▶│                │
         │                        │  (expired tokens)  │                │
         │                        │◀───────────────────│                │
         │                        │                    │                │
         │                   5. For each token:        │                │
         │                      - verify valid_until   │                │
         │                        has passed           │                │
         │                      - verify ticket expired│                │
         │                      - check not revoked    │                │
         │                      - check past cooloff   │                │
         │                        │                    │                │
         │                   6.   │                    │   Withdraw     │
         │                        │────────────────────│──────────────▶│
         │                        │  matching values   │               │
         │                        │◀───────────────────│───────────────│
         │                        │                    │                │
         │                   7. Transfer replacement   │                │
         │                      tokens to new wallet   │                │
         │                      ticket                 │                │
         │                        │                    │                │
         │                   8. Request revocation of  │                │
         │                      old wallet's group     │                │
         │                        │                    │                │
         │                   9. Mark binding consumed  │                │
         │                      Log recovery           │                │
         │  RecoverTokensReply    │                    │                │
         │◀───────────────────────│                    │                │
         │  (recovered tokens)    │                    │                │
```

### Recovery Eligibility Checks

For each token found via `FindByHolder`, ALL must hold:

1. **Token expired**: `now > token.base.transfer.tags[valid_until]`
   (wall-clock based, immutable)

2. **Ticket expired**: `current_epoch > ticket.created_on + ticket.lifetime`
   (epoch-based, burns down even if unused)

3. **Not revoked**: token's base signature not in revocation set

4. **Past cooling-off**: `current_epoch > token_expiry_epoch + recovery_cooloff_epochs`
   (configurable, default 2 epochs). This gives honest holders time to
   validate/swap their tokens before recovery can override ownership.

5. **Binding valid**: binding state is Active (not expired, revoked, or consumed)

6. **Old wallet not revoked**: if the lost wallet's credential group is already
   in the revocation set (e.g., prior double-spend), recovery is denied.
   `CumulativeRevocation` invariant: once revoked, permanently revoked.

## Timing Constraints

### Recovery Timeline

```
Token minted ──── Token transferred ──── Wallet lost
                                              │
            ┌─────────────────────────────────┘
            │
            ▼
    Token valid_until expires (wall-clock)
            │
            ├── Ticket lifetime expires (epoch-based)
            │   (may occur before or after token expiry,
            │    depending on configuration)
            │
            ▼
    Both expired
            │
            ├── Cooling-off period (recovery_cooloff_epochs)
            │
            ▼
    Recovery eligible
```

**Example with typical parameters:**
- `T_epoch` = 86400s (24h)
- Token `valid_until` = 90 days after mint
- Ticket `T_life` = 7 epochs (7 days) for medium tickets
- `recovery_cooloff_epochs` = 2

Worst case: wallet lost just after token minted → 90 days + 2 epochs.
Best case: wallet lost near token expiry → a few days + cooloff.

### Interaction with Network Partitions

During a network partition:
- **Epoch advances** for connected peers; partitioned wallets' tickets burn
  down even if unused.
- **Token valid_until** is wall-clock based and unaffected.
- Recovery cannot proceed while the system is partitioned (the recovery server
  needs TokenMap access and mint/treasury connectivity).
- After partition heals, the standard expiry + cooloff timeline applies.

During a system-wide outage:
- Epochs do NOT advance; tickets do NOT expire.
- Token valid_until continues ticking (wall-clock based).
- Recovery is blocked until the system comes back online.

## Edge Cases

### Tokens mid-transfer when wallet is lost

The token's last transfer recipient in the TokenMap determines the holder.
If the token was successfully transferred to another wallet before loss, that
wallet is the holder — not the lost wallet. If the transfer was in-flight
(not yet validated), the TokenMap will show the token at the last validated
state. In either case, recovery only applies to tokens where the lost
wallet's credential is the last known holder.

### Split tokens with one half in lost wallet

Each split half is tracked independently by the TokenMap (separate token
entries with split tags). Only the half whose last holder is the lost wallet's
credential is eligible for recovery. The other half, held by an honest peer,
is unaffected.

**Value conservation**: The TokenMap enforces `SplitConservation` — split
halves must sum to the original value. Recovery of one half does not affect
the other.

### Attacker claims recovery on already-spent tokens

This is prevented by multiple layers:

1. **Fork detection**: If the attacker spent the tokens elsewhere, the TokenMap
   will detect a fork (two different histories for the same token). Fork
   without valid split tags → double-spend → revocation.

2. **Revocation blocks recovery**: `RevokedCannotRecover` invariant — if the
   wallet's credential group is revoked (due to detected double-spend),
   recovery is denied.

3. **Token expiry is immutable**: The `valid_until` timestamp is set at mint
   time and cannot be extended. An attacker cannot keep tokens alive past
   expiry.

4. **Cooloff prevents racing**: The mandatory cooling-off period ensures that
   honest holders have time to validate their tokens before the original
   owner can claim recovery.

### Lost wallet comes back online after recovery claim

If recovery has completed:
- The old wallet's credential group has been revoked (step 8 of the flow).
- Any attempt to transfer or refresh tickets will fail.
- If the old wallet tries to spend tokens that were recovered, the TokenMap
  will detect the fork → double-spend → the old wallet is already revoked.

If recovery is pending (cooloff period):
- The original wallet can call `RevokeBinding` to cancel its own binding,
  halting recovery.
- This is the intended "contest" mechanism for false loss claims.

### Split-key wallets (JavaCard)

When a JavaCard wallet is physically lost, the card's secret key share is
unrecoverable. Recovery cannot transfer the old credential — it must:
1. Register a new wallet with the registrar (new keys, new attestation)
2. Obtain new tickets from the clerk
3. Use the delegate proof to claim recovery
4. Receive tokens on the new wallet's tickets

This is exactly the standard recovery flow — the delegate proof replaces
the need for the old wallet's credentials.

### Multiple recovery claims for the same wallet

The binding state machine prevents this:
- After successful recovery, the binding transitions to `Consumed`.
- A consumed binding cannot be used again.
- If the wallet registered multiple bindings (different delegates), only
  the first successful claim consumes its binding. Subsequent claims using
  other bindings would find no eligible tokens (already recovered).

### Binding expired before wallet loss

If the binding expired and was not refreshed, recovery is not possible.
The tokens will expire naturally and the value is absorbed by the operator.
Users are encouraged to keep bindings fresh, similar to how they must
keep tickets fresh.

## Privacy Analysis

### What the recovery server learns

- **At binding time**: The NAC signature reveals the wallet belongs to a
  particular NAC group (but not which specific wallet). The TTC public key
  hash is stored, but the TTC key itself is only meaningful to the operator
  infrastructure.

- **At recovery time**: The recovery server learns the TTC public key (to
  query the TokenMap) and the set of tokens recovered. This is the same
  level of information the operator already has via the TokenMap.

- **Delegate privacy**: For ECDSA delegates, the recovery server learns the
  delegate public key but not the delegate's identity. For TTC delegates,
  the recovery server learns the delegate wallet's TTC key, but the delegate's
  ECDAA signature does not reveal its ticket history.

### What third parties learn

- Recovery transactions are logged server-side but not exposed to peers.
- The replacement tokens issued to the new wallet are fresh (from the mint
  pool) with no linkage to the original expired tokens.
- The new wallet's tickets are randomized per-ticket (standard ECDAA
  unlinkability).

### Rate-limiting without de-anonymization

The NAC signature's basename = current epoch creates linkable pseudonyms
within an epoch. The recovery server can detect if the same NAC credential
creates multiple bindings in one epoch without knowing which wallet it is.
This prevents abuse (registering thousands of bindings) without
de-anonymizing the wallet.

## Interaction with Other Systems

### Escrow

Escrowed tokens have an `escrow` tag in their transfer. Recovery does not
override escrow — tokens with active escrow tags are ineligible for recovery
until the escrow resolves. After escrow release, standard expiry + cooloff
applies.

### Revocation

The recovery system both checks and triggers revocation:
- **Checks**: Recovery is denied if the lost wallet's group is already revoked.
- **Triggers**: After recovery completes, the old wallet's group is revoked.

This creates a one-way gate: recovery → revocation. A revoked wallet cannot
subsequently recover more tokens (e.g., tokens that expire later).

### Trim/Swap Service

The recovery server uses the same infrastructure as the swap service for
token issuance. Specifically:
- The recovery server holds a wallet with pre-withdrawn tokens
- It transfers from its pool to the claimant's ticket
- If the pool is depleted, it withdraws more from the mint/treasury
- The `trimmed_from` tag in newly issued tokens creates a provenance link
  from the replacement token back to the recovery event

## TokenMap Extension: FindByHolder

The TokenMap requires a new index to support recovery. Currently, tokens
are indexed by base signature (token ID). Recovery needs to query by
holder credential.

### New Index Table

```sql
CREATE TABLE holder_index (
    credential_hash BLOB NOT NULL,  -- SHA-256 of TTC credential bytes
    token_id        BLOB NOT NULL,  -- Token base signature
    updated_epoch   INTEGER NOT NULL,
    PRIMARY KEY (credential_hash, token_id)
);
```

### Maintenance

When `update_impl()` processes a token submission (new, extension, or split):
1. Extract the last transfer recipient's credential from the token history
2. Compute SHA-256 of the credential bytes
3. Upsert into `holder_index`

When a token is marked as a double-spend (fork detected):
- The `holder_index` entry remains (it's still useful for forensics)
- The revocation table entry prevents recovery

### FindByHolder RPC

```protobuf
rpc FindByHolder (FindByHolderRequest) returns (FindByHolderReply);

message FindByHolderRequest {
    bytes ttc_public_key = 1;
    bool expired_only = 2;  // Filter to tokens past valid_until
}

message FindByHolderReply {
    repeated Entry entries = 1;
}
```

The implementation:
1. Hash the TTC public key
2. Query `holder_index` for matching `credential_hash`
3. Join with `tokens` table to get full token entries
4. If `expired_only`, filter by `valid_until < now`
5. Exclude tokens in the revocation set

## Security Properties (Summary)

| Property | Mechanism | Formal Invariant |
|----------|-----------|------------------|
| No double-recovery | Binding consumed on success | `NoDoubleRecovery` |
| No active-token recovery | Expiry checks + cooloff | `NoActiveRecovery` |
| Revoked cannot recover | Revocation set check | `RevokedCannotRecover` |
| Old wallet disabled | Post-recovery revocation | `OldWalletRevokedAfterRecovery` |
| Value conservation | Mint replacement, not creation | `ConservationOfValue` |
| Delegate authorization | Cryptographic proof verification | `DelegateRequired` |
| Rate-limited binding | NAC basename per epoch | (Implementation check) |

See `formal/RecoveryProtocol.tla` for the complete formal specification.
