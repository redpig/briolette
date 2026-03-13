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

### Supercapacitor (100mF, 3.3V)
- Stored energy: 544mJ
- Usable energy (down to 2.0V): ~290mJ
- Transactions per full charge: **~7** (at 72mAs × 3.3V = 238mJ each)
  - Actually ~238mJ/transaction, so 290mJ/238mJ ≈ 1.2 transactions
  - The supercap alone is marginal for a full transaction
  - Supplemented by NFC field energy during the tap

### NFC Field Harvesting
- Typical NFC field: 5-10mA at 3.3V
- Over a 4-second tap: 16-33mAs harvested
- This covers ~20-45% of a transaction's energy need

### Optional LiPo (50mAh, 3.7V)
- Stored energy: 666J (huge relative to need)
- Transactions per full charge: **~2500**
- Self-discharge: ~2-3% per month
- Shelf life before recharge: months

### Recommendation

For the keychain form factor, use **both** a small supercap (10-47mF)
and a thin LiPo coin cell (LIR1254, ~60mAh, 12.5mm diameter):
- Supercap handles peak current during e-ink refresh
- LiPo provides the bulk energy storage
- NFC harvesting tops up the supercap during each tap
- USB-C charges the LiPo when connected

This combination ensures the credstick works reliably even after months
of sitting idle, while keeping the form factor small.
