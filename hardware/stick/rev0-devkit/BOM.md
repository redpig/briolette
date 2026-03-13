# Rev 0 — Bill of Materials (Dev Kit)

All parts are readily available from major retailers. No MOQ.
Prices as of early 2026; check links for current pricing.

## Option A: XIAO ePaper Kit Base (Recommended)

| # | Component | Part | Qty | ~Price | Source |
|---|-----------|------|-----|--------|--------|
| 1 | MCU + ePaper driver | Seeed XIAO ePaper DIY Kit (EN04, nRF52840) | 1 | $9.90 | [Seeed Studio](https://www.seeedstudio.com/XIAO-ePaper-DIY-Kit-EN04.html) |
| 2 | E-ink display | 1.54" 200x200 ePaper (add-on with EN04 kit) | 1 | $5.85 | [Seeed Studio](https://www.seeedstudio.com/XIAO-ePaper-DIY-Kit-EN04.html) |
| 3 | Secure element | Adafruit ATECC608 Breakout (STEMMA QT) | 1 | $4.95 | [Adafruit #4314](https://www.adafruit.com/product/4314) |
| 4 | I2C cable | STEMMA QT / Qwiic cable 100mm | 1 | $0.95 | [Adafruit](https://www.adafruit.com/product/4210) |
| 5 | NFC antenna | 13.56 MHz flex PCB antenna or hand-wound coil | 1 | $2-5 | [Molex via DigiKey](https://www.digikey.com/) |
| 6 | Battery (optional) | 3.7V LiPo 400mAh (JST-PH connector) | 1 | $6 | [Adafruit](https://www.adafruit.com/) |
|   | **Total** | | | **~$30** | |

## Option B: Standalone XIAO + Separate Display

| # | Component | Part | Qty | ~Price | Source |
|---|-----------|------|-----|--------|--------|
| 1 | MCU | Seeed XIAO nRF52840 Plus | 1 | $9.90 | [Seeed Studio](https://www.seeedstudio.com/Seeed-Studio-XIAO-nRF52840-Plus-p-6359.html) |
| 2 | E-ink display | Waveshare 1.02" 128x80 ePaper Module | 1 | $7 | [Waveshare](https://www.waveshare.com/1.02inch-e-paper-module.htm) |
| 3 | Secure element | Adafruit ATECC608 Breakout (STEMMA QT) | 1 | $4.95 | [Adafruit #4314](https://www.adafruit.com/product/4314) |
| 4 | I2C cable | STEMMA QT / Qwiic cable 100mm | 1 | $0.95 | [Adafruit](https://www.adafruit.com/product/4210) |
| 5 | NFC antenna | 13.56 MHz flex PCB antenna or hand-wound coil | 1 | $2-5 | [Molex via DigiKey](https://www.digikey.com/) |
| 6 | Jumper wires | Female-female dupont wires (SPI for display) | 6 | $2 | Any supplier |
| 7 | Battery (optional) | 3.7V LiPo 400mAh | 1 | $6 | [Adafruit](https://www.adafruit.com/) |
|   | **Total** | | | **~$33** | |

## Notes

- **Option B with Waveshare 1.02"** is closer to the keychain form factor
  target (display is ~34mm x 22mm active area). Good for sizing the final
  product.
- **Option A with 1.54"** is easier to develop against (more pixels, kit
  handles all display wiring).
- The ATECC608 breakout uses I2C address 0x60 by default. The XIAO
  nRF52840's I2C and the ATECC608 are both 3.3V — no level shifter needed.
- The EN04 kit includes a battery connector with charging IC and power
  switch — no extra charging hardware needed.
- For NFC, the XIAO nRF52840 exposes NFC1/NFC2 pads on the bottom.
  You'll need to solder an antenna to these pads. A simple hand-wound
  coil works for bench testing; use a proper flex antenna for range.

## Tools Needed

- Soldering iron (for NFC antenna pads only)
- USB-C cable
- Multimeter (optional, for debugging)
