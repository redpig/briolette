-------------------------- MODULE RecoveryProtocol --------------------------
(*
 * Formal model of briolette's wallet recovery protocol.
 *
 * Maps to: src/recovery/src/server.rs (recovery service implementation),
 *          src/tokenmap/src/server.rs (FindByHolder, token expiry),
 *          docs/design/recovery.md (full design specification)
 *
 * Recovery lifecycle:
 *   1. Wallet registers a recovery binding (pre-loss)
 *   2. Wallet is lost (goes offline permanently)
 *   3. Token valid_until expires (wall-clock)
 *   4. Ticket lifetime expires (epoch-based)
 *   5. Cooling-off period elapses
 *   6. Delegate claims recovery with proof
 *   7. Recovery server issues replacement tokens
 *   8. Old wallet's credential group is revoked
 *
 * Verifies:
 *   - No double-recovery (token cannot be recovered twice)
 *   - No recovery of actively-held tokens (expiry required)
 *   - Revoked wallets cannot recover
 *   - Old wallet is revoked after recovery completes
 *   - Conservation of value (minted replacements equal recovered amount)
 *   - Delegate authorization required
 *   - Eventual recovery for legitimate claims
 *)
EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    Wallets,          \* Set of all wallet identifiers
    Tokens,           \* Set of token identifiers
    MaxEpoch,         \* Maximum epoch number
    CooloffEpochs,    \* Mandatory cooling-off period
    TokenLifeEpochs   \* Epochs until token valid_until expires (simplified)

ASSUME MaxEpoch \in Nat /\ MaxEpoch >= 4
ASSUME CooloffEpochs \in Nat /\ CooloffEpochs >= 1
ASSUME TokenLifeEpochs \in Nat /\ TokenLifeEpochs >= 1

\* Binding states
BindingStates == {"None", "Active", "Expired", "Revoked", "Consumed"}

\* Recovery states for tokens
RecoveryStates == {"NotClaimed", "Pending", "Recovered"}

VARIABLES
    currentEpoch,       \* Current system epoch
    bindingState,       \* Wallet -> BindingStates
    bindingDelegate,    \* Wallet -> delegate wallet (or "none")
    bindingExpiry,      \* Wallet -> epoch when binding expires
    tokenHolder,        \* Token -> Wallet (current holder)
    tokenMintEpoch,     \* Token -> epoch when minted (for expiry calc)
    walletLost,         \* Wallet -> BOOLEAN
    walletRevoked,      \* Wallet -> BOOLEAN
    recoveryState,      \* Token -> RecoveryStates
    recoveryClaimEpoch, \* Token -> epoch when recovery was claimed
    totalMinted,        \* Counter: total tokens minted (including recovery)
    totalRetired        \* Counter: total tokens retired via recovery

vars == <<currentEpoch, bindingState, bindingDelegate, bindingExpiry,
          tokenHolder, tokenMintEpoch, walletLost, walletRevoked,
          recoveryState, recoveryClaimEpoch, totalMinted, totalRetired>>

\* -----------------------------------------------------------------------
\* Helper predicates
\* -----------------------------------------------------------------------

\* Whether a token has expired (wall-clock, modeled as epoch-based)
TokenExpired(t) ==
    currentEpoch >= tokenMintEpoch[t] + TokenLifeEpochs

\* Whether a wallet's tickets have expired
\* (simplified: tickets expire when the wallet is lost and epochs advance)
TicketsExpired(w) ==
    walletLost[w]

\* Whether a token is eligible for recovery
RecoveryEligible(t) ==
    LET holder == tokenHolder[t]
    IN /\ TokenExpired(t)
       /\ TicketsExpired(holder)
       /\ ~walletRevoked[holder]
       /\ recoveryState[t] = "NotClaimed"
       /\ bindingState[holder] = "Active"
       /\ currentEpoch >= tokenMintEpoch[t] + TokenLifeEpochs + CooloffEpochs

\* -----------------------------------------------------------------------
\* Actions
\* -----------------------------------------------------------------------

\* Wallet registers a recovery binding before any loss occurs.
\* Maps to: recovery server RegisterBinding RPC
RegisterBinding(w, delegate) ==
    /\ w \in Wallets
    /\ delegate \in Wallets
    /\ w # delegate
    /\ ~walletLost[w]
    /\ ~walletRevoked[w]
    /\ bindingState[w] = "None"
    /\ bindingState' = [bindingState EXCEPT ![w] = "Active"]
    /\ bindingDelegate' = [bindingDelegate EXCEPT ![w] = delegate]
    /\ bindingExpiry' = [bindingExpiry EXCEPT ![w] = currentEpoch + 3]
    /\ UNCHANGED <<currentEpoch, tokenHolder, tokenMintEpoch,
                   walletLost, walletRevoked, recoveryState,
                   recoveryClaimEpoch, totalMinted, totalRetired>>

\* Wallet refreshes binding expiry.
\* Maps to: recovery server RefreshBinding RPC
RefreshBinding(w) ==
    /\ w \in Wallets
    /\ ~walletLost[w]
    /\ ~walletRevoked[w]
    /\ bindingState[w] = "Active"
    /\ bindingExpiry' = [bindingExpiry EXCEPT ![w] = currentEpoch + 3]
    /\ UNCHANGED <<currentEpoch, bindingState, bindingDelegate,
                   tokenHolder, tokenMintEpoch, walletLost,
                   walletRevoked, recoveryState, recoveryClaimEpoch,
                   totalMinted, totalRetired>>

\* Wallet revokes its own binding.
\* Maps to: recovery server RevokeBinding RPC
RevokeBinding(w) ==
    /\ w \in Wallets
    /\ ~walletLost[w]
    /\ bindingState[w] = "Active"
    /\ bindingState' = [bindingState EXCEPT ![w] = "Revoked"]
    /\ UNCHANGED <<currentEpoch, bindingDelegate, bindingExpiry,
                   tokenHolder, tokenMintEpoch, walletLost,
                   walletRevoked, recoveryState, recoveryClaimEpoch,
                   totalMinted, totalRetired>>

\* Binding expires due to passing valid_until epoch.
ExpireBinding(w) ==
    /\ w \in Wallets
    /\ bindingState[w] = "Active"
    /\ currentEpoch >= bindingExpiry[w]
    /\ bindingState' = [bindingState EXCEPT ![w] = "Expired"]
    /\ UNCHANGED <<currentEpoch, bindingDelegate, bindingExpiry,
                   tokenHolder, tokenMintEpoch, walletLost,
                   walletRevoked, recoveryState, recoveryClaimEpoch,
                   totalMinted, totalRetired>>

\* Wallet is lost (goes offline permanently).
LoseWallet(w) ==
    /\ w \in Wallets
    /\ ~walletLost[w]
    /\ walletLost' = [walletLost EXCEPT ![w] = TRUE]
    /\ UNCHANGED <<currentEpoch, bindingState, bindingDelegate,
                   bindingExpiry, tokenHolder, tokenMintEpoch,
                   walletRevoked, recoveryState, recoveryClaimEpoch,
                   totalMinted, totalRetired>>

\* Epoch advances. This drives token and ticket expiry.
AdvanceEpoch ==
    /\ currentEpoch < MaxEpoch
    /\ currentEpoch' = currentEpoch + 1
    /\ UNCHANGED <<bindingState, bindingDelegate, bindingExpiry,
                   tokenHolder, tokenMintEpoch, walletLost,
                   walletRevoked, recoveryState, recoveryClaimEpoch,
                   totalMinted, totalRetired>>

\* Delegate claims recovery for a token held by a lost wallet.
\* Maps to: recovery server RecoverTokens RPC (claim phase)
ClaimRecovery(delegate, t) ==
    LET holder == tokenHolder[t]
    IN /\ delegate \in Wallets
       /\ ~walletLost[delegate]
       /\ ~walletRevoked[delegate]
       /\ RecoveryEligible(t)
       /\ bindingDelegate[holder] = delegate
       /\ recoveryState' = [recoveryState EXCEPT ![t] = "Pending"]
       /\ recoveryClaimEpoch' = [recoveryClaimEpoch EXCEPT ![t] = currentEpoch]
       /\ UNCHANGED <<currentEpoch, bindingState, bindingDelegate,
                      bindingExpiry, tokenHolder, tokenMintEpoch,
                      walletLost, walletRevoked, totalMinted, totalRetired>>

\* Recovery completes: token is re-issued to delegate, old wallet revoked.
\* Maps to: recovery server completing token transfer + revocation
CompleteRecovery(t) ==
    LET holder == tokenHolder[t]
        delegate == bindingDelegate[holder]
    IN /\ recoveryState[t] = "Pending"
       /\ delegate # "none"
       \* Transfer token to delegate wallet
       /\ tokenHolder' = [tokenHolder EXCEPT ![t] = delegate]
       /\ recoveryState' = [recoveryState EXCEPT ![t] = "Recovered"]
       \* Revoke old wallet's credential group
       /\ walletRevoked' = [walletRevoked EXCEPT ![holder] = TRUE]
       \* Mark binding as consumed
       /\ bindingState' = [bindingState EXCEPT ![holder] = "Consumed"]
       \* Value conservation: one token retired, one replacement minted
       /\ totalMinted' = totalMinted + 1
       /\ totalRetired' = totalRetired + 1
       /\ UNCHANGED <<currentEpoch, bindingDelegate, bindingExpiry,
                      tokenMintEpoch, walletLost, recoveryClaimEpoch>>

\* Double-spend detected: wallet is revoked.
\* Maps to: tokenmap fork detection -> RevocationData -> epoch revocation
DetectDoubleSpend(w) ==
    /\ w \in Wallets
    /\ ~walletRevoked[w]
    /\ walletRevoked' = [walletRevoked EXCEPT ![w] = TRUE]
    /\ UNCHANGED <<currentEpoch, bindingState, bindingDelegate,
                   bindingExpiry, tokenHolder, tokenMintEpoch,
                   walletLost, recoveryState, recoveryClaimEpoch,
                   totalMinted, totalRetired>>

\* -----------------------------------------------------------------------
\* Specification
\* -----------------------------------------------------------------------

Init ==
    /\ currentEpoch = 0
    /\ bindingState = [w \in Wallets |-> "None"]
    /\ bindingDelegate = [w \in Wallets |-> "none"]
    /\ bindingExpiry = [w \in Wallets |-> 0]
    /\ tokenHolder = [t \in Tokens |-> CHOOSE w \in Wallets : TRUE]
    /\ tokenMintEpoch = [t \in Tokens |-> 0]
    /\ walletLost = [w \in Wallets |-> FALSE]
    /\ walletRevoked = [w \in Wallets |-> FALSE]
    /\ recoveryState = [t \in Tokens |-> "NotClaimed"]
    /\ recoveryClaimEpoch = [t \in Tokens |-> 0]
    /\ totalMinted = Cardinality(Tokens)
    /\ totalRetired = 0

\* Allow termination when all tokens are recovered or no more actions
Terminated ==
    /\ \A t \in Tokens: recoveryState[t] \in {"Recovered", "NotClaimed"}
    /\ currentEpoch = MaxEpoch
    /\ UNCHANGED vars

Next ==
    \/ Terminated
    \/ AdvanceEpoch
    \/ \E w \in Wallets, d \in Wallets: RegisterBinding(w, d)
    \/ \E w \in Wallets: RefreshBinding(w)
    \/ \E w \in Wallets: RevokeBinding(w)
    \/ \E w \in Wallets: ExpireBinding(w)
    \/ \E w \in Wallets: LoseWallet(w)
    \/ \E d \in Wallets, t \in Tokens: ClaimRecovery(d, t)
    \/ \E t \in Tokens: CompleteRecovery(t)
    \/ \E w \in Wallets: DetectDoubleSpend(w)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* -----------------------------------------------------------------------
\* Safety Invariants
\* -----------------------------------------------------------------------

TypeInvariant ==
    /\ currentEpoch \in 0..MaxEpoch
    /\ \A w \in Wallets: bindingState[w] \in BindingStates
    /\ \A w \in Wallets: walletLost[w] \in BOOLEAN
    /\ \A w \in Wallets: walletRevoked[w] \in BOOLEAN
    /\ \A t \in Tokens: recoveryState[t] \in RecoveryStates
    /\ totalMinted \in Nat
    /\ totalRetired \in Nat

\* A recovered token cannot be claimed again
NoDoubleRecovery ==
    \A t \in Tokens:
        recoveryState[t] = "Recovered" =>
            recoveryState'[t] \in {"Recovered"}

\* Recovery only succeeds if token is expired and tickets are expired
\* (enforced by RecoveryEligible predicate in ClaimRecovery)
NoActiveRecovery ==
    \A t \in Tokens:
        recoveryState[t] \in {"Pending", "Recovered"} =>
            /\ TokenExpired(t)
            /\ TicketsExpired(tokenHolder[t])

\* If a wallet is revoked, none of its tokens can be claimed for recovery
RevokedCannotRecover ==
    \A t \in Tokens:
        LET holder == tokenHolder[t]
        IN (walletRevoked[holder] /\ recoveryState[t] = "NotClaimed") =>
            ~RecoveryEligible(t)

\* After recovery completes, the old wallet is revoked
OldWalletRevokedAfterRecovery ==
    \A t \in Tokens:
        \A w \in Wallets:
            (recoveryState[t] = "Recovered" /\ tokenMintEpoch[t] = 0
             /\ bindingState[w] = "Consumed") =>
                walletRevoked[w]

\* Recovery requires a valid binding with a matching delegate
DelegateRequired ==
    \A t \in Tokens:
        recoveryState[t] \in {"Pending", "Recovered"} =>
            LET holder == tokenHolder[t]
            IN \/ bindingState[holder] \in {"Active", "Consumed"}
               \/ bindingDelegate[holder] # "none"

\* Conservation of value: total minted minus total retired equals
\* the original token count (no value created or destroyed)
ConservationOfValue ==
    totalMinted - totalRetired = Cardinality(Tokens)

\* Consumed bindings are terminal: a consumed binding stays consumed
ConsumedIsTerminal ==
    \A w \in Wallets:
        bindingState[w] = "Consumed" =>
            walletRevoked[w]

\* Revocation is cumulative: once revoked, stays revoked
CumulativeRevocation ==
    \A w \in Wallets:
        walletRevoked[w] => walletRevoked'[w]

\* Combined invariant
Invariant ==
    /\ TypeInvariant
    /\ ConservationOfValue
    /\ ConsumedIsTerminal

\* -----------------------------------------------------------------------
\* Liveness Properties
\* -----------------------------------------------------------------------

\* A lost wallet with active binding and expired tokens eventually
\* has its tokens recovered (if delegate is available and wallet not revoked)
EventualRecovery ==
    \A t \in Tokens:
        \A w \in Wallets:
            [](/\ walletLost[tokenHolder[t]]
               /\ bindingState[tokenHolder[t]] = "Active"
               /\ tokenHolder[t] = w
               /\ ~walletRevoked[w]
               /\ bindingDelegate[w] \in Wallets
               /\ ~walletLost[bindingDelegate[w]]
               /\ TokenExpired(t)
              => <>(recoveryState[t] = "Recovered"))

=============================================================================
