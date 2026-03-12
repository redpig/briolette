# Briolette Security Audit Report

**Date:** 2026-03-11
**Scope:** Full codebase review — cryptographic protocols, server infrastructure,
wallet implementation, JavaCard applet, mobile FFI, and formal verification.
**Version:** 0.1.0 (experimental/research prototype)
**Repository:** github.com/google/briolette
**License:** Apache 2.0

---

## Executive Summary

Briolette is an experimental framework for researching offline digital currency
(CBDC-like) designs. It implements a direct-exchange token system using ECDAA
(Elliptic Curve Direct Anonymous Attestation) group signatures for anonymous yet
accountable transfers, with optional JavaCard-based split-key signing for
hardware-backed security.

The project is explicitly research-grade and not production-ready. This audit
identifies **Critical**, **High**, **Medium**, and **Low** severity findings,
along with informational observations. Findings are prioritized by exploitability
and impact in the context of eventual production deployment.

### Summary of Findings

| Severity     | Count |
|--------------|-------|
| Critical     |     1 |
| High         |     2 |
| Medium       |    11 |
| Low          |     7 |
| Informational|     9 |

---

## 1. System Architecture Overview

### 1.1 Components

| Component     | Language  | Role |
|---------------|-----------|------|
| Registrar     | Rust      | Issues ECDAA credentials to wallets (network + transfer) |
| Clerk         | Rust      | Issues time-limited signed tickets (ECDSA P-256), manages epochs |
| Mint          | Rust      | Creates tokens backed by signed tickets |
| Validator     | Rust      | Verifies token chains and checks tokenmap for double-spend |
| TokenMap      | Rust      | SQLite-backed double-spend detection and fork analysis |
| Swapper       | Rust      | Exchanges expired/invalid tokens for fresh ones |
| Bridge        | Rust      | Ethereum/Bitcoin deposit bridge (L1 interop) |
| Wallet        | Rust      | Client library + CLI for token management |
| Mobile FFI    | Rust      | UniFFI bridge exposing wallet to Kotlin/Swift |
| JavaCard      | Java      | Split-key ECDAA signing with bloom filter double-spend prevention |
| Android App   | Kotlin    | KMP + Compose Multiplatform mobile wallet |
| iOS App       | Swift     | SwiftUI + KMP framework mobile wallet |

### 1.2 Cryptographic Primitives

- **ECDAA (v0):** BN254 pairing-friendly curve via `substrate-bn` (~100-bit security)
- **ECDAA (v1):** BLS12-381 via `bls12_381_plus` (128-bit security, RFC 9380 hash-to-curve)
- **Token signatures:** ECDSA P-256 (`p256` crate) with recovery ID for mint/clerk signing
- **Hashing:** SHA-256 for all hash operations
- **Randomness:** `OsRng` (system CSPRNG) for all random generation
- **Token binding:** SHA-256 of descriptor → chained through transfer history signatures

### 1.3 Protocol Flow

1. **Registration:** Wallet generates ECDAA keypair → sends to Registrar → receives group credential
2. **Ticket acquisition:** Wallet signs ticket request with NAC credential → Clerk issues time-limited ECDSA-signed tickets
3. **Token minting:** Wallet presents ticket + amount to Mint → receives tokens with signed base history
4. **Transfer:** Sender signs new history entry (ECDAA) with recipient's ticket → token chain extends
5. **Validation:** Tokens verified cryptographically (chain integrity) + checked against TokenMap (double-spend)
6. **Epoch management:** Clerk publishes signed EpochUpdate with key material, service URIs, and revocation data

---

## 2. Critical Findings

### C-1: No Transport Confidentiality (No TLS)

**Severity:** Medium (reduced from Critical — application-layer authentication
exists; the gap is confidentiality, not authentication)
**Location:** All gRPC client/server connections (`src/proto/src/lib.rs:37`, all `*_main.rs`)

All inter-service gRPC communication uses plaintext channels. The `multiconnect()`
helper in `BrioletteClientHelper` connects via either TCP or UNIX domain sockets
with no TLS configuration.

However, briolette does not rely on TLS for authentication. Application-layer
cryptographic authentication is already in place for all security-critical RPCs:

- **Clerk GetTickets / RefreshTickets:** Wallet signs requests with its NAC
  credential (ECDAA, epoch-bound basename). The clerk verifies the NAC
  signature, checks TTC group membership, and looks up ticket policy by NAC
  group public key (`clerk/src/server.rs:199-213`, `328-351`).
- **Mint GetTokens:** Wallet presents a clerk-signed ticket; the mint verifies
  the ticket signature against its trusted ticket signing keys and confirms
  the credential belongs to the TTC group (`mint/src/server.rs:107-120`).
- **Registrar RegisterCall:** Hardware attestation (Android KM or iOS App
  Attest) with cryptographic binding of ECDAA keys to the attestation
  challenge (`registrar/src/attestation.rs`).
- **Peer-to-peer transfers:** Token history is ECDAA-signed; recipients verify
  the full signature chain and credential-to-ticket binding.
- **Intentionally public endpoints:** GetEpoch (epoch data is gossipped
  between peers anyway) and ValidateTokens (tokens are not secret; merchants
  must be able to validate before accepting) require no authentication by
  design.

Using TLS mutual authentication (mTLS with per-device certificates) would be
counterproductive — the purpose of NAC-based authentication is that wallets
prove group membership via ECDAA without revealing individual identity. mTLS
would undermine this privacy property.

**Impact:** Without TLS, a network-level eavesdropper can:
- Correlate IP addresses to ticket bundles and token transfers (privacy leak)
- Observe registration traffic linking hardware attestation to IP addresses
- Potentially perform traffic analysis to link wallets across sessions

A network-level MITM could attempt to modify messages in transit, but
application-layer signatures (NAC over requests, ECDAA over token history)
would cause verification failures at the receiver.

**Recommendation:** Add TLS for confidentiality (server-authenticated TLS, not
mTLS). This protects against eavesdroppers correlating network identifiers to
wallet activity. At minimum, deploy with TLS-terminating proxies; for direct
integration use `tonic::transport::ServerTlsConfig`. Do not add client
certificate authentication — wallet authentication must remain at the
application layer via NAC/ECDAA to preserve unlinkability.

### C-2: Algorithm::NONE Still Accepted at Registration

**Severity:** Critical (reduced from original — attestation is implemented but
NONE fallback remains)
**Location:** `src/registrar/src/server.rs:215-250`, `src/proto/proto/registrar.proto:77-94`

Hardware attestation is now implemented for both Android Key Attestation
(`ANDROID_KM_ATTESTATION`) and Apple App Attest (`IOS_APP_ATTEST`). The
registrar verifies certificate chains, attestation challenges, and
cryptographically binds ECDAA public keys to the attestation
(`src/registrar/src/attestation.rs`). Attestation results drive a tiered NAC
issuer system: the registrar maintains per-security-level NAC keypairs
(Low/Medium/High) and selects the issuer based on attestation strength
(`server.rs:154-196`, `server.rs:327-352`). Hardware-backed attestation
(TEE/StrongBox or App Attest) caps at Medium; reaching High additionally
requires a smartcard split-key proof (`SplitKeyProof` in `registrar.proto:131-138`).

The remaining risk is that `Algorithm::NONE` is still accepted as a fallback,
which assigns `SecurityLevel::Low`. In production, NONE registrations should
be rejected or heavily restricted (e.g., online-only tickets with lifetime 0-1
epochs via the `GroupPolicy` mechanism in `clerk.proto:91-100`).

```
// registrar.proto:
enum Algorithm {
  NONE = 0;       // Fallback — assigns Low security tier
  ANDROID_KM_ATTESTATION = 1;  // Implemented (attestation.rs)
  IOS_APP_ATTEST = 2;          // Implemented (attestation.rs)
}
```

**Impact:** With NONE still accepted, Sybil attacks remain possible but are now
mitigated by the tiered system: NONE wallets receive Low-tier NAC credentials
with short ticket lifetimes, limiting their exposure window. Attested wallets
receive Medium or High tier credentials with longer lifetimes.

**Recommendation:** For production, either reject `Algorithm::NONE` entirely or
configure the clerk's `GroupPolicy` to give the Low-tier NAC group a ticket
lifetime of 0 (online-only), effectively requiring attestation for offline
transacting.

---

## 3. High Findings

### H-1: BN254 Curve Provides Insufficient Security (v0)

**Severity:** High
**Location:** `src/crypto/src/v0.rs`

The v0 ECDAA implementation uses BN254 (also called FP256BN), which provides
approximately 100-bit security after the Kim-Barbulescu attack on small-
characteristic pairings. This is below the NIST-recommended 128-bit minimum.

The codebase acknowledges this in `src/crypto/src/v1.rs:18`:
> "128-bit security (vs ~100-bit for BN254 post Kim-Barbulescu)"

However, v0 remains the default used by all server components (`use
briolette_crypto::v0` in registrar, clerk, mint, etc.) and the JavaCard applet.
v1 (BLS12-381) exists but is not wired into the protocol.

**Impact:** The primary cryptographic primitive is below modern security
thresholds. While not immediately exploitable, it reduces the long-term security
margin, especially for a financial system.

**Recommendation:** Complete the migration to v1 (BLS12-381). Update all server
components and the JavaCard applet to use v1 as the default, with v0 as a
deprecated fallback.

### H-2: JavaCard EC Math Implemented but Not Yet Tested on Hardware

**Severity:** Medium (reduced from High — stubs replaced with real implementations)
**Location:** `src/javacard/applet-sim/ECMath.java`, `src/javacard/applet-hw/ECMath.java`

The previously stubbed EC math operations have been replaced with two real
implementations selected at build time:

- **Simulator (`applet-sim/ECMath.java`):** Uses `java.math.BigInteger` for
  affine BN254 arithmetic — double-and-add scalar multiplication, point
  addition/doubling, `scalarMulAdd` (a + b*c mod r), scalar negation, and
  modular reduction. This is functional for jCardSim testing.
- **Hardware (`applet-hw/ECMath.java`):** Uses JCMathLib's `ECPoint` and
  `BigNat` classes, which leverage the card's RSA engine for modular arithmetic
  and the ECDH engine for scalar multiplication. Build with
  `gradle -PUSE_JCMATHLIB=true`.

The `BrioletteApplet` itself delegates all EC operations to `ECMath.*` static
methods (`BrioletteApplet.java:704-731`), so the same applet code works with
either backend.

**Remaining concerns:**
- The JCMathLib backend has not been tested on physical JavaCard hardware.
  Card-specific `OperationSupport.setCard()` configuration is needed per target.
- `verifySwapAuthSchnorr()` implementation status should be verified.
- The simulator backend uses `BigInteger` which is non-constant-time; this is
  acceptable for testing but the JCMathLib backend inherits the card's native
  constant-time properties.

**Recommendation:** Test the JCMathLib build on target JavaCard hardware
(e.g., JCOP4 P71) and verify all split-key protocol operations end-to-end.
Run the existing jCardSim test suite against both backends.

### H-3: Service-to-Service Auth Uses Deterministic Nonce

**Severity:** High
**Location:** `src/service_auth/src/lib.rs:65-68`

The service identity keypair is generated using a deterministic nonce derived
from the service group name:

```rust
let nonce = format!("service-{}", group.as_str_name()).into_bytes();
v0::generate_wallet_keypair(&nonce, &mut secret_key, &mut public_key);
```

While `generate_wallet_keypair` uses `OsRng` for the secret key (which is fine),
the public key's Schnorr proof is bound to this deterministic nonce. More
critically, the hardware ID for service registration is also deterministic:

```rust
let hw_id = Sha256::digest(&nonce).to_vec();
```

And the HWID signature uses `Algorithm::None`:
```rust
hwid_signature: Some(Signature {
    algorithm: Algorithm::None.into(),
    signature: vec![],
    public_key: vec![],
}),
```

**Impact:** Any entity that knows the service group name can register as that
service and obtain valid NAC credentials. Combined with C-1 (no TLS), this
allows impersonation of any Briolette service.

**Recommendation:** Use unique, randomly-generated service identities with
proper credential management (e.g., HSM-backed keys, rotation).

### H-4: Bloom Filter False Positives Recoverable via Swap but Path Not Wired

**Severity:** Medium (reduced from High — architectural recovery mechanism exists)
**Location:** `src/javacard/applet/BloomFilter.java:28-30`,
`src/javacard/applet/BrioletteApplet.java:343-359`

The bloom filter is tuned for ~1000 transactions per epoch with a 1% false
positive rate (9585 bits, 7 hash functions). A false positive causes the card
to reject a legitimate basename with `SW_BASENAME_USED` (0x6A84).

```java
// Parameters (tuned for ~1000 basenames/epoch, 1% false positive rate):
//   - Bit array: 9585 bits = 1199 bytes (rounded up to 1200)
//   - Hash functions: 7
```

The intended recovery mechanism exists: the swap authorization flow
(`INS_SIGN_COMMIT_SWAP`, 0x13) performs Schnorr verification of swap server
authorization and bypasses the bloom filter entirely
(`BrioletteApplet.java:349-352`). A wallet that encounters a false positive
can request swap authorization from the swapper service, then re-sign using
the swap flow. This is architecturally sound — the false positive triggers a
swap, which gives the wallet a fresh token with a new basename, resolving the
collision.

**Remaining gap:** The wallet does not yet implement this recovery path. The
`SmartCard::sign_commit()` trait collapses all APDU errors into `None`,
preventing the wallet from distinguishing a bloom filter rejection from other
failures. The wallet needs to:
1. Surface the APDU status word so it can detect `SW_BASENAME_USED`
2. On bloom filter rejection, request swap authorization from the swapper
3. Re-attempt signing via `INS_SIGN_COMMIT_SWAP` with the authorization

**Recommendation:** Wire up the bloom filter rejection → swap recovery path in
the wallet. This requires exposing APDU status words through the `SmartCard`
trait and adding retry logic in `sign_split()`. The bloom filter parameters
themselves are reasonable for the card's EEPROM constraints — the recovery
mechanism is the correct architectural answer rather than enlarging the filter.

### ~~H-5: Token Split Validation Is Incomplete~~ → Downgraded to Low

**Severity:** ~~High~~ **Low** (conservation is enforced by the TokenMap)
**Location:** `src/proto/src/briolette/token.rs:140-160`, `src/tokenmap/src/server.rs:650-680`

**Original finding:** Split value validation only checks that individual split
amounts don't exceed the original value, but does not verify the sum.

**Correction:** The TokenMap's `token_is_second_split()` function enforces sum
conservation when the second half of a split arrives:

```rust
// tokenmap/src/server.rs:675-679
let total = known_amount + unknown_amount;
let original_total = token.descriptor.clone().unwrap().value.clone().unwrap();
if total == original_total {
    return true;  // Valid split
}
// Falls through to double-spend detection if sums don't match
```

This is tested with `token_is_second_split_valid` (6+4=10 passes),
`token_is_second_split_invalid_amounts` (6+6=12≠10 fails), and
`token_is_second_split_wrong_currency_code` (mismatched codes fail).

Per-token `verify()` correctly checks `split_amount <= original_value` — this is
all a single token can verify since it only sees one side of the split. The sum
check inherently requires both halves, which only the TokenMap has.

**Remaining risk:** During purely offline peer-to-peer transfers, a receiving
wallet cannot verify sum conservation because it only sees one split half. This
is a fundamental property of the split design, not a bug. The TokenMap catches
any inflation when tokens are eventually validated online.

**Recommendation:** Consider adding a `remaining_value` hint to the token
structure so offline peers can do a courtesy check, but this is defense-in-depth
rather than a security gap.

---

## 4. Medium Findings

### M-1: Epoch Hardcoded to 86400 Seconds

**Severity:** Medium
**Location:** `src/proto/src/briolette/token.rs:30`

```rust
const EPOCH_SECONDS: u32 = 86400;  // TODO: make configurable
```

The epoch duration is hardcoded and cannot be adjusted without code changes.
This affects ticket lifetime, bloom filter reset frequency, and revocation
propagation speed.

**Recommendation:** Make epoch duration configurable via the EpochUpdate
broadcast from the Clerk.

### M-2: SQLite TokenMap Has No Connection Encryption

**Severity:** Medium
**Location:** `src/tokenmap/src/server.rs:39`

```rust
let conn = Connection::open(db_path).await?;
```

The TokenMap database stores sensitive data (token histories, abuse records,
revocation data) in an unencrypted SQLite file. No WAL mode is configured,
and there is no mention of SQLCipher or other encryption-at-rest.

**Recommendation:** Use SQLCipher for encryption-at-rest, enable WAL mode for
concurrent access, and configure appropriate file permissions.

### M-3: Wallet Secret Keys Stored in Plaintext JSON

**Severity:** Medium
**Location:** `src/wallet/src/lib.rs` (WalletData serialization)

Wallet data, including ECDAA secret keys and credentials, is serialized as
plaintext JSON. The mobile apps store this in NSUserDefaults (iOS) and
SharedPreferences (Android) — neither of which is encrypted by default.

**Recommendation:** Use platform keystores (Android Keystore / iOS Keychain)
for key material. Encrypt the wallet JSON blob with a key derived from the
platform keystore.

### M-4: Clerk Private Key Written as PKCS#8 DER

**Severity:** Medium
**Location:** `src/clerk/src/server.rs:99-101`

```rust
pub fn write_key(&self, data_file: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let sk: SecretKey = self.ticket_signing_key.clone().into();
    std::fs::write(&data_file, sk.to_pkcs8_der().unwrap().as_bytes())?;
    Ok(true)
}
```

The clerk's ticket signing key is written to disk as unencrypted PKCS#8 DER.
File permissions are not explicitly set (defaults to umask). The registrar
similarly writes issuer secret keys to disk in raw binary.

**Recommendation:** Use encrypted PKCS#8 (PKCS#8 with PBES2) or an HSM for
server-side key storage. Set file permissions to 0600.

### M-5: v0 hash_to_g1 Uses Try-and-Increment (Non-Constant-Time)

**Severity:** Medium
**Location:** `src/crypto/src/v0.rs:144-170`

```rust
fn hash_to_g1(data: &[u8]) -> G1 {
    let mut counter: u32 = 0;
    loop {
        // Try to interpret hash as x coordinate
        // ... check if y^2 = x^3 + b has a solution
        counter += 1;
        if counter > 10000 {
            panic!("hash_to_g1: failed after 10000 iterations");
        }
    }
}
```

The v0 hash-to-curve uses try-and-increment, which is non-constant-time and
leaks information about the hash input through timing. The v1 implementation
correctly uses RFC 9380 compliant SWU mapping via `bls12_381_plus`.

**Impact:** Timing side-channel during signing operations could leak information
about the message being signed. Particularly relevant for the JavaCard context
where power analysis is a concern.

**Recommendation:** Migrate to v1's RFC 9380 hash-to-curve. If v0 must be
retained, implement Elligator or SWU mapping for BN254.

### M-6: No Rate Limiting on Server APIs

**Severity:** Medium
**Location:** All server `*_main.rs` files

No rate limiting, authentication throttling, or circuit breakers are implemented
on any server endpoint. Combined with C-2 (no hardware attestation), this allows
unlimited credential issuance, ticket generation, and token minting.

**Recommendation:** Add per-IP and per-credential rate limiting. Implement
exponential backoff for failed authentication attempts.

### M-7: Panic on Amount Currency Mismatch

**Severity:** Medium
**Location:** `src/proto/src/briolette/token.rs` (Amount::add implementation)

The `Amount::add()` method panics on currency code mismatch rather than returning
an error. In a server context, this could cause denial-of-service if a malformed
token with mismatched currency codes is submitted.

**Recommendation:** Return `Result<Amount, BrioletteErrorCode>` instead of
panicking. All callers should handle the error gracefully.

### M-8: Mobile FFI Marshals Data as Untyped Dictionaries

**Severity:** Medium
**Location:** `mobile/shared/src/iosMain/kotlin/com/briolette/wallet/IosPlatform.kt`,
`mobile/iosApp/iosApp/WalletBridge.swift`

The iOS FFI bridge marshals wallet state as `Map<String, Any?>` dictionaries
with string-keyed lookups. This is fragile and provides no compile-time type
safety. Missing keys are silently replaced with defaults (empty strings, 0
values).

**Recommendation:** Use typed data structures or Kotlin serialization for
cross-boundary data transfer instead of untyped dictionaries.

---

## 5. Low Findings

### L-1: Secret Key File Permissions Not Enforced

**Severity:** Low
**Location:** `src/registrar/src/server.rs:74-84`

Key files are written via `std::fs::write()` without setting restrictive file
permissions. The default umask applies, which may leave keys world-readable.

### L-2: Key Rotation Mechanism Exists but Is Not Exercised

**Severity:** Low (informational)
**Location:** `src/proto/proto/clerk.proto:59-74`, `src/wallet/src/lib.rs:175-184`

Key rotation is supported by the existing protocol. The `ExtendedEpochData`
message uses `repeated` fields for all rotatable key types:

```protobuf
repeated bytes ttc_group_public_keys = 1;
repeated bytes epoch_signing_keys = 2;
repeated bytes ticket_signing_keys = 3;
repeated bytes mint_signing_keys = 4;
```

The wallet already validates signatures against the full key list
(`lib.rs:687-695` for ticket signing keys, `lib.rs:769-777` for epoch signing
keys). Rotation is performed by including both old and new keys in one epoch,
then dropping the old key in a subsequent epoch. The `AddEpoch` RPC on the
Clerk (`clerk.proto:30`) is the control plane for publishing new key sets.

The remaining gap is operational: no tooling or runbook exists for performing a
rotation, and the initial epoch signing key is trusted on first contact
(`// TODO: We trust the first epoch signing keys we fetch` in `lib.rs`). This
bootstrap trust is acceptable when the wallet trusts its registrar (which
provides the initial clerk URI and keys during registration).

### L-3: Bloom Filter Epoch Reset Has No Authentication

**Severity:** Low
**Location:** `src/javacard/applet/BrioletteApplet.java:618-635`

The RESET_BLOOM APDU (INS 0x30) only checks that the new epoch is strictly
greater than the current epoch. There is no authentication of who is sending
the reset command — any entity with NFC access to the card can reset the bloom
filter.

### L-4: SET_SWAP_PUBKEY Has No Access Control

**Severity:** Low
**Location:** `src/javacard/applet/BrioletteApplet.java:646-657`

The SET_SWAP_PUBKEY APDU can be called at any time by any entity with NFC
access. The comment says "Only allowed during personalization (before first
signing session)" but this restriction is not enforced in code.

### L-5: Recovery ID Appended to Signatures Without Length Check

**Severity:** Low
**Location:** `src/mint/src/server.rs:141-148`, `src/clerk/src/server.rs`

The ECDSA recovery byte is appended to signature bytes without explicit
documentation of the resulting length (64 + 1 = 65 bytes). Consumers must
know to strip the last byte before verification. This implicit protocol
increases the surface for interop bugs.

### L-6: Simulation Module Excluded From Workspace

**Severity:** Low
**Location:** `Cargo.toml:3`

The `src/simulation` directory is excluded from the workspace, meaning it
doesn't receive workspace-wide dependency updates or audit checks.

---

## 6. Informational Observations

### I-1: Formal TLA+ Specifications

The `formal/` directory contains 5 TLA+ specifications (1,489 lines total)
covering BrioletteSystem, ForkDetection, GossipConvergence, P2PTransaction,
RevocationProtocol, and TokenLifecycle. This is excellent practice for a
financial protocol.

### I-2: Zero Unsafe Blocks

The entire Rust codebase contains zero `unsafe` blocks, relying entirely on
safe abstractions from well-maintained crates (`substrate-bn`, `bls12_381_plus`,
`p256`).

### I-3: Well-Structured Error Handling

Error codes are centralized in protobuf (`ErrorCode` enum in `common.proto`)
and consistently propagated through the gRPC stack. Most functions return
`Result<T, BrioletteError>`.

### I-4: Bridge Module (Ethereum/Bitcoin)

The bridge module (`src/bridge/`) includes Ethereum (via Alloy) and Bitcoin
deposit processing. This module is feature-gated (`#[cfg(feature = "alloy")]`)
and adds significant additional attack surface that warrants separate review.

### I-5: Dependency Audit

Key cryptographic dependencies and their versions:
- `substrate-bn 0.6` — BN254 pairing library
- `bls12_381_plus 0.8` — BLS12-381 with hash-to-curve
- `p256 0.12.0` — NIST P-256 ECDSA
- `sha2 0.10` — SHA-256
- `rand 0.8` — OS-backed CSPRNG
- `uniffi 0.25` — Mozilla's FFI binding generator
- `tonic 0.8` — gRPC framework
- `rusqlite` (via `tokio-rusqlite`) — SQLite

All cryptographic crates are from the RustCrypto project or well-known sources.
A `cargo audit` scan for known vulnerabilities is recommended.

### I-6: ECDAA Implementation Is Custom

Both v0 and v1 ECDAA implementations are custom-written rather than using a
reviewed library. The algorithms appear correct based on comparison with the
Brickell & Li ECDAA scheme, but custom cryptographic implementations carry
inherent risk.

### I-7: Token Verification Chain Is Sound

The token verification logic in `token.rs` properly chains signatures through
history entries:
- Base signature verified against mint key (ECDSA P-256)
- History entries verified against transfer credential (ECDAA)
- Each entry's `previous_signature` links to the prior entry
- Current holder's ticket expiration is checked
- Token-level `valid_until` tag is checked
- Split values are validated per-token (`split <= original`); sum conservation is enforced by the TokenMap's `token_is_second_split()` when both halves are presented

### I-8: ECDAA Pairing Verification Is Standard

The ECDAA signature verification performs two pairing checks:
1. `e(R, Y) == e(S, P2)` — verifies the A/B credential relationship
2. `e(T, P2) == e(R + W, X)` — verifies the C/D credential relationship

This matches the standard ECDAA verification equations.

### I-9: v1 Uses Proper Domain Separation

The v1 implementation uses RFC 9380 compliant domain separation strings:
```rust
const DOMAIN_HASH_TO_G1: &[u8] = b"BRIOLETTE-V1-BLS12381_XMD:SHA-256_SSWU_RO_";
const DOMAIN_HASH_TO_SCALAR: &[u8] = b"BRIOLETTE-V1-BLS12381_XMD:SHA-256_";
```

This prevents cross-protocol attacks between hash-to-curve and hash-to-scalar
operations.

---

## 7. Recommendations Summary

### Immediate (Pre-Deployment Blockers)

1. **Disable Algorithm::NONE** in production or restrict Low-tier to online-only (C-2)
2. **Migrate to BLS12-381** (v1) as the default curve (H-1)
3. **Consider adding `remaining_value` hint** for offline split verification (H-5 — conservation already enforced by TokenMap)

### Short-Term

4. **Test JavaCard JCMathLib build** on target hardware (H-2 — code exists)
5. **Add TLS for confidentiality** — server-authenticated, not mTLS (C-1)
6. Encrypt key material at rest (M-3, M-4)
7. Add rate limiting to all server endpoints (M-6)
8. Replace panics with errors for untrusted input (M-7)
9. Fix service authentication to use unique identities (H-3)
10. Add bloom filter epoch reset authentication (L-3, L-4)

### Long-Term

11. Build key rotation tooling and operational runbook (L-2 — protocol support exists)
12. Add comprehensive integration test suite (currently requires live servers)
13. Commission formal review of custom ECDAA implementation (I-6)
14. Complete `cargo audit` for dependency vulnerabilities (I-5)
15. Review bridge module (Ethereum/Bitcoin) separately (I-4)

---

## 8. Test Coverage Assessment

| Crate            | Unit Tests | Integration Tests | Coverage Estimate |
|------------------|------------|-------------------|-------------------|
| briolette-proto  | 23         | 0                 | ~60%              |
| briolette-wallet | 28 pass    | 9 (need servers)  | ~45%              |
| briolette-mobile-ffi | 17    | 0                 | ~55%              |
| briolette-crypto | Tests in module | 0            | ~40%              |
| Server crates    | 0          | Via wallet tests  | ~20%              |

The unit test suite passes (all 68 unit tests across 3 crates). The 9
integration tests in `briolette-wallet` require running server infrastructure
and are expected to fail in isolation.

---

## Appendix A: File Structure

```
briolette/
├── Cargo.toml                    # Workspace root
├── formal/                       # TLA+ specifications (5 specs, 1489 LOC)
├── docs/design/                  # Design documentation
├── src/
│   ├── crypto/src/               # ECDAA v0 (BN254) and v1 (BLS12-381)
│   ├── proto/                    # Protobuf definitions + Rust extensions
│   ├── registrar/                # Credential issuance server
│   ├── clerk/                    # Ticket issuance + epoch management
│   ├── mint/                     # Token creation server
│   ├── validate/                 # Token verification server
│   ├── tokenmap/                 # SQLite double-spend detection
│   ├── swapper/                  # Token exchange server
│   ├── bridge/                   # Ethereum/Bitcoin deposit bridge
│   ├── receiver/                 # Token receiving server
│   ├── service_auth/             # NAC-based service-to-service auth
│   ├── wallet/                   # Client wallet library + CLI
│   ├── mobile-ffi/               # UniFFI Rust bridge for mobile
│   ├── javacard/applet/          # JavaCard split-key signing applet
│   └── simulation/               # Simulation tools (excluded from workspace)
└── mobile/                       # KMP mobile apps (Android + iOS)
```

## Appendix B: Cryptographic Algorithm Summary

| Operation                | Algorithm            | Curve/Size       | Security Level |
|--------------------------|----------------------|------------------|----------------|
| Group signatures (v0)    | ECDAA (Brickell-Li)  | BN254            | ~100-bit       |
| Group signatures (v1)    | ECDAA (Brickell-Li)  | BLS12-381        | 128-bit        |
| Token/ticket signing     | ECDSA                | NIST P-256       | 128-bit        |
| Hash-to-curve (v0)       | Try-and-increment    | BN254            | Non-CT         |
| Hash-to-curve (v1)       | SWU (RFC 9380)       | BLS12-381        | Constant-time  |
| Hashing                  | SHA-256              | 256-bit          | 128-bit        |
| Random generation        | OsRng (CSPRNG)       | System           | Platform-dependent |
| Swap authorization       | Schnorr              | BN254 G1         | ~100-bit       |
| Bloom filter hash        | SHA-256 → 7×16-bit   | 9585 bits        | ~1% FP @ 1000  |

---

*This audit is based on static code analysis. No dynamic testing, fuzzing, or
penetration testing was performed. Findings reflect the codebase state as of
2026-03-11 on branch `claude/split-signatures-secure-hardware-AnOGI`.*
