# Roadmap: from ferrox to a CapCut-class mobile editor

**Goal:** a multi-track video editor for Android + iOS, with ferrox as the
creative engine.

**Architecture decision (locked):**
- **Hardware codecs** for encode/decode/preview — Android `MediaCodec`, iOS
  `VideoToolbox`. ferrox does *not* encode/decode video on the hot path.
- **ferrox = the creative engine** — filters, compositing, effects, color,
  transitions, audio mixing, the timeline model. Pure Rust, shared across both
  platforms via the UniFFI SDK we already built.

```
  HW decode (platform)  ──frames──▶  ferrox engine (Rust)  ──frames──▶  HW encode (platform)
   MediaCodec/VideoToolbox          timeline · composite ·            MediaCodec/VideoToolbox
                                    filters · transitions · color        → MP4 (H.264/HEVC)
```

This keeps export/preview fast (hardware) while the differentiated creative
logic stays in one portable Rust codebase.

---

## Where the line sits: ferrox vs. platform vs. app

| Concern | Owner | Status |
|---|---|---|
| H.264/HEVC encode + decode | **Platform** (MediaCodec/VideoToolbox) | to build (native glue) |
| Real-time preview / playback clock | **Platform** + app | to build |
| Camera / gallery import, permissions | **App** (Kotlin/Swift) | to build |
| Frame filters & color | **ferrox** | ✅ mostly exists |
| Timeline / clips / tracks / trim | **ferrox** (model) + app (UI) | to build |
| Compositing / layers / transitions | **ferrox** | to build |
| Audio decode/mix/encode | **ferrox** (mix) + platform (AAC) | partial |
| Text/sticker/overlay render | **ferrox** (drawtext exists) | partial |
| Project save/load, undo/redo | **ferrox** (model) | to build |

ferrox's existing AV1 encoder and software H.264/VP8/VP9 decoders become
**fallback / non-real-time / WASM-web** paths, not the mobile hot path.

---

## Milestones

Each milestone is independently shippable and de-risks the next.

### M0 — SDK foundation ✅ (done)
UniFFI bindings, Android `.aar`-ready jniLibs, iOS XCFramework, image/filter API.
*Proven: builds for both platforms, Kotlin + Swift APIs generate.*

### M1 — Photo editor app (2–4 wks)
Prove the SDK in a real app before touching video.
- Expose full image op set over UniFFI (crop, rotate, adjust, text, overlay).
- Frame ⇄ platform bitmap zero-/low-copy path (`ByteArray`/`Data` ↔ `Bitmap`/`UIImage`).
- Tiny Android + iOS app: pick photo → edit → save.
- **Exit:** a shippable photo editor; SDK round-trip validated on device.

### M2 — Native codec bridge (3–5 wks) ← the critical path
The piece that makes video possible at all.
- Android: `MediaCodec` decode (MP4→frames) + encode (frames→MP4), `MediaMuxer`.
- iOS: `VideoToolbox` + `AVAssetWriter`/`AVAssetReader`.
- Define the **frame interchange** between platform codecs and ferrox
  (`PixelFormat::Yuv420p` / `Rgba8`; agree on stride/colorspace; minimise copies).
- **Exit:** decode an MP4 to frames, hand to ferrox, encode back to MP4 — on
  device, both platforms. No editing yet, just the pipe.

### M3 — Single-clip editor (3–4 wks)
First real "video editor."
- ferrox: per-frame filter/adjust/resize over a decoded stream (mostly exists).
- App: import → trim (in/out) → one filter → export MP4 via M2.
- Audio: pass-through (re-mux original audio).
- **Exit:** trim + filter + export one clip, hardware-fast.

### M4 — Timeline engine in ferrox (5–8 wks)
The core differentiator. **Belongs in Rust** so both platforms share it.
- Data model: `Project → Track[] → Clip[]` with source ref, in/out, position,
  transform, effect stack. Serialisable (project save/load).
- Composition: given a timeline time `t`, produce the output frame —
  z-ordered layer composite (overlay already exists), per-clip transform.
- A `compose_frame(project, t) -> Frame` entry point the preview + exporter share.
- **Exit:** ferrox renders any timeline position to a frame, deterministically.

### M5 — Real-time preview (4–6 wks)
- Playback clock + AV-sync in the app, pulling `compose_frame` results.
- Frame cache / decode-ahead; seek-to-frame (uses keyframe index from demux).
- Render to `SurfaceView`/`Metal`/`GL` — consider moving the composite to the
  GPU path (ferrox already has a `wgpu` `gpu` feature) for 30/60fps.
- **Exit:** scrub + play a multi-clip timeline smoothly on device.

### M6 — Editor features (ongoing)
Transitions (cross-dissolve, wipe), keyframed effects, text/stickers/captions,
multi-track audio mix + ducking, speed ramps, color grading/LUTs, export presets
(720/1080/4k, social aspect ratios). Each is a ferrox engine addition + app UI.

### M7 — Polish & scale
Background export, project autosave/recovery, large-project memory management,
thumbnails/filmstrip, crash-safety, store readiness.

---

## Biggest risks (call them out early)

1. **M2 frame interchange** is the make-or-break. Colorspace (BT.601 vs 709),
   stride/alignment, and copy count between MediaCodec/VideoToolbox and ferrox
   determine whether preview hits frame rate. Prototype this first.
2. **Real-time compositing** likely needs the GPU (`gpu` feature / wgpu), not the
   CPU filters, once layers stack up. Plan for it in M4/M5.
3. **Audio** is currently the weakest area for editing: ferrox decodes most
   formats but has no mixer; AAC encode is platform-side. Mixing graph is new work.
4. **Scope.** A full timeline is the *goal*, not milestone 1 — M1→M3 exist
   specifically so you ship and learn before committing to M4+.

---

## What's reusable everywhere

Everything in ferrox (M1, M3 filters, M4 timeline, M6 effects) is **one Rust
codebase shared by Android, iOS, and the existing WASM/web build**. Only the
codec bridge (M2) and UI are per-platform. That shared core is the whole reason
to base the SDK on ferrox.
```
