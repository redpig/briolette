# Rev 1 — Bill of Materials (Custom PCB)

Production BOM for the integrated credstick board.
Prices are per-unit at qty 10 (prototype run) and qty 1000 (production).

## Core Components

| # | Component | MPN | Package | Qty | ~$1 | ~$1k | Source |
|---|-----------|-----|---------|-----|-----|------|--------|
| 1 | MCU | nRF52840-QIAA-R7 | aQFN-73 (7×7mm) | 1 | $3.80 | $3.20 | DigiKey / Mouser |
| 2 | Secure element | ATECC608B-MAHDA-S | UDFN-8 (2×3mm) | 1 | $0.75 | $0.55 | DigiKey |
| 3 | E-ink display | GDEY0154D67 (Good Display) | 24-pin FPC | 1 | $8.00 | $4.00 | Good Display |
| 4 | 32 MHz crystal | ABM8-32.000MHZ-B2-T (Abracon) | 3.2×2.5mm | 1 | $0.40 | $0.25 | DigiKey |
| 5 | 32.768 kHz crystal | ABS07-32.768KHZ-T (Abracon) | 3.2×1.5mm | 1 | $0.35 | $0.20 | DigiKey |
| 6 | Supercapacitor | SCCR14E505SRB (Kyocera AVX) 5F 3V | 14mm cylindrical THT | 2 | $3.00 | $2.00 | DigiKey / Mouser |
| 7 | Piezo harvester | PPA-1014 (Mide/CEDRAT) or equiv | ~25×5mm cantilever | 1 | $5.00 | $2.00 | Piezo.com |
| 8 | USB-C connector | USB4135-GF-A (GCT) | SMD mid-mount | 1 | $0.60 | $0.35 | DigiKey |
| 9 | Charge current limiter | TPS2553DBVR (TI) | SOT-23-6 | 1 | $0.50 | $0.30 | DigiKey |
| 10 | LDO regulator | AP2112K-3.3TRG1 (Diodes Inc) | SOT-23-5 | 1 | $0.50 | $0.30 | DigiKey |
| 11 | ESD protection | IP4292CZ12-8TTL,1 (Nexperia) | TSSOP-16 | 1 | $0.50 | $0.30 | DigiKey |
| 12 | Piezo rectifier | BAT54S,215 (Nexperia) | SOT-23 | 1 | $0.15 | $0.08 | DigiKey |

## Passive Components

| # | Component | MPN | Value | Package | Qty | ~$1 | ~$1k |
|---|-----------|-----|-------|---------|-----|-----|------|
| 13 | Decoupling cap | CL05B104KO5NNNC (Samsung) | 100nF 16V X7R | 0402 | 8 | $0.10 | $0.04 |
| 14 | Bulk cap | CL05A106MQ5NUNC (Samsung) | 10µF 6.3V X5R | 0402 | 2 | $0.10 | $0.05 |
| 15 | Crystal load cap (32M) | CL05C120JB5NNNC (Samsung) | 12pF 50V C0G | 0402 | 2 | $0.05 | $0.02 |
| 16 | Crystal load cap (32k) | CL05C6R8CB5NNNC (Samsung) | 6.8pF 50V C0G | 0402 | 2 | $0.05 | $0.02 |
| 17 | NFC matching cap | CL05C181JB5NNNC (Samsung) | 180pF 50V C0G | 0402 | 2 | $0.05 | $0.02 |
| 18 | NFC series inductor | LQW15AN390NJ0D (Murata) | 390nH | 0402 | 1 | $0.10 | $0.05 |
| 19 | I2C pull-up | RC0402FR-074K7L (Yageo) | 4.7kΩ 1% | 0402 | 2 | $0.02 | $0.01 |
| 20 | Reset pull-up | RC0402FR-0710KL (Yageo) | 10kΩ 1% | 0402 | 1 | $0.02 | $0.01 |
| 21 | ILIM resistor | RC0402FR-0710KL (Yageo) | 10kΩ 1% (sets ~100mA) | 0402 | 1 | $0.02 | $0.01 |
| 22 | LED resistor | RC0402FR-071KL (Yageo) | 1kΩ 1% | 0402 | 1 | $0.02 | $0.01 |
| 23 | CC pull-downs | RC0402FR-075K1L (Yageo) | 5.1kΩ 1% | 0402 | 2 | $0.02 | $0.01 |
| 24 | Piezo buffer cap | CL21A107MQCLNNC (Samsung) | 100µF 6.3V X5R | 0805 | 1 | $0.15 | $0.08 |

## Mechanical / Indicators

| # | Component | MPN | Description | Qty | ~$1 | ~$1k |
|---|-----------|-----|-------------|-----|-----|------|
| 25 | FPC connector | FH12-24S-0.5SH(55) (Hirose) | 24-pin 0.5mm bottom-contact | 1 | $0.30 | $0.15 |
| 26 | Tactile switch | SKQGABE010 (Alps Alpine) | 3.9×2.9×1.7mm SMD, 160gf | 2 | $0.30 | $0.16 |
| 27 | Charge LED | LTST-C150GKT (Lite-On) | Green 0402 LED | 1 | $0.10 | $0.05 |
| 28 | Keyring hole | — | 3mm plated through-hole in PCB | 1 | $0 | $0 |
| 29 | Enclosure | — | 3D-printed PETG or injection-molded | 1 | $3.00 | $0.50 |

## NFC Antenna (PCB Trace)

Not a discrete component — designed as copper traces on the bottom PCB
layer. Rectangular spiral, ~25mm × 20mm, 4-5 turns, 0.3mm trace width,
0.3mm spacing. Tuned to 13.56 MHz with the matching network (items 17-18).

## Cost Summary

| | Qty 10 (proto) | Qty 1000 (prod) |
|---|----------------|-----------------|
| Components | ~$20 | ~$13 |
| PCB (4-layer, 60×30mm) | ~$5 | ~$1.50 |
| Assembly (PCBA) | ~$8 | ~$2 |
| E-ink display | ~$8 | ~$4 |
| Enclosure | ~$3 | ~$0.50 |
| **Total per unit** | **~$44** | **~$21** |

## Key Sourcing Notes

- **nRF52840**: Widely available from DigiKey/Mouser. No known shortages
  as of early 2026. Lead time ~0 (in stock).
- **ATECC608B**: Commodity part, in stock everywhere. The UDFN-8 package
  is preferred for size; SOIC-8 is available if needed for hand rework.
- **E-ink display**: Good Display is the primary source. The GDEW0154M09
  is EOL — use the **GDEY0154D67** replacement. For smaller (1.02"),
  contact Good Display directly or use Waveshare modules.
- **Supercapacitors (SCC 5F 3V)**: Two in parallel = 10F. Kyocera AVX
  cylindrical, ~14mm dia × 20mm. Infinite cycle life, no degradation.
  For thinner form factor: PrizmaCap SCP series (15F, 2.1V, 0.8mm thin)
  but needs boost converter.
- **Piezo harvester**: Cantilever-style piezoelectric element. Generates
  50-200µW from ambient keychain motion, 1-5mW from deliberate shaking.
- **Tactile switches**: Alps SKQGABE010 is 3.9×2.9mm, low profile (1.7mm),
  widely available. Alternatives: C&K PTS636 or Panasonic EVQ-P0N02B.
- **FPC connector**: Hirose FH12-24S-0.5SH(55) is the standard 24-pin
  0.5mm bottom-contact ZIF for e-ink displays. Verify pin orientation
  matches your display's FPC cable.
- **Passives**: Samsung CL-series MLCC and Yageo RC-series resistors are
  commodity parts with excellent availability. Any 0402 equivalent works.

## PCBA Ordering Guide

### Prototype Quantities (1-50 boards)

| Service | Strengths | Typical Cost (10 boards) | Turnaround |
|---------|-----------|--------------------------|------------|
| **JLCPCB** (recommended) | Cheapest PCB+assembly combo. Huge parts library. KiCad plugin for BOM/CPL export. | PCB ~$8 + PCBA ~$30-50 + parts | 5-7 days + shipping |
| **PCBWay** | Good quality, flexible on special requests (4-layer, odd shapes). Quote-based assembly. | PCB ~$15 + PCBA ~$50-80 + parts | 5-10 days + shipping |
| **OSH Park** | US-based, excellent quality. Purple boards. No assembly — PCB only. | PCB ~$50 (4-layer) | 12 days |
| **MacroFab** | US-based turnkey. Upload design, they source + assemble. Higher cost but hands-off. | ~$150-300 per board | 10-15 days |

### Production Quantities (100-10,000+)

| Service | Strengths | Notes |
|---------|-----------|-------|
| **JLCPCB** | Price leader up to ~5k units. PCBA + stencil + parts sourcing. | Extended parts library may need manual quotes for nRF52840 |
| **PCBWay** | Good mid-volume. Can handle custom enclosures too. | Quote-based, responsive support |
| **Elecrow** | Competitive pricing, similar to JLCPCB. Good for 100-1000 range. | Shenzhen-based |
| **Seeed Studio Fusion** | Turnkey from PCB to enclosure. Good for crowdfunding projects. | Quote-based for >100 units |
| **Contract manufacturer (local)** | For 5k+ units, a local CM gives better quality control and logistics. | Get quotes from 3+ CMs |

### Recommended Workflow

1. **Export from KiCad**: Generate Gerbers, BOM CSV, and CPL (component placement list)
2. **JLCPCB plugin**: Install the JLCPCB KiCad plugin — it auto-generates BOM + CPL in their format
3. **Upload to JLCPCB**: Select 4-layer, 1.6mm thickness, ENIG finish (for NFC antenna pads)
4. **Parts sourcing**: Most passives and the LDO/ESD are in JLCPCB's basic/extended library. The nRF52840, ATECC608B, and USB-C connector may need to be ordered from DigiKey and shipped to JLCPCB (consignment) or quoted as extended parts
5. **THT components**: Supercapacitors and piezo are through-hole/manual — assemble these yourself after receiving SMD-assembled boards
6. **Display + enclosure**: Order separately from Good Display and your 3D printer / injection mold vendor

### PCB Finish Note

Use **ENIG (Electroless Nickel Immersion Gold)** finish, not HASL. ENIG gives
flat pads for QFN soldering and better NFC antenna conductivity. Cost adder
is ~$5-10 per panel at JLCPCB.

## Alternative: Credit Card Form Factor

If keychain is too tight, a credit card (85.6mm × 53.98mm × ~2mm) gives
much more room for a larger display (2.13"), bigger battery, and easier
NFC antenna. The BOM stays the same; only the PCB dimensions and antenna
layout change.
