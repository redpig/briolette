# JLCPCB Ordering Guide — Briolette Hardware

Step-by-step instructions for ordering the Credstick and Solar Relay PCBs
from JLCPCB with SMD assembly (PCBA).

## 1. Generate Manufacturing Files

Run the export script from the `hardware/` directory on a machine with
KiCad 8.0+ installed:

```bash
cd hardware/
python3 export_jlcpcb.py
```

This produces two zip files:
- `stick/rev1-pcb/gerbers/briolette-credstick-jlcpcb.zip`
- `relay/gerbers/briolette-relay-jlcpcb.zip`

Each zip contains gerbers, drill files, BOM CSV, and CPL (centroid) CSV.

**Alternative (manual export from KiCad):**
1. Open the `.kicad_pcb` file in KiCad
2. File → Fabrication Outputs → Gerbers (.gbr)
   - Select all copper layers (F.Cu, In1.Cu, In2.Cu, B.Cu)
   - Select F.Paste, B.Paste, F.SilkS, B.SilkS, F.Mask, B.Mask, Edge.Cuts
   - Click "Plot"
3. File → Fabrication Outputs → Drill Files (.drl)
   - Format: Excellon, Metric, Decimal
   - Uncheck "Merge PTH and NPTH"
   - Click "Generate Drill File"
4. File → Fabrication Outputs → Component Placement (.pos)
   - Format: CSV, Millimeters
   - This is the centroid / CPL file
5. Tools → Edit Symbol Fields → Export as CSV
   - Or use the JLCPCB KiCad plugin for auto-formatted BOM + CPL

## 2. Route the PCB in KiCad

The `.kicad_pcb` files have all components placed with correct footprints,
pads, and net assignments, but **signal traces have not been routed** (except
the NFC antenna spirals on B.Cu). You must route traces before exporting
gerbers.

### 2a. Open the board in KiCad

```
kicad stick/rev1-pcb/kicad/stick.kicad_pcb
# or
kicad relay/kicad/relay.kicad_pcb
```

When KiCad opens the board, you'll see a **ratsnest** — thin lines showing
all unrouted connections. Each line connects two pads that share a net.

### 2b. Import the schematic netlist (optional)

If the ratsnest doesn't appear, import the netlist from the schematic:
1. Open the PCB editor
2. **Tools → Update PCB from Schematic** (or press F8)
3. Click **Update PCB**

This synchronizes the schematic net assignments with the board. The net
assignments are already embedded in the PCB pads, so this step may not
be needed — but it ensures everything is consistent.

### 2c. Route traces

Use KiCad's interactive router to connect all pads:

1. Press **X** to enter routing mode
2. Click a pad to start a trace, click another pad on the same net to finish
3. Use **V** during routing to add a via and switch layers
4. The 4-layer stackup means:
   - **F.Cu** — Signal traces and component connections
   - **In1.Cu** — GND plane (zone fill handles most GND connections via vias)
   - **In2.Cu** — VCC plane (zone fill handles most VCC connections via vias)
   - **B.Cu** — NFC antenna (already routed) + additional signals if needed

**Routing tips for these boards:**
- **GND/VCC connections**: Drop a via from the pad — the inner plane zone
  pour connects automatically. No trace needed.
- **QFN breakout**: Route the nRF52840's 48 pins outward from the center,
  using both F.Cu and B.Cu with vias. Fan-out the inner pins first.
- **Crystal traces**: Keep Y1/Y2 traces short (<10mm) and avoid routing
  other signals near the crystal area.
- **USB D+/D-**: Route as a differential pair (90Ω impedance target). Keep
  traces equal length and close together.
- **SPI bus**: Keep SCK/MOSI/CS traces reasonably short to the FPC connector.
- **I2C bus** (relay): SDA/SCL can be longer — pull-ups (R8/R9) handle
  signal integrity.
- **BQ25504 inductor** (relay): Keep the L2 → U3 trace short and wide
  (0.5mm+) — this carries the boost converter switching current.

### 2d. Run DRC

After routing, run **Inspect → Design Rules Check** (or press Shift+D):
- Fix any clearance violations (traces too close)
- Fix any unconnected nets (missed connections)
- Fix any short circuits

### 2e. Fill zones

**Edit → Fill All Zones** (press B) to generate the copper pours on the
inner layers. This creates the GND and VCC planes.

### 2f. Re-export gerbers

Once DRC passes with no errors, re-run the export script:
```bash
python3 export_jlcpcb.py
```

The resulting zip files now contain fully-routed gerbers ready for fab.

## 3. Upload to JLCPCB

Go to [jlcpcb.com](https://www.jlcpcb.com/) and click **"Order Now"** (or
**"Instant Quote"**).

### Step 2a: Upload Gerbers

Click **"Add gerber file"** and upload the zip file. JLCPCB will parse the
gerbers and show a board preview. Verify:

- Board dimensions match (60×30mm for credstick, 70×45mm for relay)
- All layers are detected (should show 4 copper layers)
- Board outline looks correct with rounded corners

## 4. PCB Specification Settings

Select these options for both boards:

| Setting | Value | Notes |
|---------|-------|-------|
| **Base Material** | FR-4 | Standard |
| **Layers** | **4** | Both boards are 4-layer |
| **Dimensions** | Auto-detected | Verify: 60×30mm (credstick) or 70×45mm (relay) |
| **PCB Qty** | 5 (minimum) | 5 is cheapest for prototypes |
| **Product Type** | Industrial/Consumer Electronics | |
| **Different Design** | 1 | Unless panelizing |
| **Delivery Format** | Single PCB | |
| **PCB Thickness** | **1.6mm** | Standard, matches stackup |
| **PCB Color** | Your choice | Green is cheapest, black looks nice |
| **Silkscreen** | White (default) | |
| **Surface Finish** | **ENIG** | Required for QFN pads and NFC antenna. Do NOT use HASL — the uneven surface makes QFN soldering unreliable |
| **ENIG Thickness** | 1U" (default) | |
| **Outer Copper Weight** | 1 oz | Standard |
| **Inner Copper Weight** | 0.5 oz | Standard for 4-layer |
| **Via Covering** | Tented | Default, fine for our via sizes |
| **Board Outline Tolerance** | ±0.2mm (regular) | |
| **Confirm Production File** | Yes | Recommended for first order — JLCPCB engineers review your files |
| **Remove Order Number** | Yes | Specify position, or "Yes" to let JLCPCB place it |
| **Flying Probe Test** | Fully Test (default) | |
| **Gold Fingers** | No | |
| **Castellated Holes** | No | |
| **Edge Plating** | No | |

### Important: Do NOT select these
- **Impedance Control**: Not needed (we aren't routing controlled-impedance traces)
- **Paper between PCBs**: Only if you care about scratches
- **4-Wire Kelvin Test**: Overkill for prototypes

## 5. Enable SMT Assembly (PCBA)

Toggle **"PCB Assembly"** on. This is below the PCB options.

| Setting | Value | Notes |
|---------|-------|-------|
| **PCBA Type** | Economic | Use Standard only if Economic can't place a part |
| **Assembly Side** | **Top Side** | All SMD components are on the front |
| **PCBA Qty** | Match your PCB qty (e.g. 5) | |
| **Tooling Holes** | Added by JLCPCB | Let them add tooling holes automatically |
| **Confirm Parts Placement** | Yes | Recommended — they'll send a placement preview for approval |

Click **"Next"** to proceed to BOM & CPL upload.

## 6. Upload BOM and CPL

Upload the two CSV files from the zip:
- **BOM file**: `stick-BOM-JLCPCB.csv` or `relay-BOM-JLCPCB.csv`
- **CPL file**: `stick-CPL-JLCPCB.csv` or `relay-CPL-JLCPCB.csv`

Click **"Process BOM & CPL"**.

### 6a: Match Parts to LCSC Numbers

JLCPCB will attempt to auto-match components. For any unmatched parts, you
need to search and assign LCSC part numbers manually.

#### Credstick — Key LCSC Part Numbers

| Component | MPN | Suggested LCSC # | Library |
|-----------|-----|-------------------|---------|
| nRF52840 | nRF52840-QIAA-R7 | C190794 | Extended |
| AP2112K-3.3 | AP2112K-3.3TRG1 | C51118 | Basic |
| TPS2553 | TPS2553DBVR | C136019 | Extended |
| IP4292CZ12 | IP4292CZ12-8TTL,1 | C558459 | Extended |
| BAT54S | BAT54S,215 | C8598 | Basic |
| USB-C connector | USB4135-GF-A | C2688138 | Extended |
| 100nF 0402 | CL05B104KO5NNNC | C1525 | Basic |
| 10µF 0402 | CL05A106MQ5NUNC | C15525 | Basic |
| 12pF 0402 C0G | CL05C120JB5NNNC | C1547 | Basic |
| 6.8pF 0402 C0G | CL05C6R8CB5NNNC | C414445 | Extended |
| 10kΩ 0402 | RC0402FR-0710KL | C25744 | Basic |
| 1kΩ 0402 | RC0402FR-071KL | C11702 | Basic |
| 5.1kΩ 0402 | RC0402FR-075K1L | C25905 | Basic |
| 32MHz crystal | ABM8-32.000MHZ-B2-T | C255909 | Extended |
| 32.768kHz crystal | ABS07-32.768KHZ-T | C32176 | Extended |
| Tactile switch | SKQGABE010 | C139797 | Extended |
| 0402 Green LED | LTST-C150GKT | C125098 | Extended |
| FPC 24-pin | FH12-24S-0.5SH(55) | C506793 | Extended |

#### Relay — Additional Key LCSC Part Numbers

| Component | MPN | Suggested LCSC # | Library |
|-----------|-----|-------------------|---------|
| PN7150 | PN7150B0HN/C11002Y | C2828498 | Extended |
| BQ25504 | BQ25504RGTR | C134428 | Extended |
| TCA8418 | TCA8418RTWR | C527068 | Extended |
| 2.2µH inductor | LPS4018-222MRC | C108793 | Extended |
| 4.7kΩ 0402 | RC0402FR-074K7L | C25900 | Basic |
| 100µF 0805 | CL21A107MQCLNNC | C250069 | Extended |
| 22µF 0805 | CL21A226MQQNNNE | C45783 | Extended |
| 4.7µF 0402 | CL05A475KQ5NRNC | C23733 | Basic |
| 0402 Red LED | LTST-C150KRKT | C125099 | Extended |
| 0402 Blue LED | LTST-C150TBKT | C125107 | Extended |
| 5.76MΩ 0402 | RC0402FR-075M76L | C137849 | Extended |
| 8.06MΩ 0402 | RC0402FR-078M06L | C352364 | Extended |

**Note**: LCSC part numbers may change over time. Always verify availability
on [lcsc.com](https://www.lcsc.com/) before ordering. Basic parts are cheaper
to assemble (~$0.0017/pad); Extended parts cost ~$0.007/pad extra.

### 6b: Review Component Placement

After matching parts, JLCPCB shows a placement preview. Check that:
- All components are on the correct side (Top)
- Rotation looks correct (IC pin 1 matches pad 1)
- No components overlap or hang off the board edge

**Common rotation fix**: JLCPCB sometimes rotates parts by 90° or 180°
relative to KiCad. If a part looks wrong, note the correction in the
JLCPCB interface — they have a rotation offset tool.

## 7. Parts Not Available on JLCPCB

Some components will likely not be in JLCPCB's library or may be out of
stock. You have two options:

### Option A: Consignment Parts
1. Order the parts from DigiKey/Mouser
2. Ship them to JLCPCB's Shenzhen warehouse
3. JLCPCB will use your parts during assembly
4. Add ~3-5 extra days for receiving

### Option B: Hand-Solder After Receiving
These components are through-hole or special and should be hand-soldered:

**Credstick:**
- Supercapacitors (2× SCCR14E505SRB) — THT, cylindrical
- Piezo harvester (PPA-1014) — cantilever, glue + wire
- E-ink display — connects via FPC cable after assembly

**Relay:**
- Supercapacitors (2× SCCR14E106SRB) — THT, cylindrical
- Piezo harvester (PPA-1014) — cantilever, glue + wire
- Solar cell (KXOB25-05X3F) — attach with conductive epoxy or solder tabs

## 8. Review and Pay

1. Review the final quote breakdown:
   - PCB fabrication cost
   - Assembly (setup + per-pad cost)
   - Component costs
   - Shipping
2. Select shipping method:
   - **DHL/FedEx** (~$20-30, 5-7 days) — recommended for prototypes
   - **Standard shipping** (~$5-10, 15-25 days) — budget option
3. Pay and wait for production file review (if you selected "Confirm
   Production File")

## 9. Post-Assembly Checklist

When boards arrive:

1. **Visual inspection**: Check solder joints under magnification (loupe or
   microscope). Look for solder bridges on QFN pads (nRF52840, PN7150,
   BQ25504)
2. **Continuity check**: Verify no shorts between VCC and GND with a
   multimeter
3. **Power test** (credstick): Connect USB-C, verify 3.3V on LDO output
4. **Power test** (relay): Connect USB-C, verify BQ25504 output and 3.3V
   LDO output
5. **Solder THT components**: Supercapacitors, piezo harvester
6. **Attach peripherals**: E-ink display (credstick), solar cell (relay)
7. **Flash firmware**: Connect SWD programmer (J-Link or ST-Link) to the
   nRF52840 SWD pads
8. **NFC test**: Use an NFC-enabled phone to verify antenna tuning

## Quick Reference: Order Summary

### Credstick Order
| Item | Selection |
|------|-----------|
| Gerber zip | `briolette-credstick-jlcpcb.zip` |
| Layers | 4 |
| Thickness | 1.6mm |
| Surface finish | ENIG |
| Assembly | Top side, Economic PCBA |
| Board size | 60 × 30 mm |

### Relay Order
| Item | Selection |
|------|-----------|
| Gerber zip | `briolette-relay-jlcpcb.zip` |
| Layers | 4 |
| Thickness | 1.6mm |
| Surface finish | ENIG |
| Assembly | Top side, Economic PCBA |
| Board size | 70 × 45 mm |

## Cost Estimates (as of early 2026)

| | Credstick (5 pcs) | Relay (5 pcs) |
|---|---|---|
| PCB fabrication | ~$8-15 | ~$10-18 |
| ENIG surcharge | ~$5-10 | ~$8-12 |
| Assembly setup | ~$8 | ~$8 |
| SMD components | ~$15-25 | ~$20-35 |
| Shipping (DHL) | ~$20-30 | ~$20-30 |
| **Total** | **~$55-90** | **~$65-105** |

Prices decrease significantly at higher quantities.
