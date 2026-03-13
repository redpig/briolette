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
  │                                          │
  │    ╔══════════════════════════╗          │
  │    ║   NFC Trace Antenna     ║          │
  │    ╚══════════════════════════╝          │
  └─────────────────────────────────────────┘
         ~60mm x 30mm (top view)
```

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
              │  ESD + Charge  │
              │  (IP4292CZ12)  │
              │  + MCP73831    │
              └───────┬───────┘
                      │ VBUS/VBAT
              ┌───────┴───────┐     ┌────────────┐
              │   nRF52840    │─I2C─│  ATECC608B │
              │   (QFN-48)    │     │  (UDFN-8)  │
              │               │     └────────────┘
              │  CryptoCell   │
              │  NFC1/NFC2 ───┼──── PCB Trace Antenna
              │               │
              │  SPI ─────────┼──── E-Ink Display (FPC)
              │               │
              │  GPIO ────────┼──── Button(s)
              └───────────────┘
                      │
              ┌───────┴───────┐
              │  Supercap     │
              │  (100mF 3.3V) │
              └───────────────┘
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
- **ATECC608B UDFN-8** (2x3mm): Tiny footprint, I2C only. The SOIC-8
  variant works too if hand-soldering is preferred.
- **NFC as PCB trace**: Eliminates a discrete antenna component. The
  bottom copper layer has a rectangular spiral trace tuned to 13.56 MHz
  with a matching network (two capacitors).
- **Supercapacitor**: Allows the device to complete a transaction even
  if the NFC field drops briefly. Also buffers the e-ink refresh surge
  current (~40mA peak for ~1s).
- **1.02" e-ink preferred**: Smaller display fits the keychain target.
  128x80 pixels is enough for balance + 2-line status. The 1.54" is an
  option if we relax to credit-card size.
