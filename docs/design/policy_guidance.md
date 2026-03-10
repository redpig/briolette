# Policy Guidance: Tuning Briolette for Production

This document provides mathematical constraints, policy tradeoffs, and
concrete guidance for operators configuring a Briolette deployment. All
parameters interact — changing one affects the bounds on others. The
constraint checker at the end of this document can be used to validate
a complete configuration.

## Parameter Inventory

### Timing Parameters

| Symbol | Parameter | Current Default | Where Defined |
|--------|-----------|-----------------|---------------|
| `T_epoch` | Epoch duration (seconds) | 86400 (24h) | `clerk/epoch_generate.rs:35`, `proto/token.rs:30` |
| `T_life` | Ticket lifetime (epochs) | Per-ticket | `proto/token.proto:27` (TicketData.lifetime) |
| `T_valid` | Token valid-until (timestamp) | Per-token | `proto/token.proto:58` (Tag.valid_until) |
| `G_max` | Maximum ticket groups | 64 | `clerk/epoch_generate.rs:36` |

### Derived Timing

| Symbol | Formula | Meaning |
|--------|---------|---------|
| `T_ticket_abs` | `created_on + (T_life * T_epoch)` | Absolute ticket expiration |
| `T_evict` | `T_detect + T_epoch + T_gossip + T_life` | Total eviction time |
| `T_detect` | `O(1)` or `1/v` epochs expected | Time to first validation revealing abuse |
| `T_gossip` | `O(D)` transaction rounds | Epoch convergence across honest subgraph |

### Network Topology Parameters

| Symbol | Meaning | Typical Values |
|--------|---------|----------------|
| `D` | Diameter of honest transaction subgraph | Line: N-1, Ring: floor(N/2), Grid: 2(sqrt(N)-1) |
| `v` | Validation probability per epoch per holder | Operator-tunable via epoch data |
| `N` | Number of active wallets | Deployment-specific |

## Fundamental Constraints

### Constraint 1: Ticket Lifetime vs. Eviction Bound

The worst-case eviction time for a double-spender is:

```
T_evict = T_detect + 1 + D + T_life    (in epochs)
```

Where `T_detect = 1` for mandatory validation (banks, merchants) or
`E[T_detect] = 1/v` for probabilistic validation.

**Operator choice**: Lower `T_life` shrinks the eviction window but
forces wallets online more frequently. The key constraint:

```
T_life >= T_outage_tolerance
```

where `T_outage_tolerance` is the longest acceptable connectivity
outage (in epochs) before wallets lose the ability to transact.

### Constraint 2: Epoch Velocity vs. Gossip Convergence

Gossip convergence takes `D` transaction rounds. If the epoch advances
faster than gossip can propagate, some honest peers will be unable to
transact because counterparties reject their stale epoch.

**Hard constraint**:

```
T_epoch > D * T_avg_transaction_interval
```

In practice, `T_epoch = 86400s` is safe for any deployment where peers
transact at least once per day on average. For sparse deployments
(rural, limited connectivity), either increase `T_epoch` or ensure the
`Synchronize` fallback covers the gap.

### Constraint 3: Ticket Lifetime vs. Outage Resilience

During a connectivity outage:
- Epoch gossip stalls (no new transactions propagating updates)
- Ticket clocks keep ticking (expiration is wall-clock based)
- Wallets cannot request new tickets

If all tickets expire during an outage, the wallet is dead until
connectivity returns. The constraint:

```
T_life_max * T_epoch > T_max_expected_outage
```

where `T_life_max` is the longest-lived ticket in the wallet's set.

### Constraint 4: Group Count vs. Collateral Damage

When a double-spend is detected, the offender's ticket group is
revoked. All other honest wallets in that group lose those tickets.
Expected collateral per revocation event:

```
collateral_wallets = N_active / G_max
```

With `G_max = 64` and 1M wallets, each revocation affects ~15,600
wallets. These wallets can still transact with other ticket groups and
request new tickets at the clerk. But if they only have tickets in the
revoked group, they must go online.

**Constraint**:

```
G_max >= ceil(N_active / max_acceptable_collateral)
```

### Constraint 5: Validation Frequency vs. Detection Latency

Expected detection time for a double-spend:

```
E[T_detect] = 1 / (1 - (1-v)^H)    (epochs)
```

where `H` is the number of holders who have seen the token and `v` is
the per-holder validation probability per epoch. For a single holder:

```
E[T_detect] = 1/v    (epochs)
```

Banks and merchants should set `v = 1` (validate every token on deposit).
For peer-to-peer, the operator can recommend `v` in epoch extended data
based on threat level.

## Elastic Ticket Lifetimes

### Design

Rather than a single `T_life` for all tickets, the clerk issues tickets
with a distribution of lifetimes:

| Ticket Class | Lifetime | Count per Request | Purpose |
|-------------|----------|-------------------|---------|
| Short | 1-3 epochs | Majority (~80%) | Normal transactions, fast revocation |
| Medium | 7-14 epochs | Some (~15%) | Outage buffer, weekend/travel coverage |
| Long | 30-90 epochs | Few (~5%) | Fixed addresses, Venmo-style persistent IDs |

### Spending Order

Wallets MUST spend shortest-lived tickets first. This ensures:

1. **Normal operation**: System behaves as if `T_life` is short (1-3
   epochs). Revocation enforcement is fast.
2. **Outage resilience**: If connectivity drops, wallets fall back to
   medium then long tickets. No cliff-edge failure.
3. **Graceful degradation**: The probability of total ticket exhaustion
   during an outage decreases exponentially with the number of lifetime
   tiers.

### Eviction Bound Under Elastic Lifetimes

In normal operation (no outage), eviction is bounded by the short
ticket lifetime:

```
T_evict_normal = T_detect + 1 + D + T_life_short
```

In the worst case (attacker hoards long tickets before going rogue):

```
T_evict_worst = T_detect + 1 + D + T_life_long
```

The operator controls this tradeoff by limiting the number of long-lived
tickets per request and per NAC pseudonym per epoch.

### Interaction with Outages

During an outage of duration `T_out`:

```
tickets_surviving(T_out) = count(tickets where remaining_life > T_out)
```

With the elastic distribution above, a 3-epoch outage loses ~80% of
tickets (the short ones) but preserves ~20% (medium + long). A 14-epoch
outage loses ~95% but preserves the long-lived tickets. The wallet
remains functional throughout.

**Critical invariant**: Epoch gossip also stalls during outages, so
revocation enforcement also stalls. Honest wallets are not disadvantaged
by holding older epochs during the outage — their peers are in the
same state.

### Long-Lived Tickets and Revocation

Long-lived tickets are the binding constraint on `T_evict_worst`. Two
mitigations:

1. **Group bitfield in epoch data**: The epoch already carries
   `group_bitfield`. When a double-spender's group is revoked, all
   tickets in that group — including long-lived ones — are rejected
   by peers who have the updated epoch. Eviction time is bounded by
   `T_gossip`, not `T_life`, for any peer that has converged.

2. **Validation age scaling**: Peers can be configured to require online
   validation for tokens backed by tickets older than a threshold. This
   is a policy hint in epoch extended data, not a protocol change:

   ```
   if ticket.age_in_epochs() > validation_age_threshold:
       require_online_validation()
   ```

## Configuration Checker

An operator should validate the following before deployment. All times
are in epochs unless noted.

### Must-hold constraints

```
1. T_life_short >= 1
   (Tickets must survive at least one epoch)

2. T_life_max * T_epoch >= T_max_expected_outage
   (Longest tickets must outlive expected outages)

3. T_epoch > D * T_avg_txn_interval
   (Epochs must not advance faster than gossip can propagate)

4. G_max >= N_active / max_acceptable_collateral
   (Group count limits blast radius of each revocation)

5. T_life_short < T_life_medium < T_life_long
   (Lifetime tiers must be strictly ordered)

6. count_long_per_request * T_life_long < abuse_tolerance
   (Long ticket count * lifetime bounds worst-case abuse window)
```

### Should-hold recommendations

```
7. T_epoch >= 3600
   (Sub-hour epochs create excessive churn for minimal security gain)

8. v_bank = 1.0, v_merchant >= 0.5, v_peer >= 0.1
   (Validation frequency should scale with transaction value/trust)

9. T_life_short <= 3
   (Short tickets beyond 3 epochs provide diminishing privacy benefit
    for increasing eviction delay)

10. T_life_long <= 90
    (Beyond 90 epochs, the eviction worst-case exceeds typical
     fraud investigation timelines)

11. max_tickets_per_request <= 20
    (Limits ticket stockpiling by malicious wallets)

12. count_short : count_medium : count_long ~= 80 : 15 : 5
    (Distribution should heavily favor short tickets)
```

### Example configurations

#### Urban high-connectivity deployment

```
T_epoch         = 86400s (24h)
T_life_short    = 2 epochs (2 days)
T_life_medium   = 7 epochs (1 week)
T_life_long     = 30 epochs (1 month)
G_max           = 64
max_tickets     = 15 per request (12 short, 2 medium, 1 long)
v_merchant      = 1.0
v_peer          = 0.2
```

Eviction (normal): `1 + 1 + D + 2` = `D + 4` epochs
Eviction (worst): `1 + 1 + D + 30` = `D + 32` epochs
Outage tolerance: 30 days

#### Rural / low-connectivity deployment

```
T_epoch         = 86400s (24h)
T_life_short    = 5 epochs (5 days)
T_life_medium   = 14 epochs (2 weeks)
T_life_long     = 90 epochs (3 months)
G_max           = 64
max_tickets     = 10 per request (8 short, 1 medium, 1 long)
v_merchant      = 1.0
v_peer          = 0.05
```

Eviction (normal): `1 + 1 + D + 5` = `D + 7` epochs
Eviction (worst): `1 + 1 + D + 90` = `D + 92` epochs
Outage tolerance: 90 days

#### High-security / low-latency deployment

```
T_epoch         = 3600s (1h)
T_life_short    = 24 epochs (24h)
T_life_medium   = 168 epochs (1 week)
T_life_long     = 720 epochs (1 month)
G_max           = 64
max_tickets     = 20 per request (16 short, 3 medium, 1 long)
v_merchant      = 1.0
v_peer          = 0.5
```

Eviction (normal): `1 + 1 + D + 24` = `D + 26` epochs (~26 hours)
Eviction (worst): `1 + 1 + D + 720` = `D + 722` epochs (~30 days)
Outage tolerance: 30 days

## Tradeoff Summary

| Knob | Turn Up | Turn Down |
|------|---------|-----------|
| `T_epoch` | Slower state churn, better for sparse networks | Faster revocation enforcement, more overhead |
| `T_life_short` | Better outage tolerance | Faster normal-case eviction |
| `T_life_long` | Better outage tolerance, persistent addresses | Wider worst-case eviction window |
| `G_max` | Less collateral damage per revocation | More state in epoch bitfield |
| `v` (validation freq) | Faster detection | More online validation traffic |
| `max_tickets_per_request` | More flexibility for wallets | More tickets a rogue wallet can stockpile |
| `count_long / count_total` | Better disaster resilience | Wider worst-case abuse window |

## Open Design Questions

1. **Ticket lifetime in epoch data**: Should the clerk's ticket lifetime
   distribution be published in the epoch extended data so wallets and
   peers can validate ticket ages against expected policy? This would
   enable the validation-age-scaling mitigation without hardcoding
   thresholds.

2. **Per-NAC ticket budget**: The clerk currently has no enforced limit
   on tickets per NAC per epoch. Adding a budget (e.g., 20 tickets per
   NAC pseudonym per epoch) would bound stockpiling. The NAC pseudonym
   linkability within an epoch makes this enforceable.

3. **Adaptive epoch velocity**: Could `T_epoch` be shortened during
   active abuse (faster revocation propagation) and lengthened during
   quiet periods (less overhead)? The `epoch_seconds` field in
   `ExtendedEpochData` already supports this, but wallets would need
   to handle variable-length epochs gracefully.

4. **Swap service as ticket refresh**: When a wallet's tickets are
   expiring and it cannot reach the clerk, could a swap service
   (which already handles token refresh) also issue short-lived
   emergency tickets? This would require the swap service to have
   ticket-signing authority, which has trust implications.
