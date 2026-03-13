# Credstick-to-Credstick Payments

## The Problem

The current phone-to-phone payment flow uses QR codes:
1. Receiver shows their SignedTicket as a QR code
2. Sender scans it, signs tokens transferring them to that ticket
3. Sender shows signed tokens as a QR code
4. Receiver scans to import tokens

With credsticks (NFC-only devices), there's no camera to scan QR codes,
and two NFC tags can't directly talk to each other — NFC requires one
active initiator (reader/phone) and one passive target (tag/card).

## Credstick-to-Credstick: Why It Needs a Phone

Two credsticks cannot directly communicate. NFC is asymmetric:
- **NFC reader** (phone): Provides the RF field, initiates communication
- **NFC tag** (credstick): Powered by the field, responds to commands

Two tags can't power each other. You need a phone (or any NFC reader)
as the intermediary.

## Solution: Phone as PoS Relay (No Keys Required)

The phone acts as a dumb relay / point-of-sale terminal. It holds
**no keys** and **no tokens** — it just shuttles APDUs between the
two credsticks and optionally hosts the transaction details.

### Flow: Credstick A Pays Credstick B

```
  Credstick A           Phone (PoS)          Credstick B
  (sender)              (relay)               (receiver)
     │                     │                      │
     │                     │──── Tap B ───────────│
     │                     │  READ_TICKET          │
     │                     │◄──── SignedTicket ────│
     │                     │                      │
     │──── Tap A ──────────│                      │
     │  TRANSFER(ticket,   │                      │
     │    amount)           │                      │
     │                     │                      │
     │  E-ink shows:       │                      │
     │  "Pay 3 tokens?"    │                      │
     │  "◄ No    Yes ►"    │                      │
     │                     │                      │
     │  User presses ► to  │                      │
     │  confirm on credstick                      │
     │                     │                      │
     │  [BLS sign + attest]│                      │
     │────► signed tokens ─│                      │
     │                     │                      │
     │                     │──── Tap B ───────────│
     │                     │  RECEIVE(tokens)      │
     │                     │◄──── accepted ───────│
     │                     │                      │
     │──── Tap A ──────────│                      │
     │  CONFIRM(accepted)  │                      │
     │  [updates display]  │                      │
     ▼                     ▼                      ▼
  "-3 tokens"          "Done!"             "+3 tokens"
```

**Important**: The sender's credstick displays the proposed amount on its
e-ink screen and waits for the user to press the confirm button before
signing any tokens. This prevents a malicious relay from altering the
amount — the credstick independently shows what it's being asked to sign.
See `button-pin-auth.md` for the full button interaction model.

### APDU Protocol (New Commands for Credstick)

| Command | INS | Data In | Data Out |
|---------|-----|---------|----------|
| READ_TICKET | 0x10 | — | SignedTicket (protobuf) |
| TRANSFER | 0x20 | ticket + amount | signed Token[] (protobuf) |
| RECEIVE | 0x30 | Token[] (protobuf) | accepted (bool) |
| CONFIRM | 0x40 | accepted (bool) | — (updates display) |
| GET_BALANCE | 0x50 | — | Amount (protobuf) |

### What the Phone PoS App Does

The phone needs a minimal app (or even an NFC-capable web page) that:

1. **Hosts transaction details** — amount, description, line items
   (mirrors the `TransactionItem` from `receiver.proto`)
2. **Taps receiver credstick** → reads their `SignedTicket`
3. **Taps sender credstick** → sends TRANSFER APDU with the ticket,
   amount, and sender's own tokens; receives signed transfer tokens back
4. **Taps receiver credstick** → sends RECEIVE APDU with the tokens
5. **Taps sender credstick** → sends CONFIRM to update display

The phone never sees private keys or performs any signing. It's purely
a data shuttle. Any phone with NFC can do this — even a stranger's phone.

### Mobile App Changes Required

The existing `PayScreen.kt` and `ReceiveScreen.kt` use QR codes for
phone-to-phone transfer. For credstick support, we need:

1. **New `CredstickPayScreen`**: Replaces QR scan with NFC tap sequence.
   Same flow as `PayScreen` but reads ticket via NFC instead of camera.

2. **New `PosRelayScreen`**: The PoS relay mode. Phone holds no keys.
   Guides user through the 4-tap sequence above.

3. **`WalletRepository` additions**:
   - No new crypto — the phone just ferries bytes
   - New methods: `readCredstickTicket()`, `sendToCredstick()`,
     `receiveFromCredstick()`
   - These are thin wrappers around Android `IsoDep` / iOS `NFCTagReaderSession`

4. **Receiver service (receiver.proto) adaptation**:
   The existing `Initiate → Transact → Transfer` flow maps cleanly:
   - `Initiate` = tap receiver, get ticket + items
   - `Transact` = tap sender, propose tokens
   - `Transfer` = tap receiver, deliver signed tokens

### Received Funds: Store-and-Forward

When a credstick **receives** tokens via NFC, it stores them in flash.
These tokens are cryptographically valid but haven't been validated
against the tokenmap (double-spend check). The credstick can:

1. **Hold them**: Display shows "+3 tokens (unvalidated)" — the tokens
   are usable for further peer transfers but the receiver bears risk
2. **Forward to phone later**: Next time the credstick taps a phone
   running the wallet app, the phone can pull the received tokens and
   submit them to the Receiver service for validation
3. **Re-spend them**: Transfer the received tokens to another credstick
   or phone. The next entity in the chain inherits the validation risk

This is analogous to physical cash: you accept it on faith during the
transaction, and the bank (tokenmap) validates it when deposited.

## Power Impact: Can Supercaps Handle It?

### Single Transaction (Sender Side)
- BLS12-381 signing: ~37.5mAs
- NFC active (2 taps × ~2s): ~10mAs
- E-ink update: ~32mAs
- **Total: ~80mAs at ~3V = ~240mJ**

### Single Transaction (Receiver Side)
- Ticket read (passive): ~5mAs
- Token receive + verify: ~20mAs
- E-ink update: ~32mAs
- **Total: ~57mAs at ~3V = ~171mJ**

### Supercap (10F at 3V) Budget
- Usable energy: **25J**
- Sender transactions: **~104**
- Receiver transactions: **~146**

**Verdict: Supercaps are sufficient.** No battery needed. A 10F supercap
handles >100 transactions. USB-C refill takes ~30 seconds.

### Does Piezo Help?

Yes, meaningfully:
- Passive keychain carry: 1.4-5.8J/day (offsets self-discharge ~2.5J/day)
- Deliberate 10s shake: 50-200mJ (not enough alone for a full txn)
- **Main value**: Keeps supercap from fully discharging during idle
  periods, so the device is always ready for a quick tap

A user who carries the credstick daily on their keychain and
occasionally charges via USB-C has a device that effectively never dies.

## Future: Credstick-to-Credstick Without Phone

### Option A: BLE (Bluetooth Low Energy)
The nRF52840 has BLE built in. Two credsticks could negotiate directly
over BLE — but this requires both to be actively powered (not just
field-powered), adding significant complexity and power draw.

### Option B: NFC Peer-to-Peer (LLCP)
NFC has a peer-to-peer mode (LLCP/SNEP) where two NFC-enabled devices
can exchange data bidirectionally. However:
- Both devices need to be active (not tag mode)
- The nRF52840's NFC peripheral only supports tag mode (Type 2/4)
- Would need an external NFC controller (e.g., PN532) for P2P
- Adds BOM cost and complexity

### Option C: One Credstick Emulates a Reader
If one credstick could act as an NFC reader (not just a tag), it could
directly power and communicate with the other. But:
- The nRF52840's NFC is tag-only
- NFC reader mode requires significantly more power (generating the field)
- Would need ~100-200mA to generate a field — too much for supercap

**Recommendation**: Phone relay is the right approach. It's simpler,
more power-efficient, and every potential user already has a phone.
