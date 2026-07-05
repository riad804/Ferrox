//! Vector **masks** — Rectangle, Ellipse and Polygon regions with feathering and
//! inversion, evaluated in normalised `[0,1]²` clip space so they are
//! resolution-independent and (later) keyframe-animatable.
//!
//! A mask multiplies a frame's alpha by its per-pixel coverage, so it composes
//! naturally with the keyer, color grade and blend stages of the compositor.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};

/// A feather-able, invertible vector mask in normalised `[0,1]` coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Mask {
    /// Axis-aligned rectangle at `(x, y)` of size `w × h`.
    Rectangle {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default)]
        feather: f32,
        #[serde(default)]
        invert: bool,
    },
    /// Ellipse centred at `(cx, cy)` with radii `(rx, ry)`.
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        #[serde(default)]
        feather: f32,
        #[serde(default)]
        invert: bool,
    },
    /// Closed polygon through `points` (even-odd fill).
    Polygon {
        points: Vec<[f32; 2]>,
        #[serde(default)]
        feather: f32,
        #[serde(default)]
        invert: bool,
    },
}

impl Mask {
    /// Coverage in `[0,1]` at normalised point `(u, v)` (1 = fully inside).
    pub fn coverage(&self, u: f32, v: f32) -> f32 {
        let (signed, feather, invert) = match self {
            Mask::Rectangle { x, y, w, h, feather, invert } => {
                let dx = (u - x).min(x + w - u);
                let dy = (v - y).min(y + h - v);
                (dx.min(dy), *feather, *invert)
            }
            Mask::Ellipse { cx, cy, rx, ry, feather, invert } => {
                let nx = if *rx > 0.0 { (u - cx) / rx } else { f32::INFINITY };
                let ny = if *ry > 0.0 { (v - cy) / ry } else { f32::INFINITY };
                let nd = (nx * nx + ny * ny).sqrt();
                // Convert normalised-radius distance to a spatial-ish signed distance.
                ((1.0 - nd) * rx.min(*ry), *feather, *invert)
            }
            Mask::Polygon { points, feather, invert } => {
                (polygon_signed_distance(points, u, v), *feather, *invert)
            }
        };
        let mut cov = feathered(signed, feather);
        if invert {
            cov = 1.0 - cov;
        }
        cov
    }

    /// Multiply the alpha of every pixel of an `Rgba8` frame by this mask's
    /// coverage. Coordinates map the frame's extent to `[0,1]²`.
    pub fn apply_frame(&self, frame: &mut Frame) -> Result<()> {
        if frame.format != PixelFormat::Rgba8 {
            return Err(Error::Filter(format!("mask needs Rgba8, got {:?}", frame.format)));
        }
        let (w, h) = (frame.width.max(1), frame.height.max(1));
        let (fw, fh) = ((w - 1).max(1) as f32, (h - 1).max(1) as f32);
        for y in 0..h {
            let v = y as f32 / fh;
            for x in 0..w {
                let u = x as f32 / fw;
                let cov = self.coverage(u, v).clamp(0.0, 1.0);
                let idx = ((y * frame.width + x) * 4 + 3) as usize;
                frame.data[idx] = (frame.data[idx] as f32 * cov).round() as u8;
            }
        }
        Ok(())
    }
}

/// Map a signed distance (positive inside) to `[0,1]` coverage across a feather
/// band. `feather <= 0` gives a crisp edge.
fn feathered(signed: f32, feather: f32) -> f32 {
    if feather <= 0.0 {
        if signed >= 0.0 {
            1.0
        } else {
            0.0
        }
    } else {
        (0.5 + signed / feather).clamp(0.0, 1.0)
    }
}

/// Signed distance from `(u,v)` to a polygon boundary: `+dist` inside (even-odd),
/// `-dist` outside. `dist` is the nearest-edge distance.
fn polygon_signed_distance(points: &[[f32; 2]], u: f32, v: f32) -> f32 {
    if points.len() < 3 {
        return -f32::INFINITY;
    }
    let mut inside = false;
    let mut min_d2 = f32::INFINITY;
    let n = points.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (points[i][0], points[i][1]);
        let (xj, yj) = (points[j][0], points[j][1]);
        // Even-odd ray cast.
        if ((yi > v) != (yj > v)) && (u < (xj - xi) * (v - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        // Distance to edge segment (i, j).
        min_d2 = min_d2.min(point_seg_dist2(u, v, xi, yi, xj, yj));
        j = i;
    }
    let d = min_d2.sqrt();
    if inside {
        d
    } else {
        -d
    }
}

/// Squared distance from point `(px,py)` to segment `(ax,ay)-(bx,by)`.
fn point_seg_dist2(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    (px - cx).powi(2) + (py - cy).powi(2)
}
