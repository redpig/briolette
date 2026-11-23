# Rev 1 — Custom PCB

First integrated board. All components on a single PCB targeting a
keychain or USB-stick form factor.

## Target Form Factor

**Keychain fob / thick USB stick:**
- ~60mm x 30mm x 6mm (roughly the size of a car key fob)
- USB-C connector on one end
- E-ink display window on one face
- NFC antenna as PCB trace coil (no external antenna)
- Keyring hole on opposite end from USB-C

```
  ┌─────────────────────────────────────────┐
  │  ○  ┌─────────────────────┐             │
  │ key │                     │  ┌────────┐ │
  │ring │    E-Ink Display    │  │ USB-C  │ │
  │     │     1.02" / 1.54"   │  └────────┘ │
  │     └─────────────────────┘             │
  │     [◄]                 [►]             │
  │    ╔══════════════════════════╗          │
  │    ║   NFC Trace Antenna     ║          │
  │    ╚══════════════════════════╝          │
  └─────────────────────────────────────────┘
         ~60mm x 30mm (top view)
```

The two buttons [◄] [►] sit below the display, aligned with the
"No" and "Yes" labels shown during transaction confirmation. They
also serve as PIN entry (left/right + timing), balance navigation,
and general UI control. See `docs/button-pin-auth.md` for details.

## Layer Stack (4-layer PCB)

| Layer | Purpose |
|-------|---------|
| Top | Components, USB-C, display connector, buttons |
| Inner 1 | Ground plane |
| Inner 2 | Power plane + I2C/SPI routing |
| Bottom | NFC antenna trace coil |

## Block Diagram

```
                    USB-C
                      │
              ┌───────┴───────┐
              │  ESD Protect   │
              │  (IP4292CZ12)  │
              └───────┬───────┘
                      │ VBUS
              ┌───────┴───────┐     ┌──────────────────┐
              │  Charge path   │     │ Supercaps        │
              │  (current-     │─────│ 2× 5F 3V (par.) │
              │  limited)      │     │ = 10F total      │
              └───────────────┘     └────────┬─────────┘
                                             │ VSCAP (2.0-3.0V)
              ┌──────────────────────────────┴─┐
              │  LDO or buck-boost               │
              │  (3.0V → stable 3.3V or direct)  │
              └───────────┬─────────────────────┘
                          │ VCC
              ┌───────────┴───────────┐
              │       nRF52840        │     ┌──────────────┐
              │       (QFN-48)        │─7816│  SIM Card    │
              │                       │     │  (nano-SIM)  │
              │  CryptoCell 310       │     │  push-push   │
              │                       │     └──────────────┘
              │  NFC1/NFC2 ───────────┼──── PCB Trace Antenna
              │                       │
              │  SPI ─────────────────┼──── E-Ink Display (FPC)
              │                       │
              │  GPIO ────────────────┼──── Button(s) + Piezo
              └───────────────────────┘

              ┌───────────────────────┐
              │  Piezo Harvester      │
              │  (cantilever + rect.) │──── VSCAP (trickle)
              └───────────────────────┘
```

## Design Files

KiCad project files will be added here as the design progresses.

```
rev1-pcb/
├── README.md          # This file
├── BOM.md             # Production bill of materials
├── kicad/             # KiCad project (future)
│   ├── stick.kicad_pro
│   ├── stick.kicad_sch
│   └── stick.kicad_pcb
├── gerbers/           # Manufacturing files (future)
└── 3d/                # Enclosure models (future)
```

## Design Decisions

- **nRF52840 QFN-48** (not WLCSP): QFN is hand-solderable with hot air
  for prototyping. WLCSP requires reflow and X-ray inspection.
- **Nano-SIM push-push connector** (Molex 78800-0001): Low-profile
  (1.25mm height), flush-insert push-push mechanism. ISO 7816 interface
  (SIM_IO, SIM_CLK, SIM_RST). User-replaceable SIM card provides the
  same tamper-resistant SE functions as the old ATECC608B (P-256 ECDSA,
  key storage, monotonic counters) but is swappable and field-upgradeable.
  Production-robust: standard connector used in billions of phones.
- **NFC as PCB trace**: Eliminates a discrete antenna component. The
  bottom copper layer has a rectangular spiral trace tuned to 13.56 MHz
  with a matching network (two capacitors).
- **Supercapacitors (2× 5F 3V in parallel = 10F)**: Primary energy
  storage. Infinite cycle life (>1M cycles), no degradation, no fire
  risk, wide temperature range. 25J stored, ~96 transactions per full
  charge. Self-discharge (~5-10%/day) offset by piezo harvesting.
  USB-C recharge takes seconds, not hours.
- **Piezo energy harvester**: Cantilever-mounted piezoelectric element
  converts keychain motion into electrical energy. Passive carry offsets
  supercap self-discharge; deliberate shaking adds ~10-50mJ in 10s.
  Rectified via a Schottky bridge + small buffer cap into VSCAP rail.
- **No battery**: Deliberate choice for infinite device lifetime. No
  capacity fade, no swelling, no replacement needed. The supercap +
  piezo + USB-C combination covers all use cases.
- **1.02" e-ink preferred**: Smaller display fits the keychain target.
  128x80 pixels is enough for balance + 2-line status. The 1.54" is an
  option if we relax to credit-card size.
