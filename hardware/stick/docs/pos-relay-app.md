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

### Transaction Flow (2 Taps, Not 4)

Since the receiver credstick isn't present, the flow simplifies:

```
  Customer          PoS App (Phone)         Tokenmap (if online)
  Credstick
     │                    │                        │
     │                    │  [Stored ticket +      │
     │                    │   epoch data ready]    │
     │                    │                        │
     │──── Tap ──────────│                        │
     │  TRANSFER(ticket,  │                        │
     │    amount, tokens)  │                        │
     │  [BLS sign + attest]│                        │
     │────► signed tokens ─│                        │
     │                    │                        │
     │                    │── Online? ─────────────│
     │                    │   Validate tokens      │
     │                    │   Check double-spend   │
     │                    │◄── valid / invalid ────│
     │                    │                        │
     │──── Tap ──────────│                        │
     │  CONFIRM(result)   │                        │
     │  [updates display] │                        │
     ▼                    ▼                        ▼
  "-5 tokens"        "✓ Received 5"
```

**Only 2 taps needed** (not 4), because the receiver credstick isn't
involved at transaction time.

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

### Screen 2: Tap to Pay (Single Tap)

```
┌─────────────────────────┐
│                         │
│      💳 ──→ 📱          │
│                         │
│  Tap customer's         │
│  credstick to pay       │
│                         │
│  Amount: 5 tokens       │
│                         │
│  ● Online (validating)  │  ← or "○ Offline (trust)"
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

| Step | Command | INS | Data In | Data Out |
|------|---------|-----|---------|----------|
| 1 | TRANSFER | 0x20 | amount + items | ticket + epoch (PoS → credstick) |
| 1b | TRANSFER_RESP | — | — | signed Token[] (credstick → PoS) |
| 2 | CONFIRM | 0x40 | accepted (bool) + validation_status | — |
| — | READ_TICKET | 0x10 | — | SignedTicket (setup only) |
| — | SWEEP | 0x50 | Token[] | accepted (bool) (collection) |

The TRANSFER command is a two-phase exchange within a single NFC session:
1. PoS sends the stored ticket + amount to the credstick
2. Credstick signs and returns tokens in the same session
3. PoS validates (online or offline)
4. PoS sends CONFIRM with result

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
