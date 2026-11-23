# Standalone PoS Relay App

## Motivation

The full Briolette wallet app (PayScreen, ReceiveScreen, etc.) is designed
for phone-as-wallet scenarios where the phone holds keys and tokens. For
accepting credstick payments, a merchant needs something simpler: a
point-of-sale terminal that holds their receiving ticket, validates
incoming tokens when online, and accumulates payments for later collection.

A standalone PoS app makes this explicit: it's a single-purpose app that
acts as a merchant terminal. The receiver's credstick is tapped once
during setup, then doesn't need to be present for transactions.

## Why Standalone (Not a Mode in the Wallet App)

1. **Different trust model**: The wallet app manages private keys and token
   storage. The PoS holds only a receiver ticket (public) and cached epoch
   data. Separating them reduces attack surface.

2. **Different users**: A merchant installs the PoS app; customers use the
   wallet app (or just a credstick). Forcing merchants to install a full
   wallet app with key generation is unnecessary friction.

3. **Simpler audit**: A standalone app with NFC relay logic + optional
   online validation is trivially auditable compared to the full wallet.

4. **Offline-capable, online-enhanced**: Works fully offline with token
   accumulation, but when connected can validate tokens against the
   tokenmap and participate in epoch gossip.

## Key Design Principle: Receiver Not Present

The receiver credstick (merchant's) is **not present at transaction time**.
Instead:

1. Merchant taps their credstick to the PoS app **once** during setup
2. PoS stores the `SignedTicket` persistently
3. All subsequent transactions use the stored ticket
4. Merchant taps their credstick again later to **sweep** accumulated tokens

This is how real card terminals work — the merchant's bank account is
configured once, not re-entered per sale.

## Functional Spec

### Setup Flow (One-Time)

```
┌─────────────────────────────────────────────────────┐
│                  PoS Setup                           │
│                                                      │
│  ┌───────────┐   ┌──────────────┐   ┌────────────┐ │
│  │  Tap       │──▶│  Store       │──▶│  Ready     │ │
│  │  merchant  │   │  ticket +    │   │  for       │ │
│  │  credstick │   │  epoch data  │   │  payments  │ │
│  └───────────┘   └──────────────┘   └────────────┘ │
└─────────────────────────────────────────────────────┘
```

### Transaction Modes

The credstick supports two transaction modes that separate the act of
**proposing** payment from **committing** to it. This separation means
the user never needs to press a button while in the NFC field — the
physical act of tapping again IS the confirmation.

### Mode 1: 2-Tap (Fast, Less Private)

The default mode. Optimized for speed at the cost of revealing which
tokens would be spent before the user commits.

```
  Customer          PoS App (Phone)         Tokenmap (if online)
  Credstick
     │                    │                        │
     │                    │  [Stored ticket +      │
     │                    │   epoch data ready]    │
     │                    │                        │
     │──── Tap 1 ────────│                        │
     │  INITIATE(ticket,  │                        │
     │    items, epoch)   │                        │
     │  + TRANSACT        │                        │
     │                    │                        │
     │  Credstick e-ink:  │                        │
     │  "Pay 5 tokens?"   │                        │
     │                    │                        │
     │  Returns UNSIGNED  │                        │
     │  proposed tokens ──│                        │
     │                    │                        │
     │  [User lifts       │── Online? ─────────────│
     │   credstick,       │   Validate tokens      │
     │   reads display,   │   Check double-spend   │
     │   decides]         │◄── valid / invalid ────│
     │                    │                        │
     │──── Tap 2 ────────│                        │
     │  TRANSFER(tx_id,   │                        │
     │   accept=true)     │                        │
     │  (tapping again    │                        │
     │   = consent)       │                        │
     │                    │                        │
     │  [BLS sign + attest]                        │
     │────► signed tokens─│                        │
     │  [updates display] │                        │
     ▼                    ▼                        ▼
  "-5 tokens"        "✓ Received 5"
```

**How it works:**
1. **Tap 1**: PoS sends proposal (ticket + amount). Credstick displays
   the amount on e-ink and returns **unsigned tokens** that would fulfill
   the payment. The PoS can immediately begin online validation.
2. **Between taps**: User lifts credstick, reads their display ("Pay 5
   tokens?"), and decides. Meanwhile, the PoS validates the unsigned
   tokens against the tokenmap if online.
3. **Tap 2**: User taps again — this physical action IS consent. The PoS
   sends accept/reject. If accepted, the credstick signs and returns
   the signatures. If rejected (e.g., double-spend detected), the
   credstick is informed and no signing occurs.

**Privacy tradeoff**: The PoS sees which specific tokens would be spent
before the user commits. This allows the merchant (or tokenmap) to check
for double-spends, but also reveals token history to the PoS before
finalization. The unsigned tokens are cryptographically useless — they
can't be spent without signatures — but the metadata is exposed.

**No button needed**: The user confirms by tapping again. If they don't
want to pay (wrong amount, changed their mind), they simply walk away.
The e-ink display retains the "Pay 5 tokens?" message until the next
interaction, so there's no time pressure.

### Mode 2: 3-Tap (Private, More Friction)

For privacy-sensitive users. Tokens aren't revealed until after the
user has seen and accepted the proposal.

```
  Customer          PoS App (Phone)         Tokenmap (if online)
  Credstick
     │                    │                        │
     │──── Tap 1 ────────│                        │
     │  INITIATE(ticket,  │                        │
     │    items, epoch)   │                        │
     │                    │                        │
     │  Credstick e-ink:  │                        │
     │  "Pay 5 tokens?"   │                        │
     │                    │                        │
     │  Returns: tx_id    │                        │
     │  (no tokens yet)   │                        │
     │                    │                        │
     │  [User reads       │                        │
     │   display, decides]│                        │
     │                    │                        │
     │──── Tap 2 ────────│                        │
     │  TRANSACT(tx_id)   │                        │
     │  (consent to       │                        │
     │   reveal tokens)   │                        │
     │                    │                        │
     │  Returns UNSIGNED  │                        │
     │  proposed tokens ──│── Online? ─────────────│
     │                    │   Validate tokens      │
     │  [Long hold: stay  │   Check double-spend   │
     │   on reader while  │◄── valid / invalid ────│
     │   PoS validates]   │                        │
     │                    │                        │
     │  TRANSFER(tx_id,   │                        │
     │   accept=true)     │                        │
     │  [BLS sign + attest]                        │
     │────► signed tokens─│                        │
     │                    │                        │
     ▼                    ▼                        ▼
  "-5 tokens"        "✓ Received 5"


  Alternative: Taps 2+3 separate if PoS validation is slow

     │──── Tap 2 ────────│                        │
     │  TRANSACT(tx_id)   │                        │
     │  unsigned tokens ──│── Online? ─────────────│
     │                    │   Validate tokens      │
     │                    │◄── valid / invalid ────│
     │                    │                        │
     │──── Tap 3 ────────│                        │
     │  TRANSFER(tx_id,   │                        │
     │   accept=true)     │                        │
     │  [BLS sign + attest]                        │
     │────► signed tokens─│                        │
     ▼                    ▼                        ▼
```

**Tap 2+3 merge (long hold)**: In practice, the user can hold the
credstick on the reader during tap 2. The credstick sends unsigned
tokens, the PoS validates (possibly just milliseconds for local crypto
check), and immediately requests signatures — all in one sustained
contact. The user experiences this as a single "long tap" (~3-5 seconds)
rather than three separate taps. If validation takes longer (online
check), the user lifts and does a third tap.

**Privacy advantage**: No token data is revealed until tap 2, after the
user has explicitly consented. A malicious PoS that sends a bogus
proposal gets nothing — just an "OK, I see the proposal" from tap 1.

### Mode Selection

The credstick's privacy mode is a user preference set during setup:

| Mode | Taps | Privacy | Speed | Default For |
|------|------|---------|-------|-------------|
| Fast (2-tap) | 2 | Lower | ~3s total | Small amounts, daily use |
| Private (3-tap) | 2-3 | Higher | ~5-8s total | Large amounts, sensitive |

The credstick can auto-select based on amount threshold: fast mode
below N tokens, private mode above. Configurable via USB setup.

### Screen 1: Transaction Setup

```
┌─────────────────────────┐
│  Briolette PoS          │
│  Merchant: [ticket...] ✓│
│                         │
│  Amount: [___] tokens   │
│                         │
│  Description:           │
│  [Coffee + pastry     ] │
│                         │
│  ┌───────────────────┐  │
│  │   Start Payment   │  │
│  └───────────────────┘  │
│                         │
│  ── Recent ──           │
│  ✓ 5 tokens  10:32am   │
│  ✓ 3 tokens   9:15am   │
│  ⏳ 2 tokens  9:01am   │ ← unvalidated (was offline)
└─────────────────────────┘
```

### Screen 2: Waiting for Taps

```
┌─────────────────────────┐
│                         │
│      💳 ──→ 📱          │
│                         │
│  Tap customer's         │
│  credstick (tap 1/2)    │
│                         │
│  Amount: 5 tokens       │
│                         │
│  ● Online (will check)  │  ← or "○ Offline"
└─────────────────────────┘
```

After tap 1 (proposal sent, unsigned tokens received):

```
┌─────────────────────────┐
│  Validating tokens...   │
│  ✓ Crypto valid         │
│  ● Checking tokenmap... │
│                         │
│  Tap again to confirm   │
│      💳 ──→ 📱          │
│                         │
│  Amount: 5 tokens       │
└─────────────────────────┘
```

### Screen 3: Result

```
┌─────────────────────────┐
│                         │
│        ✓ Received!      │
│                         │
│  5 tokens (validated)   │  ← or "(unvalidated)" if offline
│  12:34 PM               │
│                         │
│  ┌───────────────────┐  │
│  │   New Payment     │  │
│  └───────────────────┘  │
└─────────────────────────┘
```

## Online Validation (When Connected)

When the PoS has internet connectivity, it acts as a smart terminal:

### 1. Epoch Gossip

The PoS participates in the Gossip protocol to stay current:

```
On app launch (if online):
  → Call Clerk service for current EpochData
  → Cache locally
  → Use cached epoch for offline periods

During transaction (if online):
  → Compare sender's epoch with cached epoch
  → Exchange EpochUpdate if mismatched (sender gets updated too)
```

### 2. Token Validation

Before accepting tokens, the PoS can check them:

```
Sender taps credstick → PoS receives proposed tokens
  │
  ├── Online path:
  │   → Validate.CheckTokens(tokens) against tokenmap
  │   → Verify no double-spend
  │   → Verify mint signatures
  │   → Verify epoch freshness
  │   → Accept or reject with confidence
  │
  └── Offline path:
      → Verify token chain cryptographically (BLS pairing checks)
      → Verify mint signature against cached mint public keys
      → Check local bloom filter for known-bad tokens (if maintained)
      → Accept on faith (like physical cash)
      → Mark as "unvalidated" in transaction log
```

### 3. Batch Validation (Background)

Tokens received while offline can be validated later when connectivity
returns:

```
Connectivity restored:
  → For each unvalidated token batch:
    → Validate.CheckTokens(tokens)
    → Update transaction log: "unvalidated" → "validated" or "REJECTED"
    → Alert merchant if any tokens were double-spent
```

### 4. Receiver Service Proxy

The PoS effectively acts as a lightweight Receiver service, implementing
the same protocol but adapted for NFC instead of gRPC:

| Receiver RPC | PoS Equivalent |
|--------------|----------------|
| `Initiate` | Stored ticket returned via APDU on first tap |
| `Gossip` | Epoch exchange during NFC handshake |
| `Transact` | Token proposal validated locally + online |
| `Transfer` | Token delivery confirmed on second tap |

## APDU Protocol

The APDU protocol mirrors the existing `receiver.proto` RPC protocol
directly. Each RPC maps to an APDU command — the PoS is translating
between gRPC semantics and NFC APDUs, not inventing a new protocol.

### Mapping: receiver.proto → APDU

| Receiver RPC | APDU | INS | Direction | Purpose |
|--------------|------|-----|-----------|---------|
| `Initiate` | INITIATE | 0x10 | PoS → credstick | Send ticket + items + epoch; get tx_id |
| `Gossip` | GOSSIP | 0x12 | bidirectional | Exchange epoch updates if mismatched |
| `Transact` | TRANSACT | 0x20 | credstick → PoS | Propose unsigned tokens for settlement |
| `Transfer` | TRANSFER | 0x30 | credstick → PoS | Send final signatures to commit |

### Setup / Management APDUs

| Command | INS | Data In | Data Out |
|---------|-----|---------|----------|
| READ_TICKET | 0x11 | — | SignedTicket (merchant setup) |
| SWEEP | 0x50 | Token[] | accepted (bool) (token collection) |
| GET_BALANCE | 0x51 | — | Amount (protobuf) |

### 2-Tap Flow (Fast Mode)

Maps to: Initiate + Transact in one tap, Transfer in the second.

```
Tap 1: SELECT AID
       → INITIATE(ticket, epoch, items)    -- mirrors InitiateReply
       ← tx_id + epoch
       → TRANSACT(tx_id, methods[])        -- credstick selects tokens
       ← unsigned Token[] per method       -- mirrors TransactRequest
       (e-ink updated: "Pay N tokens?")
       (user lifts credstick, enters PIN if needed)

       [PoS validates unsigned tokens online if connected]

Tap 2: SELECT AID
       → TRANSFER(tx_id, accept/reject)    -- mirrors TransactReply
       ← signed Token[] (final History)    -- mirrors TransferRequest
       (e-ink updated: "-N tokens")
```

The credstick returns unsigned tokens during TRANSACT (the proposal).
On tap 2, the PoS tells the credstick whether it accepts, and if so,
the credstick signs and returns the final signatures. This directly
parallels the gRPC flow where `Transact` proposes and `Transfer`
finalizes.

### 3-Tap Flow (Private Mode)

Maps to: Initiate on tap 1, Transact on tap 2, Transfer on tap 2/3.

```
Tap 1: SELECT AID
       → INITIATE(ticket, epoch, items)
       ← tx_id + epoch
       (e-ink updated: "Pay N tokens?" — no tokens revealed yet)
       (user lifts credstick, enters PIN if needed)

Tap 2: SELECT AID
       → TRANSACT(tx_id, methods[])
       ← unsigned Token[] per method
       [PoS validates; if long hold, continues in same session:]
       → TRANSFER(tx_id, accept/reject)
       ← signed Token[]

 -- OR, if PoS needs time to validate online: --

Tap 2: → TRANSACT → ← unsigned tokens
       (user lifts; PoS validates async)

Tap 3: → TRANSFER(tx_id, accept) → ← signed tokens
```

Taps 2+3 merge naturally via long hold: the credstick sends unsigned
tokens on TRANSACT, the PoS validates (milliseconds for local crypto),
then immediately sends TRANSFER in the same NFC session. The user
experiences a single ~3-5s "long tap."

### State Machine

The credstick tracks protocol state across NFC sessions, mirroring
the Receiver server's transaction state tracking:

```
IDLE ──INITIATE──▶ INITIATED ──TRANSACT──▶ PROPOSED
  ▲                     │                      │
  │                     │ (timeout 5min)       │ (timeout 5min)
  │                     ▼                      ▼
  └──────────── EXPIRED ◄──────────────── EXPIRED

PROPOSED ──TRANSFER(accept)──▶ COMPLETE ──▶ IDLE
PROPOSED ──TRANSFER(reject)──▶ IDLE (no signing)
INITIATED ──GOSSIP──▶ INITIATED (epoch updated, retry Initiate)
```

State is persisted in RAM (supercap-backed). If the credstick fully
loses power between taps, the proposal expires and the user starts
over. The 5-minute timeout prevents stale proposals from lingering.

### Note on TransactRequest Adaptation

In the gRPC protocol, the **sender** calls `Transact` on the
**receiver** to propose tokens. In the APDU protocol, the roles are
inverted for the credstick-as-sender case: the PoS (acting as
receiver) sends INITIATE to the credstick, and the credstick responds
to TRANSACT by selecting and returning its own tokens. The message
formats (`TransactionItemMethod`, `Token`) remain identical — only the
transport direction changes from client-initiated gRPC to
reader-initiated APDU.

## Token Accumulation and Sweep

The PoS accumulates received tokens in local storage (SQLite). The
merchant periodically sweeps them to their credstick or wallet:

### Sweep to Credstick

```
┌─────────────────────────┐
│  Sweep Tokens           │
│                         │
│  Accumulated:           │
│  ✓ 142 tokens (valid)   │
│  ⏳ 23 tokens (pending) │
│                         │
│  Tap merchant credstick │
│  to collect             │
│                         │
│      📱 ──→ 💳          │
└─────────────────────────┘
```

### Sweep to Wallet (Online)

If the merchant also runs the wallet app, tokens can be transferred
directly via the Receiver service over the network — no NFC needed.

## Implementation

### Platform: Kotlin Multiplatform (KMP)

```
mobile/
├── wallet/          # Existing wallet app (keys, tokens, QR)
└── pos/             # New: standalone PoS terminal
    ├── shared/
    │   └── src/
    │       ├── commonMain/
    │       │   └── kotlin/com/briolette/pos/
    │       │       ├── PosApp.kt            # App entry + nav
    │       │       ├── SetupScreen.kt       # Merchant credstick registration
    │       │       ├── PaymentScreen.kt     # Amount entry + tap to pay
    │       │       ├── ResultScreen.kt      # Success/fail
    │       │       ├── SweepScreen.kt       # Token collection
    │       │       ├── ApduProtocol.kt      # APDU constants + builders
    │       │       ├── TokenStore.kt        # SQLite token accumulation
    │       │       ├── OnlineValidator.kt   # Tokenmap + Clerk gRPC client
    │       │       └── EpochCache.kt        # Cached epoch data
    │       ├── androidMain/
    │       │   └── NfcTerminal.kt           # Android IsoDep wrapper
    │       └── iosMain/
    │           └── NfcTerminal.kt           # iOS NFCTagReaderSession
    └── androidApp/
        └── AndroidManifest.xml              # NFC + internet permissions
```

### Dependencies

```
pos/shared → proto         (protobuf message types)
pos/shared → validate      (token verification — BLS pairing checks)
pos/shared → clerk client  (epoch gossip, optional)
pos/shared → (NO dependency on wallet/shared — no private keys)
```

The PoS depends on the `validate` module for cryptographic verification
of token chains. This is the key difference from a "dumb relay" — the
PoS is a **smart terminal** that verifies tokens itself when possible.

### Token Store Schema

```sql
CREATE TABLE received_tokens (
    id INTEGER PRIMARY KEY,
    tx_id BLOB NOT NULL,
    timestamp INTEGER NOT NULL,
    amount INTEGER NOT NULL,
    description TEXT,
    sender_ticket_hash BLOB,
    token_data BLOB NOT NULL,          -- serialized Token[]
    validation_status TEXT NOT NULL     -- 'pending', 'valid', 'invalid'
        DEFAULT 'pending',
    validated_at INTEGER,
    swept INTEGER NOT NULL DEFAULT 0   -- 1 = collected by merchant
);
```

## Connectivity Modes

| Mode | Validation | Gossip | Risk |
|------|-----------|--------|------|
| **Online** | Full tokenmap check | Live epoch sync | Minimal (double-spend caught) |
| **Degraded** | Cached epoch + crypto verify | Stale epoch | Low (crypto valid, epoch may lag) |
| **Offline** | Crypto verify only | No gossip | Medium (like accepting cash) |

The PoS gracefully degrades. It never refuses a transaction due to
lack of connectivity — it just adjusts the validation confidence level.

## Security Notes

- The PoS stores a `SignedTicket` (public data, not a secret)
- No private keys are stored or generated
- Token data in SQLite should be encrypted at rest (Android Keystore /
  iOS Keychain for the encryption key)
- PIN/biometric lock on the PoS app prevents unauthorized sweep
- Transaction log hashes are privacy-preserving (no identity linkage)

## Web Version (Future)

WebNFC (Chrome on Android) could enable a zero-install PoS experience:
navigate to a URL, enter amount, tap credsticks. WebNFC currently
supports NDEF only, not ISO-DEP, so this requires either Chrome
extending WebNFC or using a WebUSB bridge.
