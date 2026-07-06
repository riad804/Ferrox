//! [`Clip`] — a placed visual clip with transform, animation, and effects.

use serde::{Deserialize, Serialize};

use crate::blend::BlendMode;
use crate::color::ColorGrade;
use crate::keyer::Keyer;
use crate::mask::Mask;

use super::source::ClipSource;
use super::transform::{ClipAnimation, Transform};

/// A single placed clip on a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub source: ClipSource,
    /// Timeline start time, in seconds.
    pub start: f64,
    /// How long the clip is visible, in seconds (must be > 0 to ever show).
    pub duration: f64,
    #[serde(default)]
    pub transform: Transform,
    /// Optional keyframe animation overriding transform fields over time.
    #[serde(default, skip_serializing_if = "ClipAnimation::is_empty")]
    pub animation: ClipAnimation,
    /// Layer blend mode against the layers beneath.
    #[serde(default, skip_serializing_if = "BlendMode::is_normal")]
    pub blend: BlendMode,
    /// Per-clip color grade (applied before compositing).
    #[serde(default, skip_serializing_if = "ColorGrade::is_identity")]
    pub color: ColorGrade,
    /// Optional chroma-key applied before compositing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyer: Option<Keyer>,
    /// Optional vector mask multiplying the clip's alpha.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Mask>,
}

impl Clip {
    /// A clip of `source` visible for `[start, start + duration)` at `transform`.
    pub fn new(source: ClipSource, start: f64, duration: f64, transform: Transform) -> Self {
        Self {
            source,
            start,
            duration,
            transform,
            animation: ClipAnimation::default(),
            blend: BlendMode::default(),
            color: ColorGrade::default(),
            keyer: None,
            mask: None,
        }
    }

    /// Attach keyframe animation and return `self`.
    pub fn with_animation(mut self, animation: ClipAnimation) -> Self {
        self.animation = animation;
        self
    }

    /// Set the layer blend mode and return `self`.
    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        self.blend = blend;
        self
    }

    /// Set the per-clip color grade and return `self`.
    pub fn with_color(mut self, color: ColorGrade) -> Self {
        self.color = color;
        self
    }

    /// Attach a chroma-keyer and return `self`.
    pub fn with_keyer(mut self, keyer: Keyer) -> Self {
        self.keyer = Some(keyer);
        self
    }

    /// Attach a vector mask and return `self`.
    pub fn with_mask(mut self, mask: Mask) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Whether this clip is visible at timeline time `t` (seconds).
    pub fn is_active(&self, t: f64) -> bool {
        self.duration > 0.0 && t >= self.start && t < self.start + self.duration
    }

    /// The clip's transform at timeline time `t`, with any animation applied
    /// (evaluated at clip-local time `t - start`).
    pub fn effective_transform(&self, t: f64) -> Transform {
        let mut tf = self.transform;
        if !self.animation.is_empty() {
            self.animation.apply(&mut tf, t - self.start);
        }
        tf
    }
}

/// An ordered stack of clips. Lower track index = drawn first (further back).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub clips: Vec<Clip>,
}

impl Track {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a clip and return `self` for fluent construction.
    pub fn with_clip(mut self, clip: Clip) -> Self {
        self.clips.push(clip);
        self
    }
}
