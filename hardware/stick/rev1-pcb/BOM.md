# Rev 1 — Bill of Materials (Custom PCB)

Production BOM for the integrated credstick board.
Prices are per-unit at qty 10 (prototype run) and qty 1000 (production).

## Core Components

| # | Component | Part Number | Package | Qty | ~$1 | ~$1k | Source |
|---|-----------|------------|---------|-----|-----|------|--------|
| 1 | MCU | nRF52840-QIAA-R7 | QFN-48 (6x6mm) | 1 | $3.80 | $3.20 | [DigiKey](https://www.digikey.com/) / [Mouser](https://www.mouser.com/) |
| 2 | Secure element | ATECC608B-MAHDA-S | UDFN-8 (2x3mm) | 1 | $0.75 | $0.55 | [DigiKey](https://www.digikey.com/en/products/detail/microchip-technology/ATECC608B-MAHDA-S/13415130) |
| 3 | E-ink display | Good Display GDEY0154D67 (1.54") or equivalent 1.02" | FPC | 1 | $8 | $4 | [Good Display](https://www.good-display.com/) |
| 4 | 32 MHz crystal | ABM8-32.000MHZ-B2-T | 3.2x2.5mm | 1 | $0.40 | $0.25 | DigiKey |
| 5 | 32.768 kHz crystal | ABS07-32.768KHZ-T | 3.2x1.5mm | 1 | $0.35 | $0.20 | DigiKey |
| 6 | Supercapacitor | AVX BestCap BZ015B104ZSB (100mF 3.3V) | Radial | 1 | $2.50 | $1.80 | DigiKey |
| 7 | USB-C connector | GCT USB4135-GF-A | SMD mid-mount | 1 | $0.60 | $0.35 | DigiKey |
| 8 | LiPo charge IC | MCP73831T-2ACI/OT | SOT-23-5 | 1 | $0.55 | $0.40 | DigiKey |
| 9 | LDO regulator | AP2112K-3.3TRG1 | SOT-23-5 | 1 | $0.30 | $0.20 | DigiKey |
| 10 | ESD protection | IP4292CZ12-8TTL,1 | TSSOP-16 | 1 | $0.50 | $0.30 | DigiKey |

## Passive Components

| # | Component | Value | Package | Qty | ~$1 | ~$1k |
|---|-----------|-------|---------|-----|-----|------|
| 11 | Decoupling caps | 100nF | 0402 | 8 | $0.10 | $0.05 |
| 12 | Bulk cap | 10µF | 0402 | 2 | $0.10 | $0.05 |
| 13 | NFC matching cap | 180pF (tuned) | 0402 | 2 | $0.05 | $0.02 |
| 14 | NFC series inductor | 390nH | 0402 | 1 | $0.10 | $0.05 |
| 15 | Pull-up resistors | 4.7kΩ (I2C) | 0402 | 2 | $0.02 | $0.01 |
| 16 | Charge resistor | 2kΩ (500mA charge) | 0402 | 1 | $0.02 | $0.01 |
| 17 | LED + resistor | Charge status LED | 0402 | 1+1 | $0.10 | $0.05 |

## Mechanical

| # | Component | Description | Qty | ~$1 | ~$1k |
|---|-----------|-------------|-----|-----|------|
| 18 | FPC connector | 24-pin 0.5mm pitch (for e-ink) | 1 | $0.30 | $0.15 |
| 19 | Tactile switch | 3x3mm SMD button | 1 | $0.15 | $0.08 |
| 20 | Keyring hole | 3mm plated through-hole in PCB | 1 | $0 | $0 |
| 21 | Enclosure | 3D-printed or injection-molded shell | 1 | $3 | $0.50 |

## NFC Antenna (PCB Trace)

Not a discrete component — designed as copper traces on the bottom PCB
layer. Rectangular spiral, ~25mm x 20mm, 4-5 turns, 0.3mm trace width,
0.3mm spacing. Tuned to 13.56 MHz with the matching network (items 13-14).

## Cost Summary

| | Qty 10 (proto) | Qty 1000 (prod) |
|---|----------------|-----------------|
| Components | ~$18 | ~$12 |
| PCB (4-layer, 60x30mm) | ~$5 | ~$1.50 |
| Assembly (PCBA) | ~$8 | ~$2 |
| E-ink display | ~$8 | ~$4 |
| Enclosure | ~$3 | ~$0.50 |
| **Total per unit** | **~$42** | **~$20** |

## Key Sourcing Notes

- **nRF52840**: Widely available from DigiKey/Mouser. No known shortages
  as of early 2026. Lead time ~0 (in stock).
- **ATECC608B**: Commodity part, in stock everywhere. The UDFN-8 package
  is preferred for size; SOIC-8 is available if needed for hand rework.
- **E-ink display**: Good Display is the primary source. The GDEW0154M09
  is EOL — use the **GDEY0154D67** replacement. For smaller (1.02"),
  contact Good Display directly or use Waveshare modules.
- **Supercapacitor**: If the BestCap is too large physically, consider a
  thin-film supercap (e.g., Murata DMH series) or a small LiPo coin cell
  (LIR2032) instead.

## Alternative: Credit Card Form Factor

If keychain is too tight, a credit card (85.6mm x 53.98mm x ~2mm) gives
much more room for a larger display (2.13"), bigger battery, and easier
NFC antenna. The BOM stays the same; only the PCB dimensions and antenna
layout change.
