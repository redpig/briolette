-------------------------- MODULE RevocationProtocol --------------------------
(*
 * Formal model of briolette's revocation lifecycle: detection through eviction.
 *
 * Maps to: src/tokenmap/src/server.rs (RevocationData creation),
 *          src/clerk/src/server.rs (epoch publishing with revocation lists),
 *          src/simulation/briolettesim/src/main.rs (do_transaction revocation check),
 *          docs/design/security_model.md (Invariant 4, Eviction Theorem)
 *
 * Lifecycle:
 *   1. Double-spend detected -> RevocationData created
 *   2. Operator publishes new epoch with revocation list
 *   3. Gossip propagates epoch to all honest peers
 *   4. Honest peers reject transactions with revoked wallets
 *   5. Revoked wallet's tickets expire, clerk refuses new tickets
 *
 * Verifies:
 *   - Cumulative revocation (once revoked, always revoked)
 *   - Enforcement (honest peers at epoch >= k reject revoked wallet)
 *   - Eviction completeness (detected => eventually evicted)
 *   - No false revocation (only detected double-spenders get revoked)
 *)
EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    Wallets,         \* Set of all wallet identifiers
    Honest,          \* Subset of honest wallets
    Edges,           \* Topology edges for gossip
    MaxEpoch,        \* Maximum epoch number
    TicketLifetime   \* Number of epochs a ticket is valid

ASSUME Honest \subseteq Wallets
ASSUME MaxEpoch \in Nat /\ MaxEpoch >= 2
ASSUME TicketLifetime \in Nat /\ TicketLifetime >= 1

VARIABLES
    epoch,           \* Function: Wallet -> current epoch knowledge
    currentEpoch,    \* Latest epoch published by authority
    revoked,         \* Function: Epoch -> Set(Wallet) revoked in that epoch
    detected,        \* Set of wallets whose double-spend has been detected
    evicted,         \* Set of wallets fully unable to transact
    ticketExpiry,    \* Function: Wallet -> epoch when current tickets expire
    hasValidTicket,  \* Function: Wallet -> BOOLEAN
    txnRejected      \* Set of (wallet, peer) pairs where transaction was rejected

vars == <<epoch, currentEpoch, revoked, detected, evicted,
          ticketExpiry, hasValidTicket, txnRejected>>

\* -----------------------------------------------------------------------
\* Helpers
\* -----------------------------------------------------------------------

\* Cumulative revocation set up to epoch e
RevokedAtEpoch(e) ==
    UNION {revoked[i] : i \in 0..e}

\* Is wallet w revoked at epoch e?
IsRevoked(w, e) ==
    w \in RevokedAtEpoch(e)

\* Can wallet w transact with honest peer p?
CanTransact(w, p) ==
    /\ p \in Honest
    /\ hasValidTicket[w]
    /\ ~IsRevoked(w, epoch[p])

\* Is wallet fully evicted? (no honest peer will transact with it)
IsFullyEvicted(w) ==
    /\ \A p \in Honest: IsRevoked(w, epoch[p])
    /\ ~hasValidTicket[w]

\* -----------------------------------------------------------------------
\* Actions
\* -----------------------------------------------------------------------

\* Step 1: Validator detects a double-spend
\* Maps to: tokenmap update_impl detecting fork -> RevocationData
DetectDoubleSpend(w) ==
    /\ w \in Wallets
    /\ w \notin Honest  \* Only dishonest wallets double-spend
    /\ w \notin detected
    /\ detected' = detected \union {w}
    /\ UNCHANGED <<epoch, currentEpoch, revoked, evicted,
                   ticketExpiry, hasValidTicket, txnRejected>>

\* Step 2: Operator publishes new epoch, automatically including all
\* detected-but-not-yet-revoked wallets in the revocation list.
\* Maps to: clerk publishing EpochUpdate with revocation bitfield.
\* In the real system, the operator's epoch-advance process checks for
\* pending revocations and includes them.
AdvanceEpoch ==
    /\ currentEpoch < MaxEpoch
    /\ LET newlyRevoked == {w \in detected : ~IsRevoked(w, currentEpoch)}
       IN revoked' = [revoked EXCEPT ![currentEpoch + 1] =
            revoked[currentEpoch] \union newlyRevoked]
    /\ currentEpoch' = currentEpoch + 1
    /\ UNCHANGED <<epoch, detected, evicted, ticketExpiry, hasValidTicket, txnRejected>>

\* Step 3: Gossip propagates epoch between transacting peers
\* Maps to: do_transaction gossip in briolettesim
GossipEpoch(w1, w2) ==
    /\ w1 \in Wallets /\ w2 \in Wallets
    /\ w1 # w2
    /\ \E e \in Edges: w1 \in e /\ w2 \in e
    /\ w1 \in Honest \/ w2 \in Honest
    /\ LET maxE == IF epoch[w1] >= epoch[w2] THEN epoch[w1] ELSE epoch[w2]
       IN epoch' = [epoch EXCEPT
            ![w1] = IF w1 \in Honest THEN maxE ELSE epoch[w1],
            ![w2] = IF w2 \in Honest THEN maxE ELSE epoch[w2]]
    /\ UNCHANGED <<currentEpoch, revoked, detected, evicted,
                   ticketExpiry, hasValidTicket, txnRejected>>

\* Direct synchronization with clerk
SynchronizeWithClerk(w) ==
    /\ w \in Honest
    /\ epoch' = [epoch EXCEPT ![w] = currentEpoch]
    /\ UNCHANGED <<currentEpoch, revoked, detected, evicted,
                   ticketExpiry, hasValidTicket, txnRejected>>

\* Step 4: Honest peer attempts transaction with wallet, checks revocation
\* Maps to: do_transaction revocation check (briolettesim lines 86-100)
AttemptTransaction(w, peer) ==
    /\ peer \in Honest
    /\ w \in Wallets
    /\ w # peer
    /\ LET txnEpoch == IF epoch[w] >= epoch[peer] THEN epoch[w] ELSE epoch[peer]
       IN IF IsRevoked(w, txnEpoch)
          THEN /\ txnRejected' = txnRejected \union {<<w, peer>>}
               /\ UNCHANGED <<epoch, currentEpoch, revoked, detected,
                              evicted, ticketExpiry, hasValidTicket>>
          ELSE UNCHANGED vars

\* Step 5: Ticket expiry check — advance epoch and check if tickets expired
TicketExpiryCheck(w) ==
    /\ w \in Wallets
    /\ hasValidTicket[w] = TRUE
    /\ currentEpoch > ticketExpiry[w]
    /\ hasValidTicket' = [hasValidTicket EXCEPT ![w] = FALSE]
    /\ UNCHANGED <<epoch, currentEpoch, revoked, detected, evicted,
                   ticketExpiry, txnRejected>>

\* Step 5b: Wallet tries to get new tickets, rejected if revoked
RequestTickets(w) ==
    /\ w \in Wallets
    /\ hasValidTicket[w] = FALSE
    /\ IF IsRevoked(w, currentEpoch)
       THEN \* Rejected — wallet becomes evicted if all peers also reject
            /\ evicted' = evicted \union
                 (IF \A p \in Honest: IsRevoked(w, epoch[p])
                  THEN {w} ELSE {})
            /\ UNCHANGED <<epoch, currentEpoch, revoked, detected,
                           ticketExpiry, hasValidTicket, txnRejected>>
       ELSE \* Granted new tickets
            /\ ticketExpiry' = [ticketExpiry EXCEPT ![w] = currentEpoch + TicketLifetime]
            /\ hasValidTicket' = [hasValidTicket EXCEPT ![w] = TRUE]
            /\ UNCHANGED <<epoch, currentEpoch, revoked, detected, evicted, txnRejected>>

\* Update eviction status
CheckEviction(w) ==
    /\ w \in detected
    /\ w \notin evicted
    /\ IsFullyEvicted(w)
    /\ evicted' = evicted \union {w}
    /\ UNCHANGED <<epoch, currentEpoch, revoked, detected,
                   ticketExpiry, hasValidTicket, txnRejected>>

\* -----------------------------------------------------------------------
\* Specification
\* -----------------------------------------------------------------------

Init ==
    /\ epoch = [w \in Wallets |-> 0]
    /\ currentEpoch = 0
    /\ revoked = [e \in 0..MaxEpoch |-> {}]
    /\ detected = {}
    /\ evicted = {}
    /\ ticketExpiry = [w \in Wallets |-> TicketLifetime]
    /\ hasValidTicket = [w \in Wallets |-> TRUE]
    /\ txnRejected = {}

Next ==
    \/ \E w \in Wallets: DetectDoubleSpend(w)
    \/ AdvanceEpoch
    \/ \E w1 \in Wallets, w2 \in Wallets: GossipEpoch(w1, w2)
    \/ \E w \in Wallets: SynchronizeWithClerk(w)
    \/ \E w \in Wallets, p \in Wallets: AttemptTransaction(w, p)
    \/ \E w \in Wallets: TicketExpiryCheck(w)
    \/ \E w \in Wallets: RequestTickets(w)
    \/ \E w \in Wallets: CheckEviction(w)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* -----------------------------------------------------------------------
\* Safety Invariants
\* -----------------------------------------------------------------------

TypeInvariant ==
    /\ \A w \in Wallets: epoch[w] \in 0..MaxEpoch
    /\ currentEpoch \in 0..MaxEpoch
    /\ detected \subseteq Wallets
    /\ evicted \subseteq Wallets
    /\ \A w \in Wallets: hasValidTicket[w] \in BOOLEAN

\* Revocation lists are cumulative across epochs
CumulativeRevocation ==
    \A e1 \in 0..MaxEpoch: \A e2 \in 0..MaxEpoch:
        e1 <= e2 => revoked[e1] \subseteq RevokedAtEpoch(e2)

\* Enforcement: honest peer with epoch >= revocation epoch rejects revoked wallet
RevocationEnforced ==
    \A w \in detected: \A p \in Honest:
        IsRevoked(w, epoch[p]) =>
            (<<w, p>> \in txnRejected \/ ~(\E e \in Edges: w \in e /\ p \in e))

\* No false revocation: only detected wallets appear in revocation lists
NoFalseRevocation ==
    \A e \in 0..MaxEpoch: revoked[e] \subseteq detected

\* Evicted wallets are a subset of detected wallets
EvictedSubsetDetected ==
    evicted \subseteq detected

\* Once evicted, wallet cannot have valid tickets
EvictedMeansNoTickets ==
    \A w \in evicted: ~hasValidTicket[w]

\* Combined invariant
Invariant ==
    /\ TypeInvariant
    /\ CumulativeRevocation
    /\ NoFalseRevocation
    /\ EvictedSubsetDetected

\* -----------------------------------------------------------------------
\* Liveness Properties
\* -----------------------------------------------------------------------

\* Every detected double-spender is eventually evicted
EvictionCompleteness ==
    \A w \in Wallets \ Honest:
        [](w \in detected => <>(w \in evicted))

=============================================================================
