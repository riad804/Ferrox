//! The editing **timeline** data model — the piece that turns ferrox from a
//! frame-processing library into a video *editing* engine.
//!
//! A [`Project`] holds ordered [`Track`]s of [`Clip`]s (and [`AudioTrack`]s of
//! [`AudioClip`]s) placed on a shared time axis (seconds). The model is fully
//! `serde`-serialisable, so `Project::to_json` / `Project::from_json` are project
//! save/load. The [`crate::compositor`] and [`crate::audio::mixer`] consume it.
//!
//! Split into focused submodules; the public surface is unchanged (re-exported
//! here and at the crate root).

mod audio;
mod clip;
mod project;
mod source;
mod transform;

pub use audio::{AudioClip, AudioClipSource, AudioTrack, Fade};
pub use clip::{Clip, Track};
pub use project::Project;
pub use source::ClipSource;
pub use transform::{ClipAnimation, Transform};
