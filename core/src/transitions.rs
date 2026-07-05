//! **Spatial transitions** — built as pre-canned [`ClipAnimation`] curves rather
//! than special-cased render passes, honoring "the keyframe engine is king".
//!
//! A transition is just keyframed transform/opacity applied to the *incoming*
//! clip; two overlapping clips on the timeline therefore blend through the normal
//! compositor with no bespoke logic. Slide and Zoom are pure transforms; Fade is
//! opacity. (Wipe/Iris additionally need an animated mask — landing with the
//! keyframed-mask increment.)
//!
//! All builders take clip-local seconds and produce curves over `[0, secs]`.

use serde::{Deserialize, Serialize};

use crate::anim::{Curve, Easing, Keyframe};
use crate::timeline::ClipAnimation;

/// Direction an incoming clip slides in from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Factory of transition animations for an incoming clip.
pub struct Transition;

impl Transition {
    /// Fade opacity 0→1 over the first `secs` seconds.
    pub fn fade_in(secs: f64) -> ClipAnimation {
        ClipAnimation::fade_in(secs)
    }

    /// Fade opacity 1→0 over the last `secs` seconds of a `clip_dur` clip.
    pub fn fade_out(clip_dur: f64, secs: f64) -> ClipAnimation {
        ClipAnimation::fade_out(clip_dur, secs)
    }

    /// Slide the clip in from `dir`, starting `offset_px` off-position and easing
    /// to its resting transform over `secs`, fading opacity in alongside.
    pub fn slide_in(dir: Direction, offset_px: i32, secs: f64, ease: Easing) -> ClipAnimation {
        let start = offset_px as f32;
        let axis = Curve::keyed(vec![Keyframe::new(0.0, start).with_ease(ease), Keyframe::new(secs, 0.0)]);
        let mut anim = ClipAnimation {
            opacity: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(secs, 1.0)])),
            ..ClipAnimation::default()
        };
        match dir {
            Direction::Left => anim.x = Some(negate(&axis)),
            Direction::Right => anim.x = Some(axis),
            Direction::Up => anim.y = Some(negate(&axis)),
            Direction::Down => anim.y = Some(axis),
        }
        anim
    }

    /// Zoom the clip in: scale 0→1 (with a circular reveal feel) and opacity 0→1
    /// over `secs`.
    pub fn zoom_in(secs: f64, ease: Easing) -> ClipAnimation {
        ClipAnimation {
            scale: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0).with_ease(ease), Keyframe::new(secs, 1.0)])),
            opacity: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(secs, 1.0)])),
            ..ClipAnimation::default()
        }
    }
}

/// Negate the values of a curve (used to mirror a slide direction).
fn negate(c: &Curve) -> Curve {
    match c {
        Curve::Const(v) => Curve::Const(-v),
        Curve::Keyed(keys) => {
            Curve::Keyed(keys.iter().map(|k| Keyframe { t: k.t, v: -k.v, ease: k.ease }).collect())
        }
    }
}
