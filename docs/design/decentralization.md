# Decentralizing Briolette: Design Exploration

## Context

Briolette currently relies on a central operator running several services: Mint, Registrar, Clerk (epochs/tickets), TokenMap (double-spend detection), Validate, Swapper, Recovery, and Bridge. The user wants to explore how Briolette could operate without a central authority, covering both blockchain-connected and network-partitioned (offline mesh) scenarios.

This is a **design document only** — no code changes. It analyzes each centralized dependency and proposes concrete decentralization mechanisms.

---

## 1. Inventory of Centralized Dependencies

| Service | What it does | Trust it provides | How often wallets contact it |
|---------|-------------|-------------------|------------------------------|
| **Mint** | Creates tokens (P-256 ECDSA base signature) | Value backing, supply cap | On deposit/withdrawal only |
| **Registrar** | Issues NAC + TTC ECDAA credentials after hardware attestation | Sybil resistance, device integrity | Once per wallet lifetime (+ migration) |
| **Clerk** | Issues signed tickets, advances epochs, publishes revocation bitfield | Rate-limiting (tickets/epoch), revocation propagation, key rotation | Every epoch (~24h) for tickets; gossip fills gaps |
| **TokenMap** | Stores all token histories, detects forks/double-spends | Authoritative double-spend detection | On validate calls (merchant deposits, periodic checks) |
| **Validate** | Crypto-verifies tokens + checks TokenMap | Confirmation that tokens aren't double-spent | Optional per-transaction; mandatory on bank deposit |
| **Swapper** | Exchanges long-history tokens for fresh ones; Schnorr auth for bloom bypass | Token refresh, bloom filter management | When token history is long or basename collision |
| **Recovery** | Pre-loss binding, post-loss token replacement | Lost wallet value recovery | Rare (binding registration, loss events) |
| **Bridge** | L1 deposits → minting, token → L1 withdrawal | Fiat/crypto on-ramp/off-ramp | On entry/exit from Briolette system |

### What can already work offline
- Token transfers (ECDAA signatures, peer-to-peer)
- Epoch gossip (piggybacks on transactions, O(D) convergence)
- Local cryptographic verification (any device can verify a token chain)
- Bloom filter double-spend prevention (hardware-enforced on credstick)

### What currently requires the operator
- Credential issuance (Registrar)
- Ticket issuance and refresh (Clerk)
- Authoritative double-spend detection (TokenMap)
- Epoch advancement and revocation publication (Clerk.AddEpoch)
- Token refresh/swap (Swapper)
- Value creation (Mint)

---

## 2. Smart Contracts for Epoch Management

### Current mechanism
The operator calls `Clerk.AddEpoch(EpochUpdate)` containing:
- `EpochData { epoch, group_bitfield, extended_epoch_data_hash }`
- Signed by the operator's epoch signing key
- Contains revocation bitfield (64 groups), key rotation data, service URIs

### Decentralized proposal: Epoch Contract

**On-chain epoch advancement** based on transaction velocity:

```
contract BrioletteEpoch {
    uint64 public currentEpoch;
    uint64 public epochStartTime;
    uint256 public txnCountThisEpoch;
    bytes32 public currentEpochHash;

    // Epoch advances when:
    // (a) EPOCH_DURATION seconds have passed, OR
    // (b) txnCountThisEpoch exceeds VELOCITY_THRESHOLD
    function advanceEpoch(bytes32 newEpochHash, bytes calldata revocationBitfield) external {
        require(block.timestamp >= epochStartTime + EPOCH_DURATION
                || txnCountThisEpoch >= VELOCITY_THRESHOLD);
        // Verify caller is an authorized epoch proposer (staked validator)
        // or verify a quorum of validator signatures
        ...
    }

    // Anyone can submit a transaction count proof (Merkle proof of TokenMap state)
    function reportTransactionVelocity(bytes calldata proof) external {
        // Increment txnCountThisEpoch based on verified proof
    }
}
```

**Epoch proposers** replace the single operator:
- Staked validators who run TokenMap nodes
- Must post a bond (slashable for publishing invalid epochs)
- Epoch data is proposed → challenged → finalized (optimistic rollup pattern)
- Challenge: submit proof that the proposed epoch omits a known revocation

**What changes:**
- Epoch signing key → validator committee threshold signature
- `ExtendedEpochData` published to IPFS/Arweave, hash on-chain
- Wallets verify epoch signatures against the contract's validator set rather than a single operator key
- Gossip protocol unchanged — still propagates the latest epoch peer-to-peer

**Trust tradeoff:** Instead of trusting a single operator to publish honest epochs, trust shifts to an economic majority of staked validators. A dishonest epoch (suppressing revocations) is challengeable on-chain.

**Offline impact:** Epoch advancement itself doesn't need to be online — it's a background process. Wallets learn epochs via gossip during offline transactions. The contract just replaces *who decides* the next epoch, not *how wallets receive it*.

---

## 3. Decentralized Revocation

### Current mechanism
1. TokenMap detects fork → creates `RevocationData` (NAC signature, TTC signature, groups, token_id)
2. Operator includes revoked groups in next `EpochData.group_bitfield`
3. Wallets enforce revocation via gossip-propagated epoch data

### Decentralized proposal: Verifier Contract + Revocation Registry

```
contract RevocationRegistry {
    mapping(bytes32 => bool) public revokedGroups;

    // Anyone can submit a double-spend proof
    function proveDoubleSpend(
        bytes calldata token1_history,  // Token with transfer at index i to recipient A
        bytes calldata token2_history,  // Same token with transfer at index i to recipient B
        bytes calldata ecdaa_proof      // Proof that both signatures share pseudonym K
    ) external {
        // Verify both share the same base signature (same token)
        // Verify the fork index (same previous_signature = same basename)
        // Verify pseudonym K is identical (same signer)
        // Extract group_number from the abusive ticket
        // Mark group as revoked
        revokedGroups[groupHash] = true;
        emit WalletRevoked(pseudonymK, groupNumbers);
    }
}
```

**On-chain ECDAA verification challenge:** BLS12-381 pairing verification is expensive on EVM. Options:
1. **BN254 (current V0):** EVM has `bn256` precompile (EIP-197). Pseudonym comparison is just G1 point equality — cheap.
2. **BLS12-381 (V1):** No EVM precompile yet. Options:
   - EIP-2537 BLS12-381 precompile (proposed but not widely deployed)
   - Optimistic verification: submit claim → challenge period → if challenged, use off-chain computation with on-chain bisection (BitVM-style)
   - ZK proof of double-spend (prove off-chain, verify SNARK on-chain)

**Practical approach for BN254 (V0):**
The pseudonym K is a G1 point. Proving double-spend requires showing:
- Two History entries with the same `previous_signature` (basename)
- Both produce the same K point in their ECDAA signature
- But different recipient tickets

This is verifiable on-chain with the bn256 precompile — just equality checks on G1 points plus signature structure parsing.

**For BLS12-381 (V1):** Use a ZK-SNARK (Groth16 on BN254) that proves "I know two ECDAA signatures on BLS12-381 that share the same pseudonym K but different recipients." Verify the SNARK on-chain. The SNARK circuit is fixed and can be pre-compiled.

**Recovery contract:** Similarly, the recovery process can be governed by a contract:
- Binding registration stored on-chain (hash of binding, not full data)
- Recovery claims verified by contract logic (expiry checks, delegate proof verification)
- Token replacement authorized by the contract, executed by any mint with a valid pool

**Trust tradeoff:** Revocation becomes trustless — anyone with evidence of double-spending can trigger revocation. No operator gatekeeping. The risk is griefing (false proofs), mitigated by requiring valid cryptographic evidence.

---

## 4. On-chain Device Attestation Providers

### Current mechanism
The Registrar validates hardware attestation (Android KeyStore, iOS App Attest, Card P-256) and issues NAC+TTC ECDAA credentials. This requires the Registrar to maintain root CA trust stores and issuance keys.

### Decentralized proposal: Attestation Provider Registry

```
contract AttestationRegistry {
    struct Provider {
        address addr;
        bytes nacGroupPublicKey;  // The NAC GPK this provider can issue for
        uint256 stake;            // Slashable bond
        uint256 credentialsIssued;
        bool active;
    }

    mapping(address => Provider) public providers;

    // Anyone can become an attestation provider by staking
    function registerProvider(bytes calldata nacGPK) external payable {
        require(msg.value >= MIN_STAKE);
        providers[msg.sender] = Provider(msg.sender, nacGPK, msg.value, 0, true);
    }

    // Slash a provider whose issued credentials are involved in fraud
    function slashProvider(
        address provider,
        bytes calldata revocationProof  // proof linking revoked credential to this provider
    ) external {
        // Verify the proof
        // Slash the provider's stake
        // Distribute to challenger
    }
}
```

**How it works:**
- Multiple independent attestation providers (hardware vendors, auditors, community groups)
- Each provider manages its own NAC group key pair
- The Clerk contract recognizes all registered providers' NAC GPKs
- Providers are incentivized by fees for credential issuance
- Providers are punished (slashed) if their issued credentials are disproportionately involved in double-spends (indicates weak attestation)

**Tiered trust without a single authority:**
- HIGH tier providers: hardware vendors running attestation (Google, Apple, card manufacturers)
- MEDIUM tier: community-run providers with KYC or social attestation
- LOW tier: self-attestation (limited ticket lifetime, online-only)

The `GroupPolicy` mechanism in `ExtendedEpochData` already supports per-NAC-group ticket lifetimes. Different attestation providers naturally get different policy parameters based on their track record.

**Trust tradeoff:** No single registrar bottleneck. Trade-off: attestation quality varies by provider. Mitigated by the existing tiered ticket lifetime mechanism — weak attestation = short ticket lifetime = must come online more often.

---

## 5. Extended Offline via Regional Swap Services

### Current mechanism
Tokens have `valid_until` expiration and tickets have epoch-based `lifetime`. Both require periodic online access to the Clerk (tickets) and Swapper (token refresh). The Swapper also provides Schnorr authorization for bloom filter bypass.

### Decentralized proposal: Regional Swap Federations

**Architecture:**
```
Region A                          Region B
┌─────────────────────┐          ┌─────────────────────┐
│ Solar Relay Mesh     │          │ Solar Relay Mesh     │
│   ┌─────────┐       │          │   ┌─────────┐       │
│   │Relay+Swap│◄──────│──Inter──▶│───│Relay+Swap│       │
│   │ Service  │       │  Region  │   │ Service  │       │
│   └────┬────┘       │  Sync    │   └────┬────┘       │
│        │             │          │        │             │
│   Credstick mesh     │          │   Credstick mesh     │
│   (offline txns)     │          │   (offline txns)     │
└─────────────────────┘          └─────────────────────┘
```

**Regional swap service = relay with a token pool:**
- A solar relay operator stakes tokens (either their own or delegated)
- The relay can perform local swaps: accept a long-history token, return a fresh one from its pool
- The relay signs swaps with its own TTC credential (it's a registered wallet)
- Periodically, the relay syncs with the broader network or other relays to refresh its pool

**Bloom filter bypass for regional swaps:**
Currently, `Swapper.AuthorizeSwap` returns a Schnorr signature `(c, s)` that the JavaCard verifies against a stored swap server public key. For regional swaps:
- Each regional swap service has its own keypair
- At registration, the credstick stores multiple authorized swap public keys (regional + central)
- OR: a single swap authorization key is a threshold key shared among authorized swap services

**Extended offline ticket model:**
- Regional swap services can issue "regional tickets" — shorter-lived tickets signed by a regional authority
- These regional tickets are only accepted within the region (relay mesh validates against known regional keys)
- When connectivity returns, regional tickets are exchanged for standard tickets from the Clerk

**Token expiration extension:**
- The `valid_until` tag is immutable once set by the Mint. Cannot be extended.
- Instead: regional swap services perform a "regional trim" — accept the expiring token and issue a fresh one from their pool with a new `valid_until`
- The regional swap service takes on the risk that the old token might be double-spent
- Incentive: the regional service earns a small fee (encoded as a split tag)

**Trust model:**
- Regional swap services are bonded (staked) participants
- If they issue tokens backed by double-spent collateral, they lose their stake
- The gossip protocol propagates revocations even in the offline mesh — if a double-spend is detected, the revocation bitfield reaches all relays within O(D) transactions

### What limits offline duration today
1. **Ticket expiration** (epoch-based lifetime) — forces wallet online for new tickets
2. **Token valid_until** (wall-clock) — forces token refresh
3. **Epoch staleness** — peers may reject transactions from wallets with very old epochs
4. **Bloom filter capacity** — finite bloom filter means limited offline transactions before collision

### What regional services fix
1. Ticket refresh can happen at a regional swap service (issues regional tickets)
2. Token refresh via regional swap (bonded, risk-bearing)
3. Epoch propagation via relay gossip mesh (already works)
4. Bloom filter bypass via regional Schnorr authorization

**What they don't fix:** Initial credential issuance still requires an attestation provider (can be regional if decentralized per section 4).

---

## 6. ZK Provers for Offline Chain Compression

### The problem
Each ECDAA transfer appends ~300+ bytes to the token history (ECDAA signature + credential + transfer metadata). After 10 hops, a token is ~3KB+. After 50 offline hops in a mesh, it could be 15KB+. NFC transfer speed and credstick storage limit practical chain length.

The `theory_of_operation.md` already identifies this: "Each verifiable proof of transfer is appended to the token causing it to grow in size."

### Decentralized proposal: ZK History Compression at Relay Points

**What the ZK proof proves:**
Given a token with history entries `H_0, H_1, ..., H_n`, a ZK proof attests:

1. **Chain integrity:** Each `H_i.signature` is a valid ECDAA signature over `H_i.transfer` using `basename = H_{i-1}.signature`
2. **No double-spend evidence:** Each `H_i`'s pseudonym `K_i` is unique within the chain (no self-spending loops that might indicate local laundering)
3. **Ticket validity:** Each `H_i.transfer.recipient` is a valid SignedTicket (P-256 ECDSA verification against known ticket signing keys)
4. **Split conservation:** If any `H_i` carries `split_value` tags, the sum equals the token's `Descriptor.value`
5. **Current holder:** The final `H_n.transfer.recipient` matches a claimed current holder ticket

**What the proof does NOT prove:**
- That the token wasn't double-spent on a *different* chain (that requires the TokenMap)
- That revocations are current (that requires epoch data)

**Proof structure:**
```
CompressedToken {
    descriptor: Descriptor,          // Original token descriptor (value, version)
    base: History,                   // Original mint signature (must be verifiable)
    compressed_proof: bytes,         // ZK proof over H_1..H_{n-1}
    current_transfer: History,       // H_n (current holder's entry, verifiable directly)
    pseudonym_commitments: [bytes],  // Pedersen commitments to each K_i (for future double-spend checking)
    proof_metadata: {
        chain_length: u32,           // How many hops were compressed
        oldest_epoch: u64,           // Earliest epoch in the compressed chain
        prover_id: bytes,            // Which relay/prover created this proof
    }
}
```

**Verification on a constrained device (credstick/relay):**
- ZK proof verification must be feasible on nRF52840 (64MHz Cortex-M4, 256KB RAM)
- **Groth16** SNARK: verification is 3 pairings (~6ms on BN254 with software, faster with hardware). But BLS12-381 pairings would be slow.
- **For BN254 (V0):** Groth16 verification is ~10-20ms on a fast MCU — feasible
- **For BLS12-381 (V1):** Groth16 verification on BN254 proving BLS12-381 statement = expensive circuit. Alternative: use **STARKs** (no pairings for verification, just hashes) — verification is ~100KB proof but hash-only verification
- **Practical compromise:** Use **Plonky2** or **RISC Zero** style proofs with hash-based verification. The relay (which has more compute than a credstick) generates the proof. The credstick only needs to verify a hash chain + Merkle proof.

**Where compression happens:**
- At solar relay points during transactions
- A relay with sufficient compute (e.g., Raspberry Pi class) generates the ZK proof
- The compressed token replaces the full history
- The relay stores the full history locally as a backup (for dispute resolution)

**Pseudonym commitment scheme for deferred double-spend detection:**
The compressed proof includes Pedersen commitments to each pseudonym K_i:
```
C_i = K_i * g + r_i * h  (where r_i is random blinding)
```
When the token eventually reaches a TokenMap (or on-chain verifier), the prover can open the commitments. If K_i matches a known double-spend pseudonym, the fork is detected.

**Incentive for provers:**
- Relay operators who compress tokens earn a micro-fee (split tag)
- Compressed tokens transact faster (smaller NFC payload) — natural user preference
- Relays that produce invalid proofs lose their bond

### Practical assessment
- **Feasible today for BN254 (V0):** Groth16 circuits for ECDAA signature verification exist in research. Circuit size for N ECDAA verifications is large but tractable for N < 20.
- **Hard for BLS12-381 (V1):** BLS12-381 arithmetic inside a BN254 SNARK is expensive (field emulation). Would need BLS12-381-native proof system or recursive SNARKs.
- **Alternative: trusted compression:** A bonded relay simply attests "I verified this chain of N transfers" with its own ECDAA signature. Not zero-knowledge, but much simpler. The relay's reputation/stake backs the attestation.

---

## 7. Decentralized Relay Abuse Prevention

### Attack vectors for compromised relays
1. **Epoch suppression:** Relay refuses to gossip new epoch data, keeping peers on old (pre-revocation) epochs
2. **Selective forwarding:** Relay drops revocation data while forwarding transactions
3. **Replay attacks:** Relay presents old epoch data as current
4. **Collusion with double-spenders:** Relay helps double-spender by not reporting to TokenMap
5. **Front-running:** Relay learns transaction details and uses them

### Existing defenses (already in Briolette)
- **Gossip convergence** (security_model.md Invariant 3): In a connected graph, honest peers route around a single malicious relay in O(D) rounds
- **Epoch monotonicity:** Wallets always take `max(source.epoch, target.epoch)` — can't be downgraded
- **Ticket expiration:** Forces wallets online periodically regardless of relay behavior
- **Cryptographic verification:** Any device can independently verify token chains — relays can't forge signatures

### Additional decentralized mechanisms

**a) Relay staking and reputation:**
```
contract RelayRegistry {
    struct Relay {
        address operator;
        uint256 stake;
        uint256 txnsProcessed;
        uint256 epochsGossiped;
        uint256 slashCount;
        bytes ttcPublicKey;  // Relay's wallet credential
    }

    // Slash relay for provable misbehavior
    function slashRelay(
        address relay,
        bytes calldata proof  // e.g., signed receipt showing relay served old epoch
    ) external { ... }
}
```

**b) Epoch freshness receipts:**
When a relay processes a transaction, both parties can request a signed receipt:
```
RelayReceipt {
    relay_ttc_signature: bytes,     // Relay signs with its TTC
    epoch_served: u64,              // What epoch the relay claimed
    transaction_hash: bytes,        // Hash of the transaction
    timestamp: u64,
}
```
If a wallet later discovers the relay served a stale epoch (by comparing with a fresher epoch it receives via another path), it can submit the receipt as evidence of misbehavior → slash the relay's stake.

**c) Relay mesh redundancy incentives:**
- Wallets prefer to transact through relays with higher reputation scores
- Reputation is earned by processing transactions and correctly propagating epochs
- Multiple overlapping relay coverage areas create natural redundancy
- A single dishonest relay has limited impact because transactions also gossip through other relays and direct peer connections

**d) Cryptographic relay accountability:**
Relays must sign all gossip messages with their TTC credential (ECDAA basename = epoch number):
- If a relay signs two different epoch states for the same epoch number → same basename → linkable → caught
- If a relay refuses to sign → other relays and wallets route around it
- The ECDAA linkability that catches wallet double-spenders also catches relay dishonesty

**e) Decentralized anti-Sybil for relays:**
- Physical proof: solar relays are tied to physical locations (solar panel, NFC range)
- Economic proof: staked bond per relay
- Social proof: relay operators known to local communities
- Cross-validation: relays in overlapping coverage areas can challenge each other's epoch claims

### What remains hard
- A relay that is the *only* path between two network partitions has outsized influence. No amount of cryptography fixes the last-mile physical topology problem.
- Mitigation: design relay mesh density requirements for coverage. Isolated single-relay communities accept higher risk (same as single-ISP villages today).

---

## 8. Putting It Together: Two Deployment Models

### Model A: Blockchain-Connected Decentralization
For regions with internet connectivity:

```
        Ethereum/Bitcoin L1
    ┌──────────┬──────────────┐
    │  Epoch   │  Revocation  │  Attestation
    │ Contract │  Registry    │  Provider Registry
    └────┬─────┴──────┬───────┘
         │            │
    ┌────┴────────────┴────┐
    │  Validator Network    │  (Staked nodes running TokenMap,
    │  (replaces operator)  │   Mint, Clerk, Swapper)
    └────┬────────────┬────┘
         │            │
    ┌────┴────┐  ┌────┴────┐
    │ Region A│  │ Region B│
    │ Relays  │  │ Relays  │
    └─────────┘  └─────────┘
```

- Epochs, revocations, attestation all governed by smart contracts
- TokenMap replicated across validator nodes (consensus on fork detection)
- Mint becomes a contract-authorized function (not a single server)
- Wallets and relays interact with whichever validator is nearest

### Model B: Network-Partitioned Mesh (the interesting one)
For regions without internet, using solar relay mesh:

```
    ┌──Regional Swap──┐     ┌──Regional Swap──┐
    │   Federation A   │────│   Federation B   │
    │  (bonded relays) │    │  (bonded relays) │
    └───────┬─────────┘    └───────┬─────────┘
            │                       │
    ┌───────┴───────┐       ┌──────┴────────┐
    │  Solar Relay   │       │  Solar Relay   │
    │  Mesh (local)  │       │  Mesh (local)  │
    │                │       │                │
    │  ZK prover     │       │  ZK prover     │
    │  (compresses   │       │  (compresses   │
    │   token chains)│       │   token chains)│
    └───────┬───────┘       └──────┬────────┘
            │                       │
      Credstick mesh          Credstick mesh
      (offline txns)          (offline txns)
```

Key properties:
- **Epochs:** Regional epoch advancement by federation consensus (relay operators vote). Syncs with global epoch when connectivity returns.
- **Revocations:** Local double-spend detection via relay-maintained token caches. Revocations propagated within the mesh immediately, synced globally later.
- **Attestation:** Pre-registered credentials only. No new wallet onboarding while offline (acceptable — wallets are provisioned when internet is available).
- **Token refresh:** Regional swap services with bonded token pools.
- **Chain compression:** ZK provers at relay points (or simpler: trusted relay attestation).
- **Abuse prevention:** Relay staking, reputation, cross-validation, ECDAA accountability.

### What must stay centralized (or federated) even in Model B
1. **Initial credential issuance** — requires hardware attestation verification against root CAs. Must happen when internet is available.
2. **Global supply cap enforcement** — the total value in circulation must be tracked. Regional services can't create value ex nihilo.
3. **Cross-region double-spend detection** — if someone double-spends a token in Region A and Region B simultaneously, detection requires cross-region sync. Regional federations must eventually reconcile.

---

## 9. Recommended Implementation Path

This is a research exploration — no code changes now. But if we were to incrementally decentralize:

**Phase 1 (smallest change, highest value):**
- Move epoch advancement to a smart contract (on existing Bitcoin/Ethereum bridge)
- Move revocation to an on-chain registry with double-spend proof submission
- Keeps: central TokenMap, central Mint, central Registrar

**Phase 2 (regional resilience):**
- Implement regional swap services (bonded relay operators with token pools)
- Add bloom filter authorization for regional swap keys
- Implement relay reputation tracking
- Keeps: central TokenMap as global source of truth, central credential issuance

**Phase 3 (ZK compression, research-heavy):**
- Design and implement ZK circuits for ECDAA chain verification (start with BN254/V0)
- Deploy ZK provers on relay hardware
- Add compressed token format to the proto definitions
- Keeps: eventual reconciliation with global TokenMap

**Phase 4 (full decentralization):**
- Replicate TokenMap across a validator network with consensus
- Decentralize credential issuance via attestation provider registry
- Cross-region reconciliation protocol for network partitions

---

## 10. Design Decisions (Resolved)

### Supply Model: Simple Elastic Policy
**Decision:** Policy-managed with a simple rule — constant supply × multiplier based on active wallets. Can bootstrap as L1-backed or fixed, then transition to elastic.

**Mechanism:**
```
contract ElasticMint {
    uint256 public baseSupply;         // Initial fixed supply
    uint256 public activeWalletCount;  // Updated from epoch data

    // Supply target = baseSupply * multiplier(activeWallets)
    // Multiplier is a simple piecewise function:
    //   1.0x for < 10K wallets
    //   1.5x for 10K-100K wallets
    //   2.0x for 100K-1M wallets
    //   etc.
    //
    // This prevents value concentration as the network grows
    // (avoiding the problem where splits dominate all transactions
    // because tokens are "too valuable" relative to goods).

    function adjustSupply(uint256 newActiveCount, bytes calldata epochProof) external {
        // Verify epoch proof showing active wallet count
        // If supply target > current supply: authorize minting
        // If supply target < current supply: no forced burn — natural expiry shrinks supply
    }
}
```

**Why this works for Briolette:** Tokens have `valid_until` expiration. If the supply target decreases, the Mint simply stops issuing new tokens and existing tokens expire naturally. No forced burns needed. The "active wallets" metric comes from epoch data (ticket issuance rate = proxy for active wallets).

**Regional implication:** During weeks-long partitions, regional federations use their pre-staked token pools. They cannot mint new tokens. This is a feature — it prevents regional inflation. When connectivity returns, the global supply adjusts based on actual activity.

### Offline Duration: Weeks+ (Remote/Conflict Zones)
**Decision:** Design for weeks-long network partitions. Regional federations must be nearly autonomous.

**Implications for each subsystem:**
- **Tickets:** Regional ticket lifetime must be ≥ 4 weeks. Regional swap services must be able to issue and refresh regional tickets without Clerk access.
- **Token expiration:** Regional swap services must maintain deep token pools (months of local transaction volume) to keep refreshing tokens.
- **Epoch data:** Regional epoch advancement must be self-sufficient — federation consensus among local relays, not dependent on global epoch contract.
- **Bloom filters:** With weeks of offline transactions, bloom filter capacity (currently sized for ~100 transactions per ticket lifetime) may need expansion, or regional swap auth must be readily available.
- **Double-spend detection:** Regional TokenMap replicas maintained by relay federations. Cross-region reconciliation happens when connectivity returns. During partition, regional detection is best-effort.
- **Credential issuance:** Wallets MUST be provisioned before entering an offline region. No new wallet onboarding while partitioned. This is acceptable — conflict/remote zone deployments would pre-provision devices.

### ZK Chain Compression: Dual-Track Approach
**Decision:** Deploy trusted relay attestation immediately as the practical path. Research ZK compression in parallel. Swap in ZK proofs when ready.

**Track 1 — Trusted Relay Attestation (deploy first):**
- Relay verifies full token chain cryptographically
- Issues a "compression attestation" signed with its bonded TTC credential
- Compressed format: `base + attestation_signature + current_transfer`
- Trust model: relay stake backs the attestation. Invalid attestation → slash
- Advantage: deployable on existing relay hardware (nRF52840 or Raspberry Pi)
- Limitation: trust is economic (stake), not cryptographic (ZK)

**Track 2 — ZK Proofs (research in parallel):**
- Design Groth16 circuit for BN254 ECDAA chain verification
- Target: compress N transfers into ~200-byte proof + pseudonym commitments
- Benchmark circuit size and prover time on relay-class hardware
- Evaluate recursive SNARKs for BLS12-381 (V1) support
- No deployment timeline commitment — this is research

**Transition plan:** Both tracks produce the same `CompressedToken` wire format. The `proof_type` field distinguishes them. Verifiers that support ZK proofs prefer them; all verifiers accept trusted attestations. Over time, as ZK provers deploy, trusted attestations phase out naturally.

## Key Files Referenced

- `src/proto/proto/clerk.proto` — EpochUpdate, EpochData, GetTickets, AddEpoch
- `src/proto/proto/tokenmap.proto` — RevocationData, Entry, Abuse, FindByHolder
- `src/proto/proto/registrar.proto` — RegisterRequest, HardwareId, Signature, Algorithm
- `src/proto/proto/swapper.proto` — AuthorizeSwap (Schnorr bloom bypass)
- `src/proto/proto/mint.proto` — GetTokens
- `src/proto/proto/token.proto` — Token, History, Transfer, Tag, SignedTicket
- `src/proto/proto/bridge.proto` — L1 deposit/withdrawal
- `src/proto/proto/recovery.proto` — RecoveryBinding, RecoverTokens
- `docs/design/theory_of_operation.md` — Token lifecycle, gossip, ticketing, revocation
- `docs/design/security_model.md` — Invariants 1-4, eviction completeness theorem
- `docs/design/recovery.md` — Recovery protocol, timing constraints
- `docs/design/bitcoin_l2.md` — Existing Bitcoin bridge architecture
