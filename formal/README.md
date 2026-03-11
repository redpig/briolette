# Briolette Formal Verification (TLA+)

TLA+ specifications that formally model briolette's core protocol properties.
These specs verify the invariants documented in `docs/design/security_model.md`
using the TLC model checker.

## Prerequisites

- Java 11+ (for TLC model checker)
- TLA+ tools: download `tla2tools.jar` from
  https://github.com/tlaplus/tlaplus/releases

Or use the VS Code TLA+ extension for an integrated experience.

## Specifications

| Module | File | Source Code | Properties Verified |
|--------|------|-------------|---------------------|
| Fork Detection | `ForkDetection.tla` | `src/tokenmap/src/server.rs` | Exhaustive 4-way classification, double-spend detection, no false positives, split value conservation |
| Token Lifecycle | `TokenLifecycle.tla` | `src/mint/src/server.rs`, `src/wallet/src/lib.rs` | Value conservation, split correctness, unique ownership, history bounds |
| Gossip Convergence | `GossipConvergence.tla` | `src/simulation/briolettesim/src/main.rs` | Epoch monotonicity, bounded convergence (diameter), eventual convergence |
| P2P Transaction | `P2PTransaction.tla` | `src/receiver/src/server.rs` | Valid state transitions, amount matching, peer binding, timeout guarantees |
| Revocation Protocol | `RevocationProtocol.tla` | `src/tokenmap/src/server.rs`, `src/clerk/src/server.rs` | Cumulative revocation, enforcement, no false revocation, eviction completeness |
| Composed System | `BrioletteSystem.tla` | Full system | End-to-end value conservation, double-spend-to-eviction pipeline, honest wallet liveness |

## Running TLC

Check each module independently:

```bash
# Check fork detection (fastest, most self-contained)
java -jar tla2tools.jar -config ForkDetection.cfg ForkDetection.tla

# Check token lifecycle
java -jar tla2tools.jar -config TokenLifecycle.cfg TokenLifecycle.tla

# Check gossip convergence (line topology)
java -jar tla2tools.jar -config GossipConvergence.cfg GossipConvergence.tla

# Check P2P transaction state machine
java -jar tla2tools.jar -config P2PTransaction.cfg P2PTransaction.tla

# Check revocation protocol
java -jar tla2tools.jar -config RevocationProtocol.cfg RevocationProtocol.tla

# Check composed system (largest state space — runs longest)
java -jar tla2tools.jar -config BrioletteSystem.cfg BrioletteSystem.tla
```

For multi-core checking, add `-workers auto`:

```bash
java -jar tla2tools.jar -workers auto -config ForkDetection.cfg ForkDetection.tla
```

## Mapping TLA+ to Security Model Invariants

| security_model.md | TLA+ Module | TLA+ Property |
|-------------------|-------------|---------------|
| Invariant 1: Basename Linkability | ForkDetection | Cryptographic abstraction: same history prefix = same basename → same pseudonym. Fork detection depends on this. |
| Invariant 2: Fork Detection | ForkDetection | `ExhaustiveClassification`, `DoubleSpendAlwaysDetected`, `SplitConservation` |
| Invariant 3: Gossip Convergence | GossipConvergence | `MonotonicEpoch`, `EventualConvergence` |
| Invariant 4: Revocation Enforcement | RevocationProtocol | `RevocationEnforced`, `CumulativeRevocation`, `NoFalseRevocation` |
| Eviction Completeness Theorem | RevocationProtocol, BrioletteSystem | `EvictionCompleteness`, `DoubleSpendLeadsToEviction` |

## Cryptographic Abstraction

Cryptographic operations are modeled as abstract logical predicates:

| Real Operation | TLA+ Abstraction |
|----------------|------------------|
| ECDAA sign with basename | Deterministic function: same (walletId, basename) → same pseudonym |
| ECDSA signature verification | Boolean oracle, TRUE for honest signers |
| Credential randomization | Unique abstract IDs per ticket |
| Token history signature chain | Sequence of abstract signature values; chain validity is structural |

This is sound because the security model invariants are stated at this
abstraction level. Cryptographic correctness of ECDAA/ECDSA is a separate
concern not addressed by TLA+.

## Topology Configurations

The gossip convergence module can be checked against different topologies by
modifying the `Edges` constant in `GossipConvergence.cfg`:

```
\* Line: diameter 3
Edges = {{w1, w2}, {w2, w3}, {w3, w4}}

\* Ring: diameter 2
Edges = {{w1, w2}, {w2, w3}, {w3, w4}, {w4, w1}}

\* Star (add w5): diameter 2
Wallets = {w1, w2, w3, w4, w5}
Edges = {{w1, w2}, {w1, w3}, {w1, w4}, {w1, w5}}

\* Complete: diameter 1
Edges = {{w1, w2}, {w1, w3}, {w1, w4}, {w2, w3}, {w2, w4}, {w3, w4}}
```

## Tuning Model Parameters

The `.cfg` files use small parameters for tractable model checking. To explore
larger state spaces (at the cost of longer checking times):

- Increase `MaxHistory` to test deeper transfer chains
- Increase `MaxEpoch` to test longer revocation lifecycles
- Add more wallets to test richer topologies
- Increase `MaxSubmissions` to test more TokenMap interactions

If TLC runs out of memory, reduce parameters or use `-fpbits` and `-fp` flags
for disk-based state storage.

## Interpreting Results

- **Model checking completed. No error found.** — All invariants hold for all
  reachable states. The property is verified up to the model bounds.

- **Error: Invariant X is violated.** — TLC found a counterexample. This either
  reveals a real protocol issue or a modeling error. The error trace shows the
  exact sequence of actions leading to the violation.

- **Temporal properties were violated.** — A liveness property failed. The
  counterexample shows an infinite execution where the "eventually" condition
  never holds (usually indicates missing fairness or a real liveness bug).
