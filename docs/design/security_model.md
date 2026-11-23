# Security Model: Revocation and Gossip Bounds

This document describes the mathematical invariants and bounds that guarantee
a double-spending wallet is detected and evicted from the Briolette system.

## Definitions

Let:
- `N` = number of active wallets (peers) in the system
- `G = (V, E)` = the transaction graph where `V` = wallets and `E` = transaction pairs
- `D = diameter(G)` = the maximum shortest path between any two peers
- `E_k` = epoch `k`, a signed state update containing revocation lists
- `T_life` = ticket lifetime in epochs
- `W` = a wallet with secret key `sk_W`
- `K(sk, bsn)` = pseudonym function, `K = H(bsn)^sk` in G1 (ECDAA)

## Invariant 1: Basename Linkability (Cryptographic Detection)

**Statement**: If wallet `W` signs token `T` at history index `i` to two
different recipients `R_a` and `R_b`, both signatures share the same
pseudonym `K_W = H(sig_{i-1})^{sk_W}`.

**Proof sketch**: Both transfers use `basename = signature[i-1]` (the previous
history entry's signature). ECDAA with a fixed basename and fixed secret key
deterministically computes `K = H(basename)^{sk}`. Since `sk_W` and
`basename = sig_{i-1}` are identical in both cases, `K_a = K_b`.

**Consequence**: Any entity observing both transfer signatures can link them
to the same wallet by comparing `K` points. This is the foundation of
identity revelation on double-spend.

**Tested by**: `basename_pseudonym_linkable` in `src/crypto/src/v0.rs`,
plus split-key equivalents in the same file.

## Invariant 2: Fork Detection (TokenMap Decision Tree)

**Statement**: The tokenmap classifies every submitted token into exactly one
of four categories, and double-spends are always detected.

Given a token `T_candidate` and the set of known token histories `{T_1, ..., T_m}`
sharing the same base signature (token ID):

1. **Known**: `T_candidate.history` is a prefix of some `T_j.history`
   -> No-op (already seen)

2. **Extension**: `T_j.history` is a strict prefix of `T_candidate.history`
   -> Replace `T_j` with `T_candidate` (new information, same chain)

3. **Valid split**: There exists index `k` where `T_candidate.history[k] != T_j.history[k]`,
   AND both `T_candidate.history[k]` and `T_j.history[k]` carry `SplitValue` tags,
   AND the split values sum to the token's original denomination
   -> Add parallel history (legitimate value split)

4. **Double spend (fork)**: There exists index `k` where
   `T_candidate.history[k] != T_j.history[k]`, and the fork is NOT a valid split
   -> Create `RevocationData`, set `abuse_detected = true`

**Completeness**: These four cases are exhaustive. Any token submission that
is not known, not an extension, and not a valid split MUST be a fork, and
`token_get_fork()` will return `Some((token_index, history_index))`.

**Tested by**: `fork_detection_decision_tree` and individual tests in
`src/tokenmap/src/server.rs`.

## Invariant 3: Gossip Convergence Bound

**Statement**: In a connected graph `G` where every transaction triggers a
gossip event carrying `epoch = max(source.epoch, target.epoch)`, all honest
peers converge to epoch `E` within `D` transaction rounds, where `D` is the
diameter of the honest subgraph.

**Proof**: Define `d(v)` as the shortest-path distance from any peer `v` to
the nearest peer already at epoch `E`. After one round of transactions along
edges, every honest peer `v` with an honest neighbor `u` at epoch `E`
receives `gossip_epoch = max(u.epoch, v.epoch) = E` and updates to `E`.
Thus `d(v)` decreases by at least 1 per round for all honest peers. After
`D` rounds, `d(v) = 0` for all honest `v`.

**Tight bound**: The line topology `0 -- 1 -- 2 -- ... -- N-1` achieves
diameter `N-1`, and convergence requires exactly `N-1` rounds when the
source is at one end. This is proven tight by showing that after `N-2`
rounds, the last agent has NOT yet converged.

**Double-spender effect**: A double-spender who refuses to update their
epoch acts as a "firewall" on a line topology — peers behind them cannot
learn the new epoch via gossip. This is by design: the system relies on
either (a) mesh topology providing alternate paths, or (b) periodic
`Synchronize` calls to the operator.

On a ring or grid topology, honest peers route around the double-spender:
  - Ring diameter: `floor(N/2)` -> convergence in `floor(N/2)` rounds
  - Grid `s x s`: diameter `2(s-1)` -> convergence in `2(s-1)` rounds
  - Complete graph: diameter 1 -> convergence in 1 round

**Tested by**: `gossip_convergence_bound_line_topology`,
`gossip_convergence_not_faster_than_diameter`,
`gossip_double_spender_blocks_line_propagation`,
`gossip_convergence_around_double_spender_ring`, and
`gossip_convergence_star_topology` in
`src/simulation/briolettesim/src/main.rs`.

## Invariant 4: Revocation Enforcement

**Statement**: Once an honest peer `P` has epoch `E_k` where `E_k` contains
the revocation of wallet `W`, peer `P` will reject all transactions with `W`.

**Proof**: In `do_transaction()`, the transaction epoch is
`txn_epoch = max(source.epoch, target.epoch)`. The revocation check is:
```
if epochs[txn_epoch].revocation.contains(&agent_id) -> reject
```
Since `P.epoch >= k` and revocation lists are cumulative (epoch `k+1`
contains all revocations from epoch `k`), `W` appears in
`epochs[txn_epoch].revocation` for all `txn_epoch >= k`.

**Tested by**: `revocation_check_rejects_revoked_source`,
`revocation_enforcement_after_convergence` in
`src/simulation/briolettesim/src/main.rs`.

## Theorem: Eviction Completeness Bound

**Statement**: A double-spending wallet `W` is fully evicted (unable to
transact with any honest peer) within a bounded number of steps:

```
T_evict = T_detect + T_epoch + T_gossip + T_ticket
```

Where:
- `T_detect = O(1)`: Any holder validating the double-spent token triggers
  fork detection (Invariant 2). One call to the validate server suffices.
- `T_epoch = O(1)`: The operator publishes a new epoch containing the
  revocation. One epoch update suffices.
- `T_gossip = O(D)`: Gossip propagation to all honest peers takes at most
  `D` transaction rounds (Invariant 3).
- `T_ticket = T_life`: The double-spender's existing tickets expire after
  `T_life` epochs, forcing them to request new tickets from the clerk,
  where they will be rejected.

**Total**: `T_evict = O(1 + D + T_life)`

**Worst case**: If the double-spender avoids online validation and only
transacts peer-to-peer, detection is deferred until any holder validates.
But:
- Banks always validate on deposit (`validate_total` in simulation stats)
- Merchants may validate above value thresholds
- Ticket expiration forces the wallet online within `T_life` epochs
- Recommended validation frequency may be adjusted by the operator in
  the epoch data based on threat level

**Tested by**: `double_spend_creates_revocation_data` in
`src/wallet/src/lib.rs` (proves detection -> revocation in one validate call),
plus all gossip convergence tests above.

## Summary of Test Coverage

| Invariant | Property | Test Location |
|-----------|----------|---------------|
| 1. Linkability | Same (sk, basename) -> same K | `crypto/src/v0.rs:basename_pseudonym_*` |
| 1. Unlinkability | Different basename -> different K | `crypto/src/v0.rs:basename_pseudonym_*` |
| 1. Split-key | Split signing produces same K | `crypto/src/v0.rs:split_tests` |
| 2. Fork detection | All four decision tree cases | `tokenmap/src/server.rs:fork_detection_decision_tree` |
| 2. Split validation | Valid/invalid splits | `tokenmap/src/server.rs:token_is_second_split_*` |
| 2. Extension splits | Inflated split rejection | `tokenmap/src/server.rs:token_is_extension_*_split*` |
| 2. Revocation creation | DS -> RevocationData | `wallet/src/lib.rs:double_spend_creates_revocation_data` |
| 3. Convergence | D rounds on line | `briolettesim:gossip_convergence_bound_line_topology` |
| 3. Tight bound | NOT D-1 rounds | `briolettesim:gossip_convergence_not_faster_than_diameter` |
| 3. DS blocking | DS blocks line propagation | `briolettesim:gossip_double_spender_blocks_line_propagation` |
| 3. Alternate paths | Ring routes around DS | `briolettesim:gossip_convergence_around_double_spender_ring` |
| 4. Enforcement | Converged peers reject revoked | `briolettesim:revocation_enforcement_after_convergence` |

## Open Questions

1. **Group revocation vs. individual**: Currently, only the double-spender's
   ticket group is revoked. A class-break attack would require revoking all
   groups associated with the NAC signature. The tokenmap stores per-ticket
   NAC signatures to support this escalation path.

2. **Optimal validation frequency**: The expected time to detection depends on
   the validation rate `v` (probability a holder validates per epoch). Expected
   detection time: `E[T_detect] = 1/v` epochs from the double-spend event.
   The operator can tune `v` via recommended validation thresholds in epoch data.

3. **Gossip in sparse graphs**: If the honest subgraph is disconnected
   (isolated communities with no shared transactions), gossip alone cannot
   achieve convergence. The `Synchronize` mechanism serves as the fallback,
   guaranteeing eventual convergence regardless of topology.
