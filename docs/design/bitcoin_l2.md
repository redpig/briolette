# Briolette as a Bitcoin L2

This document describes how Briolette can operate as a Bitcoin L2, providing
the same offline-capable digital currency with Bitcoin as the settlement layer
instead of (or in addition to) Ethereum.

## Motivation

Briolette's core value proposition — offline peer-to-peer transfers with ECDAA
privacy — is chain-agnostic. The L1 is only used for four functions:

1. **Asset locking**: Hold the base asset backing off-chain tokens
2. **Epoch anchoring**: Post a 32-byte commitment per epoch for auditability
3. **Withdrawals**: Release locked funds when users exit the system
4. **Key registry**: Publish authoritative signing keys

All four are feasible on Bitcoin, though with different trust trade-offs than
Ethereum. This document analyzes the design, compares the two approaches, and
explains when each is appropriate.


## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                        Bitcoin L1                             │
│                                                               │
│  Deposits:                                                    │
│    BTC sent to Taproot address derived from:                  │
│      internal_key = operator x-only pubkey                    │
│      script_path  = federation n-of-m + recovery timelock     │
│    OP_RETURN encodes ticket_hash (36 bytes)                   │
│                                                               │
│  Withdrawals:                                                 │
│    Operator + federation co-sign Taproot spend                │
│    Output: recipient with OP_CHECKSEQUENCEVERIFY timelock     │
│    Challenge period: ~1008 blocks (~7 days)                   │
│                                                               │
│  Epoch anchoring:                                             │
│    OP_RETURN: "BRI\x01" || epoch_num (8B) || hash (32B)      │
│    Total: 44 bytes (within 80-byte OP_RETURN limit)           │
│                                                               │
│  Key registry:                                                │
│    Keys published off-chain, hash anchored via OP_RETURN      │
│    Wallets fetch full key sets from operator, verify hash     │
└──────────────────────────────────────────────────────────────┘
                          │
                          │  deposit detection: UTXO scanning
                          │  withdrawal: Taproot spend construction
                          │  epoch: OP_RETURN broadcast
                          ▼
┌──────────────────────────────────────────────────────────────┐
│                    Briolette Operator                          │
│                                                               │
│  Unchanged services:                                          │
│    mint, clerk, tokenmap, validate, receiver, registrar       │
│                                                               │
│  Modified services:                                           │
│    bridge: BitcoinL1Client replaces EthereumClient            │
│    deposit_processor: scans UTXOs instead of contract events  │
│                                                               │
│  New component:                                               │
│    Federation: n-of-m co-signers for withdrawal authorization │
│    (replaces smart contract fraud proof verification)          │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────┐
│                    Wallets (unchanged)                         │
│                                                               │
│  ECDAA transfers, offline peer-to-peer, gossip-based state    │
│  No knowledge of which L1 is used                             │
└──────────────────────────────────────────────────────────────┘
```


## Deposit Flow

### Bitcoin

1. User requests a deposit address from the bridge service
2. Bridge returns a Taproot address (bech32m `bc1p...`) derived from:
   - Internal key: operator's x-only public key
   - Script path: federation threshold check + emergency recovery timelock
3. User sends BTC to this address with an OP_RETURN output containing:
   - `"BRI\x02"` prefix (4 bytes)
   - `ticket_hash` (32 bytes) — hash of their Briolette SignedTicket
4. Bridge monitors for UTXOs at the deposit address
5. After 6 confirmations, bridge mints Briolette tokens to the ticket
6. Bridge marks deposit as processed (local state only — **no on-chain cost**)

### Ethereum (existing)

1. User calls `deposit(ticketHash)` on the BrioletteBridge contract, sending ETH
2. Contract emits `Deposited` event
3. Bridge detects event and mints tokens
4. Bridge calls `markDepositProcessed(depositId)` on-chain (**costs gas**)

### Comparison

| Aspect                  | Ethereum              | Bitcoin                     |
|-------------------------|-----------------------|-----------------------------|
| User action             | Contract call         | Standard BTC send           |
| Deposit confirmation    | ~12s (1 block)        | ~60min (6 blocks)           |
| Processed marker        | On-chain tx (gas)     | Local state only (free)     |
| Ticket hash delivery    | Contract parameter    | OP_RETURN output            |
| Wallet compatibility    | Needs Web3 wallet     | Any Bitcoin wallet + bridge |


## Withdrawal Flow

### Bitcoin

1. User submits tokens to bridge service (same as Ethereum flow)
2. Bridge verifies token chain (ECDAA signatures, no double-spend)
3. Operator constructs a Taproot transaction:
   - Input: UTXO(s) from deposit pool
   - Output: recipient address with `OP_CHECKSEQUENCEVERIFY` timelock
     (1008 blocks ≈ 7 days)
   - Federation members co-sign (verifying the token chain off-chain)
4. Transaction is broadcast to Bitcoin network
5. After timelock expires, recipient can spend the output

### Challenge mechanism

On Ethereum, anyone can challenge a withdrawal by submitting an ECDAA fraud
proof to the smart contract, which verifies it on-chain using the bn256 pairing
precompile.

On Bitcoin, this is not possible — Bitcoin Script cannot perform pairing-based
cryptography. Instead:

- **Federation veto**: Federation members independently verify the token chain.
  If they detect fraud (double-spend, invalid ECDAA signature), they refuse to
  co-sign the withdrawal transaction. Since the Taproot spend requires both
  operator and federation signatures, the withdrawal is blocked.

- **Post-broadcast challenge**: If a withdrawal transaction is already
  broadcast, the federation can publish a revocation OP_RETURN and the operator
  is expected to create a conflicting (replacement) transaction before the
  timelock expires. This requires RBF (Replace-By-Fee) support.

- **Future: BitVM**: The BitVM protocol could enable trustless on-chain
  verification of ECDAA pairings via bisection-style fraud proof games. This
  would bring Bitcoin closer to Ethereum's trust model but adds significant
  complexity.


## Epoch Anchoring

Both chains anchor epoch commitments as a 32-byte hash. The difference is
purely in the mechanism:

| Aspect        | Ethereum                    | Bitcoin                         |
|---------------|-----------------------------|---------------------------------|
| Mechanism     | Contract storage write      | OP_RETURN transaction           |
| Size          | 32 bytes (storage slot)     | 44 bytes (prefix + epoch + hash)|
| Cost          | ~55,000 gas (~$4/day)       | ~$0.50-2.00/day                 |
| Queryability  | Contract view function      | Parse OP_RETURN from blocks     |
| Permanence    | Immutable (contract state)  | Immutable (blockchain)          |


## Fraud Proof Comparison

This is the most significant architectural difference.

### Ethereum: Trustless on-chain fraud proofs

The `EcdaaVerifier.sol` contract uses EVM's bn256 pairing precompile (EIP-197)
to verify ECDAA credential equations on-chain:

```
e(A, Y) == e(B, P2)
e(C, P2) == e(A+D, X)
```

Anyone can submit a challenge. The smart contract is the arbiter. No trust
assumption beyond the EVM's correctness.

### Bitcoin: Federated off-chain fraud proofs

ECDAA pairing verification happens off-chain. Federation members:
1. Receive the token chain data
2. Verify ECDAA signatures locally
3. Sign withdrawal approvals only if verification passes

The trust assumption is that at least `t` of `n` federation members are honest.

### When is the federated model acceptable?

The federated model is appropriate when:

- **The operator is a known entity**: A central bank or regulated institution
  operating a CBDC-style system already has legal accountability. The
  federation provides defense-in-depth, not primary security.

- **The off-chain detection is the primary defense**: Briolette's ECDAA
  linkability and tokenmap double-spend detection catch fraud before it reaches
  L1. The on-chain fraud proof is a last resort for a malicious operator,
  which is already an extreme scenario.

- **Federation members are independent**: If federation keys are held by
  independent auditors, central bank branches, or regulatory bodies, collusion
  is difficult.

- **Bitcoin's security model is preferred**: Some deployments may prefer
  Bitcoin's proof-of-work security and censorship resistance over Ethereum's
  proof-of-stake model.


## Cost Comparison

Assumes: Ethereum gas = 30 gwei, Bitcoin fee = 20 sat/vbyte, BTC = $60,000,
ETH = $2,500.

| Operation            | Ethereum L1   | Bitcoin L1    | Notes                     |
|----------------------|---------------|---------------|---------------------------|
| Deposit              | ~$7.10        | ~$3.50        | BTC: standard Taproot tx  |
| Mark processed       | ~$2.25        | $0.00         | BTC: local state only     |
| Initiate withdrawal  | ~$5.60        | ~$4.00        | BTC: Taproot + timelock   |
| Complete withdrawal  | ~$2.60        | $0.00         | BTC: timelock auto-expire |
| Epoch anchor (daily) | ~$4.00        | ~$1.50        | BTC: OP_RETURN            |
| Fraud proof          | ~$15.00       | N/A           | BTC: off-chain            |
| **Monthly (1k users)** | **~$2,050** | **~$1,000**   | Significant savings       |

Key insight: Bitcoin eliminates two on-chain transactions per deposit/withdrawal
cycle (`markDepositProcessed` and `completeWithdrawal`) because these are
handled by local state and timelocks respectively.


## Implementation

The implementation lives alongside the Ethereum bridge code:

```
src/bridge/src/
├── l1.rs                        # Chain-agnostic L1Client trait
├── ethereum.rs                  # Ethereum implementation (existing)
├── alloy_client.rs              # Ethereum production client (existing)
├── deposit_processor.rs         # Ethereum deposit processor (existing)
├── bitcoin.rs                   # Bitcoin L1 implementation (new)
├── bitcoin_deposit_processor.rs # Bitcoin deposit processor (new)
├── server.rs                    # Bridge service (uses L1Client trait)
├── key_registry.rs              # Key management (chain-agnostic)
└── lib.rs                       # Feature-gated module exports
```

The `L1Client` trait in `l1.rs` defines the chain-agnostic interface. Both
`EthereumClient` (via adapter) and `BitcoinL1Client` implement it, allowing
the bridge to operate with either chain at runtime based on configuration.

### Feature flags

```toml
[features]
default = []
alloy = [...]       # Ethereum production client
bitcoin = [...]     # Bitcoin production client (future: rust-bitcoin deps)
```

### Configuration

The bridge server selects the L1 backend via the `BRIOLETTE_L1_CHAIN`
environment variable:

- `ethereum` (default): Use the Ethereum bridge contract
- `bitcoin`: Use Bitcoin Taproot + OP_RETURN


## Open Questions

1. **Federation key management**: How are federation keys distributed and
   rotated? A production deployment needs a ceremony protocol.

2. **UTXO management**: The operator needs a strategy for managing the deposit
   UTXO pool — consolidation, coin selection, fee estimation.

3. **Reorg handling**: Bitcoin reorgs of 1-2 blocks are common. The 6-block
   confirmation requirement handles this, but edge cases (deep reorgs during
   deposit processing) need careful handling.

4. **BitVM integration**: When BitVM matures, it could replace the federation
   model with trustless on-chain fraud proofs. The `L1Client` trait makes this
   a drop-in replacement.

5. **Lightning Network integration**: Could Briolette deposits/withdrawals use
   Lightning channels instead of on-chain transactions? This would reduce costs
   further but adds complexity around channel management and liquidity.

6. **Dual-chain operation**: Could a single Briolette deployment accept both
   ETH and BTC deposits, with tokens fungible across both? This would require
   cross-chain oracle mechanisms for supply cap enforcement.
