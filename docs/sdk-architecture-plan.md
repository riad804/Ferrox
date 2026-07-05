# Ferrox SDK — Architecture Analysis & 20-Phase Implementation Plan

> Authoritative program plan for evolving ferrox into a production-grade, cross-platform
> media-editing SDK. Follows the mandated delivery strategy: **analyze → map dependencies →
> plan → build one verified phase at a time**. No phase starts until the previous is complete,
> tested, benchmarked, documented, and backward-compatible.

---

## 1. Current state (grounded, verified 2026-07-05)

**Crates:** `core` (engine), `sdk` (Editor facade), `ai` (stub AI traits), `mobile` (UniFFI),
`web` (wasm-bindgen), plus `apps/`, `bindings/`, `dist/`, `scripts/`, `docs/`.

**Already built (maps onto the roadmap):**

| Exists today | Satisfies (partially) |
|---|---|
| `core::timeline` (`Project/Track/Clip`, serde) | domain model |
| `core::compositor::compose_frame_graded` (linear per-clip pipeline) | precursor to Render Graph (Phase 4) |
| `color` (ASC-CDL, 3D LUT), `mask`, `keyer`, `blend`, `transitions`, `anim` | effect nodes (Phases 1, 4, 10) |
| `audio` mixer + effects (EQ/comp/reverb/delay/gate), waveform | audio effects (Phase 1), waveform (Phase 2) |
| `sdk::Editor` + `Command` stack + undo/redo + JSON persistence | Command/CQRS, ProjectStorage (Phase 14) baseline |
| `sdk::export` (rav1e → progressive MP4) | Exporter plugin (Phase 1), Export renderer (Phase 9) |
| `ai` async traits (color match, roto, voice, T2V, voiceover) | AI plugins (Phase 1) |
| `gpu` feature (wgpu Resize/Blur filters) | seed for GPU Abstraction (Phase 3) |

**Not present (all greenfield):** event bus, plugin system, asset manager, resource cache, task
manager, render graph, playback engine, preview/export split, subtitle/font/vector engines,
diagnostics, profiler, streaming, collaboration, licensing.

**Rule-compliance debt to pay down (new rules):**
- **8 files exceed the 400-line cap:** `demux_graph.rs` (641), `fmp4_mux.rs` (638),
  `mobile/lib.rs` (618), `timeline.rs` (572), `audio/effects.rs` (556), `hls.rs` (462),
  `gpu.rs` (453), `mpegts_mux.rs` (419).
- `sdk::Editor` uses `Arc<Mutex<Inner>>` → should be `Arc<RwLock<_>>` (read-heavy: render/query
  dominate, writes are commands).
- No benchmark harness; examples/`//!` docs uneven across crates.

---

## 2. Target architecture (Clean Architecture / DDD)

Four layers, dependencies pointing **inward only**. The engine stays 100% platform-agnostic;
platform code lives only in `mobile/`, `web/`, `bindings/`.

```
┌───────────────────────────────────────────────────────────────┐
│ INTERFACE / ADAPTERS   mobile (UniFFI) · web (wasm) · bindings │  ← platform-specific
├───────────────────────────────────────────────────────────────┤
│ APPLICATION   Editor facade · Managers (Asset/Plugin/Task/     │  ← use cases, orchestration
│               Playback/…) · Commands (writes) · Queries (reads)│     CQRS + DI + events
├───────────────────────────────────────────────────────────────┤
│ PORTS (traits)   GpuBackend · Codec · Storage · Clock ·        │  ← the seams (DIP)
│                  TaskExecutor · EventSink · AssetStore · Plugin │
├───────────────────────────────────────────────────────────────┤
│ DOMAIN   Project · Track · Clip · Transform · ColorGrade ·     │  ← pure, no I/O, immutable
│          RenderGraph model · Animation curves · value objects  │     value objects
└───────────────────────────────────────────────────────────────┘
        INFRASTRUCTURE (implements ports): codec/mux impls, wgpu backend,
        fs/compressed storage, LRU caches, thread-pool executor, in-process event bus
```

**Crate strategy — decision:** enforce layering with **top-level modules inside `core`**
(`domain/`, `app/`, `ports/`, `infra/`) **now**, and only split into separate crates
(`ferrox-domain`, `ferrox-app`, `ferrox-infra`) later **if** compile-time or ownership boundaries
demand it. *Trade-off:* separate crates give hard, compiler-enforced boundaries and faster
incremental builds, but a premature split multiplies churn and version lock-step across 5+ crates.
Module-layering delivers 90% of the discipline at 10% of the cost and is reversible.

---

## 3. Cross-cutting architectural decisions (with trade-offs)

**D1 — Concurrency (`Editor` state).** Move `Arc<Mutex<_>>` → `Arc<parking_lot::RwLock<_>>`.
Rendering/queries take read locks (many, concurrent); commands take write locks (few). *Trade-off:*
`RwLock` risks writer starvation under heavy reads and `std::RwLock` poisons on panic — `parking_lot`
fixes both (no poisoning, fair, faster) at the cost of one well-established dependency. WASM is
single-threaded, so the lock is uncontended there; the API stays identical.

**D2 — Event bus is the Observer backbone.** A `ports::EventSink` trait + `infra::InProcessBus`
(`Arc<RwLock<Vec<Weak<dyn Listener>>>>`) with synchronous dispatch, plus an optional channel bridge
for async consumers. Commands emit events (CQRS): `execute()` → mutate → publish
`ClipAdded`/`ProjectChanged`/…. *Trade-off:* sync dispatch is simple and deterministic (good for
tests) but a slow listener blocks the command; the channel bridge decouples heavy/async listeners.
Must work single-threaded (WASM/mobile).

**D3 — Render Graph replaces the linear compositor.** A DAG of typed nodes evaluated to produce a
frame; the current `compose_frame` becomes the **CPU backend** of a graph auto-built from the
timeline. *Trade-off:* a graph adds indirection vs. today's straight loop, but it is the
prerequisite for plugin render nodes, GPU offload, per-node caching, and non-linear effects.
Migration keeps `compose_frame` working (built-in graph) so nothing breaks during the transition.

**D4 — Plugin model.** Metadata + capability negotiation + semver. **Static registration on every
platform** (compile-time registry via `inventory`/`linkme`); **dynamic loading only on desktop**
(`libloading`, behind `cfg(not(target_arch="wasm32"))` + a `dynamic-plugins` feature). *Honest note
on sandboxing:* in-process native Rust plugins are **trusted** — true isolation is only achievable
by running plugins as **WASM modules in a host** (a wasmtime-based `WasmPluginHost`), which is a
later, opt-in capability, not a Phase-1 promise.

**D5 — Immutability + CQRS.** Value objects stay immutable (`Transform`, `ColorGrade`, curves);
mutations go only through commands; reads go through queries returning snapshots/borrows. Keeps undo
exact and makes concurrency safe.

**D6 — Async & the FFI boundary.** The engine is async-capable via an injected `TaskExecutor` port
(native: thread-pool; WASM: microtask/`spawn_local`). The **FFI boundary stays synchronous** — long
operations (export, AI, proxy) return a **task handle + progress callback**, never a Rust `Future`
across UniFFI/WASM. This is a hard platform constraint, not a preference.

**D7 — DI via an `Editor` builder.** `Editor::builder().with_gpu(..).with_storage(..)
.with_executor(..).build()`. Defaults wire in-process/CPU implementations so the simple constructor
still works. Every manager is reachable from `Editor` (the single facade the API contract requires).

---

## 4. Dependency graph & recommended build order

Phase numbers below are the **roadmap's** numbers; the **build order** is topologically sorted by
dependency (why it differs from 1→20 is noted).

| Roadmap phase | Depends on |
|---|---|
| 7 Event System | — (substrate) |
| 1 Plugin System | Event(7), ports/DI |
| 5 Resource Cache | — |
| 6 Background Tasks | Event(7) |
| 2 Asset Manager | Cache(5), Tasks(6), Event(7) |
| 3 GPU Abstraction | ports |
| 4 Render Graph | Plugin(1), GPU(3), Cache(5) |
| 9 Preview/Export split | Render Graph(4), Tasks(6) |
| 8 Playback | Render Graph(4), Tasks(6), Event(7) |
| 10 Animation | domain (curves exist) |
| 12 Fonts | Asset(2), Cache(5) |
| 13 Vector (SVG/Lottie) | Render Graph(4), Fonts(12) |
| 11 Subtitles | Fonts(12), Animation(10), Render Graph(4) |
| 14 Project Storage | domain, Event(7) |
| 15 Diagnostics | Tasks(6), Render(4), Playback(8) |
| 16 Profiler | Diagnostics(15) |
| 17 Streaming | Export(9), codecs |
| 18 Collaboration | Command/Event(7,14) |
| 19 Licensing | — (wraps API) |
| 20 Testing | **continuous — every phase** |

**Milestones (build in this order):**

- **Phase 0 — Foundation & compliance** *(new, prerequisite)*: introduce the `domain/app/ports/infra`
  layering (non-breaking, via re-exports), switch `Editor` to `parking_lot::RwLock`, add the minimal
  event bus + `Editor` builder (DI), split the 8 over-cap files, add a `criterion` bench harness and
  per-crate `examples/`. Pays down all rule debt and de-risks every later phase.
- **M-A Foundations:** Plugin System (1) → Resource Cache (5) → Background Tasks (6) → Asset Manager (2).
- **M-B Rendering core:** GPU Abstraction (3) → Render Graph (4) → Preview/Export split (9).
- **M-C Time & motion:** Playback (8) → Animation (10).
- **M-D Content:** Fonts (12) → Vector (13) → Subtitles (11).
- **M-E Productionize:** Project Storage (14) → Diagnostics (15) → Profiler (16).
- **M-F Advanced/commercial:** Streaming (17) → Collaboration (18) → Licensing (19).
- **Testing (20) is not a phase** — golden-image/pixel/audio/regression/bench/fuzz gates ship *with*
  each phase.

---

## 5. Phase 0 — Foundation & compliance (build first)

Goal: satisfy the new architectural rules and lay the substrate, with **zero behaviour change** and
all existing tests green.

- **Layering:** create `core::domain`, `core::app`, `core::ports`, `core::infra` and move modules
  under them, keeping every current `pub use` path working (backward compat). No logic changes.
- **Concurrency:** `Editor` → `Arc<parking_lot::RwLock<Inner>>`; `execute` takes write, `render_frame`
  /queries take read. Public API unchanged.
- **Event bus (minimal):** `ports::EventSink` + `infra::InProcessBus`; `Editor::execute` publishes a
  first event (`ProjectChanged`). Full event catalogue lands in Phase 7.
- **DI:** `Editor::builder()` with defaulted ports; existing `Editor::new` delegates to it.
- **Split over-cap files:** e.g. `timeline.rs` → `timeline/{project,clip,track,audio,animation}.rs`;
  `audio/effects.rs` → `effects/{dynamics,eq,reverb,delay,spatial}.rs`; `fmp4_mux.rs`/`demux_graph.rs`
  → submodules. Pure moves + re-exports.
- **Tooling:** add `benches/` (criterion) and `examples/` skeletons per crate.
- **DoD:** 268 tests still pass; no file > 400 lines in touched crates; clippy clean; docs build.

---

## 6. Phase 1 — Plugin System (first feature phase)

**Placement:** `core::plugin` (pure Rust, WASM-safe). Dynamic loading isolated behind
`cfg` + `dynamic-plugins` feature. Module split (each < 400 lines):

```
core/src/plugin/
  mod.rs          facade + re-exports
  metadata.rs     PluginMetadata, semver Version, author/license
  capability.rs   Capability set + negotiation (host ⇄ plugin)
  kind.rs         PluginKind { VideoEffect, AudioEffect, Transition,
                               Exporter, Importer, Ai, RenderNode }
  traits/…        one trait per kind (effects.rs, transition.rs, exporter.rs,
                  importer.rs, ai.rs, render_node.rs)
  lifecycle.rs    Lifecycle { Registered, Enabled, Disabled, Failed } + hooks
  registry.rs     PluginRegistry (RwLock; lookup by id/kind/capability)
  manager.rs      PluginManager: discover/register/enable/disable, emits events
  static_reg.rs   compile-time registration (inventory) — all platforms
  dynamic.rs      #[cfg(desktop)] libloading loader — desktop only
  error.rs        PluginError
```

- **Adapt existing effects as built-in plugins** via thin adapter shims (do **not** rewrite the DSP):
  `color`, `keyer`, `mask`, `blend`, `transitions`, and each audio effect register at startup as
  `VideoEffect`/`AudioEffect`/`Transition` plugins. Proves the abstraction against real code.
- **Capability negotiation:** plugins declare required host capabilities + version range; the manager
  accepts/rejects and reports mismatches.
- **Lifecycle & enable/disable:** toggling a plugin emits `PluginLoaded`/`PluginDisabled` on the bus.
- **Editor API:** `editor.plugins() -> &PluginManager`.
- **Platforms:** static registration everywhere; `libloading` compiled only off-wasm behind the
  feature. WASM/mobile ship a fixed, statically-registered plugin set.
- **DoD:** unit tests (registry lookup, version/capability match, lifecycle transitions, built-in
  registration, enable/disable, error paths); a `plugins` example; a criterion bench for registry
  lookup; module docs + an `architecture` note + sequence sketch for plugin resolution.

---

## 7. Definition of Done (every phase)

Architecture note + sequence sketch · rustdoc on all public items · usage example · unit tests ·
criterion bench · golden/pixel or audio regression where visual/audible · clippy clean · **no file
> 400 lines, no fn > 60 lines** · backward compatible (old project JSON still loads) · WASM + UniFFI
still build.

---

## 8. Honest scope & risk notes

- This is a **multi-month program**, not a single change. Each phase is an independent, shippable
  increment; I build and verify one at a time.
- **True plugin sandboxing** needs a WASM host (wasmtime) — deferred, opt-in; native plugins are trusted.
- **Streaming (RTMP/HLS/DASH), Collaboration (CRDT), Licensing/telemetry** carry heavy scope and some
  external dependencies/network I/O — they live in `infra` behind ports so the domain stays pure, and
  are sequenced last.
- **Async/threads on WASM** are constrained (single-threaded, no blocking): the `TaskExecutor` port
  has a WASM implementation; the FFI surface stays sync + callbacks.
- **Render Graph migration** is the highest-risk refactor; it is gated behind keeping `compose_frame`
  as a built-in graph so the SDK never regresses mid-migration.
