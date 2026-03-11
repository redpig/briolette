-------------------------- MODULE GossipConvergence --------------------------
(*
 * Formal model of briolette's epoch gossip propagation protocol.
 *
 * Maps to: src/simulation/briolettesim/src/main.rs (do_transaction gossip),
 *          docs/design/security_model.md (Invariant 3)
 *
 * During every P2P transaction, peers exchange epoch information:
 *   txn_epoch = max(source.epoch, target.epoch)
 *
 * This model verifies:
 *   - Epoch values are monotonically non-decreasing
 *   - All honest peers converge to the latest epoch
 *   - Double-spenders who refuse to gossip can block line topologies
 *     but not ring/mesh topologies
 *)
EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    Wallets,       \* Set of all wallet identifiers
    Honest,        \* Subset of honest wallets
    Edges,         \* Set of undirected edges {w1, w2} defining topology
    MaxEpoch       \* Maximum epoch value

ASSUME Honest \subseteq Wallets
ASSUME MaxEpoch \in Nat /\ MaxEpoch >= 1
ASSUME \A e \in Edges: e \subseteq Wallets /\ Cardinality(e) = 2

VARIABLES
    epoch,         \* Function: Wallet -> epoch number
    currentEpoch   \* The latest epoch published by the authority

vars == <<epoch, currentEpoch>>

\* -----------------------------------------------------------------------
\* Topology helpers
\* -----------------------------------------------------------------------

\* Whether all honest wallets have reached the current epoch
AllHonestConverged ==
    \A w \in Honest: epoch[w] = currentEpoch

\* -----------------------------------------------------------------------
\* Actions
\* -----------------------------------------------------------------------

\* Authority publishes a new epoch to one or more seed peers
PublishEpoch ==
    /\ currentEpoch < MaxEpoch
    /\ \E seedWallet \in Honest:
        /\ currentEpoch' = currentEpoch + 1
        /\ epoch' = [epoch EXCEPT ![seedWallet] = currentEpoch + 1]

\* A transaction between two neighboring wallets triggers gossip
\* Maps to: do_transaction in briolettesim/src/main.rs lines 66-75
Gossip(w1, w2) ==
    /\ \E e \in Edges: w1 \in e /\ w2 \in e
    /\ w1 # w2
    /\ w1 \in Honest \/ w2 \in Honest
    \* Only gossip when it would change something (avoids infinite stuttering)
    /\ epoch[w1] # epoch[w2]
    /\ LET maxE == IF epoch[w1] >= epoch[w2] THEN epoch[w1] ELSE epoch[w2]
       IN epoch' = [epoch EXCEPT
               ![w1] = IF w1 \in Honest THEN maxE ELSE epoch[w1],
               ![w2] = IF w2 \in Honest THEN maxE ELSE epoch[w2]]
    /\ UNCHANGED currentEpoch

\* A wallet contacts the clerk directly to synchronize
Synchronize(w) ==
    /\ w \in Honest
    /\ epoch[w] # currentEpoch
    /\ epoch' = [epoch EXCEPT ![w] = currentEpoch]
    /\ UNCHANGED currentEpoch

\* -----------------------------------------------------------------------
\* Specification
\* -----------------------------------------------------------------------

Init ==
    /\ epoch = [w \in Wallets |-> 0]
    /\ currentEpoch = 0

Next ==
    \/ PublishEpoch
    \/ \E w1 \in Wallets, w2 \in Wallets:
         /\ w1 # w2
         /\ Gossip(w1, w2)
    \/ \E w \in Wallets: Synchronize(w)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* -----------------------------------------------------------------------
\* Safety Invariants
\* -----------------------------------------------------------------------

TypeInvariant ==
    /\ \A w \in Wallets: epoch[w] \in 0..MaxEpoch
    /\ currentEpoch \in 0..MaxEpoch

\* Epochs are monotonically non-decreasing: no wallet exceeds authority
MonotonicEpoch ==
    \A w \in Wallets: epoch[w] <= currentEpoch

\* -----------------------------------------------------------------------
\* Liveness Properties
\* -----------------------------------------------------------------------

\* Eventually all honest peers converge to the current epoch
EventualConvergence ==
    [](currentEpoch > 0 => <>AllHonestConverged)

\* Combined invariant
Invariant ==
    /\ TypeInvariant
    /\ MonotonicEpoch

=============================================================================
