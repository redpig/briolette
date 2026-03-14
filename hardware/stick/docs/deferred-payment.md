# Deferred Payment: Credstick-Only Transfers via Code

> **Status**: Future extension (v2). Does not replace any existing transfer
> protocol — this builds on the live relay-mediated flow as an additional
> capability. The goal is a natural, highly resilient evolution that works
> in scenarios where no relay is present at payment time.
>
> **Hardware impact**: The v1 credstick and relay hardware designs should
> account for this by reserving flash storage and APDU command space.
> No firmware changes needed for v1 — just don't paint ourselves into
> a corner.

## The Gap

Credstick-to-credstick transfers require a relay device (phone or solar
relay) because NFC is asymmetric — two tags can't power each other. This
means every peer transfer needs a third device present at transaction time.

For merchants with a storefront, a phone or solar relay (~$19) is fine.
But for casual peer-to-peer transfers — splitting costs, paying a neighbor,
sending money with a family member — requiring a relay at the moment of
exchange is friction that physical cash doesn't have.

## Deployment Reality

The relay requirement shapes deployment patterns:

| Context | Relay Availability | Notes |
|---------|-------------------|-------|
| Merchant storefront | Dedicated phone or solar relay | Always available, part of doing business |
| Urban family | ~1 per household | Shared device, like a kitchen scale |
| Rural village | ~1 per village (market square) | Community resource at central market |
| Peer-to-peer (casual) | **Gap** — neither party may have one | This is the problem |

For merchants and community use, the relay model works well. The gap is
casual transfers between two people who each only carry a credstick.

## Solution: Deferred Payment via Merchant/Recipient Code

Instead of requiring both parties and a relay to be co-located at
transaction time, split the transfer into two phases:

1. **Sign phase** (credstick only): The sender prepares and signs a
   payment using a saved or entered recipient code
2. **Deliver phase** (at any relay, later): The sender drops off the
   pre-signed tokens at a relay for the recipient to collect

This is analogous to writing a check: you prepare the payment alone,
then it gets delivered and deposited through the banking system.

### Recipient Codes

The receiver's `SignedTicket` is the cryptographic identity needed to
transfer tokens. Today it's exchanged via NFC tap or QR code. For
deferred payments, we need ways to represent it on a credstick:

| Method | How It Works | UX |
|--------|-------------|-----|
| **Saved contacts** | Receiver's ticket stored in credstick flash, associated with a name/number | Select from list on e-ink display |
| **QR scan at any time** | Scan a merchant's posted QR code with a phone, push ticket to credstick via NFC | One-time setup per merchant |
| **Short numeric code** | Truncated hash of ticket, resolved at relay drop-off | Type on credstick buttons |
| **NFC tag sticker** | Passive NFC tag containing ticket, posted at merchant's stall | Tap credstick to the sticker |

The **saved contacts** approach is most practical for repeat payments
(rent, regular merchants, family). The **NFC tag sticker** is interesting
for merchants — a $0.10 NFC sticker on the counter lets any credstick
read the merchant's ticket without a relay.

### NFC Tag Stickers: Ultra-Cheap "Merchant ID"

A passive NFC tag (NTAG216, ~$0.10-0.30) can store up to 888 bytes.
A `SignedTicket` serialized via protobuf is ~200-300 bytes. This fits
easily. The sticker:

- Is powered by the credstick reader's field (wait — the credstick is
  tag-only too). **Problem**: Two tags can't talk.
- **Alternative**: The sticker is read by the customer's phone, which
  pushes the ticket to the credstick. Or the solar relay reads the
  sticker during setup.

Actually, for the deferred payment model, the sticker approach requires
a reader at some point. The more practical options are:

1. **Pre-saved**: Merchant ticket saved to credstick during an earlier
   relay-mediated interaction
2. **QR code → phone → credstick**: Scan QR once, push to credstick
3. **Manual entry**: Short merchant code (like a phone number) entered
   via buttons, resolved to full ticket at relay drop-off

### Flow: Credstick-Only Payment

```
  Sender Credstick                    (later)  Relay          Receiver
     │                                            │               │
     │  [Select recipient from                    │               │
     │   saved contacts or enter                  │               │
     │   merchant code]                           │               │
     │                                            │               │
     │  E-ink: "Pay Alice 5 tokens?"              │               │
     │  [Enter PIN if required]                   │               │
     │  [Button press = confirm]                  │               │
     │                                            │               │
     │  ── Signs tokens locally ──                │               │
     │  BLS sign with recipient's                 │               │
     │  saved SignedTicket as                      │               │
     │  transfer target                           │               │
     │                                            │               │
     │  E-ink: "-5 tokens (pending                │               │
     │          delivery)"                        │               │
     │                                            │               │
     │  [Tokens signed and stored                 │               │
     │   in flash, tagged for                     │               │
     │   delivery to Alice]                       │               │
     │                                            │               │
     ╞══════ minutes, hours, days ═══════════════╡               │
     │                                            │               │
     │  ──── Tap relay ──────────────────────────│               │
     │  DROP_OFF (0x40)                           │               │
     │  [signed tokens + recipient ticket hash]   │               │
     │                                            │               │
     │  E-ink: "-5 tokens (delivered)"            │               │
     │                                            │               │
     │                                            │  Stores tokens│
     │                                            │  in deposit   │
     │                                            │  box for Alice│
     │                                            │               │
     │                                            ╞═══ later ════╡
     │                                            │               │
     │                                            │──── Tap ─────│
     │                                            │  Alice sweeps │
     │                                            │  her deposits │
     │                                            │               │
     ▼                                            ▼               ▼
  Balance                                     Relay acts      "+5 tokens
  updated                                     as deposit      (unvalidated)"
                                              box
```

### What "Signing Locally" Means

The credstick already has everything it needs to sign a transfer:

1. **Full BLS12-381 ECDAA private key** (in nRF52840 flash, protected by
   APPROTECT fuse) — the credstick is a standalone signing device, no
   phone or split-key needed
2. **Recipient's SignedTicket** (saved contact — contains their TTC
   credential, which is the basename for the transfer signature)
3. **Token selection logic** (pick tokens from flash to fulfill amount)

This is a key property of the credstick: it holds its own complete ECDAA
credential and signs independently. Split-key is the *phone* wallet's
security model (phone + credstick together for a higher attestation tier).
A credstick operating standalone has full signing capability — which is
exactly what makes deferred payment possible without any other device
present at signing time.

The signing process is identical to what happens during a live NFC
transfer — the credstick signs tokens transferring them to the
recipient's ticket. The only difference: instead of handing the signed
tokens to a relay immediately, they're stored in flash for later delivery.

### Credstick Firmware Changes

New state for tokens in flash:

```
Token States:
  OWNED        — tokens belonging to this credstick
  PENDING_SEND — signed for a recipient, awaiting relay drop-off
  RECEIVED     — received from another party, unvalidated
```

New APDUs:

| Command | INS | Data In | Data Out |
|---------|-----|---------|----------|
| PREPARE_PAYMENT | 0x40 | recipient_id + amount | tx_preview (for e-ink) |
| CONFIRM_PAYMENT | 0x41 | PIN_ok flag | signed tokens stored internally |
| DROP_OFF | 0x42 | — | PENDING_SEND tokens + recipient info |
| LIST_DEPOSITS | 0x43 | recipient_ticket_hash | Token[] (for sweep) |

Note: PREPARE_PAYMENT and CONFIRM_PAYMENT are **internal** operations
triggered by button presses, not NFC APDUs. The credstick handles the
entire payment flow on-device. DROP_OFF is the NFC APDU used when
tapping a relay.

### The Relay as Deposit Box ("Cryptographic Village Bank")

The relay gains a new role beyond real-time transaction relay: it becomes
a **deposit box** where pre-signed tokens wait for collection.

```
Relay Storage:
┌──────────────────────────────────────────────┐
│  Deposit Box                                  │
│                                               │
│  ┌─────────────────────────────────────────┐ │
│  │ Recipient: Alice (ticket hash 0xA3F...)  │ │
│  │ Tokens: 5 (from sender hash 0x7B2...)    │ │
│  │ Deposited: 2024-01-15 14:30              │ │
│  │ Status: awaiting collection              │ │
│  ├─────────────────────────────────────────┤ │
│  │ Recipient: Bob (ticket hash 0x91C...)    │ │
│  │ Tokens: 3 (from sender hash 0xE5A...)    │ │
│  │ Deposited: 2024-01-15 09:15              │ │
│  │ Status: awaiting collection              │ │
│  └─────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

When Alice taps the relay, it checks for deposits matching her ticket
hash and transfers them to her credstick.

### Relay Firmware Changes

The relay needs persistent storage for deposits. Options:

| Storage | Capacity | Cost | Notes |
|---------|----------|------|-------|
| nRF52840 flash (unused portion) | ~200 KB | $0 | ~600 token deposits |
| External QSPI flash (MX25R4035F) | 4 MB | ~$0.50 | ~13,000 deposits |

For a village relay handling a few dozen daily transactions, the internal
flash is sufficient. A busy merchant relay might benefit from external
flash.

New relay APDUs:

| Command | INS | Data In | Data Out |
|---------|-----|---------|----------|
| DROP_OFF | 0x42 | signed Token[] + recipient_hash | deposit_id |
| CHECK_DEPOSITS | 0x43 | my_ticket_hash | count + total amount |
| COLLECT | 0x44 | my_ticket_hash + signature proof | Token[] |

**COLLECT requires proof**: The recipient must prove they own the ticket
hash by signing a challenge with their TTC credential. This prevents
someone from collecting another person's deposits by guessing their
ticket hash.

### Short Merchant Codes

For the "type in a code" flow, we need a way to map short human-friendly
codes to full SignedTickets. Options:

**Option A: Relay-Resolved Codes**

The relay maintains a registry of known merchants:

```
Code    Merchant          Ticket Hash
001     Market Stall A    0xA3F...
002     Market Stall B    0x91C...
003     Village Elder     0xE5A...
```

The sender enters "001" on their credstick. At drop-off, the relay
resolves it to the full ticket. This requires the sender to visit a relay
that knows the recipient — works well for village-scale deployments where
everyone uses the same relay.

**Option B: Truncated Ticket Hash**

Use the first N digits of the ticket hash as a short code. For a village
of ~1000 people, 6 hex digits (24 bits) gives <0.01% collision probability.
The credstick stores the short hash; the relay resolves it during drop-off
or the recipient proves ownership during collection.

**Option C: Hierarchical Codes (Like Phone Numbers)**

```
[region][village][merchant] = 3-digit code
  01      03       07      = "010307"
```

Managed by whoever operates the relay infrastructure. More structured
but requires coordination.

**Recommendation**: Option A for simplicity. A village relay with a
local merchant registry is the natural fit. Option B as fallback for
inter-village transfers.

## Security Considerations

### Double-Spend Window

Deferred payments widen the double-spend window. With live transfers,
the receiver (or relay) can immediately check for double-spends if
online. With deferred payments:

- Tokens are signed at time T₁
- Tokens are dropped off at time T₂ (T₂ - T₁ could be hours or days)
- During that window, the sender still physically possesses the signed
  tokens and could attempt to spend the same tokens in a live transaction

**Mitigation**: The credstick firmware marks tokens as PENDING_SEND
immediately upon signing. A well-behaved credstick won't double-spend
them. A tampered credstick could, but this is the same threat model as
any offline transfer — ultimately caught by the tokenmap when the tokens
reach online infrastructure.

This is no worse than physical cash: if you hand someone an IOU, they
bear risk until they deposit it. The ECDAA revocation system catches
the cheater eventually.

### Relay Trust

The relay sees signed tokens in transit. It could:

- **Drop tokens** (refuse to store): Sender retains tokens, can re-drop
  elsewhere. Annoying but not a theft.
- **Claim to deliver but don't**: Sender shows "delivered" but recipient
  never gets them. Mitigated by receipts (relay signs a deposit receipt
  the sender can verify).
- **Copy tokens and try to spend them elsewhere**: Tokens are signed to
  the recipient's ticket — they're cryptographically bound and useless
  to the relay. This is the key security property.

The relay is a **dumb deposit box**, not a trusted intermediary.

### Deposit Expiry

Uncollected deposits should expire after a configurable period (e.g.,
30 days). Expired deposits can be reclaimed by the sender (they still
have the tokens' signing history). The relay evicts stale deposits to
free storage.

## Deployment Scenarios

### Village Market ("Cryptographic Village Bank")

```
                    ┌──────────────┐
                    │ Solar Relay  │
                    │ (market      │
                    │  square)     │
                    └──────┬───────┘
                           │
            ┌──────────────┼──────────────┐
            │              │              │
       ┌────▼────┐   ┌────▼────┐   ┌────▼────┐
       │ Merchant│   │ Merchant│   │ Farmer  │
       │ A       │   │ B       │   │ C       │
       └─────────┘   └─────────┘   └─────────┘

1. Merchants register tickets at relay (one-time setup)
2. Customers save merchant codes to their credsticks
3. At home: customer signs payment on credstick (no relay needed)
4. At market: customer taps relay to drop off payments
5. Merchant taps relay to collect accumulated payments
6. Relay acts as village bank — deposit box for everyone
```

One solar relay serves the entire village. Customers can prepare
payments at home and batch-drop them at the market.

### Urban Merchant

```
┌──────────────┐     ┌──────────────┐
│ Phone PoS    │     │ Solar Relay  │
│ (countertop) │     │ (backup)     │
└──────┬───────┘     └──────┬───────┘
       │                     │
  Live payments         Deferred payments
  (real-time)           (drop-off box)
```

The merchant runs a phone PoS for live payments but also accepts
deferred drop-offs. Customers who frequent the store save the merchant
code and can prepare payments at home.

### Family Transfer

```
  Parent                             Child
  Credstick                          Credstick
     │                                  │
     │ "Send 10 tokens to              │
     │  Junior (saved contact)"        │
     │ [PIN] [confirm]                 │
     │                                  │
     │── later, at home relay ──────────│
     │  DROP_OFF                        │
     │                     COLLECT ─────│
     │                                  │
```

Family members save each other as contacts. A shared home relay acts
as the family deposit box. Kids can collect their allowance by tapping
the relay.

## Relationship to Existing Protocol

Deferred payment reuses the existing transfer protocol almost entirely:

| Existing | Deferred Equivalent |
|----------|-------------------|
| `Initiate` (get receiver ticket) | Load from saved contacts |
| `Transact` (propose tokens) | Internal: select tokens from flash |
| `Transfer` (sign and deliver) | Sign locally, store as PENDING_SEND |
| NFC delivery to receiver | DROP_OFF at relay + COLLECT by receiver |

The cryptographic operations are identical. Only the transport changes:
instead of real-time NFC relay, it's store-and-forward via the deposit
box relay.

## Open Questions

1. **Deposit notification**: How does Alice know she has deposits waiting?
   Options: (a) she checks periodically by tapping the relay, (b) a
   community board/display at the relay shows pending deposit counts
   (no amounts, just "Alice: 2 deposits"), (c) word of mouth.

3. **Multi-relay deposits**: If a sender drops tokens at Relay A but the
   recipient usually uses Relay B, how do deposits flow between relays?
   Options: (a) they don't — sender must use the same relay as recipient,
   (b) relays sync deposits when they're occasionally connected (sneakernet
   or when a phone bridges them), (c) recipients check all local relays.

4. **Receipt/confirmation**: Should the relay give the sender a
   cryptographic receipt proving the deposit was stored? This would let
   the sender prove they paid even if the relay later fails.
