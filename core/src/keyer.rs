//! **Chroma keyer** — green/blue-screen removal with soft edges and spill
//! suppression. Distance to the key colour is measured in chroma (Cb/Cr) space
//! so luminance differences don't affect the key. Pixels inside `tolerance`
//! become transparent; a `softness` band gives an anti-aliased matte; `despill`
//! removes the key colour bleeding onto retained foreground edges.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};

/// A chroma-key configuration applied to an `Rgba8` frame before compositing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyer {
    /// The key colour (e.g. green screen), RGB.
    pub key: [u8; 3],
    /// Chroma distance under which pixels are fully keyed out (0..~1).
    pub tolerance: f32,
    /// Width of the soft matte band beyond `tolerance` (0..~1).
    pub softness: f32,
    /// Suppress key-colour spill on retained pixels.
    #[serde(default)]
    pub despill: bool,
}

impl Keyer {
    /// A green-screen keyer with sensible defaults.
    pub fn green() -> Self {
        Self { key: [0, 177, 64], tolerance: 0.12, softness: 0.1, despill: true }
    }

    /// Apply the key in place: sets alpha per the chroma matte and, if enabled,
    /// despills retained pixels.
    pub fn apply_frame(&self, frame: &mut Frame) -> Result<()> {
        if frame.format != PixelFormat::Rgba8 {
            return Err(Error::Filter(format!("keyer needs Rgba8, got {:?}", frame.format)));
        }
        let (kcb, kcr) = cbcr(self.key[0], self.key[1], self.key[2]);
        // Index of the key's dominant channel — the one that spills.
        let kmax = (0..3).max_by(|&a, &b| self.key[a].cmp(&self.key[b])).unwrap_or(1);

        for px in frame.data.chunks_exact_mut(4) {
            let (cb, cr) = cbcr(px[0], px[1], px[2]);
            let dist = ((cb - kcb).powi(2) + (cr - kcr).powi(2)).sqrt();

            let matte = if dist <= self.tolerance {
                0.0
            } else if self.softness > 0.0 && dist < self.tolerance + self.softness {
                (dist - self.tolerance) / self.softness
            } else {
                1.0
            };
            px[3] = (px[3] as f32 * matte).round() as u8;

            // Despill retained/edge pixels: clamp the dominant key channel down
            // to the average of the other two.
            if self.despill && matte > 0.0 {
                let others = (0..3).filter(|&c| c != kmax).map(|c| px[c] as u16).sum::<u16>() / 2;
                if (px[kmax] as u16) > others {
                    px[kmax] = others as u8;
                }
            }
        }
        Ok(())
    }
}

/// Rec.601 Cb/Cr for an 8-bit RGB pixel, each returned in `[0, 1]`.
fn cbcr(r: u8, g: u8, b: u8) -> (f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let cb = -0.168736 * r - 0.331264 * g + 0.5 * b + 0.5;
    let cr = 0.5 * r - 0.418688 * g - 0.081312 * b + 0.5;
    (cb, cr)
}
