//! Keyframe **animation** — the engine that lets any scalar parameter (position,
//! scale, rotation, opacity, effect intensity, audio gain, …) vary over time.
//!
//! A [`Curve`] is either a constant or a sorted list of [`Keyframe`]s. Each
//! segment interpolates with an [`Easing`] (linear, hold/step, ease presets, or
//! an arbitrary CSS-style cubic-bezier). `sample(t)` clamps outside the keyframe
//! range. Transitions (fade, cross-dissolve, …) are built on top of this — they
//! are just keyframed opacity/transform curves.

use serde::{Deserialize, Serialize};

/// How a segment interpolates from one keyframe to the next.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    /// Straight linear ramp.
    #[default]
    Linear,
    /// Step — hold the start value until the next keyframe.
    Hold,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Arbitrary CSS-style cubic-bezier control points `(x1, y1, x2, y2)`.
    Bezier { x1: f32, y1: f32, x2: f32, y2: f32 },
}

impl Easing {
    /// Map a normalised segment progress `u` in [0,1] to an eased output in [0,1].
    pub fn ease(self, u: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Easing::Linear => u,
            Easing::Hold => 0.0,
            Easing::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, u),
            Easing::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, u),
            Easing::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, u),
            Easing::Bezier { x1, y1, x2, y2 } => cubic_bezier(x1, y1, x2, y2, u),
        }
    }
}

/// One keyframe: a `value` at time `t` (seconds), and the [`Easing`] used to
/// interpolate **from this keyframe to the next**.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub t: f64,
    pub v: f32,
    #[serde(default)]
    pub ease: Easing,
}

impl Keyframe {
    pub fn new(t: f64, v: f32) -> Self {
        Self { t, v, ease: Easing::Linear }
    }

    pub fn with_ease(mut self, ease: Easing) -> Self {
        self.ease = ease;
        self
    }
}

/// An animation curve for a single scalar parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Curve {
    /// A fixed value for all time.
    Const(f32),
    /// A keyframed value. Keyframes should be sorted by `t` (use [`Curve::keyed`]).
    Keyed(Vec<Keyframe>),
}

impl Curve {
    /// Build a keyed curve, sorting keyframes by time.
    pub fn keyed(mut keys: Vec<Keyframe>) -> Self {
        keys.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
        Curve::Keyed(keys)
    }

    /// Sample the curve at time `t` (seconds). Clamps before the first / after
    /// the last keyframe.
    pub fn sample(&self, t: f64) -> f32 {
        match self {
            Curve::Const(v) => *v,
            Curve::Keyed(keys) => {
                match keys.as_slice() {
                    [] => 0.0,
                    [only] => only.v,
                    _ => {
                        if t <= keys[0].t {
                            return keys[0].v;
                        }
                        let last = keys.len() - 1;
                        if t >= keys[last].t {
                            return keys[last].v;
                        }
                        // Find the segment [k0, k1] containing t.
                        let i = keys.partition_point(|k| k.t <= t) - 1;
                        let k0 = &keys[i];
                        let k1 = &keys[i + 1];
                        let span = (k1.t - k0.t).max(f64::MIN_POSITIVE);
                        let u = ((t - k0.t) / span) as f32;
                        k0.v + (k1.v - k0.v) * k0.ease.ease(u)
                    }
                }
            }
        }
    }
}

/// Evaluate a CSS-style cubic-bezier timing curve at parameter `u` (its x-axis
/// input), returning the eased y value. Control points are `(0,0)`, `(x1,y1)`,
/// `(x2,y2)`, `(1,1)`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, u: f32) -> f32 {
    // Bezier basis for the given axis control values c1, c2 (with p0=0, p3=1).
    let axis = |c1: f32, c2: f32, s: f32| {
        let mt = 1.0 - s;
        3.0 * mt * mt * s * c1 + 3.0 * mt * s * s * c2 + s * s * s
    };
    // Invert x(s) = u for the parameter s via Newton's method + bisection fallback.
    let mut s = u;
    for _ in 0..8 {
        let x = axis(x1, x2, s) - u;
        if x.abs() < 1e-6 {
            break;
        }
        // dx/ds
        let mt = 1.0 - s;
        let dx = 3.0 * mt * mt * x1 + 6.0 * mt * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2);
        if dx.abs() < 1e-6 {
            break;
        }
        s = (s - x / dx).clamp(0.0, 1.0);
    }
    axis(y1, y2, s)
}
