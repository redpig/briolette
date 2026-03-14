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
deferred payments, the sender's credstick must have the full ticket
at signing time. The ticket contains the recipient's randomized TTC
credential — a pre-commitment that the recipient must later prove
ownership of to spend the tokens. You can't sign to a credential you
don't have, so the full ticket must be acquired before any deferred
payment.

| Method | How It Works | UX |
|--------|-------------|-----|
| **Relay-mediated** | Customer taps relay, relay pushes registered merchant tickets to credstick | First visit to market |
| **Live transaction** | Receiver's ticket received during normal Initiate flow; credstick saves it | First payment via relay |
| **QR → phone → credstick** | Scan merchant's posted QR code with phone, push ticket to credstick via NFC | One-time, needs a phone |
| **USB import** | Import ticket data from file via USB setup tool | Setup at home |

All methods require a one-time interaction with some device (relay,
phone, or computer) to bootstrap the contact. After that, the credstick
can sign deferred payments to that contact from anywhere, indefinitely
(until the ticket expires).

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

### Contact Acquisition: How Tickets Get Saved

Since deferred payment requires the recipient's full `SignedTicket` on
the credstick at signing time, the "type in a short code" flow from
the original design doesn't work — you can't sign to a credential you
don't have. Short codes can still serve as **human-friendly labels**
for already-saved contacts, but the full ticket must be acquired first.

**Ways to acquire a contact's ticket:**

| Method | When | How |
|--------|------|-----|
| Relay-mediated | First visit to merchant/relay | Tap relay → relay pushes merchant tickets to credstick |
| Live transaction | First payment via relay | Receiver's ticket is received as part of the Initiate flow; credstick saves it |
| USB import | Setup at home | Import ticket data from a file or QR code via USB tool |
| Phone bridge | Any time with a phone | Scan merchant's posted QR code with phone, push ticket to credstick via NFC |

The **relay-mediated** path is the natural bootstrapping flow for the
village market scenario:

```
Market day setup (one-time per merchant):
  1. Merchant taps relay → relay stores merchant's ticket
  2. Customer taps relay → relay pushes all registered merchant
     tickets to the credstick as saved contacts
  3. Credstick now has: "Market A (001)", "Market B (002)", etc.
  4. Future payments to these merchants can be deferred
```

The relay acts as a **contact directory** in addition to a deposit box.
New customers visiting the market for the first time tap the relay once
to acquire all local merchant contacts. After that, they can prepare
payments at home.

Short codes (001, 002, etc.) are local labels assigned by the relay
for human convenience — they're displayed on the e-ink when selecting
a contact, but the cryptographic binding is always the full ticket.

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

## Why Tickets Are Required (Not Optional)

An earlier version of this document explored replacing the recipient's
`SignedTicket` with a claim-code commitment for "stranger payments."
That analysis was wrong. Here's why tickets are fundamental, not just
a convenience.

### The Ticket as Pre-Commitment

From `token.proto`:

```protobuf
message Transfer {
  SignedTicket recipient = 1;     // contains recipient's randomized credential
  repeated Tag tags = 2;
  bytes previous_signature = 3;   // used as ECDAA basename
}
```

The `SignedTicket` contains the recipient's **randomized TTC credential**.
This isn't just a label — it's a **pre-commitment**. When tokens are
transferred to a ticket, the recipient's credential is baked into the
token's transfer history. To spend those tokens next, the recipient
must prove they hold the secret key behind that credential by using it
to sign the next transfer.

This is how the chain of custody works:

```
mint signs → Transfer{ recipient: Alice's credential }
                                     │
             Alice proves she holds ─┘ this credential
             by signing the next transfer with it
                     │
                     └─→ Transfer{ recipient: Bob's credential }
                                               │
                          Bob proves he holds ──┘ this credential
                          ...
```

Each link in the chain is cryptographically verified: the signer of
transfer N must prove they own the credential named in transfer N-1.
**Without a real credential in the Transfer, the chain breaks.** There's
no credential for the next spender to prove ownership of.

### Why Claim-Codes Can't Replace Tickets

If we signed to `H(pickup_secret)` instead of a real credential:

1. The collector receives the tokens at the relay
2. The collector wants to spend them (transfer to a merchant)
3. The merchant's wallet verifies the token chain
4. It finds a `claim_hash` where a credential should be
5. **The collector can't prove they "own" a hash** — there's no ECDAA
   credential to sign with. The chain of custody is broken.

A self-transfer at collection doesn't fix this: the self-transfer needs
the collector to prove they held the credential from the *previous*
hop — but the previous hop's "credential" was a hash, not a real TTC
credential.

### Consequence: Saved Contacts Are Required

Deferred payment **requires** the sender to have the recipient's
`SignedTicket` saved in advance. This is not a limitation we can
engineer away — it's a fundamental property of the credential chain.

This means the "pay a stranger" scenario requires a prior interaction
to exchange tickets:

```
First meeting (at a relay):
  Alice taps relay → relay reads Alice's ticket
  Bob taps relay → relay pushes Alice's ticket to Bob's credstick
  (Bob now has Alice as a saved contact)

Later (anywhere, no relay needed):
  Bob signs a deferred payment to Alice using her saved ticket
  Bob drops off at relay whenever convenient
  Alice collects
```

The first meeting is the bootstrapping step. After that, Bob can pay
Alice from anywhere without any device present. This is like exchanging
bank account numbers — you do it once, then wire transfers work forever
(until the ticket expires and needs renewal).

### The Pickup Code's Actual Role

Since tokens are always signed to a real ticket, the pickup code is
purely a **UX verification aid** — not a cryptographic binding:

- It lets the recipient visually confirm "yes, that deposit is mine"
- It's derived from the signed token data for integrity checking
- The relay matches deposits to recipients via the ticket hash in the
  token, not via the pickup code
- The tokens are cryptographically bound to the recipient's credential
  regardless of any pickup code

This is actually *better* security than the claim-code model: even a
compromised relay can't steal the tokens, because they're signed to a
credential only the recipient holds. The relay is truly a dumb deposit
box with zero trust required.

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

5. **Ticket expiry and contact refresh**: Tickets have a limited lifetime
   (N epochs). Saved contacts will go stale. How does a credstick learn
   that a contact's ticket has expired? Options: (a) relay pushes updated
   tickets during any tap, (b) credstick refuses to sign to an expired
   ticket and displays "contact expired — visit relay", (c) contacts
   include a "valid_until" hint so the credstick can warn proactively.
