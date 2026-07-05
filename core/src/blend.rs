//! Photoshop / Porter-Duff **blend modes** for layer compositing.
//!
//! Each mode is a separable per-channel function `blend(d, s)` on straight
//! (non-premultiplied) values in `[0, 1]`. The compositor combines it with the
//! source coverage `a` via `out = d·(1 − a) + a·blend(d, s)`, which reduces to
//! plain "over" when the mode is [`BlendMode::Normal`].

use serde::{Deserialize, Serialize};

/// How a clip's pixels combine with the layers beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Add,
}

impl BlendMode {
    /// True for the default pass-through mode (used to omit it from JSON).
    pub fn is_normal(&self) -> bool {
        matches!(self, BlendMode::Normal)
    }

    /// Blend a backdrop channel `d` with a source channel `s` (both in `[0,1]`).
    pub fn blend(self, d: f32, s: f32) -> f32 {
        let out = match self {
            BlendMode::Normal => s,
            BlendMode::Multiply => d * s,
            BlendMode::Screen => 1.0 - (1.0 - d) * (1.0 - s),
            BlendMode::Overlay => hard_light(s, d),
            BlendMode::HardLight => hard_light(d, s),
            BlendMode::Darken => d.min(s),
            BlendMode::Lighten => d.max(s),
            BlendMode::Difference => (d - s).abs(),
            BlendMode::Exclusion => d + s - 2.0 * d * s,
            BlendMode::Add => d + s,
            BlendMode::SoftLight => soft_light(d, s),
        };
        out.clamp(0.0, 1.0)
    }
}

/// HardLight(d, s): overlay of `s` onto `d` (also the kernel for Overlay swapped).
fn hard_light(d: f32, s: f32) -> f32 {
    if s <= 0.5 {
        2.0 * d * s
    } else {
        1.0 - 2.0 * (1.0 - d) * (1.0 - s)
    }
}

/// W3C soft-light.
fn soft_light(d: f32, s: f32) -> f32 {
    if s <= 0.5 {
        d - (1.0 - 2.0 * s) * d * (1.0 - d)
    } else {
        let dd = if d <= 0.25 { ((16.0 * d - 12.0) * d + 4.0) * d } else { d.sqrt() };
        d + (2.0 * s - 1.0) * (dd - d)
    }
}
