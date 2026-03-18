# Power Analysis

## Operating States

| State | Current | Duration | Energy |
|-------|---------|----------|--------|
| System OFF (idle) | 0.3µA | indefinite | — |
| NFC field detect wake | 1mA | 5ms | 5µAs |
| BLS12-381 scalar mul | 15mA | 2s | 30mAs |
| BLS12-381 hash-to-curve | 10mA | 0.5s | 5mAs |
| ATECC608B P-256 sign | 15mA | 50ms | 0.75mAs |
| NFC transceive (active) | 5mA | 3s | 15mAs |
| E-ink full refresh | 40mA | 0.8s | 32mAs |
| E-ink partial refresh | 20mA | 0.3s | 6mAs |
| USB connected (charging) | 500mA in | — | charging |

## Transaction Power Budget

A single tap-to-pay transaction:

| Phase | Duration | Energy |
|-------|----------|--------|
| Wake + init | 10ms | 0.05mAs |
| NFC handshake | 200ms | 1mAs |
| Receive APDU | 100ms | 0.5mAs |
| BLS12-381 sign | 2.5s | 37.5mAs |
| ATECC608B attest | 50ms | 0.75mAs |
| NFC response | 100ms | 0.5mAs |
| E-ink update | 800ms | 32mAs |
| Shutdown | 5ms | 0.025mAs |
| **Total** | **~3.8s** | **~72mAs** |

## Energy Sources

### Primary: Supercapacitors (2× 5F 3V in parallel = 10F)

No battery — supercaps are the sole energy storage.

**Why supercaps over LiPo:**
- Cycle life: >1,000,000 cycles vs ~500 for LiPo
- No capacity degradation over time
- No swelling, no fire risk, no thermal runaway
- -40°C to +70°C operating range (LiPo: 0°C to 45°C)
- Charges in seconds, not hours
- Device lifetime: effectively forever (limited by other components)

**Energy math:**
- Stored energy: 0.5 × 10F × 3.0² = **45J**
- Usable energy (3.0V → 2.0V cutoff): 0.5 × 10 × (9 - 4) = **25J**
- Per transaction: 72mAs × 3.0V avg = **~216mJ**
- Transactions per full charge: **~115**
- USB-C recharge (1A): ~30 seconds to full

**Self-discharge (the main tradeoff):**
- ~5-10% per day at room temperature
- After 1 day: ~90-95% charge (~100+ transactions)
- After 1 week: ~50% charge (~55 transactions)
- After 1 month: ~5% charge (~5 transactions)
- After 2 months idle: effectively empty

This is acceptable for a keychain device that gets regular USB-C
top-ups or pocket motion (see piezo below).

### Secondary: Piezo Energy Harvesting

A piezoelectric cantilever converts mechanical motion to electricity,
offsetting supercap self-discharge and extending idle shelf life.

**Harvesting scenarios:**
| Scenario | Power | Duration | Energy |
|----------|-------|----------|--------|
| Passive keychain carry (walking) | 50-200µW | 8h/day | 1.4-5.8J/day |
| Pocket jostling (seated) | 10-50µW | 8h/day | 0.3-1.4J/day |
| Deliberate shake | 1-5mW | 10s | 10-50mJ |
| Vigorous shake | 5-20mW | 10s | 50-200mJ |

**Self-discharge vs harvesting:**
- Supercap self-discharge at 50% charge: ~2.5J/day
- Passive carry harvest: ~1.4-5.8J/day
- **Verdict**: Active keychain carry roughly offsets self-discharge!
  A device on a daily-use keyring stays charged indefinitely without
  USB-C. A device sitting on a desk drains in ~2 months.

**Shake-to-pay (emergency):**
- 10s vigorous shake: 50-200mJ
- One transaction: ~216mJ
- Need ~10-20s of vigorous shaking for one transaction
- Not great UX, but works as emergency fallback

### Tertiary: NFC Field Harvesting

- Typical NFC field: 5-10mA at 3.0V = 15-30mW
- Over a 4-second tap: 60-120mJ harvested
- Covers ~28-55% of a transaction's energy need
- Supplements the supercap during every transaction

### Quaternary: USB-C

- Fastest charge: supercaps reach full in ~30 seconds at 1A
- Any USB-C port or charger works
- No special charge IC needed — just current limiting
  (supercap charge current = VUSB - VSCAP / ESR, can be huge;
  a 1A current limiter protects the USB source)

### Combined Power Budget

| Source | Daily energy | Notes |
|--------|-------------|-------|
| Supercap (full) | 25J (one-time) | ~115 transactions |
| Passive piezo harvest | 1.4-5.8J/day | Offsets self-discharge |
| NFC per transaction | 60-120mJ | ~28-55% of transaction cost |
| USB-C (30s charge) | 25J | Full refill |
| Self-discharge loss | -2.5J/day (at 50%) | Main drain when idle |

**Typical daily-carry scenario:**
1. Morning: supercap at ~90% from overnight idle (piezo-offset)
2. Walking to transit: passive harvesting tops up
3. Tap to pay: NFC field + supercap powers transaction
4. Evening: plug into USB-C for 30s while charging phone
5. Repeat indefinitely — device never "dies"

**Worst case (forgotten in drawer for 2 months):**
1. Supercap fully discharged
2. Pick up, plug into USB-C for 30 seconds → fully charged
3. Or shake vigorously for 15-20s → enough for 1 transaction
