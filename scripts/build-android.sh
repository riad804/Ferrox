#!/usr/bin/env bash
# Build ferrox-mobile for Android and lay out jniLibs + Kotlin bindings.
#
# Requirements:
#   - Android NDK (set ANDROID_NDK_HOME, or ANDROID_HOME/ndk/<ver>)
#   - cargo-ndk:  cargo install cargo-ndk
#   - rustup android targets (added automatically below)
#
# Output:
#   bindings/kotlin/io/ferrox/sdk/ferrox_mobile.kt    ← Kotlin API
#   dist/android/jniLibs/<abi>/libferrox_mobile.so    ← native libs per ABI
#
# Consume from an Android module:
#   - copy jniLibs/ into src/main/jniLibs/ (or point sourceSets at dist/android)
#   - copy the Kotlin file into src/main/java/
#   - add `net.java.dev.jna:jna:5.x@aar` to the module's dependencies
set -euo pipefail
cd "$(dirname "$0")/.."

CRATE=ferrox-mobile
LIBNAME=libferrox_mobile.so
PROFILE=release
OUT=dist/android
BINDINGS=bindings/kotlin

# ABIs to ship. arm64-v8a + armeabi-v7a cover ~all phones; x86_64 for emulators.
ABIS=(arm64-v8a armeabi-v7a x86_64)

# Resolve NDK.
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  if [[ -n "${ANDROID_HOME:-}" && -d "$ANDROID_HOME/ndk" ]]; then
    ANDROID_NDK_HOME="$ANDROID_HOME/ndk/$(ls "$ANDROID_HOME/ndk" | sort -V | tail -1)"
    export ANDROID_NDK_HOME
  else
    echo "ERROR: set ANDROID_NDK_HOME (or ANDROID_HOME with an installed ndk)" >&2
    exit 1
  fi
fi
echo "▸ NDK: $ANDROID_NDK_HOME"

command -v cargo-ndk >/dev/null || { echo "ERROR: cargo install cargo-ndk" >&2; exit 1; }

echo "▸ ensuring rust targets"
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android >/dev/null

echo "▸ building .so per ABI into jniLibs"
rm -rf "$OUT/jniLibs"; mkdir -p "$OUT/jniLibs"
ABI_ARGS=(); for a in "${ABIS[@]}"; do ABI_ARGS+=(-t "$a"); done
cargo ndk "${ABI_ARGS[@]}" -o "$OUT/jniLibs" build -p "$CRATE" --release

# ferrox-core also produces a cdylib; only ferrox_mobile must ship in the app.
find "$OUT/jniLibs" -name 'libferrox_core.so' -delete

echo "▸ generating Kotlin bindings"
rm -rf "$BINDINGS"
# Use any built .so as the metadata source for binding generation.
SRC_SO="$OUT/jniLibs/arm64-v8a/$LIBNAME"
[[ -f "$SRC_SO" ]] || SRC_SO=$(find "$OUT/jniLibs" -name "$LIBNAME" | head -1)
cargo run -q -p "$CRATE" --bin uniffi-bindgen -- generate \
  --library "$SRC_SO" --language kotlin --out-dir "$BINDINGS"

echo "✓ done"
echo "  jniLibs   : $OUT/jniLibs/<abi>/$LIBNAME"
echo "  kotlin api: $BINDINGS/io/ferrox/sdk/ferrox_mobile.kt"
echo "  NOTE: add  net.java.dev.jna:jna:5.14.0@aar  to your Gradle deps"
