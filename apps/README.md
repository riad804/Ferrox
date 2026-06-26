# M1 — Photo editor apps

Reference Android + iOS apps that prove the ferrox SDK end-to-end:

> **pick photo → `ImageSession(bytes)` → chained edits in Rust → render → save JPEG**

Both apps use the stateful `ImageSession` from `ferrox-mobile`: decode once, apply
a chain of edits in memory, render the preview via `toRgba8()` straight into a
platform bitmap (no PNG round-trip), and export JPEG on save.

Edits wired up: brightness, contrast, grayscale, blur, rotate 90°, crop square,
reset. (The SDK also exposes saturation, negate, flip, resize, thumbnail, draw
text — add buttons as needed.)

## Status

| | Build | Verified |
|---|---|---|
| SDK (`ferrox-mobile`) | host + iOS + Android | `cargo test -p ferrox-mobile` (9 tests) |
| iOS app | `xcodebuild` simulator | **BUILD SUCCEEDED** against the xcframework |
| Android app | Gradle (needs Android Studio) | every SDK call matches the generated bindings |

## Build & run — iOS

```sh
# 1. build the native SDK + xcframework
./scripts/build-ios.sh
# 2. copy it into the app + generate the Xcode project (needs: brew install xcodegen)
./scripts/sync-ios-sdk.sh
# 3. open & run
open apps/ios/FerroxPhotoEditor.xcodeproj
```

Notes:
- `import` is **not** needed — the generated `Generated/Ferrox.swift` is compiled
  into the app target; it imports the C module `FerroxFFI` from the xcframework.
- Deployment target is iOS 16 (SwiftUI `PhotosPicker`).

## Build & run — Android

```sh
# 1. build the native SDK (.so per ABI + Kotlin bindings)
./scripts/build-android.sh
# 2. copy jniLibs + Kotlin bindings into the app
./scripts/sync-android-sdk.sh
# 3. open apps/android in Android Studio and Run, or:
cd apps/android && ./gradlew installDebug   # needs a local Gradle wrapper / Studio
```

Notes:
- Requires the JNA runtime UniFFI depends on: `net.java.dev.jna:jna:5.14.0@aar`
  (already in `app/build.gradle.kts`).
- `minSdk 24`, ABIs `arm64-v8a / armeabi-v7a / x86_64` (match the build script).

## What's generated vs. committed

The native libs (`jniLibs`, `Ferrox.xcframework`) and the generated bindings are
**build artifacts** — gitignored in each app, recreated by the `build-*`/`sync-*`
scripts. Only hand-written app source is committed.

## Next (M2)

The native codec bridge (MediaCodec / VideoToolbox ⇄ ferrox frames). See
[../docs/roadmap-video-editor.md](../docs/roadmap-video-editor.md).
