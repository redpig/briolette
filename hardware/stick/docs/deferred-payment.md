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

## Do We Still Need Tickets in v2?

In v1, the `SignedTicket` does three things:

1. **Pre-commitment**: Contains the recipient's randomized TTC credential.
   The recipient must later prove they hold the secret key behind this
   credential to spend the tokens — this is what makes the chain of
   custody work.

2. **Policy enforcement**: The clerk embeds group numbers (for revocation
   bitfield checking), expiration, and optional max transaction sizes.

3. **Velocity management**: Ticket requests are signed by the NAC with
   a basename bound to the current epoch. The clerk limits how many
   tickets a given NAC pseudonym can get per epoch, preventing a single
   wallet from flooding the system with receiving addresses.

In a decentralized v2 where credsticks operate offline for extended
periods and there may be no clerk to issue tickets, #2 and #3 lose
their enforcement point. This raises the question: **could the recipient
just generate a fresh randomized credential at payment receipt time,
eliminating the ticket entirely?**

### What the Credential Actually Does

The chain of custody requires that each transfer embeds a credential
the recipient can later sign with:

```
Transfer N:   signed by holder of credential from Transfer N-1
              embeds recipient credential for Transfer N+1's signer

mint → cred_A → cred_B → cred_C → ...
       sender   signs     signs
       embeds   with      with
       cred_A   cred_A    cred_B
```

This is the non-negotiable part: you need **a real ECDAA credential**
in each transfer for the chain to be verifiable. But does that
credential need to come from a **signed ticket** (clerk-certified)?
Or could it be a **bare randomized credential** that the recipient
generates on the spot?

### Option 1: Keep Signed Tickets (v1 model)

Tickets = clerk-certified credential bundles. The clerk's P-256
signature proves:
- The credential was issued to a wallet with a valid NAC
- The credential has a group number (for revocation)
- The credential has an expiration
- The credential was rate-limited (N tickets per epoch)

**For deferred payment**: Sender must have recipient's `SignedTicket`
saved in advance. Works well for known contacts (merchants, family).
Doesn't work for strangers without a prior ticket exchange.

**Strengths**: Policy enforcement, revocation grouping, rate limiting.
**Weaknesses**: Requires clerk (centralized), tickets expire (need
refresh), can't pay strangers without prior interaction.

### Option 2: Bare Credentials (Randomized at Receipt Time)

The recipient generates a fresh randomized TTC credential on the fly
when receiving payment. No clerk involved. The credential is just the
cryptographic pre-commitment — no policy wrapping.

```
Receive flow (v2):
  1. Sender initiates payment
  2. Receiver's credstick generates a fresh randomized credential
     from its TTC secret key (standard ECDAA randomization)
  3. Credential is embedded in the Transfer
  4. Sender signs the transfer with their credential
  5. Done — no clerk, no signed ticket needed
```

**For deferred payment**: This changes the model significantly. The
sender can't pre-sign to a credential that doesn't exist yet (the
receiver would generate it at receipt time). Two sub-options:

**2a: Sender signs to a saved bare credential**

The receiver pre-generates some randomized credentials and shares them
(like pre-signed blank checks). The sender saves these as contacts.
When doing a deferred payment, the sender signs to one of the saved
credentials. The receiver later claims the tokens by proving they hold
the secret key.

This is functionally similar to v1 tickets but without the clerk
signature — just a bare credential. The receiver can generate as many
as they want, with no rate limiting.

**2b: Credential exchange happens at the relay**

For live payments at a relay, the receiver generates a fresh credential
on the spot during the tap sequence. The relay mediates the exchange.
No pre-saved contact needed.

For deferred payments, the sender still needs a saved credential —
but it's a bare one from a prior interaction, not a clerk-certified
ticket.

**Strengths**: No clerk dependency, works offline indefinitely,
simpler protocol.
**Weaknesses**: No policy enforcement (no group numbers, no expiration,
no rate limiting), revocation becomes harder.

### Option 3: Hybrid (Bare Credentials + Optional Policy Layer)

The protocol accepts both:
- **Bare credentials** (just the randomized TTC) for peer-to-peer
- **Signed tickets** (clerk-certified) when online and when policy
  matters

```protobuf
message Transfer {
  oneof destination {
    SignedTicket ticket = 1;       // v1: clerk-certified, has policy
    bytes credential = 5;         // v2: bare randomized TTC credential
  }
  repeated Tag tags = 2;
  bytes previous_signature = 3;
}
```

The verifier checks:
- **If ticket**: Verify clerk signature, check group against revocation
  bitfield, verify expiration. Full policy enforcement.
- **If bare credential**: Just verify it's a valid ECDAA credential
  from an acceptable TTC group key. No policy checks. The chain of
  custody still holds, but without the policy envelope.

This lets v2 credsticks operate in "bare mode" for offline peer-to-peer
while still interacting with the full policy system when online (e.g.,
when depositing at a bank, swapping tokens, or going through a
connected relay).

### What v2 Loses Without Tickets

| v1 Ticket Feature | What Happens Without It |
|-------------------|------------------------|
| Group number (revocation bitfield) | Revocation must work at the TTC/NAC level instead of the ticket level. Coarser — revoking a TTC group affects all credentials, not just recent tickets |
| Expiration | No forced rotation of receiving credentials. Wallets could reuse one credential forever (linkability risk) |
| Max transaction size | No per-ticket policy. Must be enforced at the hardware/wallet level only |
| Rate limiting (N tickets/epoch) | No limit on receiving credential generation. A wallet could flood the system with addresses |
| Clerk signature | No third-party proof that the credential was legitimately issued |

### What v2 Gains

| Benefit | Impact |
|---------|--------|
| No clerk dependency | Works fully offline, no epoch-gated ticket refresh |
| Simpler protocol | No ticket request flow, no ticket signing keys |
| Pay anyone | Receiver generates credential on the spot, no prior exchange needed for live payments |
| Truly decentralized | No central authority needed for routine operations |
| Better privacy | No clerk tracking ticket issuance per NAC |

### Impact on Deferred Payment

With bare credentials (Option 2 or 3):

**Saved contacts still need a credential** (not a ticket, but a bare
randomized credential). The sender needs *something* to embed in the
Transfer at signing time. Whether that something is a `SignedTicket` or
a bare credential is a protocol distinction, but the UX requirement is
the same: the sender must have acquired the recipient's credential
before signing.

**Live payments at a relay get simpler**: The receiver generates a fresh
credential during the tap. No need to have pre-acquired a ticket from
a clerk. This makes the relay interaction fully self-contained — no
prior online setup required.

**The pickup code role is unchanged**: Still a UX verification aid
derived from the signed token data, regardless of whether the
destination is a ticket or bare credential.

### Recommendation

**Option 3 (hybrid)** for v2. The protocol should accept both signed
tickets and bare credentials. This gives us:

- **Offline credstick mesh**: Bare credentials, no clerk needed
- **Connected infrastructure**: Signed tickets for policy enforcement
  when the wallet has access to a clerk
- **Gradual migration**: v1 systems continue to work, v2 adds bare
  credential support

The key insight: the credential pre-commitment is the immutable
requirement (the chain of custody demands it). The *policy wrapping*
around that credential (group number, expiration, clerk signature) is
the part that can become optional in a decentralized v2.

For the deferred payment design, this means: saved contacts store either
a `SignedTicket` or a bare credential. The signing flow is identical
either way — the credstick doesn't care whether the credential has a
clerk signature around it.

## Deriving Group Revocation from the TTC

If v2 uses bare credentials (no clerk-assigned group numbers), can we
still get efficient group-based revocation? The v1 approach is elegant:
64-bit bitfield gossiped with every transaction, receivers check their
peer's group bit. Can we derive equivalent functionality from the TTC
credential itself?

### The Core Tension

Group numbers must be:
- **Checkable** by the receiver to enforce the revocation bitfield
- **Deterministic** for a given wallet (so revocation sticks)
- **Unlinkable** across transactions (so they don't deanonymize)

In v1 these properties come from the clerk assigning a group number to
each ticket and the receiver checking the clerk's signature. Without a
clerk, we need the group to emerge from the TTC itself.

### Approach A: Hash-Derived Group (Simple, Linkable)

```
group_idx = hash(TTC_public_commitment) mod 64
```

The wallet's TTC credential deterministically maps to a group. Any
verifier can compute this from the credential.

**Problem**: The credential is randomized per transaction (that's the
whole point of ECDAA), so `hash(randomized_credential) mod 64` gives
a different result each time. And if you use the *un-randomized*
credential, every transaction reveals the same group — linkable.

**Verdict**: Doesn't work as stated. The randomization that gives
privacy destroys the deterministic group derivation.

### Approach B: VRF-Based Group (Per-Epoch, Verifiable)

Use the TTC secret key with a Verifiable Random Function:

```
(group_proof, group_idx) = VRF(sk, epoch_number)
group_idx = VRF_output mod 64
```

Properties:
- Deterministic for a given (sk, epoch) — same wallet always gets
  the same group within an epoch
- Verifiable — the receiver checks the VRF proof
- Changes per epoch — unlinkable across epoch boundaries
- Doesn't reveal sk

**Problem**: The VRF proof itself is a pseudonym — if two transactions
in the same epoch produce the same VRF proof, they're linkable to the
same wallet. This is exactly the basename-linkability property, but
now applied to every transaction (not just double-spends).

**Mitigation**: The group assignment changes every epoch. Within a
single epoch, transactions from the same wallet are linkable via the
VRF proof, but across epochs they're not. Whether this is acceptable
depends on how many transactions a wallet does per epoch (~24h).

**Verdict**: Workable but trades some intra-epoch privacy for
revocation capability. A wallet doing 5 transactions per epoch has
all 5 linkable to each other (but not to the wallet's identity).

### Approach C: Committed Group with ZK Proof (Private, Complex)

The wallet commits to its group and proves in zero-knowledge that the
committed group is not in the revocation bitfield:

1. During TTC issuance, the registrar and wallet agree on
   `group_idx = hash(sk) mod 64`
2. In each transaction, the wallet provides:
   - A Pedersen commitment: `C = g^group_idx * h^r` (fresh r each time)
   - A ZK proof: "I know group_idx and r such that C = g^group_idx * h^r
     AND bit group_idx in the revocation_bitfield is 0"

The commitment is unlinkable (fresh r each time). The ZK proof
convinces the receiver the wallet isn't revoked without revealing which
group it's in.

**ZK proof construction**: For a 64-bit bitfield where K bits are set
(K groups revoked), the wallet proves membership in the set of 64-K
non-revoked groups. Using Sigma protocols (OR-composition), this is
O(64) in proof size — 64 parallel Schnorr proofs composed with an
OR gate. With BLS12-381, each Schnorr proof element is ~48 bytes.
Total proof overhead: ~3-6 KB.

**Problem**: 3-6 KB per transaction is heavy for a credstick NFC
exchange (~200 bytes budget at the APDU level) and for token history
growth. Also, generating 64 parallel Schnorr proofs on an nRF52840
is expensive (~64 scalar muls = ~120 seconds at current BLS12-381
performance). Not viable.

**Optimization**: Use a more efficient ZK system:

- **Bulletproofs range proof**: Prove `group_idx ∈ [0, 63]` and
  `revocation_bitfield[group_idx] == 0`. Proof size: ~700 bytes.
  But Bulletproofs are expensive to generate (~seconds on nRF52840).

- **Sparse bitfield**: If only 1-3 groups are revoked (common case),
  the proof simplifies to "my group ≠ 7 AND my group ≠ 23 AND my
  group ≠ 41." This is 3 inequality proofs, much cheaper.

**Verdict**: Theoretically ideal but likely too expensive for credstick
hardware. Worth revisiting if BLS12-381 performance improves (dedicated
accelerator, or a move to a faster curve).

### Approach D: Credential Attribute (BBS+ Evolution)

Instead of ECDAA, evolve to **BBS+ signatures** which natively support
multi-attribute credentials with selective disclosure:

```
Credential attributes:
  [0] sk           — wallet secret key
  [1] group_idx    — assigned during issuance
  [2] issued_epoch — when the credential was issued
  ...
```

With BBS+:
- The wallet can **selectively disclose** group_idx while hiding sk
- Or prove group_idx ∈ non-revoked set without disclosing it
- Credential randomization is built in (unlinkable presentations)
- Proof size: ~400-600 bytes for selective disclosure

This is the "right" long-term answer — BBS+ is essentially ECDAA's
successor for attribute-bearing anonymous credentials. The BLS12-381
curve already in Briolette v1 is the standard curve for BBS+.

**Migration path**: ECDAA credential (A, B, C, D) → BBS+ credential
with attributes. The signing and verification algebra is similar
(both pairing-based), but BBS+ adds the attribute disclosure framework.

**Problem**: BBS+ is more computationally expensive than basic ECDAA.
On an nRF52840, BBS+ proof generation with 2-3 attributes is estimated
at ~4-6 seconds (vs ~2 seconds for current ECDAA). Significant but
possibly acceptable.

**Verdict**: Best long-term approach. Natural evolution from ECDAA.
Should be evaluated for v2 or v3.

### Approach E: Issuer-Embedded Group Tag (Pragmatic)

The simplest approach that preserves v1's efficiency: during TTC
issuance, the registrar embeds the group number into the credential
nonce (`B = hash_to_g1(nonce)` where `nonce` encodes the group).
The credential is issued normally. The group is deterministic from the
credential but hidden by randomization.

At verification time, the wallet provides a **group disclosure proof**:
a proof that the randomized credential's underlying nonce encodes a
specific group number. This is a Schnorr proof relating the randomized
B point to the original nonce — one scalar multiplication, ~48 bytes.

```
Issuance:
  nonce = random || group_idx
  B = hash_to_g1(nonce)
  credential = (A, B, C, D) as usual

Verification (per transaction):
  Randomized credential: (R, S, T, W) = (A^l, B^l, C^l, D^l)
  Group proof: prove that S = B^l where B encodes group_idx
  → disclose group_idx + proof
```

**Problem**: This still requires the group to be disclosed (receiver
checks the bitfield). But disclosure of the group number (one of 64
values) reveals only ~6 bits of information. Combined with the
randomized credential (which changes every transaction), this leaks
group membership but nothing else.

**Privacy cost**: An adversary observing transactions can partition
wallets into 64 groups. Within a group, wallets remain anonymous.
Across transactions, the group is linkable (same wallet always in the
same group), which is ~6 bits of information per transaction.

**Verdict**: Pragmatic, efficient (~one extra scalar mul), but leaks
group membership. This is exactly what v1 tickets do, just moved from
the clerk to the registrar. Acceptable if the privacy cost of revealing
1-of-64 group membership is tolerable.

### Summary

| Approach | Privacy | Efficiency | Complexity | Hardware Feasible |
|----------|---------|-----------|------------|-------------------|
| A: Hash-derived | Linkable | Trivial | None | Yes (but broken privacy) |
| B: VRF | Intra-epoch linkable | 1 VRF proof (~200B) | Low | Yes |
| C: ZK committed | Full | 700B-6KB proof | High | Marginal |
| D: BBS+ attributes | Full | ~500B proof, ~5s | Medium | Stretch (v3?) |
| E: Issuer-embedded | 6-bit leak | ~48B proof | Low | Yes |

### Recommendation

For v2 credstick hardware:

**Short-term**: Approach E (issuer-embedded group tag). Minimal protocol
change from v1 — the registrar assigns the group during TTC issuance
instead of the clerk assigning it during ticketing. The 6-bit privacy
leak (1-of-64 group membership) is the same as v1 tickets. Efficient
enough for credstick hardware.

**Medium-term**: Approach B (VRF-based group). Eliminates the
registrar's group assignment (truly decentralized). Accepts intra-epoch
linkability as a tradeoff. Worth prototyping to measure real-world
privacy impact.

**Long-term**: Approach D (BBS+ credential attributes). The proper
solution — full privacy, selective disclosure, naturally supports
additional attributes beyond just group number. Requires BBS+ library
on the nRF52840 (or its successor MCU).

The key insight: **group-based revocation doesn't inherently require
tickets.** The group number can be embedded at credential issuance time
(registrar level) rather than ticketing time (clerk level). This
decouples revocation from the clerk entirely, letting bare credentials
carry their own group identity.

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
