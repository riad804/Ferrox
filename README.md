# ferrox

**One Rust engine, one editing SDK — for Android, iOS, and Web.**

ferrox is a cross-platform video / audio / image **editing engine** shipped as an SDK. A single
pure-Rust core drives a handle-based `Editor` API that binds with identical semantics to
**Kotlin & Swift** (via UniFFI) and **TypeScript** (via WebAssembly).

[![CI](https://github.com/YOUR_ORG/ferrox/actions/workflows/ci.yml/badge.svg)](https://github.com/YOUR_ORG/ferrox/actions/workflows/ci.yml)

## Workspace

| Crate | What it is |
|-------|------------|
| **`core/`** | The engine — timeline & compositor, Bézier keyframes, color grading (ASC-CDL + 3D LUT), masks, chroma key, Porter-Duff blend modes, spatial transitions, multi-track audio mixer + effects, plus codecs/muxers. Pure Rust, compiles to `wasm32`. |
| **`sdk/`** | The **Editor SDK** — a handle-based, thread-safe state machine over a `Project`, with an undo/redo command stack, JSON project persistence, frame rendering, and headless MP4 export. This is the FFI-friendly public API. |
| **`ai/`** | Optional, model-backed AI traits (auto color match, voice isolation, rotoscoping, text-to-video, voiceover). Fully decoupled — the SDK compiles and runs without it. |
| **`mobile/`** | UniFFI bindings → Kotlin (`.aar`) and Swift (`XCFramework`) for Android/iOS. |
| **`apps/`** | Reference Android + iOS apps that consume the SDK. |

## The Editor SDK

Every mutation flows through a reversible command, so undo/redo is built in. The `Editor` handle is
`Clone` + thread-safe and holds no lifetimes — exactly what the FFI layers need.

```rust
use ferrox_sdk::{Editor, Clip, ClipSource, Transform};

let editor = Editor::new(1920, 1080, 30.0);
let track = editor.add_track()?;
editor.add_clip(track, Clip::new(
    ClipSource::Solid { width: 1920, height: 1080, r: 20, g: 30, b: 40, a: 255 },
    0.0, 5.0, Transform::default(),
))?;

let rgba = editor.render_frame(1.0, 0, 0)?; // composed frame as RGBA bytes
let json = editor.save_json()?;             // persist the project
editor.undo()?;                             // full undo/redo history
# Ok::<(), ferrox_sdk::SdkError>(())
```

Projects are plain JSON (`serde`), forward/backward compatible, and identical across every platform.

## Platform surfaces

- **Android / iOS** — `mobile/` exposes the SDK through UniFFI; `scripts/build-android.sh` and
  `scripts/build-ios.sh` produce the `.aar` and `.xcframework`.
- **Web** — `core` compiles to WebAssembly; the `Editor` is exposed to JavaScript via `wasm-bindgen`
  and packaged for npm (`wasm-pack build`).
- **Rust** — depend on `ferrox-sdk` directly.

> **Status:** the Rust `Editor` SDK (state machine, undo/redo, render, export) is complete and
> tested. Surfacing the full `Editor` across UniFFI (mobile) and WASM (web) is in progress. Video
> decode/encode on the hot path uses the platform codecs (MediaCodec / VideoToolbox / WebCodecs);
> the portable engine handles compositing, color, and audio.

## Build & test

```sh
cargo build --workspace
cargo test  -p ferrox-core -p ferrox-sdk

# WebAssembly
cargo build -p ferrox-core --target wasm32-unknown-unknown --no-default-features --features wasm
```

## License

TBD — dual-license (open core + commercial SDK) planned.
