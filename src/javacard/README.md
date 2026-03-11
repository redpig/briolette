# Briolette JavaCard Applet

JavaCard applet implementing the card side of Briolette's split-key ECDAA signing
protocol. The applet performs only G1 scalar multiplications and Fr arithmetic on
BN254 — no pairings, no G2/GT operations — making it suitable for constrained
secure element hardware.

## Features

- **Split-key ECDAA signing** (Brickell & Li style): 2-phase commit/respond protocol
- **Blind join protocol**: Card generates its key share and participates in Schnorr
  proof construction without ever exporting its secret key
- **Bloom filter double-spend guard**: Tracks spent basenames to prevent signing the
  same basename twice within an epoch (defense-in-depth)
- **Swap override**: Allows bypassing bloom filter for swap server transactions
- **Version-aware APDU protocol**: P1 byte encodes curve version (BN254/BLS12-381)

## Prerequisites

- **JavaCard SDK 3.0.4+**: Set `JCDK_HOME` environment variable
- **JCMathLib**: For big number arithmetic on BN254
  - Clone from https://github.com/OpenCryptoProject/JCMathLib
  - Build and install: `mvn install -f JCMathLib/pom.xml`
  - Set `JCMATHLIB_HOME` or copy JAR to `libs/`
- **Gradle 7+**
- **JDK 11+**
- **GlobalPlatformPro** (gp.jar): For installing the applet onto a card

## Build

```bash
export JCDK_HOME=/path/to/jcdk
export JCMATHLIB_HOME=/path/to/JCMathLib/dist
gradle build
```

The output CAP file will be in `build/javacard/BrioletteApplet.cap`.

## Install

Using [GlobalPlatformPro](https://github.com/martinpaljak/GlobalPlatformPro):

```bash
java -jar gp.jar --install build/javacard/BrioletteApplet.cap
```

## APDU Protocol

CLA = `0x80`. P1 = curve version (`0x00` = BN254, `0x01` = BLS12-381). P2 = `0x00`.

| INS    | Command              | Input                      | Output                         |
|--------|----------------------|----------------------------|--------------------------------|
| `0x01` | GENERATE_KEY         | —                          | SW 9000                        |
| `0x02` | PUBLIC_KEY_SHARE     | 65B G1 point               | 65B Q_card                     |
| `0x10` | SIGN_COMMIT          | 65B S point                | 65B U_card                     |
| `0x11` | SIGN_COMMIT_BSN      | 130B S ‖ bsn_base          | 195B U_card ‖ K_card ‖ K_u     |
| `0x12` | SIGN_RESPOND         | 32B challenge              | 32B s_card                     |
| `0x13` | SIGN_COMMIT_BSN_SWAP | 130B S ‖ bsn_base          | 195B (skip bloom filter)       |
| `0x20` | JOIN_COMMIT          | 65B base point B           | 65B U_card                     |
| `0x21` | JOIN_RESPOND         | 32B challenge              | 32B s_card                     |
| `0x30` | RESET_BLOOM          | 4B epoch (BE u32)          | SW 9000                        |
| `0x31` | SET_SWAP_PUBKEY      | 65B G1 point               | SW 9000                        |
| `0x40` | GET_STATUS           | —                          | 6B flags‖epoch‖version         |

### Status Words

- `9000`: Success
- `6A80`: Wrong data length
- `6985`: Conditions not satisfied
- `6A84`: Basename already used (bloom filter hit)
- `6A86`: Unsupported curve version
- `6D00`: Unknown INS

## Architecture

```
Host (mobile/desktop)              JavaCard
─────────────────────              ────────
                                   card_sk (never exported)
                                   bloom_filter[1200 bytes]
                                   epoch_counter

1. Key Generation
   GENERATE_KEY ──────────────────→ Generate random card_sk
                                    Store in EEPROM

2. Blind Join (Credential Issuance)
   PUBLIC_KEY_SHARE(B) ───────────→ Q_card = B * card_sk
   Q_card ←───────────────────────
   host generates Q_host = B * host_sk
   Q = Q_card + Q_host

   JOIN_COMMIT(B) ────────────────→ r_card = random()
   U_card ←───────────────────────  U_card = B * r_card
   host generates U_host = B * r_host
   U = U_card + U_host
   c = H(U, B, Q, nonce)

   JOIN_RESPOND(c) ───────────────→ s_card = r_card + c * card_sk
   s_card ←───────────────────────
   s = s_card + s_host
   pk = (Q, c, s, n) → send to issuer

3. Split-Key Signing (Token Transfer)
   SIGN_COMMIT_BSN(S, bsn) ──────→ Check bloom filter
                                    r_card = random()
   U_card,K_card,K_u_card ←──────  Compute 3 scalar muls
   host computes its shares
   c = H(combined values)

   SIGN_RESPOND(c) ───────────────→ s_card = r_card + c * card_sk
   s_card ←───────────────────────
   sig = combine(s_card + s_host, ...)
```

## Implementation Notes

The applet uses placeholder stubs for EC point multiplication and modular arithmetic.
A production implementation must integrate JCMathLib:

- `ECPoint.multiplication(BigNat)` for G1 scalar multiplication
- `BigNat.modMult()` for Fr multiplication
- `BigNat.modAdd()` for Fr addition

The bloom filter uses 1200 bytes of EEPROM with 7 hash functions derived from SHA-256,
targeting ~1% false positive rate for 1000 basenames per epoch.

## Testing

For simulator testing, use [jCardSim](https://jcardsim.org/):

```bash
# Add jCardSim to classpath and run unit tests
gradle test
```

For physical card testing, use the Rust NFC transport:

```bash
cd ../../  # briolette root
cargo test -p briolette-crypto --features nfc
```
