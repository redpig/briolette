# Briolette Stick ("Credstick")

A low-power, NFC-enabled device for Briolette anonymous digital cash.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Credstick                       │
│                                                  │
│  ┌───────────┐   I2C   ┌────────────┐           │
│  │ nRF52840  │────────▶│ ATECC608B  │           │
│  │           │         │ (SE keys)  │           │
│  │ BLE+NFC   │         └────────────┘           │
│  │ CryptoCell│   SPI   ┌────────────┐           │
│  │ BLS12-381 │────────▶│  E-Ink     │           │
│  │ (software)│         │  Display   │           │
│  └─────┬─────┘         └────────────┘           │
│        │ USB                                     │
│  ┌─────┴─────┐         ┌────────────┐           │
│  │  USB-C    │         │ Supercap + │           │
│  │  (charge  │         │ NFC energy │           │
│  │  + flash) │         │ harvesting │           │
│  └───────────┘         └────────────┘           │
│                                                  │
│  ┌──────────────────────────────────┐           │
│  │       NFC Antenna (PCB trace)    │           │
│  └──────────────────────────────────┘           │
└─────────────────────────────────────────────────┘
```

## Key Functions

- **NFC-A tag**: Phone taps the stick to initiate token transfers
- **ECDAA signing**: BLS12-381 split-key operations (software, ~2-3s)
- **Manufacturer attestation**: P-256 ECDSA via ATECC608B
- **Display**: E-ink shows balance, last transaction, status (zero standby power)
- **Power**: Supercapacitor charged from NFC field or USB-C; e-ink retains image unpowered

## Board Revisions

### [rev0-devkit/](rev0-devkit/) — Piecemeal Dev Kit
Off-the-shelf development boards wired together for prototyping.
No custom PCB required. Buy, wire, flash, and go.

### [rev1-pcb/](rev1-pcb/) — Custom PCB v1
First integrated board. All components on a single PCB.
Target: credit-card or USB-stick form factor.

## Firmware

### [firmware/](firmware/)
Zephyr RTOS / Embassy-rs firmware for the nRF52840.
Implements Briolette APDU protocol, BLS12-381 signing, display driver,
and ATECC608B secure element interface.

## Docs

### [docs/](docs/)
Design notes, power analysis, antenna design, and security considerations.
