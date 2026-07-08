//! # ferrox-sdk — the Unified Editor SDK
//!
//! A handle-based, FFI-friendly facade over `ferrox-core`. It wraps a
//! [`ferrox_core::Project`] as the single source of truth and mutates it only
//! through an undo/redo [`Command`] stack. The same [`Editor`] type is driven by
//! the CLI directly and, via thin binding layers, by WASM (web) and UniFFI
//! (Android/iOS) — with identical semantics.
//!
//! ```
//! use ferrox_sdk::Editor;
//! use ferrox_core::{Clip, ClipSource, Transform};
//!
//! let editor = Editor::new(1920, 1080, 30.0);
//! let track = editor.add_track().unwrap();
//! editor.add_clip(track, Clip::new(
//!     ClipSource::Solid { width: 1920, height: 1080, r: 20, g: 30, b: 40, a: 255 },
//!     0.0, 5.0, Transform::default(),
//! )).unwrap();
//! let rgba = editor.render_frame(1.0, 0, 0).unwrap();
//! assert_eq!(rgba.len(), 1920 * 1080 * 4);
//! editor.undo().unwrap();
//! ```

pub mod commands;
pub mod editor;
pub mod error;
#[cfg(feature = "export")]
pub mod export;
pub mod project_io;

pub use commands::{
    AddClipCommand, AddKeyframeCommand, AddTrackCommand, AnimField, Command, MoveClipCommand,
    RemoveClipCommand, RemoveKeyframeCommand, SetBlendModeCommand, SetColorGradeCommand,
    SetKeyerCommand, TrimClipCommand,
};
pub use editor::{Editor, EditorBuilder};
pub use error::{Result, SdkError};
pub use ferrox_core::{Event, EventListener, EventSink, InProcessBus, NoopSink};
pub use ferrox_core::plugin::{
    Capability, CapabilitySet, Plugin, PluginKind, PluginManager, PLUGIN_API_VERSION,
};
#[cfg(feature = "export")]
pub use export::{export_mp4, ExportSettings};

/// Re-export of the core types consumers build projects from.
pub use ferrox_core::{
    AscCdl, BlendMode, Clip, ClipAnimation, ClipSource, ColorGrade, Curve, Easing, Keyer, Keyframe,
    Lut3D, Mask, Project, Track, Transform,
};
