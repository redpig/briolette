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
5. ATECC608B provides manufacturer attestation (P-256 ECDSA)
6. E-ink display updates with new balance / transaction result
7. Device returns to sleep (zero quiescent current)

**Power budget per tap:**
- nRF52840 active @ 64 MHz: ~5mA for ~3s = 15mAs
- BLS12-381 scalar mul (peak): ~15mA for ~2s = 30mAs
- E-ink full refresh: ~40mA for ~0.8s = 32mAs
- ATECC608B signing: ~15mA for ~50ms = 0.75mAs
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
- ATECC608B sleep: ~30nA
- **Total idle: <1µA**

With a 100mF supercap charged to 3.3V:
- Energy stored: 0.5 × 0.1F × 3.3² = 0.54J
- At 1µA idle: lasts ~150 hours (theoretical, self-discharge dominates)
- At 78mAs per transaction: supports ~7 transactions from full charge

With an optional LiPo (e.g., 50mAh coin cell): hundreds of transactions.

## Security Architecture

```
┌──────────────────────────────────────────────────┐
│                  Trust Boundaries                 │
│                                                   │
│  ┌─────────────┐         ┌──────────────────┐    │
│  │ ATECC608B   │         │    nRF52840      │    │
│  │             │         │                  │    │
│  │ ● Mfr key  │◄─I2C───▶│ ● ECDAA sk_half │    │
│  │   (P-256)  │         │ ● BLS12-381 ops │    │
│  │ ● Mfr cert │         │ ● Bloom filter  │    │
│  │ ● Monotonic│         │ ● APDU handler  │    │
│  │   counter  │         │ ● Display driver│    │
│  │             │         │ ● NFC stack     │    │
│  │ TAMPER-    │         │                  │    │
│  │ RESISTANT  │         │ APPROTECT fuse   │    │
│  └─────────────┘         │ (debug disabled) │    │
│                          └──────────────────┘    │
│                                                   │
│  Phone ◄──NFC──▶ nRF52840 ◄──I2C──▶ ATECC608B   │
└──────────────────────────────────────────────────┘
```

### Key Storage

| Key | Location | Protection |
|-----|----------|------------|
| ECDAA secret key (half) | nRF52840 flash | APPROTECT fuse (blocks SWD) |
| Manufacturer P-256 key | ATECC608B slot | Hardware tamper-resistant SE |
| Manufacturer certificate | ATECC608B slot | Read-only after lock |
| Bloom filter state | nRF52840 flash | Application-level integrity |

### Attestation Flow

1. Phone challenges credstick during registration
2. ATECC608B signs challenge with manufacturer P-256 key
3. Certificate chain: Card key → Manufacturer CA → Registrar trust store
4. Combined with split-key ECDAA proof → HIGH+ security tier

### Physical Security

- **ATECC608B**: Certified secure element, resistant to side-channel and
  fault injection attacks. The manufacturer key cannot be extracted.
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

## Firmware Stack

See [../firmware/](../firmware/) for implementation details.

Target: Zephyr RTOS or Embassy-rs (Rust async on bare metal).

```
┌─────────────────────────────────┐
│        Briolette Protocol       │  APDU handler, state machine
├─────────────────────────────────┤
│  BLS12-381  │  ATECC608B  │ UI │  Crypto, SE driver, display
├─────────────┼─────────────┼────┤
│    NFC-A    │    I2C      │SPI │  Hardware peripherals
├─────────────┴─────────────┴────┤
│       Zephyr / Embassy-rs      │  RTOS / async runtime
├─────────────────────────────────┤
│          nRF52840 HAL           │  Hardware abstraction
└─────────────────────────────────┘
```
