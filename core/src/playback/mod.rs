//! # Playback engine (Phase 8)
//!
//! Play / pause / stop / seek / loop / speed / reverse / frame-stepping /
//! scrubbing over a timeline, driving the Phase-9 preview renderer.
//!
//! - [`Transport`] — the pure playhead state machine (WASM-safe, deterministic);
//!   variable-timestep [`Transport::advance`] is also the frame scheduler.
//! - [`PlaybackController`] — renders the current frame at a chosen
//!   [`crate::render::RenderProfile`] and mixes synchronised audio.
//!
//! The **host owns the loop**: measure elapsed wall-clock time (native thread or
//! the browser's animation frame — the engine stays thread-free), call
//! [`PlaybackController::advance`], render, and feed the render time to an
//! [`crate::render::AdaptiveQuality`] to keep latency low. Background asset
//! decoding runs on the [`crate::task`] pool (native).
//!
//! ```
//! use ferrox_core::playback::{PlaybackController, PlayState};
//! use ferrox_core::{Project, RenderProfile};
//!
//! let project = Project::new(64, 64, 30.0);
//! let mut pc = PlaybackController::for_project(&project);
//! pc.transport().play();
//! pc.advance(0.1); // 100ms of wall-clock time elapsed
//! let _frame = pc.render(&project, &RenderProfile::preview(0.5), None).unwrap();
//! assert_eq!(pc.transport().state(), PlayState::Playing);
//! ```

pub mod controller;
pub mod transport;

pub use controller::PlaybackController;
pub use transport::{PlayState, Transport};
