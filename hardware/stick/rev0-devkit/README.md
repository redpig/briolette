# Rev 0 — Piecemeal Dev Kit

Off-the-shelf boards wired together for firmware development and protocol
testing. No soldering required beyond headers and jumper wires.

## Target Form Factor

Benchtop prototype. Individual boards connected with jumper wires or
STEMMA QT / Qwiic cables. Not pocketable — this is for firmware bringup.

## Assembly

```
                    Jumper Wires
  ┌──────────┐     ┌──────────┐     ┌──────────────┐
  │  XIAO    │─I2C─│ ATECC608 │     │  ePaper Kit  │
  │ nRF52840 │     │ Breakout │     │  (EN04 base  │
  │  Plus    │─SPI─│          │     │  + display)  │
  │ (USB-C)  │     └──────────┘     │              │
  │ (NFC)    │──────────────────────│              │
  └──────────┘                      └──────────────┘
       │
    USB-C to host (power + debug)
```

### Connections

| Signal | XIAO Pin | ATECC608 Breakout |
|--------|----------|-------------------|
| SDA    | D4 (SDA) | SDA               |
| SCL    | D5 (SCL) | SCL               |
| VCC    | 3V3      | VIN               |
| GND    | GND      | GND               |

The ePaper driver board (EN04) already has the XIAO nRF52840 integrated,
so for rev0 you have two options:

**Option A (simplest):** Use the EN04 kit as the base board (it includes
a XIAO nRF52840), and wire the ATECC608 breakout to its I2C pins.
The display is already connected. NFC antenna pads are on the XIAO.

**Option B (separate):** Use a standalone XIAO nRF52840 Plus with the
ATECC608 breakout, and a separate small e-ink display module wired
over SPI. More flexible but more wiring.

**Recommended: Option A** — fewest connections, fastest to working prototype.

## NFC Antenna

The XIAO nRF52840 has NFC antenna pads but no built-in antenna.
For rev0, solder a small NFC coil antenna or use a flex PCB antenna:
- Cut a ~4-turn rectangular coil from 28 AWG magnet wire (~35mm x 35mm)
- Or buy a 13.56 MHz flex antenna (e.g., Molex 1462360011)

## What You Can Test With This

- ATECC608B key generation and P-256 attestation
- BLS12-381 software signing performance on nRF52840
- E-ink display rendering (balance, QR codes, status)
- NFC-A tag communication with a phone
- Power consumption profiling
- Full Briolette APDU protocol over NFC
