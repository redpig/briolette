# Credstick Design Overview

## Concept

A keychain-sized device that holds Briolette anonymous digital cash.
Tap it against a phone to send or receive tokens. An e-ink screen shows
the current balance and transaction status without needing a battery
to maintain the display.

## Operating Modes

### 1. NFC Tap (Primary)

The device acts as an NFC-A tag. When a phone taps the credstick:

1. NFC field powers the nRF52840 via energy harvesting
2. Supercap buffers the power for the full transaction duration (~3-5s)
3. Phone sends Briolette APDUs over NFC (ISO-DEP / Type 4 Tag)
4. nRF52840 performs ECDAA split-key operations (BLS12-381 in software)
5. SIM card provides manufacturer attestation (P-256 ECDSA via ISO 7816)
6. E-ink display updates with new balance / transaction result
7. Device returns to sleep (zero quiescent current)

**Power budget per tap:**
- nRF52840 active @ 64 MHz: ~5mA for ~3s = 15mAs
- BLS12-381 scalar mul (peak): ~15mA for ~2s = 30mAs
- E-ink full refresh: ~40mA for ~0.8s = 32mAs
- SIM card signing: ~10mA for ~100ms = 1.0mAs
- **Total: ~78mAs per transaction**

An NFC field delivers ~5-10mA continuously. Over a 5-second tap, that's
25-50mAs harvested — not quite enough alone, so the supercap must be
pre-charged (via USB-C or a previous longer tap).

### 2. USB-C Connected

Plugged into a computer or charger:
- Charges the supercap / battery
- Firmware updates via USB DFU (nRF52840 native USB)
- Serial console for debugging
- Could act as a USB security token (future: FIDO2 + Briolette)

### 3. Idle

- E-ink retains last display contents: 0µA
- nRF52840 in System OFF: ~0.3µA (RTC wake or NFC field detect wake)
- SIM card clock-stop: ~5µA
- **Total idle: <1µA**

### Power: Supercap-Primary (No Battery)

LiPo batteries degrade over time (~500 cycles, capacity fade, swelling
risk). Supercapacitors have effectively infinite cycle life (>1M cycles),
no degradation, wider temperature range, and no fire risk. The tradeoff
is lower energy density — but with the right supercap sizing, it works.

**Supercap sizing (2× Kyocera AVX SCC 5F 3V in parallel = 10F):**
- Stored energy: 0.5 × 10F × 3.0² = 45J
- Usable energy (3.0V → 2.0V cutoff): 0.5 × 10 × (3.0² - 2.0²) = 25J
- At ~260mJ per transaction: **~96 transactions** per full charge
- USB-C charge time: seconds (supercaps charge at high current)

**Self-discharge** is the main concern. A good supercap loses ~5-10%
per day at room temperature. After a week idle: ~50% charge remaining
(~48 transactions). After a month: marginal. This is where the piezo
helps — ambient motion on a keychain keeps topping it off.

### Piezo Energy Harvesting

A piezoelectric cantilever mounted inside the enclosure converts
keychain motion (walking, pocket jostling, deliberate shaking) into
electrical energy to trickle-charge the supercap.

**Energy budget from motion:**
- Typical piezo harvester output: 50-200µW average from walking
- Deliberate shake: 1-5mW peak
- Per day of normal keychain carry: ~5-15mJ (passive)
- Per 10-second shake: ~10-50mJ
- Transaction cost: ~260mJ each

So passive carry alone won't fully charge a transaction, but it
**fights self-discharge** and keeps the supercap topped off. A
10-second deliberate shake before a transaction adds meaningful
charge. Combined with periodic USB-C top-ups, the supercap stays
usable indefinitely.

**The shake-to-pay UX:**
1. Credstick sits on keychain, piezo harvests ambient motion all day
2. Supercap self-discharge roughly offset by passive harvesting
3. Before a transaction: user shakes the credstick for a few seconds
4. Tap to pay — supercap has enough for the transaction + display update
5. If fully depleted: 5-second USB-C charge restores full capacity

### Alternative: Thin-Film Supercap (PrizmaCap)

For a thinner form factor, the Kyocera AVX PrizmaCap (SCP series)
offers 15F in a 48×45×0.8mm prismatic package — but at only 2.1V,
needs a boost converter to reach 3.0V for the nRF52840. Adds
complexity and conversion losses, but enables a credit-card thickness.

## Security Architecture

```
┌──────────────────────────────────────────────────┐
│                  Trust Boundaries                 │
│                                                   │
│  ┌─────────────┐         ┌──────────────────┐    │
│  │ SIM Card    │         │    nRF52840      │    │
│  │ (nano-SIM)  │         │                  │    │
│  │ ● Mfr key  │◄─7816──▶│ ● ECDAA sk_half │    │
│  │   (P-256)  │         │ ● BLS12-381 ops │    │
│  │ ● Mfr cert │         │ ● Bloom filter  │    │
│  │ ● Monotonic│         │ ● APDU handler  │    │
│  │   counter  │         │ ● Display driver│    │
│  │             │         │ ● NFC stack     │    │
│  │ TAMPER-    │         │                  │    │
│  │ RESISTANT  │         │ APPROTECT fuse   │    │
│  │ REMOVABLE  │         │ (debug disabled) │    │
│  └─────────────┘         └──────────────────┘    │
│                                                   │
│  Phone ◄──NFC──▶ nRF52840 ◄─ISO7816─▶ SIM Card  │
└──────────────────────────────────────────────────┘
```

The SIM card sits in a low-profile push-push nano-SIM connector
(Molex 78800-0001, 1.25mm height). Cards are user-replaceable:
push to eject, swap to transfer identity to a new credstick.

### Key Storage

| Key | Location | Protection |
|-----|----------|------------|
| ECDAA secret key (half) | nRF52840 flash | APPROTECT fuse (blocks SWD) |
| Manufacturer P-256 key | SIM card | Hardware tamper-resistant SE (CC EAL4+) |
| Manufacturer certificate | SIM card | Read-only applet storage |
| Bloom filter state | nRF52840 flash | Application-level integrity |

### Attestation Flow

1. Phone challenges credstick during registration
2. SIM card signs challenge with manufacturer P-256 key (INTERNAL AUTHENTICATE)
3. Certificate chain: SIM key → Manufacturer CA → Registrar trust store
4. Combined with split-key ECDAA proof → HIGH+ security tier

### Physical Security

- **SIM card**: CC EAL4+ certified secure element, resistant to side-channel
  and fault injection attacks. The manufacturer key cannot be extracted.
  User-replaceable via low-profile push-push connector — swap SIM to
  transfer identity to a new credstick, or replace a compromised card.
- **nRF52840**: Not a secure element. The APPROTECT fuse disables SWD
  debug access, but glitching attacks are feasible for a motivated
  attacker. The ECDAA key half stored in flash is extractable with
  physical access and equipment.
- **Mitigation**: Even if the nRF52840 key half is extracted, it's
  useless without the phone's key half (split-key protocol). An attacker
  needs both the credstick AND the phone to forge a signature.

## Display UI Concepts

### Home Screen (after transaction)
```
┌────────────────┐
│   ◉ 42 tokens  │
│                 │
│ ✓ Sent 3       │
│   12:34 today   │
└────────────────┘
```

### Transaction Confirmation (Sender)
```
┌────────────────┐
│  Pay 5 tokens?  │
│  "Coffee"       │
│                 │
│  ◄ No    Yes ►  │
└────────────────┘
```

### Waiting for Tap
```
┌────────────────┐
│                 │
│   ⟐ Tap to     │
│     pay        │
│                 │
└────────────────┘
```

### Low Power Warning
```
┌────────────────┐
│   ◉ 42 tokens  │
│                 │
│ ⚡ Charge via   │
│    USB-C       │
└────────────────┘
```

## Flash Storage Budget

The nRF52840 has 1 MB flash. Allocation:

| Region | Size | Purpose |
|--------|------|---------|
| Firmware + bootloader | ~256 KB | Application code |
| Bloom filter (revocation) | ~64 KB | Double-spend / epoch cache |
| Token storage (OWNED) | ~400 KB | ~2000 owned tokens |
| Token storage (RECEIVED) | ~200 KB | ~1000 unvalidated received tokens |
| **Reserved (v2)** | ~80 KB | ~400 PENDING_SEND tokens for deferred payment |

The v2 reserve covers the deferred payment extension (see
`deferred-payment.md`) where tokens are signed locally and stored for
later relay drop-off. The flash layout should use a simple log-structured
allocation that can grow the PENDING_SEND region into the RECEIVED region
if needed.

For heavy-use scenarios, an external QSPI flash (4 MB MX25R4035F, ~$0.50)
extends total token storage to tens of thousands of tokens across all states.

### Saved Contacts (v2 Reserve)

Deferred payments require saved recipient tickets. Each `SignedTicket` is
~200-300 bytes. Reserving ~10 KB of flash supports ~30-40 saved contacts —
sufficient for a household's regular merchants and family members.

## APDU Command Space

Current APDU assignments use INS bytes 0x10-0x51. The range 0x40-0x4F
is reserved for deferred payment extensions (DROP_OFF, COLLECT, etc.)
to avoid conflicts with future protocol versions.

## Firmware Stack

See [../firmware/](../firmware/) for implementation details.

Target: Zephyr RTOS or Embassy-rs (Rust async on bare metal).

```
┌─────────────────────────────────┐
│        Briolette Protocol       │  APDU handler, state machine
├─────────────────────────────────┤
│  BLS12-381  │  SIM Card   │ UI │  Crypto, SE driver, display
├─────────────┼─────────────┼────┤
│    NFC-A    │  ISO 7816   │SPI │  Hardware peripherals
├─────────────┴─────────────┴────┤
│       Zephyr / Embassy-rs      │  RTOS / async runtime
├─────────────────────────────────┤
│          nRF52840 HAL           │  Hardware abstraction
└─────────────────────────────────┘
```
