#!/usr/bin/env bash
# Build ferrox-mobile for iOS and bundle a Ferrox.xcframework + Swift bindings.
#
# Requirements: Xcode, rustup iOS targets (added automatically below).
# Output:
#   bindings/swift/Ferrox.swift           ← Swift API
#   dist/ios/Ferrox.xcframework           ← device + simulator static libs
#
# Consume from an Xcode project (Swift Package or drag-in):
#   - add Ferrox.xcframework to "Frameworks, Libraries, and Embedded Content"
#   - add Ferrox.swift to the target's sources
set -euo pipefail
cd "$(dirname "$0")/.."

CRATE=ferrox-mobile
LIBNAME=libferrox_mobile.a
PROFILE=release
OUT=dist/ios
BINDINGS=bindings/swift

DEVICE=aarch64-apple-ios
SIM_ARM=aarch64-apple-ios-sim
SIM_X86=x86_64-apple-ios

echo "▸ ensuring rust targets"
rustup target add "$DEVICE" "$SIM_ARM" "$SIM_X86" >/dev/null

echo "▸ building static libs (release)"
for T in "$DEVICE" "$SIM_ARM" "$SIM_X86"; do
  cargo build -p "$CRATE" --release --target "$T"
done

echo "▸ lipo: fat simulator lib (arm64 + x86_64)"
mkdir -p "target/ios-sim-fat"
lipo -create \
  "target/$SIM_ARM/$PROFILE/$LIBNAME" \
  "target/$SIM_X86/$PROFILE/$LIBNAME" \
  -output "target/ios-sim-fat/$LIBNAME"

echo "▸ generating Swift bindings + module headers"
rm -rf "$BINDINGS"
cargo run -q -p "$CRATE" --bin uniffi-bindgen -- generate \
  --library "target/$DEVICE/$PROFILE/$LIBNAME" \
  --language swift --out-dir "$BINDINGS"

# UniFFI emits a *FFI.modulemap; xcframework needs it named module.modulemap
# inside a Headers dir alongside the generated header.
HEADERS="target/ios-headers"
rm -rf "$HEADERS"; mkdir -p "$HEADERS"
cp "$BINDINGS"/*FFI.h "$HEADERS/"
cp "$BINDINGS"/*FFI.modulemap "$HEADERS/module.modulemap"

echo "▸ assembling Ferrox.xcframework"
rm -rf "$OUT"; mkdir -p "$OUT"
xcodebuild -create-xcframework \
  -library "target/$DEVICE/$PROFILE/$LIBNAME"       -headers "$HEADERS" \
  -library "target/ios-sim-fat/$LIBNAME"            -headers "$HEADERS" \
  -output "$OUT/Ferrox.xcframework"

echo "✓ done"
echo "  framework : $OUT/Ferrox.xcframework"
echo "  swift api : $BINDINGS/Ferrox.swift"
