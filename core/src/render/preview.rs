//! # Preview vs. Export (Phase 9)
//!
//! One engine, two rendering profiles:
//! - **Export** — full resolution, deterministic (bit-identical on re-render).
//! - **Preview** — a reduced-resolution render (genuinely cheaper: the whole
//!   composite runs at the smaller size) for low-latency scrubbing/playback,
//!   paired with an [`AdaptiveQuality`] controller and a frame-skip helper.
//!
//! Both go through the proven [`compose_frame_graded`]; migrating them onto the
//! render graph/backend is transparent to callers.

use crate::anim::{Curve, Keyframe};
use crate::color::Lut3D;
use crate::compositor::compose_frame_graded;
use crate::error::Result;
use crate::frame::Frame;
use crate::timeline::Project;

/// How to render a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderProfile {
    /// Output resolution as a fraction of the project size (`1.0` = full).
    pub resolution_scale: f32,
    /// Whether rendering must be deterministic (export).
    pub deterministic: bool,
}

impl RenderProfile {
    /// Full-resolution, deterministic — for final export.
    pub fn export() -> Self {
        Self { resolution_scale: 1.0, deterministic: true }
    }

    /// A reduced-resolution preview at `scale` in `(0, 1]`.
    pub fn preview(scale: f32) -> Self {
        Self { resolution_scale: scale.clamp(0.05, 1.0), deterministic: false }
    }
}

/// Render `project` at time `t` under `profile`, optionally applying an output LUT.
pub fn render(project: &Project, t: f64, profile: &RenderProfile, output_lut: Option<&Lut3D>) -> Result<Frame> {
    if profile.resolution_scale >= 0.999 {
        compose_frame_graded(project, t, output_lut)
    } else {
        let scaled = scale_project(project, profile.resolution_scale);
        compose_frame_graded(&scaled, t, output_lut)
    }
}

/// A copy of `project` scaled to `factor` — dimensions and every clip's
/// position/scale (including keyframes) shrink together, so the composite runs
/// at the smaller size while looking the same.
fn scale_project(project: &Project, factor: f32) -> Project {
    let mut p = project.clone();
    p.width = ((p.width as f32 * factor).round() as u32).max(1);
    p.height = ((p.height as f32 * factor).round() as u32).max(1);
    for track in &mut p.tracks {
        for clip in &mut track.clips {
            clip.transform.x = (clip.transform.x as f32 * factor).round() as i32;
            clip.transform.y = (clip.transform.y as f32 * factor).round() as i32;
            clip.transform.scale *= factor;
            scale_field(&mut clip.animation.x, factor);
            scale_field(&mut clip.animation.y, factor);
            scale_field(&mut clip.animation.scale, factor);
            // opacity is not spatial → unchanged.
        }
    }
    p
}

fn scale_field(field: &mut Option<Curve>, factor: f32) {
    if let Some(curve) = field {
        *curve = match curve {
            Curve::Const(v) => Curve::Const(*v * factor),
            Curve::Keyed(keys) => {
                Curve::Keyed(keys.iter().map(|k| Keyframe { t: k.t, v: k.v * factor, ease: k.ease }).collect())
            }
        };
    }
}

/// A hysteresis controller that adapts the preview resolution scale to keep
/// rendering under a frame-time budget — the "adaptive quality" of the preview.
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveQuality {
    target_ms: f32,
    min_scale: f32,
    max_scale: f32,
    scale: f32,
}

impl AdaptiveQuality {
    /// Aim for `target_ms` per rendered frame (e.g. 16.6 for 60fps).
    pub fn new(target_ms: f32) -> Self {
        Self { target_ms: target_ms.max(1.0), min_scale: 0.25, max_scale: 1.0, scale: 1.0 }
    }

    /// The current resolution scale.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Feed the last frame's render time; returns the scale to use next.
    pub fn update(&mut self, last_render_ms: f32) -> f32 {
        if last_render_ms > self.target_ms * 1.2 {
            self.scale = (self.scale * 0.8).max(self.min_scale); // too slow → drop quality
        } else if last_render_ms < self.target_ms * 0.6 {
            self.scale = (self.scale * 1.1).min(self.max_scale); // headroom → raise quality
        }
        self.scale
    }

    /// A preview profile at the current adaptive scale.
    pub fn profile(&self) -> RenderProfile {
        RenderProfile::preview(self.scale)
    }
}

/// How many frames to skip to catch up when playback has fallen `behind_secs`
/// behind at `fps` (frame-skipping for low-latency playback).
pub fn frames_to_skip(behind_secs: f64, fps: f64) -> u32 {
    if behind_secs <= 0.0 || fps <= 0.0 {
        0
    } else {
        (behind_secs * fps).floor() as u32
    }
}
