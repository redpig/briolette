# Credstick Firmware

Firmware for the nRF52840-based Briolette credstick.

## Status

**Not yet started.** This directory will contain the firmware implementation
once hardware prototyping begins with rev0.

## Planned Architecture

- **Target**: nRF52840 (ARM Cortex-M4F, 64MHz, 256KB RAM, 1MB flash)
- **Framework**: Zephyr RTOS or Embassy-rs (Rust async bare-metal)
- **Build**: west (Zephyr) or cargo (Embassy)

## Modules (Planned)

```
firmware/
├── src/
│   ├── main.rs              # Entry point, power management
│   ├── apdu.rs              # Briolette APDU protocol handler
│   ├── bls12_381.rs         # BLS12-381 scalar mul, hash-to-curve
│   ├── ecdaa.rs             # ECDAA split-key sign/join operations
│   ├── atecc608b.rs         # I2C driver for secure element
│   ├── nfc.rs               # NFC-A Type 4 Tag, ISO-DEP
│   ├── display.rs           # E-ink SPI driver + UI rendering
│   ├── bloom.rs             # Bloom filter for basename tracking
│   └── storage.rs           # Flash key/state persistence
├── Cargo.toml
└── memory.x                 # Linker script
```

## Shared Code

The BLS12-381 and ECDAA logic should share code with the existing
Briolette Rust crates where possible. The `no_std` subset of the
crypto libraries can be compiled for Cortex-M4.

## Development Workflow

1. Build and flash via USB-C (nRF52840 native USB DFU)
2. Debug via SWD (J-Link or Black Magic Probe) during development
3. Blow APPROTECT fuse for production units (disables SWD permanently)
