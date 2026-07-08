//! Per-clip [`Transform`] and its optional keyframe [`ClipAnimation`].

use serde::{Deserialize, Serialize};

use crate::anim::{Curve, Keyframe};

/// A per-clip 2-D transform applied when compositing onto the output canvas.
///
/// `x`/`y` are the top-left placement of the (scaled) clip in output pixels.
/// `scale` is a uniform factor (1.0 = original size). `opacity` is `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default = "one")]
    pub opacity: f32,
}

fn one() -> f32 {
    1.0
}

impl Default for Transform {
    fn default() -> Self {
        Self { x: 0, y: 0, scale: 1.0, opacity: 1.0 }
    }
}

impl Transform {
    /// A transform that places the clip at `(x, y)` at original size, opaque.
    pub fn at(x: i32, y: i32) -> Self {
        Self { x, y, ..Self::default() }
    }
}

/// Optional per-field keyframe animation for a clip's [`Transform`], evaluated
/// against **clip-local** time (seconds since the clip's `start`). Only the
/// fields with a curve are animated; the rest keep the static `transform`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipAnimation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Curve>,
}

impl ClipAnimation {
    /// True when no field is animated (used to omit it from serialised JSON).
    pub fn is_empty(&self) -> bool {
        self.x.is_none() && self.y.is_none() && self.scale.is_none() && self.opacity.is_none()
    }

    /// Override the animated fields of `tf` at clip-local time `local_t`.
    pub fn apply(&self, tf: &mut Transform, local_t: f64) {
        if let Some(c) = &self.x {
            tf.x = c.sample(local_t).round() as i32;
        }
        if let Some(c) = &self.y {
            tf.y = c.sample(local_t).round() as i32;
        }
        if let Some(c) = &self.scale {
            tf.scale = c.sample(local_t);
        }
        if let Some(c) = &self.opacity {
            tf.opacity = c.sample(local_t);
        }
    }

    /// Animate opacity 0→1 over the first `secs` seconds (fade in).
    pub fn fade_in(secs: f64) -> Self {
        Self {
            opacity: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(secs, 1.0)])),
            ..Self::default()
        }
    }

    /// Animate opacity 1→0 over the last `secs` seconds of a `clip_dur` clip (fade out).
    pub fn fade_out(clip_dur: f64, secs: f64) -> Self {
        Self {
            opacity: Some(Curve::keyed(vec![
                Keyframe::new((clip_dur - secs).max(0.0), 1.0),
                Keyframe::new(clip_dur, 0.0),
            ])),
            ..Self::default()
        }
    }
}
