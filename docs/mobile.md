# ferrox on Android & iOS (native SDK)

`ferrox-mobile` wraps `ferrox-core` behind a [UniFFI](https://mozilla.github.io/uniffi-rs/)
boundary and ships it as a native SDK:

- **Android** — `.so` per ABI + generated **Kotlin** API
- **iOS** — `Ferrox.xcframework` + generated **Swift** API

The mobile build uses the pure-Rust feature set only (image/audio decode,
filters, demux, GIF). The `encode` feature (rav1e/AV1, which needs NASM) and the
C-backed codecs (`vp9`, `h264`, `mp3-encode`, `opus-encode`) are intentionally
**off** — they need per-platform native toolchains. See [Extending](#extending).

## API

Mirrors the WASM bindings in [`core/src/wasm.rs`](../core/src/wasm.rs). Every call
takes/returns bytes (`ByteArray` / `Data`) and throws on failure.

| Function | Description |
|---|---|
| `decodeImageToPng(bytes)` | normalise PNG/JPEG → PNG |
| `resizeImage(bytes, w, h)` | Lanczos3 resize → PNG |
| `applyFilter(bytes, expr)` | run a filtergraph (`"blur=2.0,grayscale"`) → PNG |
| `blurImage(bytes, sigma)` | gaussian blur → PNG |
| `grayscaleImage(bytes)` | grayscale → PNG |
| `probeImage(bytes)` | metadata JSON string |
| `decodeVp8ToPng(bytes)` | VP8 keyframe → PNG |
| `decodeGifFrames(bytes)` | GIF → `[GifPngFrame]` (png + delayMs) |
| `version()` | crate version |

Failures throw `FerroxException` (Kotlin) / `FerroxError` (Swift).

## Build — iOS

```sh
./scripts/build-ios.sh
```

Produces:
- `dist/ios/Ferrox.xcframework` — device (`ios-arm64`) + simulator (`arm64`+`x86_64`)
- `bindings/swift/Ferrox.swift` — the Swift API

### Use in Xcode

1. Drag `Ferrox.xcframework` into the target → **Frameworks, Libraries, and
   Embedded Content**.
2. Add `bindings/swift/Ferrox.swift` to the target's sources.
3. Call it:

```swift
import Foundation

let jpeg = try Data(contentsOf: url)
let thumb = try resizeImage(imageData: jpeg, width: 320, height: 240)   // PNG Data
let info  = try probeImage(imageData: jpeg)                             // JSON String
let gray  = try grayscaleImage(imageData: jpeg)
```

(For SwiftPM distribution, wrap the xcframework + Ferrox.swift in a
`Package.swift` with a `binaryTarget`.)

## Build — Android

One-time setup:

```sh
cargo install cargo-ndk
# NDK auto-detected from $ANDROID_HOME/ndk, or set ANDROID_NDK_HOME
```

Then:

```sh
./scripts/build-android.sh
```

Produces:
- `dist/android/jniLibs/<abi>/libferrox_mobile.so` — arm64-v8a, armeabi-v7a, x86_64
- `bindings/kotlin/io/ferrox/sdk/ferrox_mobile.kt` — the Kotlin API

### Use in an Android module

1. Copy `dist/android/jniLibs/` into `src/main/jniLibs/` (or point a `sourceSet`
   at it).
2. Copy the Kotlin file into `src/main/java/`.
3. Add the JNA runtime UniFFI depends on, to the module's `build.gradle`:

```gradle
dependencies {
    implementation "net.java.dev.jna:jna:5.14.0@aar"
}
```

4. Call it:

```kotlin
import io.ferrox.sdk.*

val jpeg: ByteArray = assets.open("photo.jpg").readBytes()
val thumb = resizeImage(jpeg, 320u, 240u)   // PNG ByteArray
val info  = probeImage(jpeg)                 // JSON String
val gray  = grayscaleImage(jpeg)
```

> Packaging tip: to publish a single artifact, assemble the jniLibs + Kotlin
> file into an Android library module and build an `.aar`.

## Extending

Add operations by exporting more functions in
[`mobile/src/lib.rs`](../mobile/src/lib.rs) with `#[uniffi::export]` — the Kotlin
and Swift APIs regenerate automatically on the next build.

To enable C-backed codecs (`vp9`, `h264`, …) you must cross-compile their native
libs (dav1d, openh264, …) for each Android/iOS target and add the matching
features to `ferrox-core` in [`mobile/Cargo.toml`](../mobile/Cargo.toml). Start
without them.
