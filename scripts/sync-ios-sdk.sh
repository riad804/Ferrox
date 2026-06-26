#!/usr/bin/env bash
# Copy the built Ferrox.xcframework + Swift bindings into the iOS app, then
# (re)generate the Xcode project. Run scripts/build-ios.sh first.
set -euo pipefail
cd "$(dirname "$0")/.."

APP=apps/ios

[[ -d dist/ios/Ferrox.xcframework ]] || { echo "run scripts/build-ios.sh first" >&2; exit 1; }
[[ -f bindings/swift/Ferrox.swift ]] || { echo "missing swift bindings" >&2; exit 1; }

mkdir -p "$APP/Frameworks" "$APP/Generated"
rm -rf "$APP/Frameworks/Ferrox.xcframework"
cp -R dist/ios/Ferrox.xcframework "$APP/Frameworks/"
cp bindings/swift/Ferrox.swift "$APP/Generated/"

echo "✓ synced xcframework + Swift bindings into $APP"

if command -v xcodegen >/dev/null; then
  ( cd "$APP" && xcodegen generate )
  echo "✓ generated $APP/FerroxPhotoEditor.xcodeproj — open it in Xcode"
else
  echo "ℹ install xcodegen (brew install xcodegen) then run: (cd $APP && xcodegen generate)"
fi
