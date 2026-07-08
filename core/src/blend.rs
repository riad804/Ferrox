//! Photoshop / Porter-Duff **blend modes** for layer compositing.
//!
//! Each mode is a separable per-channel function `blend(d, s)` on straight
//! (non-premultiplied) values in `[0, 1]`. The compositor combines it with the
//! source coverage `a` via `out = d·(1 − a) + a·blend(d, s)`, which reduces to
//! plain "over" when the mode is [`BlendMode::Normal`].

use serde::{Deserialize, Serialize};

use crate::frame::Frame;

/// Composite `top` over `base` (both `Rgba8`) at top-left `(x, y)` using `mode`,
/// scaling the source coverage by `opacity`: `out = d·(1 − a) + a·blend(d, s)`.
/// `base` stays fully opaque. This is the shared compositing kernel used by the
/// compositor and the render backends.
pub fn composite_over(base: &mut Frame, top: &Frame, x: i32, y: i32, opacity: f32, mode: BlendMode) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity == 0.0 {
        return;
    }
    let (cw, ch) = (base.width as i32, base.height as i32);
    let (sw, sh) = (top.width as i32, top.height as i32);

    for sy in 0..sh {
        let cy = y + sy;
        if cy < 0 || cy >= ch {
            continue;
        }
        for sx in 0..sw {
            let cx = x + sx;
            if cx < 0 || cx >= cw {
                continue;
            }
            let si = ((sy * sw + sx) * 4) as usize;
            let ci = ((cy * cw + cx) * 4) as usize;

            let a = (top.data[si + 3] as f32 / 255.0) * opacity;
            if a == 0.0 {
                continue;
            }
            let inv = 1.0 - a;
            for k in 0..3 {
                let s = top.data[si + k] as f32 / 255.0;
                let d = base.data[ci + k] as f32 / 255.0;
                let out = d * inv + a * mode.blend(d, s);
                base.data[ci + k] = (out.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            base.data[ci + 3] = 255;
        }
    }
}

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
