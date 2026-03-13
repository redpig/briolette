# Solar Relay — Disconnected Credstick-to-Credstick Payments

## The Problem

The credstick-to-credstick flow requires a phone as NFC relay. But:

- Phone batteries die
- Rural areas may have no electricity for charging
- Some users may not own a smartphone
- Cash works everywhere — Briolette should too

We need a cheap, rugged, batteryless relay device that can facilitate
credstick payments without a phone, without internet, and without
external power.

## The Device: Solar Relay

A small, dedicated NFC reader/writer powered by solar + shake energy.
No screen of its own (or minimal LED indicators). Its only job: shuttle
APDUs between two credsticks, just like the phone PoS relay app.

```
┌──────────────────────────────────────┐
│  ┌──────────────────────────────┐   │
│  │     Solar Panel (top face)   │   │
│  │     ~40x30mm mono-Si cell    │   │
│  └──────────────────────────────┘   │
│                                      │
│  [1] [2] [3] [4]  ← 4 buttons      │
│                                      │
│  ● ● ●  ← 3 LEDs (power/send/recv) │
│                                      │
│  ══════════  NFC reader antenna     │
│  ║  TAP  ║  (marked zone)           │
│  ══════════                          │
│                                      │
│  ┌────────┐                          │
│  │ USB-C  │  ← Charge + firmware    │
│  └────────┘                          │
└──────────────────────────────────────┘
       ~70mm x 45mm x 8mm
```

## Key Difference from Credstick

The credstick is an NFC **tag** (passive, powered by the reader's field).
The relay is an NFC **reader** (active, generates the RF field). This is
the critical distinction — the relay powers the credsticks, not the other
way around.

## Hardware Spec

### Core Components

| Component | Part | Notes |
|-----------|------|-------|
| MCU | nRF52840-QIAA | Same as credstick — shared firmware tooling |
| NFC frontend | PN7150 or PN532 | NFC reader/writer IC (not tag mode) |
| Solar cell | 40x30mm mono-Si | ~80-150mW in direct sun, ~5-15mW shade |
| Supercap | 2x 10F 3V (20F total) | Larger than credstick for reader power |
| Piezo harvester | Same as credstick | Supplemental, for pocket carry |
| Buttons | 4x 3mm tactile switches | PIN entry (see button spec) |
| LEDs | 3x 0402 (R/G/B) | Power status, send, receive |
| USB-C | GCT USB4135-GF-A | Charge + DFU |
| Charge controller | BQ25504 | Ultra-low-power solar MPPT harvester |

### Why PN7150/PN532 Instead of nRF52840's NFC?

The nRF52840's built-in NFC peripheral is **tag-only** (Type 2/4 Tag
emulation). It cannot generate an NFC field or act as a reader. To read
credsticks, we need a dedicated NFC reader IC:

- **PN7150**: NXP's NFC controller with reader/writer + P2P + card
  emulation. I2C interface. Low power (~20mA active). Best option.
- **PN532**: Older but widely available. SPI/I2C/UART. ~100mA active
  (higher power). Good for prototyping.

The nRF52840 drives the PN7150 over I2C and handles the Briolette
protocol logic. The PN7150 handles the RF layer.

### Power Budget

**NFC reader is the expensive part:**

| Operation | Current | Duration | Energy |
|-----------|---------|----------|--------|
| PN7150 field on (reader) | 20-50mA | 3-5s per tap | 60-250mAs |
| nRF52840 active | 5mA | 5s per tap | 25mAs |
| LED indicator | 5mA | 1s | 5mAs |
| Total per tap | | | ~90-280mAs |
| Total per transaction (4 taps) | | | ~360-1120mAs |

At 3V, a single 4-tap transaction costs **~1-3.4J**.

**Supercap budget (20F at 3V):**
- Usable energy: 0.5 x 20 x (9-4) = **50J**
- Transactions per charge: **~15-50**
- USB-C recharge: ~60 seconds

**Solar budget:**
- Direct sun: 80-150mW → 288-540J per hour → **80-160 transactions/hour**
- Overcast: 10-30mW → 36-108J per hour → **10-30 transactions/hour**
- Indoor light: 1-5mW → 3.6-18J per hour → **1-5 transactions/hour**
- Shade outdoors: 5-15mW → 18-54J per hour → **5-15 transactions/hour**

**In direct sun, the relay is effectively unlimited.**

### BQ25504 Solar MPPT

The BQ25504 is a nano-power boost charger with MPPT (Maximum Power Point
Tracking). It efficiently harvests from solar cells as low as 100mV
input and charges the supercap. Key specs:

- Input voltage: 100mV–5.5V (works with any small solar cell)
- Quiescent current: 330nA (virtually zero idle drain)
- MPPT efficiency: 80-90%
- Cold start: 330mV minimum (small light is enough to boot)

This means the relay can cold-start from a dead supercap just by
sitting in sunlight for a few minutes.

## Operating Modes

### 1. Solar Idle (Default)

Solar cell charges supercap via BQ25504. No NFC activity. LEDs off.
Power LED blinks once every 10s to show charge state:
- Green: >50% charge
- Yellow: 20-50%
- Red: <20%

### 2. Transaction Mode (Button Press)

User presses button 1 to start a relay session:

```
Press [1] → Green LED on → "Ready, tap receiver credstick"
Tap receiver → Blue LED blinks → reading ticket...
              → Blue LED solid → got ticket, tap sender
Tap sender   → Relay sends ticket + amount to credstick
              → Credstick e-ink shows: "Pay 5 tokens? ◄No Yes►"
              → User presses Right button on CREDSTICK to confirm
              → Credstick signs tokens, returns to relay
              → Blue LED solid → got tokens, tap receiver again
Tap receiver → Blue LED blinks → delivering...
              → Green LED flash 3x → done!
              → (or Red LED flash 3x → failed)
```

**Critical UX: the sender's credstick displays the proposed amount
and requires explicit button confirmation before signing.** The relay
cannot forge the amount because the credstick independently shows
what it's being asked to sign. This is the same user-confirmation
flow described in `button-pin-auth.md` — the credstick never
auto-signs without the user seeing and approving the amount.

The 4-tap sequence is guided by LED color only on the relay — no
screen needed on the relay because the credstick's own e-ink display
handles the critical user-facing confirmation.

### 3. Fixed-Amount Mode (Merchant Use)

For a merchant who always charges the same amount (e.g., a bus fare):

1. Press and hold [1] during power-on → enters config mode
2. Use buttons [1-4] to set amount (binary or digit-by-digit)
3. Amount is stored in flash
4. Every subsequent button press starts a transaction for that amount

This eliminates the need for a phone entirely for simple, fixed-price
transactions (transit, vending, market stalls).

### 4. Variable-Amount Mode

For variable amounts without a screen, the relay can accept amount
input via button presses:

- Buttons [1-4] map to digits or increments
- Quick-press sequences: [2][3] = 23 tokens
- Long-press [4] = confirm amount
- LED blinks N times to confirm the entered amount

This is clunky but functional. For better UX, pair with a phone running
the PoS app — but the relay works standalone for emergencies.

## PIN Authorization on the Relay

The 4 buttons also serve as PIN entry (see `button-pin-auth.md`).
Before the relay starts a transaction, the operator can optionally
require a PIN:

1. Power on → enter 4-digit PIN via buttons
2. Green LED = authorized, relay is active for N minutes or transactions
3. Wrong PIN → red LED, lockout after 3 failures

This prevents unauthorized use if the relay is stolen.

## BOM (Estimated)

| Component | Qty 10 | Qty 1000 |
|-----------|--------|----------|
| nRF52840 | $3.80 | $3.20 |
| PN7150 | $3.50 | $2.50 |
| Solar cell (40x30mm) | $2.00 | $0.80 |
| BQ25504 | $3.50 | $2.80 |
| Supercaps (2x 10F) | $6.00 | $4.00 |
| Piezo harvester | $5.00 | $2.00 |
| 4x tactile buttons | $0.60 | $0.32 |
| 3x LEDs | $0.30 | $0.15 |
| USB-C connector | $0.60 | $0.35 |
| PCB + passives | $6.00 | $2.00 |
| Enclosure | $3.00 | $0.50 |
| **Total** | **~$34** | **~$19** |

At scale, this is a **sub-$20 device** — comparable to a basic
calculator or kitchen timer. Cheap enough to distribute to rural
merchants or community centers.

## Use Cases

### Rural Market

A village market has no reliable electricity. Each vendor has a solar
relay clipped to their stall. Customers carry credsticks. Payments
happen all day powered by sunlight. At the end of the day, vendors
tap their credstick to a phone (when one is available) to sweep
accumulated tokens to a validated wallet.

### Transit / Bus

A bus driver has a relay mounted on the dashboard. Fixed fare mode.
Passenger taps credstick → fare deducted → rider boards. Solar charges
the relay during the day. USB-C from the bus's 12V system as backup.

### Emergency / Disaster

Grid is down, cell towers are down. People still need to transact.
Solar relays + credsticks provide a fully disconnected, self-powered
payment system. No internet, no electricity, no phone required.

### Peer-to-Peer in the Field

Two hikers want to split a cost. One pulls out a solar relay from their
pack, both tap credsticks. Works on a mountaintop with zero infrastructure.

## Credstick Storage Considerations

For long periods of disconnected operation, credsticks accumulate
unvalidated tokens. The credstick flash should accommodate:

| Scenario | Tokens | Storage (~200 bytes/token) |
|----------|--------|---------------------------|
| Light daily use (5 txns/day, 7 days) | 35 | ~7 KB |
| Heavy daily use (20 txns/day, 7 days) | 140 | ~28 KB |
| Extended disconnected (30 days) | 600 | ~120 KB |
| Extreme (3 months disconnected) | 1800 | ~360 KB |

The nRF52840 has 1 MB flash. After firmware (~256 KB) and bloom
filter (~64 KB), there's **~680 KB** available for token storage —
enough for ~3400 tokens, covering months of disconnected operation.

For extra safety, an external QSPI flash (e.g., 4 MB MX25R4035F, $0.50)
could extend storage to tens of thousands of tokens.

## Relationship to Phone PoS App

The solar relay and the phone PoS app are functionally identical — both
are dumb NFC relays that shuttle APDUs. The difference:

| | Phone PoS App | Solar Relay |
|---|---------------|-------------|
| Screen | Full touchscreen | 3 LEDs |
| Input | Touch keyboard | 4 buttons |
| Power | Phone battery | Solar + supercap |
| Cost | $0 (app on existing phone) | ~$19 at scale |
| Internet | Optional (for logging) | Never needed |
| UX | Rich, guided | Minimal, LED-guided |

They're complementary. The phone app is better UX. The solar relay is
the fallback when phones aren't available.
