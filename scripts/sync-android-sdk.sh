#!/usr/bin/env bash
# Copy freshly-built ferrox native libs + Kotlin bindings into the Android app.
# Run scripts/build-android.sh first, then this.
set -euo pipefail
cd "$(dirname "$0")/.."

APP=apps/android/app/src/main

[[ -d dist/android/jniLibs ]] || { echo "run scripts/build-android.sh first" >&2; exit 1; }
[[ -f bindings/kotlin/io/ferrox/sdk/ferrox_mobile.kt ]] || { echo "missing kotlin bindings" >&2; exit 1; }

mkdir -p "$APP/jniLibs" "$APP/java/io/ferrox/sdk"
rm -rf "$APP/jniLibs"/*
cp -R dist/android/jniLibs/. "$APP/jniLibs/"
cp bindings/kotlin/io/ferrox/sdk/ferrox_mobile.kt "$APP/java/io/ferrox/sdk/"

echo "✓ synced jniLibs + Kotlin bindings into $APP"
