# Milestone kickoff prompts (M2 → M7)

Paste the prompt for the milestone you're starting into a **fresh Claude Code
session** opened at the repo root (`/Users/secondsource/StudioProjects/ferrox`).
Each prompt is self-contained: it points the new session at the context it needs,
states the goal + exit criteria, and locks the constraints already decided so it
doesn't re-litigate them.

**Decisions already locked (don't re-debate in any milestone):**
- ferrox = the creative engine (pure Rust, shared Android/iOS/WASM).
- Video encode/decode/preview = **platform hardware** (Android `MediaCodec`,
  iOS `VideoToolbox`). ferrox does NOT do video codecs on the hot path.
- SDK boundary = **UniFFI** (`mobile/` crate → Kotlin + Swift).
- Mobile feature set = pure-Rust only (no `encode`/rav1e, no C codecs).

**Context every milestone should read first:**
- `docs/roadmap-video-editor.md` — the full plan + risks.
- `docs/mobile.md` — how the SDK is built/consumed.
- `apps/README.md` — M1 apps + the build/sync script workflow.
- `mobile/src/lib.rs` — current SDK surface (`ImageSession`, free functions).

**Workflow rule for all milestones:** verify by building, not by assuming. Run
`cargo test -p ferrox-mobile`; cross-compile (`scripts/build-ios.sh` /
`build-android.sh`); for iOS, `xcodebuild` the app and require BUILD SUCCEEDED.
Keep changes unstaged unless asked to commit.

---

## M2 — Native codec bridge (the critical path)

```
We're continuing the ferrox mobile project. Read docs/roadmap-video-editor.md,
docs/mobile.md, docs/milestone-prompts.md, apps/README.md, and mobile/src/lib.rs
first to load context and the locked architecture decisions.

Implement M2: the native video codec bridge. This is the make-or-break milestone.

Goal: decode an MP4 to raw frames using platform hardware, hand each frame to
ferrox, and encode frames back to an MP4 — on both Android (MediaCodec +
MediaMuxer) and iOS (VideoToolbox + AVAssetReader/AVAssetWriter). No editing
yet; just prove the pipe end to end.

Do this:
1. Define the frame-interchange contract between the platform codecs and ferrox.
   Decide and DOCUMENT: pixel format (target ferrox PixelFormat::Yuv420p or
   Rgba8), colorspace (BT.601 vs 709), stride/row-alignment handling, and how to
   minimise copies across the FFI boundary. This contract drives everything
   downstream — write it to docs/m2-frame-interchange.md.
2. Add any SDK helpers ferrox needs for raw-frame in/out (e.g. a way to accept a
   YUV/RGBA frame + dimensions + stride and return a processed frame). Keep the
   UniFFI surface clean; mirror the ImageSession style.
3. Android: a Kotlin module that MediaCodec-decodes MP4 → frames → (optionally
   through ferrox) → MediaCodec-encodes → MediaMuxer MP4.
4. iOS: the VideoToolbox/AVFoundation equivalent.
5. Wire a tiny "decode→re-encode passthrough" test path in each app proving a
   real MP4 round-trips on device/simulator.

Exit criteria: an MP4 decodes to frames, frames pass through ferrox, and
re-encode to a playable MP4 — demonstrated on both platforms. Frame-interchange
contract documented. Flag any colorspace/stride mismatches you find.

Constraint: do NOT use ferrox software video codecs (vp8/vp9/h264/av1) on the
hot path — hardware only. Verify by building (xcodebuild for iOS).
```

---

## M3 — Single-clip editor

```
Continuing ferrox mobile. Read docs/roadmap-video-editor.md,
docs/milestone-prompts.md, docs/m2-frame-interchange.md, apps/README.md, and
mobile/src/lib.rs first. M2 (the native codec bridge) is done — build on it.

Implement M3: the first real video editor — single clip.

Goal: import one video → trim (set in/out points) → apply one ferrox filter to
every frame → export MP4, using the M2 hardware codec bridge. Audio: pass the
original audio track through (re-mux, don't re-encode).

Do this:
1. SDK: ensure ferrox can apply a per-frame filter/adjust/resize over a decoded
   stream (most of this exists for images; reuse the filter code for video
   frames). Add any missing SDK entry points.
2. Android + iOS app: import → trim UI (in/out) → pick one filter → export.
3. Use M2 for decode/encode; only the frames between go through ferrox.
4. Mux the source audio through unchanged.

Exit criteria: trim + one filter + export of a single clip, hardware-fast, on
both platforms. Verify by building (xcodebuild for iOS) and round-tripping a real
clip.

Constraint: hardware codecs only on the hot path. Keep the UniFFI surface clean.
```

---

## M4 — Timeline engine in ferrox (core differentiator)

```
Continuing ferrox mobile. Read docs/roadmap-video-editor.md,
docs/milestone-prompts.md, apps/README.md, and mobile/src/lib.rs first. M1–M3
are done (image editor, codec bridge, single-clip editor).

Implement M4: the timeline/composition engine — IN RUST inside ferrox-core (so
Android + iOS + WASM all share it). This is the core differentiator.

Goal: a serialisable timeline model and a deterministic "render any timeline
position to a frame" function the preview and exporter will both use.

Do this (in core/, then expose via mobile/):
1. Data model: Project → Track[] → Clip[]. Each Clip: source reference, in/out
   times, timeline position, transform (scale/translate/rotate), and an effect
   stack. Make it serialisable (serde) for project save/load.
2. Composition: implement compose_frame(project, time_t) -> Frame. Z-ordered
   layer compositing (reuse the existing OverlayFilter), per-clip transform,
   per-clip effect stack applied to the source frame at that time.
3. Decide how source frames are supplied: M2/M3 hardware decoders feed decoded
   frames in (the engine composes; it does NOT decode video itself).
4. Expose compose_frame + project load/save over UniFFI.
5. Unit-test composition determinism (same project+t → same frame).

Exit criteria: ferrox renders any timeline position to a frame deterministically;
projects serialise/deserialise; tests pass (cargo test). No UI required yet.

Constraints: timeline logic lives in Rust (core), not the apps. ferrox does not
decode/encode video — it composes frames handed to it. Hardware codecs stay in
the app layer.
```

---

## M5 — Real-time preview

```
Continuing ferrox mobile. Read docs/roadmap-video-editor.md,
docs/milestone-prompts.md, apps/README.md, and mobile/src/lib.rs first. M4 (the
timeline engine with compose_frame + project model) is done.

Implement M5: real-time preview of a multi-clip timeline.

Goal: scrub and play a timeline smoothly (target 30fps) on device, pulling
composed frames from the M4 engine.

Do this:
1. App-side playback clock + A/V sync, pulling compose_frame results at the right
   times via the M2 hardware decoders for source frames.
2. Frame cache / decode-ahead; seek-to-frame using the keyframe index from the
   demuxers.
3. Render to SurfaceView/GL (Android) and Metal (iOS). STRONGLY consider moving
   compositing to the GPU: ferrox already has a `gpu` feature (wgpu + WGSL,
   core/src/gpu.rs). Evaluate using it for the composite path once layers stack,
   since CPU compositing likely won't hold 30fps.
4. Measure: report actual preview fps for a 2–3 clip timeline.

Exit criteria: scrub + play a multi-clip timeline smoothly on both platforms,
with measured frame rate. Document where the GPU path was needed.

Constraints: hardware decode for sources; engine composes. Keep shared logic in
Rust where it isn't inherently platform UI.
```

---

## M6 — Editor features

```
Continuing ferrox mobile. Read docs/roadmap-video-editor.md,
docs/milestone-prompts.md, apps/README.md, and mobile/src/lib.rs first. M1–M5
are done (editor, codec bridge, single-clip, timeline engine, real-time preview).

Implement M6: the CapCut-class feature set, incrementally. Each feature = a
ferrox engine addition (Rust) + app UI. Pick the subset I name below; if I
named none, propose a prioritised order and start with the top item.

Candidate features:
- Transitions (cross-dissolve, wipe) between adjacent clips.
- Keyframed effects (animate a parameter over a clip's duration).
- Text / stickers / captions as timeline layers (drawtext exists in ferrox).
- Multi-track audio mixing + ducking (NOTE: ferrox has audio decode but NO mixer
  yet — the mix graph is new work; AAC encode stays platform-side).
- Speed ramps (variable playback rate).
- Color grading / LUTs.
- Export presets (720/1080/4k, social aspect ratios).

For each: add the engine capability in core/, expose via mobile/, add UI, test.

Exit criteria: the chosen features work end to end through export, verified by
building and round-tripping. Audio mixing, if chosen, has unit tests in core.

Constraints: engine logic in Rust/core, shared across platforms; hardware codecs
for encode/decode; keep the UniFFI surface coherent.
```

---

## M7 — Polish & scale

```
Continuing ferrox mobile. Read docs/roadmap-video-editor.md,
docs/milestone-prompts.md, and apps/README.md first. M1–M6 are done.

Implement M7: production hardening for store readiness.

Goal: make the app robust under real use and large projects.

Do this (pick what I prioritise; else start at the top):
- Background export (export continues if app is backgrounded; progress + cancel).
- Project autosave + crash recovery.
- Large-project memory management (frame/cache eviction, big-timeline stability).
- Thumbnails / filmstrip generation for the timeline.
- Crash-safety around the FFI boundary (no panics across UniFFI; graceful errors).
- Store readiness: icons, permissions copy, size budget, signing notes.

Exit criteria: the chosen items are implemented and verified (build + manual run
where applicable). Document anything that needs a real device to validate.

Constraints: keep shared logic in Rust; don't regress earlier milestones (run
cargo test -p ferrox-mobile and rebuild the apps before finishing).
```

---

### Tip
If a session needs to know exactly what the SDK exposes today, have it run:
`grep -nE "#\[uniffi::(export|constructor)\]|pub fn " mobile/src/lib.rs`
and read `bindings/swift/Ferrox.swift` / `bindings/kotlin/.../ferrox_mobile.kt`.
