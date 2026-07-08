//! [`AnimationGroup`] — a reusable, **nestable** bundle of named [`Curve`]s.
//!
//! A group animates several targets together (e.g. a "pop-in" preset animating
//! `scale` + `opacity`), can be time-**offset**, and can contain child groups
//! whose offsets compose with the parent's. Groups convert to a
//! [`crate::timeline::ClipAnimation`] (mapping the `x`/`y`/`scale`/`opacity`
//! targets) so they drop straight into the compositor.

use serde::{Deserialize, Serialize};

use super::{Curve, Keyframe};

/// A named collection of animation curves, offsettable and nestable.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AnimationGroup {
    /// Named target → curve (e.g. `("opacity", …)`).
    #[serde(default)]
    pub tracks: Vec<(String, Curve)>,
    /// Nested sub-groups; their offsets compose with this group's.
    #[serde(default)]
    pub children: Vec<AnimationGroup>,
    /// Time offset (seconds) applied to every track and child.
    #[serde(default)]
    pub offset: f64,
}

impl AnimationGroup {
    pub fn new() -> Self {
        Self::default()
    }

    /// A group with a time offset.
    pub fn with_offset(offset: f64) -> Self {
        Self { offset, ..Self::default() }
    }

    /// Add a named curve (builder style).
    pub fn track(mut self, target: impl Into<String>, curve: Curve) -> Self {
        self.tracks.push((target.into(), curve));
        self
    }

    /// Add a nested child group (builder style).
    pub fn child(mut self, group: AnimationGroup) -> Self {
        self.children.push(group);
        self
    }

    /// Sample `target` at time `t`, honouring this group's offset and searching
    /// nested children. `None` if no such target exists.
    pub fn sample(&self, target: &str, t: f64) -> Option<f32> {
        let local = t - self.offset;
        for (name, curve) in &self.tracks {
            if name == target {
                return Some(curve.sample(local));
            }
        }
        for child in &self.children {
            if let Some(v) = child.sample(target, local) {
                return Some(v);
            }
        }
        None
    }

    /// The curve for `target` with this group's (and any ancestor) offset baked
    /// into its keyframe times — for direct use as a [`Curve`].
    pub fn resolved_curve(&self, target: &str) -> Option<Curve> {
        for (name, curve) in &self.tracks {
            if name == target {
                return Some(shift_curve(curve, self.offset));
            }
        }
        for child in &self.children {
            if let Some(c) = child.resolved_curve(target) {
                return Some(shift_curve(&c, self.offset));
            }
        }
        None
    }

    /// Build a [`crate::timeline::ClipAnimation`] from the `x`/`y`/`scale`/
    /// `opacity` targets (offsets baked in).
    pub fn to_clip_animation(&self) -> crate::timeline::ClipAnimation {
        crate::timeline::ClipAnimation {
            x: self.resolved_curve("x"),
            y: self.resolved_curve("y"),
            scale: self.resolved_curve("scale"),
            opacity: self.resolved_curve("opacity"),
        }
    }
}

/// Shift a curve's keyframe times by `offset` seconds.
fn shift_curve(curve: &Curve, offset: f64) -> Curve {
    match curve {
        Curve::Const(v) => Curve::Const(*v),
        Curve::Keyed(keys) => {
            Curve::Keyed(keys.iter().map(|k| Keyframe { t: k.t + offset, v: k.v, ease: k.ease }).collect())
        }
    }
}
