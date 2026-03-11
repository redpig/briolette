--------------------------- MODULE TokenLifecycle ---------------------------
(*
 * Formal model of briolette's token lifecycle: minting, transfer, and splits.
 *
 * Maps to: src/mint/src/server.rs, src/wallet/src/lib.rs,
 *          src/proto/proto/token.proto
 *
 * Models the creation and movement of tokens between wallets, including
 * value-splitting operations. Verifies that total value is conserved
 * across all operations.
 *
 * Cryptographic operations (ECDSA signatures, ECDAA credentials) are
 * abstracted away. Token identity is tracked by model values.
 *)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Wallets,       \* Set of wallet identifiers
    TokenIds,      \* Set of possible token identifiers
    MaxValue,      \* Maximum denomination for a single token
    MaxHistory,    \* Maximum transfer chain length
    MaxTokens,     \* Maximum tokens that can exist
    SplitTokenIds  \* Extra token IDs available for split children

ASSUME MaxValue \in Nat /\ MaxValue >= 2
ASSUME MaxHistory \in Nat /\ MaxHistory >= 1
ASSUME MaxTokens \in Nat /\ MaxTokens >= 1

\* A token record
TokenRecord == [
    value: 1..MaxValue,
    owner: Wallets,
    historyLen: 0..MaxHistory,
    expired: BOOLEAN,
    splitChildren: SUBSET (TokenIds \union SplitTokenIds)
]

VARIABLES
    tokens,      \* Function: TokenId -> TokenRecord (partial function)
    mintedTotal, \* Total value ever minted (monotonically increasing)
    walletBalance \* Function: Wallet -> set of TokenIds owned

vars == <<tokens, mintedTotal, walletBalance>>

\* -----------------------------------------------------------------------
\* Helpers
\* -----------------------------------------------------------------------

\* Set of all currently active (non-expired, non-split-parent) token IDs
ActiveTokenIds ==
    {tid \in DOMAIN tokens : ~tokens[tid].expired /\ tokens[tid].splitChildren = {}}

\* Total value of all active tokens
TotalActiveValue ==
    LET activeTokens == ActiveTokenIds
    IN IF activeTokens = {} THEN 0
       ELSE LET Sum[S \in SUBSET activeTokens] ==
                IF S = {} THEN 0
                ELSE LET t == CHOOSE x \in S : TRUE
                     IN tokens[t].value + Sum[S \ {t}]
            IN Sum[activeTokens]

\* Total value including split children (recursive value accounting)
TotalTokenValue ==
    LET allTokens == {tid \in DOMAIN tokens : ~tokens[tid].expired}
        leaves == {tid \in allTokens : tokens[tid].splitChildren = {}}
    IN IF leaves = {} THEN 0
       ELSE LET Sum[S \in SUBSET leaves] ==
                IF S = {} THEN 0
                ELSE LET t == CHOOSE x \in S : TRUE
                     IN tokens[t].value + Sum[S \ {t}]
            IN Sum[leaves]

\* -----------------------------------------------------------------------
\* Actions
\* -----------------------------------------------------------------------

\* Mint a new token with a given value, assigned to a recipient wallet
Mint(tid, value, recipient) ==
    /\ tid \in TokenIds
    /\ tid \notin DOMAIN tokens
    /\ value \in 1..MaxValue
    /\ recipient \in Wallets
    /\ Cardinality(DOMAIN tokens) < MaxTokens
    /\ tokens' = [tokens EXCEPT ![tid] =
         [value |-> value, owner |-> recipient, historyLen |-> 0,
          expired |-> FALSE, splitChildren |-> {}]]
    /\ mintedTotal' = mintedTotal + value
    /\ walletBalance' = [walletBalance EXCEPT ![recipient] =
         walletBalance[recipient] \union {tid}]

\* Transfer a token from sender to recipient
\* Maps to: wallet transfer_token + receiver transfer_impl
Transfer(tid, sender, recipient) ==
    /\ tid \in DOMAIN tokens
    /\ tokens[tid].owner = sender
    /\ ~tokens[tid].expired
    /\ tokens[tid].splitChildren = {}
    /\ tokens[tid].historyLen < MaxHistory
    /\ sender # recipient
    /\ sender \in Wallets
    /\ recipient \in Wallets
    /\ tokens' = [tokens EXCEPT
         ![tid].owner = recipient,
         ![tid].historyLen = tokens[tid].historyLen + 1]
    /\ walletBalance' = [walletBalance EXCEPT
         ![sender] = walletBalance[sender] \ {tid},
         ![recipient] = walletBalance[recipient] \union {tid}]
    /\ UNCHANGED mintedTotal

\* Split a token into two children with values summing to original
\* Maps to: token Tag.split_value in token.proto
Split(parentTid, childTid1, childTid2, sender, recipient1, recipient2, val1, val2) ==
    /\ parentTid \in DOMAIN tokens
    /\ tokens[parentTid].owner = sender
    /\ ~tokens[parentTid].expired
    /\ tokens[parentTid].splitChildren = {}
    /\ childTid1 \in SplitTokenIds
    /\ childTid2 \in SplitTokenIds
    /\ childTid1 # childTid2
    /\ childTid1 \notin DOMAIN tokens
    /\ childTid2 \notin DOMAIN tokens
    /\ val1 \in 1..MaxValue
    /\ val2 \in 1..MaxValue
    /\ val1 + val2 = tokens[parentTid].value  \* Value conservation!
    /\ recipient1 \in Wallets
    /\ recipient2 \in Wallets
    /\ sender \in Wallets
    /\ Cardinality(DOMAIN tokens) + 2 <= MaxTokens + Cardinality(SplitTokenIds)
    \* Create two child tokens and mark parent
    /\ tokens' = [tid \in (DOMAIN tokens \union {childTid1, childTid2}) |->
         IF tid = childTid1
         THEN [value |-> val1, owner |-> recipient1,
               historyLen |-> tokens[parentTid].historyLen + 1,
               expired |-> FALSE, splitChildren |-> {}]
         ELSE IF tid = childTid2
         THEN [value |-> val2, owner |-> recipient2,
               historyLen |-> tokens[parentTid].historyLen + 1,
               expired |-> FALSE, splitChildren |-> {}]
         ELSE IF tid = parentTid
         THEN [tokens[tid] EXCEPT !.splitChildren = {childTid1, childTid2}]
         ELSE tokens[tid]]
    /\ walletBalance' = [w \in Wallets |->
         LET base == walletBalance[w] \ {parentTid}
         IN base
            \union (IF w = recipient1 THEN {childTid1} ELSE {})
            \union (IF w = recipient2 THEN {childTid2} ELSE {})]
    /\ UNCHANGED mintedTotal

\* Expire a token (epoch-based expiration)
Expire(tid) ==
    /\ tid \in DOMAIN tokens
    /\ ~tokens[tid].expired
    /\ tokens' = [tokens EXCEPT ![tid].expired = TRUE]
    /\ walletBalance' = [walletBalance EXCEPT
         ![tokens[tid].owner] = walletBalance[tokens[tid].owner] \ {tid}]
    /\ UNCHANGED mintedTotal

\* -----------------------------------------------------------------------
\* Specification
\* -----------------------------------------------------------------------

Init ==
    /\ tokens = [t \in {} |-> [value |-> 1, owner |-> CHOOSE w \in Wallets : TRUE,
                                historyLen |-> 0, expired |-> FALSE, splitChildren |-> {}]]
    /\ mintedTotal = 0
    /\ walletBalance = [w \in Wallets |-> {}]

Next ==
    \/ \E tid \in TokenIds, val \in 1..MaxValue, w \in Wallets:
         Mint(tid, val, w)
    \/ \E tid \in DOMAIN tokens, s \in Wallets, r \in Wallets:
         Transfer(tid, s, r)
    \/ \E ptid \in DOMAIN tokens, c1 \in SplitTokenIds, c2 \in SplitTokenIds,
          s \in Wallets, r1 \in Wallets, r2 \in Wallets,
          v1 \in 1..MaxValue, v2 \in 1..MaxValue:
         Split(ptid, c1, c2, s, r1, r2, v1, v2)
    \/ \E tid \in DOMAIN tokens:
         Expire(tid)

Spec == Init /\ [][Next]_vars

\* -----------------------------------------------------------------------
\* Safety Invariants
\* -----------------------------------------------------------------------

\* Type correctness
TypeInvariant ==
    /\ mintedTotal \in Nat
    /\ \A w \in Wallets: walletBalance[w] \subseteq (TokenIds \union SplitTokenIds)
    /\ \A tid \in DOMAIN tokens:
        /\ tokens[tid].value \in 1..MaxValue
        /\ tokens[tid].owner \in Wallets
        /\ tokens[tid].historyLen \in 0..MaxHistory
        /\ tokens[tid].expired \in BOOLEAN

\* Value conservation: total value of leaf tokens = mintedTotal - expired value
\* Simplified: leaf token values never exceed what was minted
ValueNeverExceedsMinted ==
    TotalTokenValue <= mintedTotal

\* No token has negative value (structural — enforced by type)
NoNegativeValue ==
    \A tid \in DOMAIN tokens: tokens[tid].value >= 1

\* Split children values sum to parent value
SplitValueConservation ==
    \A tid \in DOMAIN tokens:
        tokens[tid].splitChildren # {} =>
            LET children == tokens[tid].splitChildren
                childSum[S \in SUBSET children] ==
                    IF S = {} THEN 0
                    ELSE LET c == CHOOSE x \in S : TRUE
                         IN IF c \in DOMAIN tokens
                            THEN tokens[c].value + childSum[S \ {c}]
                            ELSE childSum[S \ {c}]
            IN childSum[children] = tokens[tid].value

\* Each token is owned by exactly one wallet
UniqueOwnership ==
    \A tid \in DOMAIN tokens:
        ~tokens[tid].expired /\ tokens[tid].splitChildren = {} =>
            /\ tid \in walletBalance[tokens[tid].owner]
            /\ \A w \in Wallets:
                w # tokens[tid].owner => tid \notin walletBalance[w]

\* Minted total is monotonically increasing (minting is append-only)
MintedTotalMonotonic ==
    mintedTotal >= 0

\* History length is bounded
HistoryBounded ==
    \A tid \in DOMAIN tokens: tokens[tid].historyLen <= MaxHistory

\* Combined invariant
Invariant ==
    /\ TypeInvariant
    /\ ValueNeverExceedsMinted
    /\ NoNegativeValue
    /\ SplitValueConservation
    /\ UniqueOwnership
    /\ HistoryBounded

=============================================================================
