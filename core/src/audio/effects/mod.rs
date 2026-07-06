//! Pure-Rust audio DSP effects. Every processor implements [`crate::traits::AudioFilter`],
//! so it works standalone, in an [`crate::graph::AudioGraph`], or as a clip effect
//! in the [`crate::audio::mixer`].
//!
//! [`AudioEffect`] is a serialisable parameter enum (part of a saved project);
//! [`AudioEffect::build`] turns it into the corresponding processor, and
//! [`apply_effects`] runs an ordered effect stack over a frame.
//!
//! All processors read `sample_rate` from the frame at process time and hold
//! their filter state locally for the duration of one `process_audio` call, so
//! no interior mutability is needed and `&self` stays immutable. Split into
//! focused submodules; the public surface is unchanged.

mod biquad;
mod dsp;
mod dynamics;
mod effect;
mod filters;
mod spatial;

pub use biquad::{EqBand, EqKind};
pub use dsp::db_to_linear;
pub use effect::{apply_effects, AudioEffect};
pub use filters::{EqFilter, GainFilter, NormalizeFilter, PanFilter};
pub use spatial::{DelayFilter, ReverbFilter};
