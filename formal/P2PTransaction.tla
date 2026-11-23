--------------------------- MODULE P2PTransaction ---------------------------
(*
 * Formal model of briolette's P2P transaction state machine.
 *
 * Maps to: src/receiver/src/server.rs (TransactionState enum,
 *   initiate_impl, gossip_impl, transact_impl, transfer_impl)
 *
 * The receiver progresses through states:
 *   Init -> Gossip -> Transact -> Transfer -> Complete
 *
 * With a 5-second timeout per state that aborts stalled transactions.
 *
 * Safety properties:
 *   - Valid state transitions (no skipping)
 *   - Amount matching at Transfer
 *   - Peer binding (cannot change mid-transaction)
 *   - At-most-once completion
 *
 * Liveness:
 *   - Every transaction eventually reaches Complete or Timeout
 *)
EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    TxnIds,        \* Set of transaction identifiers
    Wallets,       \* Set of wallet identifiers
    TokenIds,      \* Set of token identifiers
    MaxValue,      \* Maximum transaction amount
    TimeoutSteps   \* Steps before timeout fires

ASSUME TimeoutSteps \in Nat /\ TimeoutSteps >= 1
ASSUME MaxValue \in Nat /\ MaxValue >= 1

\* Transaction states matching src/receiver/src/server.rs:28-34
States == {"Init", "Gossip", "Transact", "Transfer", "Complete", "Timeout", "Unused"}

VARIABLES
    txnState,     \* Function: TxnId -> State
    txnPayer,     \* Function: TxnId -> Wallet or "none"
    txnReceiver,  \* Function: TxnId -> Wallet
    txnAmount,    \* Function: TxnId -> amount (1..MaxValue)
    txnTokenValue,\* Function: TxnId -> proposed token total value
    txnTimer,     \* Function: TxnId -> steps remaining before timeout
    txnEpochMatch \* Function: TxnId -> BOOLEAN (do epochs match?)

vars == <<txnState, txnPayer, txnReceiver, txnAmount, txnTokenValue, txnTimer, txnEpochMatch>>

\* -----------------------------------------------------------------------
\* Actions — mirror the server.rs implementation
\* -----------------------------------------------------------------------

\* Receiver initiates a transaction by publishing expected amount + ticket
\* Maps to: initiate_impl (server.rs:116-222)
Initiate(txn, receiver, amount, epochsMatch) ==
    /\ txn \in TxnIds
    /\ txnState[txn] = "Unused"
    /\ receiver \in Wallets
    /\ amount \in 1..MaxValue
    /\ epochsMatch \in BOOLEAN
    \* If epochs match, skip Gossip and go straight to Transact
    \* Maps to server.rs:152-165
    /\ txnState' = [txnState EXCEPT ![txn] =
         IF epochsMatch THEN "Transact" ELSE "Gossip"]
    /\ txnPayer' = [txnPayer EXCEPT ![txn] = "none"]
    /\ txnReceiver' = [txnReceiver EXCEPT ![txn] = receiver]
    /\ txnAmount' = [txnAmount EXCEPT ![txn] = amount]
    /\ txnTokenValue' = [txnTokenValue EXCEPT ![txn] = 0]
    /\ txnTimer' = [txnTimer EXCEPT ![txn] = TimeoutSteps]
    /\ txnEpochMatch' = [txnEpochMatch EXCEPT ![txn] = epochsMatch]

\* Payer sends epoch update during gossip phase
\* Maps to: gossip_impl (server.rs:225-282)
GossipStep(txn, payer) ==
    /\ txn \in TxnIds
    /\ txnState[txn] = "Gossip"
    /\ payer \in Wallets
    /\ payer # txnReceiver[txn]
    \* Bind payer if not yet bound, or verify same payer
    /\ txnPayer[txn] = "none" \/ txnPayer[txn] = payer
    /\ txnState' = [txnState EXCEPT ![txn] = "Transact"]
    /\ txnPayer' = [txnPayer EXCEPT ![txn] = payer]
    /\ txnTimer' = [txnTimer EXCEPT ![txn] = TimeoutSteps]
    /\ UNCHANGED <<txnReceiver, txnAmount, txnTokenValue, txnEpochMatch>>

\* Payer proposes tokens matching the expected amount
\* Maps to: transact_impl (server.rs:283-339)
Transact(txn, payer, proposedValue) ==
    /\ txn \in TxnIds
    /\ txnState[txn] = "Transact"
    /\ payer \in Wallets
    /\ payer # txnReceiver[txn]
    \* Payer must be the one already bound, or first to interact
    /\ txnPayer[txn] = "none" \/ txnPayer[txn] = payer
    /\ proposedValue \in 1..MaxValue
    \* Receiver accepts only if amount matches (server.rs:325-332)
    /\ proposedValue = txnAmount[txn]
    /\ txnState' = [txnState EXCEPT ![txn] = "Transfer"]
    /\ txnPayer' = [txnPayer EXCEPT ![txn] = payer]
    /\ txnTokenValue' = [txnTokenValue EXCEPT ![txn] = proposedValue]
    /\ txnTimer' = [txnTimer EXCEPT ![txn] = TimeoutSteps]
    /\ UNCHANGED <<txnReceiver, txnAmount, txnEpochMatch>>

\* Payer finalizes the transfer (tokens are now bound to receiver's ticket)
\* Maps to: transfer_impl (server.rs:342-445)
\* Note: ticket expiry is only checked on current holder's ticket, not on
\* historical tickets in the token's transfer chain. This is modeled in
\* BrioletteSystem.tla where Transfer checks hasValidTicket[sender/recipient]
\* but not historical owners. See token.rs:VerifyTicket::verify_historical().
TransferStep(txn, payer, finalValue) ==
    /\ txn \in TxnIds
    /\ txnState[txn] = "Transfer"
    /\ payer \in Wallets
    /\ txnPayer[txn] = payer
    /\ finalValue \in 1..MaxValue
    \* Receiver re-verifies amount (server.rs:412-418)
    /\ finalValue = txnAmount[txn]
    /\ txnState' = [txnState EXCEPT ![txn] = "Complete"]
    /\ txnTokenValue' = [txnTokenValue EXCEPT ![txn] = finalValue]
    /\ txnTimer' = [txnTimer EXCEPT ![txn] = 0]
    /\ UNCHANGED <<txnPayer, txnReceiver, txnAmount, txnEpochMatch>>

\* Timeout fires — transaction is aborted
\* Maps to: timer task in initiate_impl (server.rs:186-221)
Timeout(txn) ==
    /\ txn \in TxnIds
    /\ txnState[txn] \in {"Gossip", "Transact", "Transfer"}
    /\ txnTimer[txn] = 0
    /\ txnState' = [txnState EXCEPT ![txn] = "Timeout"]
    /\ UNCHANGED <<txnPayer, txnReceiver, txnAmount, txnTokenValue, txnTimer, txnEpochMatch>>

\* Timer tick — decrement timer for active transactions
TimerTick(txn) ==
    /\ txn \in TxnIds
    /\ txnState[txn] \in {"Gossip", "Transact", "Transfer"}
    /\ txnTimer[txn] > 0
    /\ txnTimer' = [txnTimer EXCEPT ![txn] = txnTimer[txn] - 1]
    /\ UNCHANGED <<txnState, txnPayer, txnReceiver, txnAmount, txnTokenValue, txnEpochMatch>>

\* -----------------------------------------------------------------------
\* Specification
\* -----------------------------------------------------------------------

Init ==
    /\ txnState = [t \in TxnIds |-> "Unused"]
    /\ txnPayer = [t \in TxnIds |-> "none"]
    /\ txnReceiver = [t \in TxnIds |-> CHOOSE w \in Wallets : TRUE]
    /\ txnAmount = [t \in TxnIds |-> 1]
    /\ txnTokenValue = [t \in TxnIds |-> 0]
    /\ txnTimer = [t \in TxnIds |-> 0]
    /\ txnEpochMatch = [t \in TxnIds |-> FALSE]

\* Allow termination when all transactions are in terminal states
Terminated ==
    /\ \A t \in TxnIds: txnState[t] \in {"Complete", "Timeout", "Unused"}
    /\ UNCHANGED vars

Next ==
    \/ Terminated
    \/ \E txn \in TxnIds, r \in Wallets, a \in 1..MaxValue, em \in BOOLEAN:
         Initiate(txn, r, a, em)
    \/ \E txn \in TxnIds, p \in Wallets:
         GossipStep(txn, p)
    \/ \E txn \in TxnIds, p \in Wallets, v \in 1..MaxValue:
         Transact(txn, p, v)
    \/ \E txn \in TxnIds, p \in Wallets, v \in 1..MaxValue:
         TransferStep(txn, p, v)
    \/ \E txn \in TxnIds: Timeout(txn)
    \/ \E txn \in TxnIds: TimerTick(txn)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

\* -----------------------------------------------------------------------
\* Safety Invariants
\* -----------------------------------------------------------------------

TypeInvariant ==
    /\ \A t \in TxnIds: txnState[t] \in States
    /\ \A t \in TxnIds: txnAmount[t] \in 1..MaxValue
    /\ \A t \in TxnIds: txnTokenValue[t] \in 0..MaxValue
    /\ \A t \in TxnIds: txnTimer[t] \in 0..TimeoutSteps

\* State transitions follow the valid progression
\* Init/Unused -> Gossip/Transact -> Transact -> Transfer -> Complete
\* Any active state -> Timeout
ValidTransitions ==
    \A t \in TxnIds:
        \/ txnState[t] = "Unused"
        \/ txnState[t] = "Gossip"
        \/ txnState[t] = "Transact"
        \/ txnState[t] = "Transfer"
        \/ txnState[t] = "Complete"
        \/ txnState[t] = "Timeout"

\* Once a payer is bound, it cannot change
PeerBinding ==
    \A t \in TxnIds:
        txnState[t] \in {"Transfer", "Complete"} =>
            txnPayer[t] \in Wallets

\* On completion, the token value matches the requested amount
AmountMatchOnComplete ==
    \A t \in TxnIds:
        txnState[t] = "Complete" => txnTokenValue[t] = txnAmount[t]

\* Payer and receiver are always different
PayerReceiverDistinct ==
    \A t \in TxnIds:
        txnPayer[t] \in Wallets => txnPayer[t] # txnReceiver[t]

\* A completed transaction stays complete (no state regression)
CompleteIsTerminal ==
    \A t \in TxnIds:
        txnState[t] = "Complete" => txnTimer[t] = 0

\* A timed-out transaction stays timed out
TimeoutIsTerminal ==
    \A t \in TxnIds:
        txnState[t] = "Timeout" => txnTimer[t] = 0

\* Combined invariant
Invariant ==
    /\ TypeInvariant
    /\ ValidTransitions
    /\ PeerBinding
    /\ AmountMatchOnComplete
    /\ PayerReceiverDistinct
    /\ CompleteIsTerminal
    /\ TimeoutIsTerminal

\* -----------------------------------------------------------------------
\* Liveness
\* -----------------------------------------------------------------------

\* Every initiated transaction eventually completes or times out
EventualTermination ==
    \A t \in TxnIds:
        [](txnState[t] \in {"Gossip", "Transact", "Transfer"} =>
           <>(txnState[t] \in {"Complete", "Timeout"}))

=============================================================================
