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

### E-ink Display Sequence

The e-ink screen is the receipt the recipient watches. It needs to build
confidence that real tokens were signed to *them* specifically, not faked.

**Step 1: Recipient Selection**
```
┌────────────────┐
│  Pay who?       │
│                 │
│ ► Alice    002  │
│   Bob      003  │
│   Market   007  │
│                 │
│  ◄ ▲ ▼ ►       │
└────────────────┘
```

Recipient sees their own name/code selected. The number is the local
contact code matching what the relay knows.

**Step 2: Amount Confirmation**
```
┌────────────────┐
│  Pay Alice      │
│  5 tokens?      │
│                 │
│  ◄ No    Yes ►  │
└────────────────┘
```

Both parties can see the amount. Sender enters PIN if required
(above threshold), then confirms.

**Step 3: Signing (brief, ~2-3 seconds)**
```
┌────────────────┐
│                 │
│  Signing...     │
│  ████████░░ 80% │
│                 │
└────────────────┘
```

The BLS signing operation takes ~2 seconds. A progress bar shows it's
doing real cryptographic work, not faking it. (A static fake screen
would be instant — the delay itself is a weak authenticity signal.)

**Step 4: Confirmation with Pickup Code**
```
┌────────────────┐
│ ✓ Sent 5       │
│ → Alice         │
│                 │
│ ◉ 37 tokens    │
│ Pickup: 7H3K   │
└────────────────┘
```

This screen is the critical trust moment. It shows:

- **✓ Sent 5**: Tokens were signed (past tense, committed)
- **→ Alice**: The specific recipient (they see their own name)
- **◉ 37 tokens**: Sender's new balance (decreased by 5 — visible proof
  the tokens left). This is important — the recipient watches the
  balance go *down*
- **Pickup: 7H3K**: A short verification code derived from the
  transaction. Alice notes this and can verify it at collection time

**The pickup code** is the key trust mechanism. It's derived from the
signed token data:

```
pickup_code = base32(SHA-256(signed_tokens || recipient_ticket)[:3])
            → 4 alphanumeric characters, ~20 bits
```

When Alice collects at the relay, the relay displays the same code
computed from the deposited tokens. If they match, Alice knows the
tokens she's collecting are the ones she watched get signed. If the
sender faked their screen, no matching deposit exists at the relay.

20 bits is low collision resistance, but the code's purpose isn't
global uniqueness — it's a spot-check between two people who were
present at signing time. For a village relay with <100 daily deposits,
collisions are negligible.

**Step 5: Post-delivery (after relay drop-off)**
```
┌────────────────┐
│ ✓ Sent 5       │
│ → Alice         │
│   (delivered)   │
│ ◉ 37 tokens    │
└────────────────┘
```

The pickup code is replaced with "(delivered)" once the relay confirms
receipt. The sender's e-ink updates on the DROP_OFF tap.

### Recipient's Perspective: Collection at Relay

When Alice taps the relay to collect:

**Solar relay (LED-only):**
- Green LED blinks N times for N deposits waiting
- After COLLECT: green LED solid = success
- Alice's credstick e-ink shows the received tokens with the same
  pickup code for verification:

```
┌────────────────┐
│ + 5 tokens      │
│   (unvalidated) │
│ ← pickup: 7H3K  │
│                 │
│ ◉ 28 tokens    │
└────────────────┘
```

**Phone PoS relay:**
```
┌─────────────────────────┐
│  Deposits for Alice      │
│                          │
│  1. 5 tokens  pickup:7H3K│
│     from: contact 001    │
│     signed: 2h ago       │
│                          │
│  Tap credstick to collect│
└─────────────────────────┘
```

Alice compares the pickup code on the relay/phone screen with what she
remembers (or wrote down) from when she watched the sender's e-ink.
Match = confidence the tokens are legitimate.

### Why This Builds Trust

| Signal | What it proves | Fakeable? |
|--------|---------------|-----------|
| Balance decreases | Tokens actually left sender's wallet | Hard — would require maintaining a fake balance across all future transactions |
| Signing delay (~2s) | Real BLS12-381 computation happened | Trivially fakeable with a timer, but absence would be suspicious |
| Recipient name shown | Payment is addressed to *you* | Easy to fake the display, but pointless — fake tokens won't appear at relay |
| Pickup code | Cryptographic binding between what you saw and what you'll collect | **Not fakeable** — derived from actual signed token data |

The e-ink display alone is *not* tamper-proof — a modified credstick
could show anything. The real security comes from the pickup code +
relay verification loop. The display is a UX convenience that gives
*immediate* confidence; the relay deposit is the *actual* guarantee.

This mirrors physical cash: you look at the bills being handed over
(immediate visual confidence), and the bank validates them later
(actual guarantee). The difference is the pickup code gives you a
way to tie the two together cryptographically.

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

## Are Tickets Still the Right Destination Binding?

This is the central v2 protocol question. In v1, every transfer signs
tokens to a recipient's `SignedTicket`. For deferred payment, we assumed
the sender would need the recipient's ticket saved in advance. But the
pickup code concept opens a different possibility: **what if the tokens
aren't signed to a ticket at all?**

### How v1 Transfer Signing Works

From `token.proto`, a Transfer binds a recipient and gets signed:

```protobuf
message Transfer {
  SignedTicket recipient = 1;     // destination binding
  repeated Tag tags = 2;
  bytes previous_signature = 3;   // basename for ECDAA double-spend detection
}
```

Two critical and **independent** mechanisms:

1. **Double-spend detection**: Uses `previous_signature` as the ECDAA
   basename. If the same token (same previous_signature) is signed twice
   by the same wallet, the two signatures produce the same pseudonym K —
   linkable, detectable, revocable. **This has nothing to do with who
   the recipient is.**

2. **Destination binding**: The `recipient` SignedTicket says "only the
   holder of this ticket's credential can spend these tokens next." This
   is the chain of custody.

Since these are independent, we can change #2 without breaking #1.

### Three Destination Models for v2

**Model A: Ticket-Bound (v1 compatible)**

Same as today. Sender has Alice's `SignedTicket` saved as a contact.
Signs tokens to her ticket. Alice collects and can immediately re-spend.

```
Transfer { recipient: Alice's SignedTicket }
```

- Requires saved contacts (sender must have Alice's ticket)
- Strongest security — tokens cryptographically bound to Alice
- Chain of custody intact
- Relay can't steal tokens (they're useless without Alice's credential)

**Model B: Claim-Code Bound (new)**

Sender doesn't have Alice's ticket. Instead, signs tokens to a
**claim commitment** — a hash of a shared secret:

```
Transfer { claim_hash: H(pickup_secret) }   // new field
```

The `pickup_secret` is generated by the sender's credstick. The short
pickup code shown on e-ink (e.g., "7H3K") is derived from it. The
sender tells Alice the code (verbally, or she reads it off the screen).

At collection, Alice presents the `pickup_secret` to the relay. The
relay verifies `H(pickup_secret) == claim_hash` in the token, then
hands over the tokens. Alice does a **self-transfer** to bind them to
her own ticket for future spending.

```
Token chain: mint → ... → sender(claim=H(secret)) → Alice(ticket)
                                                     ↑
                                          self-transfer at collection
```

- No saved contacts needed — pay anyone, even strangers
- Weaker during transit — tokens are bearer-like (anyone with the
  secret can claim). The relay is the custody point.
- Extra transfer hop (self-transfer at collection) adds to token history
- Double-spend detection still works (based on previous_signature,
  not the destination)

**Model C: Hybrid (claim with optional ticket hint)**

The sender signs to a claim commitment but also includes an optional
ticket hint — a truncated hash of the intended recipient's ticket:

```
Transfer {
  claim_hash: H(pickup_secret),
  recipient_hint: truncate(H(Alice's ticket), 8 bytes)  // optional
}
```

The relay uses the hint to notify Alice ("you have a deposit") and to
prioritize delivery. But the cryptographic binding is the claim, not the
ticket. If the hint is wrong or absent, Alice can still collect with the
pickup secret.

This gives us the flexibility of Model B with a routing optimization
from Model A.

### Comparison

| Property | A: Ticket-Bound | B: Claim-Code | C: Hybrid |
|----------|----------------|---------------|-----------|
| Need recipient's ticket? | Yes (saved) | No | Optional (for routing) |
| Pay a stranger? | No | Yes | Yes |
| Tokens bound to recipient? | Cryptographically | By shared secret | By shared secret |
| Relay can steal tokens? | No | Only if it learns the secret | Only if it learns the secret |
| Token history growth | Same as v1 | +1 hop (self-transfer) | +1 hop |
| Chain of custody | Fully verified | Gap at claim step | Gap at claim step |
| Protocol change needed? | None | New Transfer field | New Transfer field |

### Recommendation

Support both Model A and Model B. The `Transfer` message gains a new
oneof:

```protobuf
message Transfer {
  oneof destination {
    SignedTicket recipient = 1;   // v1: ticket-bound
    bytes claim_hash = 5;        // v2: claim-code-bound (32 bytes)
  }
  repeated Tag tags = 2;
  bytes previous_signature = 3;
}
```

- **Saved contact exists?** → Use ticket-bound (Model A). Strongest
  security, no extra hop.
- **No saved contact?** → Use claim-code (Model B). Flexible, works
  with anyone, but bearer-like during transit.

This is analogous to: wire transfer (you need the account number) vs.
cashier's check (anyone can deposit it with the right endorsement).

### Claim-Code Security Properties

The pickup secret needs to be strong enough that the relay can't
brute-force it, but short enough for human exchange:

```
pickup_secret: 128-bit random (generated by credstick)
pickup_code:   base32(pickup_secret[:3]) = 4-5 characters (for display)
claim_hash:    SHA-256(pickup_secret) = 32 bytes (in token)
```

Wait — if the pickup code is only 4 characters (~20 bits), can the
relay brute-force it? The relay has the `claim_hash` and just needs to
find a preimage that matches. 2^20 = ~1M attempts. At even 1M
hashes/sec on a microcontroller, that's 1 second.

**This is a problem.** The short pickup code is for human UX. The
actual claim secret must be longer. Two options:

**Option 1: Long secret, short display code**

The full `pickup_secret` is 128 bits. The 4-char pickup code is just a
*check digit* — it doesn't unlock the claim. Collection requires the
full secret, which is transferred from sender's credstick to recipient's
credstick via the relay during collection (encrypted to relay's session).

The display code is purely for human verification ("is this the right
deposit?"), not for access control.

**Option 2: Sender and recipient both tap the relay**

At drop-off, the sender gives the relay the full secret. At collection,
the recipient proves identity by owning the ticket hinted in the deposit
(if hint present) or by verbally telling the relay operator the short
code (for human-mediated relays). The relay matches and releases.

**Recommendation**: Option 1. The pickup code on the e-ink is a
verification aid, not a key. The full claim secret travels:
sender credstick → relay (at drop-off) → recipient credstick (at
collection, encrypted). The relay holds the secret in its deposit box.
The 4-char code lets humans say "yes, that's the right deposit" without
being the security mechanism.

## Amount Entry on the Credstick

For live relay-mediated transfers, the relay (phone or solar relay)
specifies the amount in the INITIATE APDU. But for deferred payments,
the credstick must accept amount input from the user via buttons alone.

### The Problem

The credstick has 2 buttons (keychain form factor) or 4 buttons (card
form factor) and a small e-ink display. Entering arbitrary amounts like
"17.50" with 2 buttons is painful. We need to make the common cases
fast and the uncommon cases possible.

### Approach: Tiered Input Methods

**Tier 1: Preset amounts from saved contacts**

Each saved contact can have associated preset amounts:

```
Saved Contacts:
  Alice (family)     — presets: 10, 20, 50
  Market Stall A     — presets: 5, 15
  Bus Fare           — preset: 2 (fixed)
```

When the user selects a contact, the default amount is pre-filled:

```
┌────────────────┐
│ Pay Market A    │
│                 │
│ Amount: [5]     │
│ ◄ 5   15 ►     │
└────────────────┘
```

L/R scrolls through the contact's presets. Long-press-both confirms.
This handles the most common case (regular payments to known recipients)
with just 2-3 button presses.

**Tier 2: Increment/decrement from a preset**

If the exact amount isn't a preset, start from the nearest one and
adjust:

```
┌────────────────┐
│ Pay Alice       │
│                 │
│ Amount: [12]    │
│ ◄-1       +1►  │
└────────────────┘
```

Short-press L/R: ±1. Long-press L/R: ±10 (or ±5, configurable).
Starting from a preset of 10, reaching 12 takes 2 short-presses of R.

**Tier 3: Digit-by-digit entry (rare, larger amounts)**

For amounts not near any preset, switch to digit-by-digit mode:

```
┌────────────────┐
│ Pay Bob         │
│                 │
│ Amount: [1_7]   │
│ ◄▼         ▲►  │
└────────────────┘
```

With 2 buttons:
- Short-press R: increment current digit (0→1→2...→9→0)
- Short-press L: decrement current digit
- Long-press R: advance to next digit position
- Long-press L: back to previous digit position
- Long-press both: confirm

Entering "17": R (→1), long-R (advance), R×7 (→7), long-both (confirm).
That's 10 presses for a 2-digit number. Not great, but workable for
rare cases.

With 4 buttons (card form factor):
- Up/Down: increment/decrement digit
- Left/Right: move between digit positions
- Much faster: "17" = Right, Up, Right, Up×7, confirm

**Tier 4: "Last amount" shortcut**

Long-press L from the contact's default screen repeats the last amount
sent to that contact. For recurring payments (weekly rent, daily bread),
this is a single button press.

### Display Flow: Amount Entry

**Select contact → select/enter amount → PIN → sign**

```
Step 1: Select contact          Step 2: Select amount
┌────────────────┐              ┌────────────────┐
│  Pay who?       │              │  Pay Alice      │
│                 │              │                 │
│ ► Alice    002  │              │  Amount: [10]   │
│   Bob      003  │              │  ◄ 10  20  50 ► │
└────────────────┘              └────────────────┘

Step 3: Confirm + PIN           Step 4: Signed
┌────────────────┐              ┌────────────────┐
│  Pay Alice      │              │  ✓ Sent 10      │
│  10 tokens?     │              │  → Alice        │
│                 │              │                 │
│  Enter PIN:     │              │  ◉ 32 tokens    │
│  _ _ _ _       │              │  Pickup: 7H3K   │
└────────────────┘              └────────────────┘
```

### Saving Presets

Presets are configured when a contact is first saved (during a
relay-mediated interaction or via USB setup):

**At a relay (one-time):**
1. Merchant taps relay → relay reads merchant's ticket
2. Customer taps relay → relay pushes merchant ticket to credstick
3. Relay also pushes merchant's preset amounts (e.g., "coffee=5,
   lunch=12") as part of the contact data
4. Customer's credstick now has the merchant as a saved contact with
   presets

**Via USB setup tool:**
- Connect credstick to computer
- Manage contacts: add/remove, set preset amounts
- Import contacts from QR codes or other credsticks

### Contact Storage Format

```
Contact {
  name: "Market A"          // 16 chars max (e-ink line width)
  code: 007                 // local short code
  ticket: SignedTicket       // full ticket data (~200-300 bytes)
  presets: [5, 15, 30]      // up to 8 preset amounts
  last_amount: 15           // most recent payment
  last_paid: epoch           // for "last amount" shortcut
}
```

~350 bytes per contact. 30 contacts = ~10.5 KB. Fits easily in the
reserved flash.

## Open Questions

1. **Deposit notification**: How does Alice know she has deposits waiting?
   Options: (a) she checks periodically by tapping the relay, (b) a
   community board/display at the relay shows pending deposit counts
   (no amounts, just "Alice: 2 deposits"), (c) word of mouth.

2. **Multi-relay deposits**: If a sender drops tokens at Relay A but the
   recipient usually uses Relay B, how do deposits flow between relays?
   Options: (a) they don't — sender must use the same relay as recipient,
   (b) relays sync deposits when they're occasionally connected (sneakernet
   or when a phone bridges them), (c) recipients check all local relays.

3. **Receipt/confirmation**: Should the relay give the sender a
   cryptographic receipt proving the deposit was stored? This would let
   the sender prove they paid even if the relay later fails.

4. **Fractional amounts**: The 2-button increment/decrement UX works well
   for whole token amounts. For fractional amounts (e.g., 17.50), the
   digit-by-digit mode needs a decimal point entry — long-press-both to
   toggle between whole and fractional digit groups? Or just restrict
   deferred payments to whole amounts and leave fractional for live relay
   transactions?

5. **Claim-code vs ticket for the relay trust model**: With claim-code
   (Model B), the relay holds the full claim secret between drop-off and
   collection. A compromised relay could claim tokens itself. With
   ticket-bound (Model A), even a compromised relay can't spend the
   tokens. Should we default to Model A (ticket-bound) when the sender
   has a saved contact, and only use Model B for stranger payments?
