# Solar Relay — Bill of Materials

Production BOM for the solar relay NFC reader board.
Prices are per-unit at qty 10 (prototype run) and qty 1000 (production).

## Core Components

| # | Component | MPN | Package | Qty | ~$1 | ~$1k | Source |
|---|-----------|-----|---------|-----|-----|------|--------|
| 1 | MCU | nRF52840-QIAA-R7 (Nordic) | aQFN-73 (7×7mm) | 1 | $3.80 | $3.20 | DigiKey / Mouser |
| 2 | NFC controller | PN7150B0HN/C11002Y (NXP) | HVQFN-40 (5×5mm) | 1 | $3.50 | $2.50 | DigiKey / Mouser |
| 3 | Solar MPPT charger | BQ25504RGTR (TI) | QFN-20 (3.5×3.5mm) | 1 | $3.50 | $2.80 | DigiKey / Mouser |
| 4 | LDO regulator | AP2112K-3.3TRG1 (Diodes Inc) | SOT-23-5 | 1 | $0.50 | $0.30 | DigiKey |
| 5 | Solar cell | KXOB25-05X3F (IXYS/Anysolar) 40×30mm | Solder tabs | 1 | $2.00 | $0.80 | DigiKey / Mouser |
| 6 | Supercapacitor | SCCR14E106SRB (Kyocera AVX) 10F 3V | 14mm cylindrical THT | 2 | $6.00 | $4.00 | DigiKey / Mouser |
| 7 | Piezo harvester | PPA-1014 (Mide/CEDRAT) or equiv | ~25×5mm cantilever | 1 | $5.00 | $2.00 | Piezo.com |
| 8 | Piezo rectifier | BAT54S,215 (Nexperia) | SOT-23 | 1 | $0.15 | $0.08 | DigiKey |
| 9 | USB-C connector | USB4135-GF-A (GCT) | SMD mid-mount | 1 | $0.60 | $0.35 | DigiKey |
| 10 | ESD protection | IP4292CZ12-8TTL,1 (Nexperia) | TSSOP-16 | 1 | $0.50 | $0.30 | DigiKey |
| 11 | Charge current limiter | TPS2553DBVR (TI) | SOT-23-6 | 1 | $0.50 | $0.30 | DigiKey |
| 12 | 32 MHz crystal | ABM8-32.000MHZ-B2-T (Abracon) | 3.2×2.5mm | 1 | $0.40 | $0.25 | DigiKey |
| 13 | 32.768 kHz crystal | ABS07-32.768KHZ-T (Abracon) | 3.2×1.5mm | 1 | $0.35 | $0.20 | DigiKey |

## Passive Components

| # | Component | MPN | Value | Package | Qty | ~$1 | ~$1k |
|---|-----------|-----|-------|---------|-----|-----|------|
| 14 | Decoupling cap | CL05B104KO5NNNC (Samsung) | 100nF 16V X7R | 0402 | 10 | $0.12 | $0.05 |
| 15 | Bulk cap | CL05A106MQ5NUNC (Samsung) | 10µF 6.3V X5R | 0402 | 3 | $0.15 | $0.06 |
| 16 | Crystal load cap (32M) | CL05C120JB5NNNC (Samsung) | 12pF 50V C0G | 0402 | 2 | $0.05 | $0.02 |
| 17 | Crystal load cap (32k) | CL05C6R8CB5NNNC (Samsung) | 6.8pF 50V C0G | 0402 | 2 | $0.05 | $0.02 |
| 18 | I2C pull-up | RC0402FR-074K7L (Yageo) | 4.7kΩ 1% | 0402 | 2 | $0.02 | $0.01 |
| 19 | Reset pull-up | RC0402FR-0710KL (Yageo) | 10kΩ 1% | 0402 | 1 | $0.02 | $0.01 |
| 20 | CC pull-downs | RC0402FR-075K1L (Yageo) | 5.1kΩ 1% | 0402 | 2 | $0.02 | $0.01 |
| 21 | ILIM resistor | RC0402FR-0710KL (Yageo) | 10kΩ 1% | 0402 | 1 | $0.02 | $0.01 |
| 22 | LED resistors | RC0402FR-071KL (Yageo) | 1kΩ 1% | 0402 | 3 | $0.03 | $0.01 |
| 23 | Piezo buffer cap | CL21A107MQCLNNC (Samsung) | 100µF 6.3V X5R | 0805 | 1 | $0.15 | $0.08 |
| 24 | BQ25504 inductor | LPS4018-222MRC (Coilcraft) | 2.2µH 1.5A sat | 4×4×1.8mm | 1 | $0.80 | $0.45 |
| 25 | BQ25504 CSTOR | CL05A475KQ5NRNC (Samsung) | 4.7µF 6.3V X5R | 0402 | 1 | $0.08 | $0.03 |
| 26 | BQ25504 CBAT | CL21A226MQQNNNE (Samsung) | 22µF 6.3V X5R | 0805 | 1 | $0.12 | $0.05 |
| 27 | BQ25504 CIN | CL05B104KO5NNNC (Samsung) | 100nF 16V X7R | 0402 | 1 | $0.02 | $0.01 |
| 28 | MPPT VOC dividers | RC0402FR-07XXX (Yageo) see note | Per BQ25504 datasheet | 0402 | 4 | $0.04 | $0.02 |
| 29 | OV/UV threshold | RC0402FR-07XXX (Yageo) see note | Per BQ25504 datasheet | 0402 | 4 | $0.04 | $0.02 |

### BQ25504 Resistor Values (Items 28-29)

Per TI BQ25504 datasheet, for 3V supercap with typical solar cell Voc ~5V:

| Function | Designation | Value | MPN |
|----------|-------------|-------|-----|
| MPPT ratio (ROC1) | R_OC1 | 5.76MΩ | RC0402FR-075M76L |
| MPPT ratio (ROC2) | R_OC2 | 8.06MΩ | RC0402FR-078M06L |
| Overvoltage (ROV1-1) | R_OV1_1 | 5.76MΩ | RC0402FR-075M76L |
| Overvoltage (ROV1-2) | R_OV1_2 | 8.87MΩ | RC0402FR-078M87L |
| Undervoltage (RUV1-1) | R_UV1_1 | 5.76MΩ | RC0402FR-075M76L |
| Undervoltage (RUV1-2) | R_UV1_2 | 13.3MΩ | RC0402FR-0713ML |
| OK threshold (ROK1) | R_OK1 | 5.76MΩ | RC0402FR-075M76L |
| OK hysteresis (ROK2) | R_OK2 | 8.87MΩ | RC0402FR-078M87L |

Adjust values based on your specific solar cell Voc and desired thresholds.
See BQ25504 datasheet Section 9.2 for the resistor divider formulas.

## NFC Reader Antenna Matching (PN7150)

| # | Component | MPN | Value | Package | Qty | ~$1 | ~$1k |
|---|-----------|-----|-------|---------|-----|-----|------|
| 30 | TX matching cap | CL05C151JB5NNNC (Samsung) | 150pF C0G | 0402 | 2 | $0.05 | $0.02 |
| 31 | RX matching cap | CL05C270JB5NNNC (Samsung) | 27pF C0G | 0402 | 1 | $0.05 | $0.02 |
| 32 | EMC filter cap | CL05B104KO5NNNC (Samsung) | 100nF X7R | 0402 | 2 | $0.02 | $0.01 |
| 33 | TX series inductor | LQW15AN390NJ0D (Murata) | 390nH | 0402 | 1 | $0.10 | $0.05 |
| 34 | Damping resistor | RC0402FR-0710RL (Yageo) | 10Ω 1% | 0402 | 1 | $0.02 | $0.01 |

**Note**: NFC reader antenna is a PCB trace spiral on B.Cu (3-turn, 35×25mm,
0.5mm trace width). Matching values are approximate — tune on the bench
with a VNA to hit 13.56 MHz resonance. Start with these values and adjust
C30/C31 to center the resonant frequency.

## Mechanical / Indicators

| # | Component | MPN | Description | Qty | ~$1 | ~$1k |
|---|-----------|-----|-------------|-----|-----|------|
| 35 | Tactile switch | SKQGABE010 (Alps Alpine) | 3.9×2.9×1.7mm SMD, 160gf | 4 | $0.60 | $0.32 |
| 36 | Red LED | LTST-C150KRKT (Lite-On) | Red 0402 | 1 | $0.10 | $0.05 |
| 37 | Green LED | LTST-C150GKT (Lite-On) | Green 0402 | 1 | $0.10 | $0.05 |
| 38 | Blue LED | LTST-C150TBKT (Lite-On) | Blue 0402 | 1 | $0.10 | $0.05 |
| 39 | M2×6 self-tapping screw | — | Stainless steel, pan head | 4 | $0.20 | $0.08 |
| 40 | Enclosure | — | 3D-printed PETG or injection-molded | 1 | $3.00 | $0.50 |

## Cost Summary

| | Qty 10 (proto) | Qty 1000 (prod) |
|---|----------------|-----------------|
| Components | ~$23 | ~$15 |
| PCB (4-layer, 70×45mm) | ~$6 | ~$2.00 |
| Assembly (PCBA) | ~$8 | ~$2.50 |
| Solar cell | ~$2 | ~$0.80 |
| Enclosure + screws | ~$3.20 | $0.58 |
| **Total per unit** | **~$42** | **~$21** |

## Key Sourcing Notes

- **PN7150**: NXP part, available from DigiKey/Mouser. The PN7150B0HN/C11002Y
  is marked NRND (Not Recommended for New Designs) by NXP. **Prefer the
  PN7160** (pin-compatible successor) for new boards. The PN7160 has lower
  power and identical I2C interface. Check lead times — NFC controller ICs
  can have longer lead times than commodity parts.
- **BQ25504**: TI ultra-low-power MPPT charger. Widely available. The RGTR
  suffix is the 20-pin QFN variant. Eval board (BQ25504EVM) is available
  for bench testing before committing to the PCB design.
- **Solar cell**: KXOB25-05X3F (IXYS/Anysolar) is a common small mono-Si
  cell. Size is ~42×26mm, Voc ~3.16V, Isc ~50mA. For exact 40×30mm, cut
  or use equivalent from Panasonic (AM-1816CA) or custom-cut cells from
  AliExpress suppliers.
- **Supercapacitors (10F 3V)**: Two units = 20F total. Same Kyocera AVX SCC
  series as credstick but higher capacitance. These are larger (~14mm dia
  × 25mm). Ensure they fit the 70×45mm board.
- **Buttons**: Alps SKQGABE010 — same as credstick. 4 needed for PIN entry
  and mode selection. Low-profile (1.7mm) fits under the case roof.
- **Inductor (BQ25504)**: Coilcraft LPS4018-222MRC is recommended in the
  BQ25504 datasheet. Critical for boost converter efficiency — do not
  substitute without checking saturation current and DCR.

## PCBA Ordering Guide

### Prototype Quantities (1-50 boards)

| Service | Strengths | Typical Cost (10 boards) | Turnaround |
|---------|-----------|--------------------------|------------|
| **JLCPCB** (recommended) | Cheapest PCB+assembly combo. Huge parts library. KiCad plugin for BOM/CPL export. | PCB ~$10 + PCBA ~$40-60 + parts | 5-7 days + shipping |
| **PCBWay** | Good quality, flexible on special requests. Quote-based assembly. | PCB ~$18 + PCBA ~$60-100 + parts | 5-10 days + shipping |
| **MacroFab** | US-based turnkey. Upload design, they source + assemble. | ~$180-350 per board | 10-15 days |

### Production Quantities (100-10,000+)

| Service | Strengths | Notes |
|---------|-----------|-------|
| **JLCPCB** | Price leader up to ~5k units. | QFN-40/QFN-20 may need extended parts quote |
| **PCBWay** | Good mid-volume. Can handle enclosures too. | Quote-based |
| **Seeed Studio Fusion** | Turnkey from PCB to enclosure. | Good for crowdfunding projects |
| **Local CM** | For 5k+ units, better QC and logistics. | Get quotes from 3+ manufacturers |

### Recommended Workflow

1. **Export from KiCad**: Gerbers, BOM CSV, CPL (component placement list)
2. **JLCPCB plugin**: Auto-generates BOM + CPL in JLCPCB format
3. **Upload**: 4-layer, 1.6mm, ENIG finish (for QFN pads + NFC antenna)
4. **Parts**: Most passives are in JLCPCB basic library. nRF52840, PN7150,
   and BQ25504 may need extended parts or DigiKey consignment
5. **THT components**: Supercapacitors are through-hole — solder manually
   after receiving SMD-assembled boards
6. **Solar cell**: Attach with conductive epoxy or solder tabs after assembly
7. **Test**: Power via USB-C first, verify BQ25504 output, then test NFC

### PCB Finish

Use **ENIG** finish for reliable QFN soldering and NFC antenna conductivity.
