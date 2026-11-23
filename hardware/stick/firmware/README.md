# Credstick Firmware

Firmware for the nRF52840-based Briolette credstick.

## Status

**Initial implementation.** Core module structure and protocol state machine
are in place. BLS12-381 scalar multiplication, NFC PAC register access, flash
I/O, and SPI display driver need hardware bring-up to complete.

## Architecture

- **Target**: nRF52840 (ARM Cortex-M4F, 64MHz, 256KB RAM, 1MB flash)
- **Framework**: Embassy-rs (Rust async bare-metal)
- **Build**: `cargo build --release` (cross-compiled to `thumbv7em-none-eabihf`)

## Modules

```
firmware/
├── src/
│   ├── main.rs           # Embassy entry point, event loop, power management
│   ├── apdu.rs           # Briolette APDU protocol handler + state machine
│   │                     # Mirrors receiver.proto: INITIATE/TRANSACT/TRANSFER
│   ├── ecdaa.rs          # ECDAA split-key signing (BLS12-381 G1 operations)
│   ├── atecc608b.rs      # I2C driver for ATECC608B secure element
│   ├── nfc.rs            # NFC-A Type 4 Tag (ISO-DEP) via nRF52840 NFCT
│   ├── display.rs        # E-ink SPI driver + UI screen rendering
│   ├── bloom.rs          # Bloom filter for basename double-spend tracking
│   ├── button.rs         # L/R button handler for PIN entry + navigation
│   └── storage.rs        # Flash persistence (log-structured, wear-leveled)
├── Cargo.toml
├── build.rs              # Linker script setup
├── memory.x              # nRF52840 memory layout
└── .cargo/
    └── config.toml       # Cross-compilation target + probe-rs runner
```

## APDU Protocol

The firmware implements the credstick side of the Briolette payment protocol.
APDUs mirror `receiver.proto` RPCs directly:

| APDU | INS | Maps To | Purpose |
|------|-----|---------|---------|
| INITIATE | 0x10 | `Initiate` | Receive payment proposal |
| READ_TICKET | 0x11 | — | Return credstick's SignedTicket |
| GOSSIP | 0x12 | `Gossip` | Exchange epoch updates |
| TRANSACT | 0x20 | `Transact` | Propose unsigned tokens |
| TRANSFER | 0x30 | `Transfer` | Sign and commit tokens |
| RECEIVE | 0x31 | — | Accept incoming signed tokens |
| SWEEP | 0x50 | — | Return tokens for merchant collection |
| GET_BALANCE | 0x51 | — | Return current balance |

## Transaction Flow

Two modes, both avoiding button presses during NFC contact:

### 2-Tap (Fast Mode)
1. **Tap 1**: INITIATE + TRANSACT → credstick shows amount, returns unsigned tokens
2. **Between taps**: User reads e-ink, enters PIN if needed (off reader)
3. **Tap 2**: TRANSFER → credstick signs and returns signatures

### 3-Tap (Private Mode)
1. **Tap 1**: INITIATE only → credstick shows amount (no tokens revealed)
2. **Tap 2**: TRANSACT → returns unsigned tokens (long hold can merge with tap 3)
3. **Tap 3**: TRANSFER → sign and commit

## TODO (Hardware Bring-Up)

- [ ] BLS12-381 G1 scalar multiplication on Cortex-M4F (currently placeholder)
- [ ] nRF52840 NFCT PAC register access (ISO-DEP / T4T)
- [ ] SPI e-ink display driver (SSD1681/IL0373 controller)
- [ ] Flash I/O via NVMC (log-structured storage)
- [ ] Hardware RNG for ephemeral scalars
- [ ] Argon2id PIN hashing (reduced parameters for 256KB RAM)
- [ ] Power management (System OFF with NFC field detect wake)
- [ ] ATECC608B integration testing on real hardware

## Shared Code

The BLS12-381 and ECDAA logic reuses `bls12_381_plus` (same crate as
`briolette-crypto::v1`) compiled in `no_std` mode for Cortex-M4F. The
`SmartCard` trait interface from `briolette-crypto::v1::split` maps
directly to the `ecdaa.rs` functions:

| SmartCard trait | Firmware function |
|-----------------|-------------------|
| `public_key_share()` | `ecdaa::public_key_share()` |
| `sign_commit()` | `ecdaa::sign_commit()` |
| `sign_respond()` | `ecdaa::sign_respond()` |
| `join_commit()` | `ecdaa::join_commit()` |
| `join_respond()` | `ecdaa::join_respond()` |

## Development Workflow

1. Build: `cargo build --release`
2. Flash via USB-C: `probe-rs run --chip nRF52840_xxAA target/thumbv7em-none-eabihf/release/briolette-credstick`
3. Debug via SWD (J-Link or Black Magic Probe) during development
4. View logs: `probe-rs attach --chip nRF52840_xxAA` (defmt-rtt)
5. Production: blow APPROTECT fuse to disable SWD permanently
