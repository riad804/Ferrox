//! Subtitle engine (Phase 11): parse SRT / WebVTT / ASS-SSA into a unified
//! [`Subtitle`] model and render cues onto frames.
//!
//! Parsing and the data model are pure and always available. Rendering builds
//! on the font rasteriser and is gated behind the `filters-extra` feature.
//!
//! ```
//! use ferrox_core::subtitle::{Subtitle, SubtitleFormat};
//! let subs = Subtitle::from_srt("1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();
//! assert_eq!(subs.active_cues(1.5).len(), 1);
//! assert_eq!(subs.active_cues(3.0).len(), 0);
//! let _ = SubtitleFormat::Srt;
//! ```

mod model;
mod parse;

pub use model::{Cue, KaraokeSegment, Subtitle};
pub use parse::SubtitleFormat;

#[cfg(feature = "filters-extra")]
mod render;
#[cfg(feature = "filters-extra")]
pub use render::{SubtitleRenderer, SubtitleStyle};
