#!/usr/bin/env bash
# Build the Rust mobile FFI library for Android and iOS targets.
#
# Prerequisites:
#   Android: cargo-ndk, Android NDK (set ANDROID_NDK_HOME)
#     rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
#     cargo install cargo-ndk
#
#   iOS: Xcode command line tools
#     rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
#
# Usage:
#   ./build-rust.sh android    # Build for Android (all ABIs)
#   ./build-rust.sh ios        # Build for iOS (device + simulator)
#   ./build-rust.sh bindings   # Regenerate UniFFI Kotlin/Swift bindings
#   ./build-rust.sh all        # Build everything

set -euo pipefail
cd "$(dirname "$0")/.."  # briolette root

CRATE="briolette-mobile-ffi"
LIB_NAME="libbriolette_mobile"
PROFILE="${PROFILE:-release}"
PROFILE_FLAG=""
if [ "$PROFILE" = "release" ]; then
    PROFILE_FLAG="--release"
fi

# ── Android ──────────────────────────────────────────────────────────────

build_android() {
    echo "==> Building Rust for Android..."

    local ANDROID_TARGETS=(
        "arm64-v8a"
        "armeabi-v7a"
        "x86_64"
    )
    local JNI_DIR="mobile/androidApp/src/main/jniLibs"

    for abi in "${ANDROID_TARGETS[@]}"; do
        echo "  -> $abi"
        cargo ndk -t "$abi" -o "$JNI_DIR" build -p "$CRATE" $PROFILE_FLAG
    done

    echo "==> Android .so files:"
    find "$JNI_DIR" -name "*.so" -exec ls -lh {} \;
}

# ── iOS ──────────────────────────────────────────────────────────────────

build_ios() {
    echo "==> Building Rust for iOS..."

    local IOS_TARGETS=(
        "aarch64-apple-ios"            # Device
        "aarch64-apple-ios-sim"        # Simulator (Apple Silicon)
        "x86_64-apple-ios"             # Simulator (Intel)
    )

    for target in "${IOS_TARGETS[@]}"; do
        echo "  -> $target"
        cargo build -p "$CRATE" --target "$target" $PROFILE_FLAG
    done

    # Create universal (fat) library for simulators
    local SIM_DIR="target/ios-simulator-universal/$PROFILE"
    mkdir -p "$SIM_DIR"
    lipo -create \
        "target/aarch64-apple-ios-sim/$PROFILE/${LIB_NAME}.a" \
        "target/x86_64-apple-ios/$PROFILE/${LIB_NAME}.a" \
        -output "$SIM_DIR/${LIB_NAME}.a" 2>/dev/null || \
    cp "target/aarch64-apple-ios-sim/$PROFILE/${LIB_NAME}.a" "$SIM_DIR/"

    # Create XCFramework
    local XCFRAMEWORK="mobile/iosApp/BrioletteMobile.xcframework"
    rm -rf "$XCFRAMEWORK"
    xcodebuild -create-xcframework \
        -library "target/aarch64-apple-ios/$PROFILE/${LIB_NAME}.a" \
        -library "$SIM_DIR/${LIB_NAME}.a" \
        -output "$XCFRAMEWORK"

    echo "==> iOS XCFramework: $XCFRAMEWORK"
}

# ── Bindings ─────────────────────────────────────────────────────────────

generate_bindings() {
    echo "==> Generating UniFFI bindings..."

    local UDL="src/mobile-ffi/src/briolette.udl"

    # Kotlin (Android)
    local KT_OUT="mobile/shared/src/androidMain/kotlin/uniffi"
    mkdir -p "$KT_OUT"
    cargo run -p "$CRATE" --bin uniffi-bindgen generate "$UDL" \
        --language kotlin --out-dir "$KT_OUT"
    echo "  -> Kotlin: $KT_OUT"

    # Swift (iOS)
    local SWIFT_OUT="mobile/iosApp/generated"
    mkdir -p "$SWIFT_OUT"
    cargo run -p "$CRATE" --bin uniffi-bindgen generate "$UDL" \
        --language swift --out-dir "$SWIFT_OUT"
    echo "  -> Swift: $SWIFT_OUT"
}

# ── Main ─────────────────────────────────────────────────────────────────

case "${1:-all}" in
    android)
        build_android
        ;;
    ios)
        build_ios
        ;;
    bindings)
        generate_bindings
        ;;
    all)
        generate_bindings
        build_android
        build_ios
        ;;
    *)
        echo "Usage: $0 {android|ios|bindings|all}"
        exit 1
        ;;
esac

echo "==> Done!"
