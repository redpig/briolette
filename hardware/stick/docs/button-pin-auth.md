# Button-Based PIN Authorization

## Motivation

A credstick without authorization is like a cash wallet with no clasp —
anyone who picks it up can spend from it. Physical possession alone
shouldn't be sufficient for high-value transactions. We need a PIN or
gesture-based authorization mechanism using minimal hardware.

## Hardware Options

### Option A: 1 Button (Morse-style / Timing-based)

A single button with short-press and long-press differentiation:

- Short press = "dit" (dot)
- Long press = "dah" (dash)
- Pause = separator

**PIN example**: `·−−· ·−· ·· −−` (4 symbols, timing-based)

**Pros**: Minimum BOM cost ($0.08), smallest footprint, fewest GPIOs
**Cons**: Slowest input, hardest to learn, error-prone under stress

### Option B: 2 Buttons (Directional / Binary)

Left and right (or up and down). PIN is a sequence of directions:

- Left = L, Right = R
- **PIN example**: `L R R L R L` (6-symbol binary sequence)

A 6-symbol L/R sequence has 2^6 = 64 combinations. An 8-symbol
sequence has 256. A 10-symbol sequence has 1024 — comparable to a
4-digit numeric PIN (10,000) in practical security given the lockout.

**Can also encode timing**: short-L, long-L, short-R, long-R = 4
symbols per button. An 8-position sequence with 4 symbols = 4^8 =
65,536 combinations — exceeding a 4-digit numeric PIN.

**Pros**: Intuitive (left/right), fast input, small footprint
**Cons**: Sequences longer than numeric PINs for equivalent entropy

### Option C: 4 Buttons (Directional Pad)

Up, Down, Left, Right — a directional pad. PIN is a sequence of
directions, like a game cheat code:

- **PIN example**: `U D L R R D` (6-direction sequence)

A 6-symbol sequence with 4 directions = 4^6 = 4,096 combinations.
An 8-symbol sequence = 65,536. Matches or exceeds a 4-digit numeric
PIN in entropy.

**With timing**: short/long press per direction = 8 symbols.
6-position sequence = 8^6 = 262,144 combinations.

**Pros**: Most intuitive, fastest input, game-pad muscle memory
**Cons**: 4 GPIOs, more PCB space, slightly higher BOM

### Option D: 4 Buttons (Numeric Clusters)

Buttons labeled 1-4. PIN is a numeric sequence from a reduced alphabet:

- **PIN example**: `3 1 4 2 1` (5-digit PIN from digits 1-4)

A 4-length PIN = 4^4 = 256 combinations (weak).
A 6-length PIN = 4^6 = 4,096 combinations (adequate with lockout).
An 8-length PIN = 4^8 = 65,536 combinations (strong).

**Pros**: Familiar numeric entry, clear labeling
**Cons**: Less intuitive than directional, same GPIO cost as Option C

## Recommendation: 2 Buttons + Timing (Option B Enhanced)

Two buttons (Left/Right) with timing gives the best tradeoff:

| Factor | 1 Button | 2 Buttons | 4 Buttons |
|--------|----------|-----------|-----------|
| BOM cost | $0.08 | $0.16 | $0.32 |
| GPIOs used | 1 | 2 | 4 |
| PCB area | 3x3mm | 6x3mm | 12x3mm |
| Input speed | Slow | Medium | Fast |
| Learnability | Hard | Easy | Easiest |
| Entropy (8 symbols) | 256 | 65,536 | 65,536+ |

Two buttons are enough for secure PIN entry while keeping the
credstick small. They also double as UI navigation for the e-ink
display (scroll balance, confirm transactions).

However, if the credstick targets the credit-card form factor (more
PCB space), 4 buttons as a directional pad are preferable for speed
and intuitiveness.

**For the keychain form factor**: 2 buttons (L/R with timing)
**For the card form factor**: 4 buttons (directional pad)

## PIN Entry Protocol

### Setup (First Use or Reset via USB)

1. User connects credstick via USB-C
2. Firmware enters PIN setup mode
3. User enters desired PIN sequence via buttons
4. User confirms by re-entering the PIN
5. PIN hash (Argon2id) stored in nRF52840 flash
6. ATECC608B monotonic counter initialized to 0 (attempt counter)

### Authentication Flow

```
Credstick powered on (NFC tap or USB)
        │
        ▼
┌─────────────────┐
│ PIN Required?    │──── No (below threshold) ───▶ Allow transaction
│ (check policy)   │
└────────┬────────┘
         │ Yes
         ▼
┌─────────────────┐
│ E-ink shows:     │
│ "Enter PIN"      │
│ ◄─ ─►           │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ User enters      │
│ L/R sequence     │
│ via buttons      │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌──────────────────┐
│ Verify PIN hash  │─No─▶│ Increment fail   │
│ (Argon2id)       │     │ counter (ATECC)  │
└────────┬────────┘     │ Show "Wrong PIN" │
         │ Yes           │ Lockout if ≥ N   │
         ▼               └──────────────────┘
┌─────────────────┐
│ Transaction      │
│ authorized       │
│ (for T seconds)  │
└─────────────────┘
```

### Attempt Limiting (Anti-Brute-Force)

The ATECC608B's hardware monotonic counter tracks failed attempts:

| Failed Attempts | Lockout |
|-----------------|---------|
| 1-3 | None (immediate retry) |
| 4-6 | 30-second delay between attempts |
| 7-9 | 5-minute delay |
| 10+ | Permanent lockout (USB reset required) |

The monotonic counter cannot be rolled back — even with physical
access to the nRF52840, the ATECC608B's counter is tamper-resistant.

### Authorization Window

After a successful PIN entry, the credstick stays authorized for a
configurable window:

| Policy | Window | Use Case |
|--------|--------|----------|
| Per-transaction | Single tap | High security |
| Time-based | 5 minutes | Convenience (shopping trip) |
| Session-based | Until power loss | Low-threat (personal use) |
| Amount threshold | PIN required above N tokens | Hybrid |

Default: **amount threshold** — transactions below 10 tokens (user
configurable) proceed without PIN. Above the threshold, PIN required.
This mirrors contactless card behavior (tap-to-pay under $100, PIN
above).

## E-Ink Display Integration

The e-ink display shows PIN entry state:

### PIN Entry
```
┌────────────────┐
│  Enter PIN:     │
│                 │
│  ◄─── ───►     │
│  * * * _       │
└────────────────┘
```

Each successful symbol shows as `*`. Current position shown as `_`.
Long-press both buttons simultaneously = submit PIN.

### PIN Accepted
```
┌────────────────┐
│  ✓ Authorized   │
│                 │
│  Tap to pay     │
│  ◉ 42 tokens    │
└────────────────┘
```

### PIN Rejected
```
┌────────────────┐
│  ✗ Wrong PIN    │
│                 │
│  2 attempts     │
│  remaining      │
└────────────────┘
```

## Button Dual-Use

The same buttons used for PIN entry also serve as general UI controls:

| Context | Left Button | Right Button |
|---------|-------------|-------------|
| PIN entry | "Left" symbol | "Right" symbol |
| Balance view | Previous token type | Next token type |
| Transaction | Decline | Accept |
| Menu (USB) | Navigate up | Navigate down |
| Confirm | (hold both) Submit | (hold both) Submit |

## Credstick Display During Receiver Proposal

During a `Transact` phase (sender's credstick is being asked to pay):

```
┌────────────────┐
│  Pay 5 tokens?  │
│  "Coffee"       │
│                 │
│  ◄ No    Yes ►  │
└────────────────┘
```

The sender presses Right to authorize or Left to decline. This
gives the user explicit consent for each transaction — the credstick
never auto-sends without button confirmation (configurable: can
disable for small amounts below threshold).

### NFC Timing During Confirmation

When the credstick receives a TRANSFER APDU, it must hold the NFC
session open while waiting for the user to press a button. The flow:

1. Relay/phone sends TRANSFER APDU with ticket + amount
2. Credstick updates e-ink display (~800ms)
3. Credstick enters "waiting for button" state
4. NFC session stays alive via keep-alive responses (WTX — Waiting
   Time Extension frames, per ISO 14443-4)
5. User has up to 30 seconds to press Accept or Decline
6. On Accept: credstick signs tokens, returns them in the APDU response
7. On Decline: credstick returns an error status word (SW 6985 =
   "Conditions of use not satisfied")
8. On timeout: credstick returns SW 6401 ("Command timeout")

The WTX mechanism is standard in ISO-DEP and allows the tag to request
more time from the reader. Both Android's IsoDep and the PN7150 reader
IC support WTX extensions, so the session remains open during the
user's decision time.

### Amount Display as Anti-Tampering

The credstick's e-ink display is the user's **only trusted display**
in this flow. A malicious relay or compromised phone could claim
"5 tokens" on its screen while sending a TRANSFER APDU for 500 tokens.
The credstick shows the actual amount from the APDU payload on its own
display — the user sees and confirms the real amount. This is analogous
to a chip card terminal showing the amount on the card reader's screen,
not the merchant's monitor.

## Security Considerations

### PIN Storage

- PIN is hashed with Argon2id (memory-hard, resistant to brute force)
- Salt derived from ATECC608B's unique serial number
- Hash stored in nRF52840 flash (protected by APPROTECT fuse)
- Raw PIN never stored anywhere

### Side-Channel Resistance

- Constant-time PIN comparison (no early-exit on mismatch)
- No audible/visible difference between correct/incorrect until
  full comparison completes
- Button debounce timing is constant regardless of PIN state

### Physical Observation (Shoulder Surfing)

With only 2 buttons and no screen showing the actual symbols (just
`*` dots), an observer can see left/right presses but the timing
component (short vs long) is harder to observe. For high-security
use, users can cover the buttons with their hand while entering.

### Lost Credstick

A lost credstick is protected by:
1. PIN lockout (10 attempts max)
2. No way to reset PIN without USB + original owner's recovery key
3. Tokens can be remotely invalidated by the owner via the phone
   wallet app (revoke the credstick's delegated ticket)
4. Split-key: tokens signed by the credstick alone are incomplete
   without the phone's key half (for credsticks using split-key mode)
