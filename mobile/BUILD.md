# Building Briolette Mobile Apps

Briolette uses Kotlin Multiplatform (KMP) with Compose Multiplatform for shared UI,
and UniFFI for Rust FFI bindings. The Rust wallet library is compiled to native
code and linked into each platform's app.

## Prerequisites

### Common

- **JDK 17+** (e.g., OpenJDK 17 or Zulu 17)
- **Rust toolchain** (stable, via rustup)
- **UniFFI CLI** (installed via the `uniffi-bindgen` binary in `src/mobile-ffi/`)

### Android

- **Android SDK** (API 35 / compileSdk 35, minSdk 26)
- **Android NDK** (for cross-compiling Rust to Android targets)
- **cargo-ndk** (`cargo install cargo-ndk`)
- **Rust Android targets**:
  ```bash
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
  ```

### iOS

- **macOS** with **Xcode 15+** (iOS 16.0 deployment target)
- **Rust iOS targets**:
  ```bash
  rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
  ```

## Project Structure

```
mobile/
├── androidApp/          # Android app module (Compose Activity)
├── iosApp/              # iOS app (SwiftUI + KMP framework)
│   ├── BrioletteWallet.xcodeproj/   # Xcode project
│   ├── iosApp/
│   │   ├── BrioletteApp.swift       # App entry point
│   │   ├── ContentView.swift        # Compose embedding
│   │   ├── WalletBridge.swift       # Swift UniFFI → KMP delegate bridge
│   │   └── briolette.swift          # UniFFI-generated Swift bindings
│   └── generated/
│       └── brioletteFFI/            # UniFFI C header + modulemap
├── shared/              # KMP shared module (Compose UI, data layer)
├── build.gradle.kts     # Root build file
├── settings.gradle.kts  # Module includes
└── gradle/
    └── libs.versions.toml  # Version catalog
```

## Step 1: Build the Rust FFI Library

### Generate UniFFI Bindings

```bash
cd src/mobile-ffi
cargo build
cargo run --bin uniffi-bindgen generate src/briolette.udl --language kotlin --out-dir ../../mobile/shared/src/androidMain/kotlin/
cargo run --bin uniffi-bindgen generate src/briolette.udl --language swift --out-dir ../../mobile/iosApp/iosApp/
```

This generates:
- Kotlin bindings at `mobile/shared/src/androidMain/kotlin/uniffi/briolette/`
- Swift bindings at `mobile/iosApp/iosApp/briolette.swift`
- C header and modulemap at `mobile/iosApp/generated/brioletteFFI/`

### Build for Android

```bash
# Build the native .so for each Android ABI
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 \
  -o mobile/androidApp/src/main/jniLibs \
  build --release -p briolette-mobile-ffi
```

This places `libbriolette_mobile.so` in each ABI directory under `jniLibs/`.

### Build for iOS

```bash
# Build for device (arm64)
cargo build --release --target aarch64-apple-ios -p briolette-mobile-ffi

# Build for simulator (arm64 + x86_64)
cargo build --release --target aarch64-apple-ios-sim -p briolette-mobile-ffi
cargo build --release --target x86_64-apple-ios -p briolette-mobile-ffi

# Create xcframework
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libbriolette_mobile.a \
  -library target/aarch64-apple-ios-sim/release/libbriolette_mobile.a \
  -output mobile/iosApp/BrioletteMobile.xcframework
```

## Step 2: Build the Apps

### Android

```bash
cd mobile

# Create local.properties pointing to your Android SDK
echo "sdk.dir=$ANDROID_HOME" > local.properties

# Build debug APK
./gradlew :androidApp:assembleDebug

# The APK is at:
# androidApp/build/outputs/apk/debug/androidApp-debug.apk
```

To install on a connected device/emulator:
```bash
./gradlew :androidApp:installDebug
```

### iOS

```bash
cd mobile

# Build the KMP shared framework for iOS
./gradlew :shared:linkDebugFrameworkIosArm64        # device
./gradlew :shared:linkDebugFrameworkIosSimulatorArm64  # simulator

# Open in Xcode
open iosApp/BrioletteWallet.xcodeproj
```

In Xcode:
1. Select the **BrioletteWallet** scheme
2. Choose your target device or simulator
3. Build & Run (Cmd+R)

The Xcode project includes a build phase that runs
`./gradlew :shared:embedAndSignAppleFrameworkForXcode` automatically.

## iOS FFI Architecture

The iOS app uses a **delegate pattern** to bridge between Swift UniFFI bindings
and Kotlin Multiplatform code:

1. **`briolette.swift`** — UniFFI-generated Swift functions (e.g., `createWallet()`, `loadWallet()`)
2. **`WalletBridge.swift`** — `SwiftWalletDelegate` class that wraps UniFFI calls and implements the `IosWalletDelegate` protocol (defined in KMP)
3. **`IosPlatform.kt`** — `IosWalletBridge` class that calls through the delegate
4. **`BrioletteApp.swift`** — calls `installWalletBridge()` at init to wire everything up

Data is marshaled as `[String: Any?]` dictionaries / `Map<String, Any?>` between Swift and Kotlin.

## Android FFI Architecture

Android uses **direct UniFFI Kotlin bindings**:

1. UniFFI generates Kotlin classes in `uniffi.briolette` package
2. `AndroidWalletBridge.kt` calls UniFFI functions on `Dispatchers.IO`
3. Extension functions convert between FFI types and KMP common types

The native `.so` is loaded automatically from `jniLibs/` by the Android runtime.

## Testing

### Rust Unit Tests

```bash
cargo test -p briolette-mobile-ffi
cargo test -p briolette-wallet
cargo test -p briolette-proto
```

### Android

```bash
cd mobile
./gradlew :shared:testDebugUnitTest
./gradlew :androidApp:testDebugUnitTest
```

### Manual Testing

Both apps connect to Briolette network services (registrar, clerk, mint, validator).
Configure the service URIs in the Settings screen, or use the defaults for a local
development environment. See the main project README for running the server
infrastructure.

## Troubleshooting

- **"CIQRCodeGenerator filter not available" on iOS Simulator**: This filter works
  on physical devices and most simulator versions. If it fails, ensure you're
  using a recent iOS Simulator runtime.

- **UniFFI version mismatch**: Ensure the UniFFI version in `src/mobile-ffi/Cargo.toml`
  matches the `uniffi-bindgen` you use to generate bindings.

- **Android NDK not found**: Set `ANDROID_NDK_HOME` or configure it in
  `local.properties`:
  ```
  ndk.dir=/path/to/android-ndk
  ```

- **iOS framework not found**: Run the KMP framework build before opening Xcode:
  ```bash
  cd mobile && ./gradlew :shared:linkDebugFrameworkIosSimulatorArm64
  ```
