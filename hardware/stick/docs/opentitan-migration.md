# OpenTitan Migration Plan

## The Problem: 2-3s ECDAA Signing

The credstick's BLS12-381 ECDAA split-key signature takes **2-3 seconds** in
software on the nRF52840's 64MHz Cortex-M4. This is an order of magnitude
slower than the target: **Visa contactless tap-and-pay speed (~500ms
end-to-end)**. The e-ink display can show "Signing..." but a 3-second
NFC hold is not competitive with existing payment infrastructure.

The bottleneck is G1 scalar multiplication on BLS12-381's 381-bit prime
field — thousands of modular multiplications that the Cortex-M4 handles
one 32-bit word at a time.

No commercial chip currently accelerates BLS12-381 in hardware. OpenTitan's
OTBN (programmable big-number accelerator with 256-bit wide registers) is
the first realistic path to hardware-accelerated pairing-friendly curve
operations in an open-source, auditable secure element.

## Current Architecture

```
Phone ◄──NFC──▶ nRF52840 (64MHz Cortex-M4)
                    │
                    ├── BLS12-381 ECDAA signing (SOFTWARE, 2-3s)
                    ├── ECDAA secret key (flash, APPROTECT only)
                    │
                    └──I2C──▶ ATECC608B ($0.55, UDFN-8)
                                ├── P-256 ECDSA (manufacturer attestation)
                                ├── Monotonic counters (PIN, epoch)
                                └── Certificate storage (72 bytes)
```

**Key weaknesses:**
1. BLS12-381 in software = 2-3s per signature (unacceptable UX)
2. ECDAA secret key in nRF52840 flash = vulnerable to glitching attacks
3. ATECC608B provides no help with BLS12-381 (P-256 only)

## Target Architecture (with OpenTitan)

```
Phone ◄──NFC──▶ nRF52840 (64MHz Cortex-M4)
                    │
                    └──SPI──▶ OpenTitan Earl Grey (100MHz RISC-V)
                                ├── OTBN: BLS12-381 G1 scalar mul (TARGET: <500ms)
                                ├── ECDAA secret key (DICE key manager, tamper-resistant)
                                ├── P-256 ECDSA (replaces ATECC608B attestation)
                                ├── Monotonic counters (OTP fuses)
                                ├── Certificate storage (1MB e-flash)
                                └── Future: PQC secure boot (SPHINCS+)
```

**What changes:**
- ATECC608B is removed entirely — OpenTitan subsumes all its functions
- BLS12-381 G1 scalar multiplication moves from nRF52840 software → OTBN hardware
- ECDAA secret key moves from nRF52840 flash → OpenTitan key manager (tamper-resistant)
- SPI replaces I2C (higher throughput for BLS12-381 payloads)

**What stays the same:**
- nRF52840 remains the main MCU (NFC tag mode, display, buttons, power)
- Phone still holds the other half of the split key
- APDU protocol unchanged — the credstick's external behavior is identical
- Briolette protocol (INITIATE, TRANSACT, TRANSFER, etc.) unchanged

## Performance Target

| Operation | Current (nRF52840 SW) | Target (OTBN) | Visa Contactless |
|-----------|----------------------|---------------|-----------------|
| G1 scalar mul | ~2000ms | <200ms | N/A |
| Full ECDAA sign | ~2500ms | <400ms | N/A |
| End-to-end tap | ~3500ms | <700ms | ~500ms |

The OTBN has 256-bit wide registers and runs at 100MHz. BLS12-381's
381-bit field fits in two 256-bit words. A single Fp multiplication
should take ~10-20 cycles on OTBN vs ~500+ cycles in Cortex-M4 software.
Conservative estimate: **10-20× speedup** on the scalar multiplication.

## OpenTitan OTBN: Why It's Suitable

The OTBN (OpenTitan Big Number accelerator) is a special-purpose coprocessor:

- **256-bit wide data path**: BLS12-381 Fp elements (381 bits) fit in two
  wide registers. The Cortex-M4 needs 12+ 32-bit registers.
- **Programmable**: Not fixed-function like ESP32's RSA accelerator. Custom
  firmware (OTBN assembly) can implement any modular arithmetic.
- **Constant-time by design**: No data-dependent timing — critical for
  side-channel resistance during ECDAA signing.
- **Isolated execution**: OTBN has its own instruction and data memory,
  separate from the main Ibex core. Key material stays within OTBN.

### What needs to be written (OTBN firmware)

1. **Fp381 modular multiplication** (Montgomery form)
2. **Fp381 modular addition/subtraction**
3. **Elliptic curve point addition** (short Weierstrass, BLS12-381 G1)
4. **Scalar multiplication** (double-and-add with constant-time ladder)
5. **Fr arithmetic** (scalar field, 255 bits — fits in a single wide register)

No pairing operations needed on the OTBN — the credstick only does G1
scalar mul and Fr arithmetic (split-key design avoids G2/GT/pairing on
the constrained side).

## SPI Interface Protocol

Communication between nRF52840 and OpenTitan over SPI:

```
Request frame:
  [1B cmd] [2B payload_len] [payload...] [2B CRC16]

Response frame:
  [1B status] [2B payload_len] [payload...] [2B CRC16]

Commands:
  0x01  SIGN_P256       — Sign 32-byte challenge with manufacturer key
  0x02  READ_CERT       — Read manufacturer certificate
  0x03  INC_COUNTER     — Increment monotonic counter (PIN/epoch)
  0x04  READ_COUNTER    — Read monotonic counter value
  0x10  ECDAA_SCALAR_MUL — G1 scalar multiplication (48B point + 32B scalar → 48B result)
  0x11  ECDAA_FR_MUL    — Fr multiplication (32B × 32B → 32B)
  0x12  ECDAA_FR_ADD    — Fr addition (32B + 32B → 32B)
  0x13  ECDAA_COMMIT    — Full ECDAA commitment (randomize + scalar mul)
  0x14  ECDAA_RESPOND   — Full ECDAA response (r + c*sk computation)
  0x20  GET_PUBLIC_KEY  — Derive public key from stored ECDAA secret
  0xFF  PING            — Health check / wake from sleep
```

The high-level commands (0x13 COMMIT, 0x14 RESPOND) keep the ECDAA secret
key entirely within OpenTitan — the nRF52840 never sees `card_sk`.

## Migration Steps

### Phase 1: OTBN BLS12-381 Firmware (can start now on FPGA emulator)

**Files to create:**
- `hardware/opentitan/otbn/fp381_mul.s` — Fp381 Montgomery multiplication
- `hardware/opentitan/otbn/fp381_add.s` — Fp381 addition/subtraction
- `hardware/opentitan/otbn/g1_point.s` — G1 point add/double
- `hardware/opentitan/otbn/scalar_mul.s` — Constant-time scalar multiplication
- `hardware/opentitan/otbn/fr_arith.s` — Fr field arithmetic
- `hardware/opentitan/otbn/ecdaa.s` — High-level ECDAA operations
- `hardware/opentitan/tests/` — Test vectors from `src/crypto/src/v1.rs`

**Development path:** OTBN has a cycle-accurate simulator (`otbn_sim`) that
runs on a host machine. No hardware needed to start development and
benchmarking.

### Phase 2: OpenTitan Application Firmware

**Files to create:**
- `hardware/opentitan/app/main.c` — Ibex main loop: SPI command dispatch
- `hardware/opentitan/app/spi_protocol.c` — SPI device interface handler
- `hardware/opentitan/app/key_manager.c` — DICE key manager integration
- `hardware/opentitan/app/attestation.c` — P-256 manufacturer attestation
- `hardware/opentitan/app/counter.c` — OTP-backed monotonic counters

### Phase 3: nRF52840 Driver

**Files to modify:**
- Replace `hardware/stick/firmware/src/atecc608b.rs` → new `opentitan.rs`
  - SPI instead of I2C
  - New command protocol (see above)
  - Higher-level ECDAA API: `commit()`, `respond()`, `get_public_key()`

- Modify `hardware/stick/firmware/src/ecdaa.rs`
  - Remove software BLS12-381 scalar multiplication
  - Replace with `opentitan::ecdaa_commit()` and `opentitan::ecdaa_respond()`
  - The nRF52840 still orchestrates the ECDAA protocol; OpenTitan is the
    crypto coprocessor

- Modify `hardware/stick/firmware/src/storage.rs`
  - ECDAA key no longer stored in nRF52840 flash
  - Key provisioning happens via OpenTitan key manager
  - `storage.rs` tracks non-secret state only (balance, tokens, bloom)

### Phase 4: Hardware Integration

**Files to modify:**
- `hardware/stick/rev1-pcb/BOM.md` — Remove ATECC608B, add OpenTitan
- KiCad schematic — SPI bus to OpenTitan, remove I2C to ATECC608B
- KiCad PCB — new component placement (OpenTitan is larger than ATECC608B)
- `hardware/stick/docs/design-overview.md` — updated architecture

**Hardware considerations:**
- OpenTitan package TBD (likely QFN, significantly larger than UDFN-8)
- Power supply: OpenTitan active current TBD, likely higher than ATECC608B
- Board space: may require PCB redesign or larger form factor
- SPI pins: nRF52840 has multiple SPI peripherals available

## Power Budget Impact

| Component | Current Design | With OpenTitan | Notes |
|-----------|---------------|----------------|-------|
| ATECC608B sleep | ~30nA | Removed | — |
| ATECC608B active | 15mA × 50ms | Removed | — |
| OpenTitan sleep | N/A | ~TBD (est. 1-10µA) | Likely higher than ATECC608B |
| OpenTitan active | N/A | ~TBD (est. 20-50mA) | OTBN at 100MHz |
| OTBN duration | N/A | <200ms (target) | vs 2000ms software |
| nRF52840 BLS12-381 | 15mA × 2000ms = 30mAs | ~5mA × 200ms = 1mAs | Idle while OTBN works |
| **Net per transaction** | **~78mAs** | **~55-65mAs (est.)** | **Faster = less total energy** |

Even if OpenTitan draws more instantaneous current, the 10× shorter
signing time likely results in **less total energy per transaction**.

## Availability Timeline

| Milestone | Status | Est. Date |
|-----------|--------|-----------|
| OTBN simulator available | Done | Now |
| CW310 FPGA dev board | Available | Now |
| zeroRISC early access chips | Shipping | Now (NDA) |
| Discrete chips on Digikey/Mouser | Not yet | Late 2026–2027 |
| Briolette OTBN BLS12-381 firmware | Not started | — |
| Briolette OpenTitan app firmware | Not started | — |
| nRF52840 driver swap | Not started | — |

## What Can Start Today

1. **OTBN firmware development** using the cycle-accurate simulator —
   no hardware needed. Write and benchmark Fp381 modular multiplication.

2. **Test vector generation** from the existing `src/crypto/src/v1.rs`
   Rust implementation — create known-answer tests for the OTBN firmware.

3. **SPI protocol design** — the nRF52840 driver can be written against
   a mock SPI responder for integration testing.

4. **CW310 FPGA prototyping** — if an FPGA board is available, run the
   full OpenTitan design with real OTBN hardware.

## Why Not Other Approaches

| Alternative | Speed | Why Not |
|-------------|-------|---------|
| Faster MCU (STM32H7 480MHz) | ~500-700ms | No NFC tag mode. Loses key form factor advantage. |
| FPGA coprocessor (ECP5) | ~100-300ms (est.) | No tamper resistance. Power hungry. No key manager. |
| TPM 2.0 DAA | ~144ms | BN256 only (weak security). No mobile phone support. |
| Accept 2-3s | 2-3s | Outside Visa tap-and-pay usability window. |
| **OpenTitan OTBN** | **<400ms (target)** | **Best option: fast + secure + open-source + key management** |

## References

- [OpenTitan Earl Grey Datasheet](https://opentitan.org/book/hw/top_earlgrey/doc/datasheet.html)
- [OTBN Programmer's Guide](https://opentitan.org/book/hw/ip/otbn/)
- [OTBN ISA Reference](https://opentitan.org/book/hw/ip/otbn/doc/isa.html)
- [OpenTitan Key Manager](https://opentitan.org/book/hw/ip/keymgr/)
- [zeroRISC Commercial Access](https://www.zerorisc.com/)
- [MIT BLS12-381 Pairing Crypto-Processor](https://arxiv.org/pdf/2201.07496)
  (reference for expected performance characteristics)
- [High-Performance BLS12-381 on FPGA (IEEE)](https://ieeexplore.ieee.org/document/10396122/)
