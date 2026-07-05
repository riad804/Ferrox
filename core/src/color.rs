//! Color-grading primitives: **ASC-CDL** (the SMPTE primary lift/gamma/gain +
//! saturation grade) and **3D LUT** (`.cube`) with trilinear interpolation.
//!
//! Both are pure-Rust and WASM-safe. `AscCdl` is small and serialises into a
//! project; `Lut3D` holds a cube's worth of samples and is applied to whole
//! frames (typically the final composite — "after blending"). Parsing a `.cube`
//! file from disk lives behind a non-WASM helper so the core stays portable.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};

/// Rec.709 luma coefficients (used for the CDL saturation step).
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// ASC Color Decision List — the industry-standard primary grade.
///
/// Per channel: `out = (in * slope + offset) ^ power`, where **slope = gain**,
/// **offset = lift**, **power = gamma**; then a global saturation around luma.
/// Identity is `slope=1, offset=0, power=1, saturation=1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AscCdl {
    #[serde(default = "one3")]
    pub slope: [f32; 3],
    #[serde(default)]
    pub offset: [f32; 3],
    #[serde(default = "one3")]
    pub power: [f32; 3],
    #[serde(default = "one")]
    pub saturation: f32,
}

fn one() -> f32 {
    1.0
}
fn one3() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl Default for AscCdl {
    fn default() -> Self {
        Self { slope: one3(), offset: [0.0; 3], power: one3(), saturation: 1.0 }
    }
}

impl AscCdl {
    /// True when this grade would leave every pixel unchanged.
    pub fn is_identity(&self) -> bool {
        self.slope == one3()
            && self.offset == [0.0; 3]
            && self.power == one3()
            && self.saturation == 1.0
    }

    /// Apply the grade to a single linear-ish RGB triplet in `[0, 1]`.
    pub fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for c in 0..3 {
            // SOP: slope/offset/power. Clamp before power to avoid NaNs on negatives.
            let v = (rgb[c] * self.slope[c] + self.offset[c]).max(0.0);
            out[c] = v.powf(self.power[c]);
        }
        if self.saturation != 1.0 {
            let luma = LUMA[0] * out[0] + LUMA[1] * out[1] + LUMA[2] * out[2];
            for c in &mut out {
                *c = luma + self.saturation * (*c - luma);
            }
        }
        [out[0].clamp(0.0, 1.0), out[1].clamp(0.0, 1.0), out[2].clamp(0.0, 1.0)]
    }

    /// Apply the grade in place to an `Rgb8`/`Rgba8` frame.
    pub fn apply_frame(&self, frame: &mut Frame) -> Result<()> {
        apply_rgb_fn(frame, |rgb| self.apply_rgb(rgb))
    }
}

/// A 3D color lookup table (cube), sampled with trilinear interpolation.
///
/// `data[r + g*size + b*size*size]` holds the mapped RGB for lattice point
/// `(r, g, b)`. The input domain is assumed `[0, 1]` (DOMAIN_MIN/MAX default).
#[derive(Debug, Clone, PartialEq)]
pub struct Lut3D {
    size: usize,
    data: Vec<[f32; 3]>,
}

impl Lut3D {
    /// Build a LUT from raw lattice samples (red-fastest ordering).
    pub fn new(size: usize, data: Vec<[f32; 3]>) -> Result<Self> {
        if size < 2 || data.len() != size * size * size {
            return Err(Error::Filter(format!(
                "Lut3D: expected {} samples for size {size}, got {}",
                size * size * size,
                data.len()
            )));
        }
        Ok(Self { size, data })
    }

    /// The identity LUT of the given lattice size (maps every input to itself).
    pub fn identity(size: usize) -> Result<Self> {
        let n = size.max(2);
        let denom = (n - 1) as f32;
        let mut data = Vec::with_capacity(n * n * n);
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    data.push([r as f32 / denom, g as f32 / denom, b as f32 / denom]);
                }
            }
        }
        Self::new(n, data)
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Parse a DaVinci Resolve `.cube` 3D LUT from text.
    pub fn parse_cube(text: &str) -> Result<Self> {
        let mut size = 0usize;
        let mut data: Vec<[f32; 3]> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut tok = line.split_whitespace();
            let head = tok.next().unwrap_or("");
            match head {
                "TITLE" | "DOMAIN_MIN" | "DOMAIN_MAX" | "LUT_1D_SIZE" => continue,
                "LUT_3D_SIZE" => {
                    size = tok
                        .next()
                        .and_then(|s| s.parse::<usize>().ok())
                        .ok_or_else(|| Error::Filter("cube: bad LUT_3D_SIZE".into()))?;
                }
                _ => {
                    // A data row is three floats; anything else is an ignored keyword.
                    let vals: Vec<f32> = line
                        .split_whitespace()
                        .filter_map(|s| s.parse::<f32>().ok())
                        .collect();
                    if vals.len() == 3 {
                        data.push([vals[0], vals[1], vals[2]]);
                    }
                }
            }
        }
        if size == 0 {
            return Err(Error::Filter("cube: missing LUT_3D_SIZE".into()));
        }
        Self::new(size, data)
    }

    /// Load and parse a `.cube` file from disk. Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_cube_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse_cube(&text)
    }

    /// Sample the LUT at `rgb` (each in `[0, 1]`) with trilinear interpolation.
    pub fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let maxi = (n - 1) as f32;
        let coord = |v: f32| v.clamp(0.0, 1.0) * maxi;
        let (rf, gf, bf) = (coord(rgb[0]), coord(rgb[1]), coord(rgb[2]));
        let (r0, g0, b0) = (rf.floor() as usize, gf.floor() as usize, bf.floor() as usize);
        let (r1, g1, b1) = ((r0 + 1).min(n - 1), (g0 + 1).min(n - 1), (b0 + 1).min(n - 1));
        let (dr, dg, db) = (rf - r0 as f32, gf - g0 as f32, bf - b0 as f32);

        let at = |r: usize, g: usize, b: usize| self.data[r + g * n + b * n * n];
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
        };

        // Interpolate along r, then g, then b.
        let c00 = lerp(at(r0, g0, b0), at(r1, g0, b0), dr);
        let c10 = lerp(at(r0, g1, b0), at(r1, g1, b0), dr);
        let c01 = lerp(at(r0, g0, b1), at(r1, g0, b1), dr);
        let c11 = lerp(at(r0, g1, b1), at(r1, g1, b1), dr);
        let c0 = lerp(c00, c10, dg);
        let c1 = lerp(c01, c11, dg);
        lerp(c0, c1, db)
    }

    /// Apply the LUT in place to an `Rgb8`/`Rgba8` frame.
    pub fn apply_frame(&self, frame: &mut Frame) -> Result<()> {
        apply_rgb_fn(frame, |rgb| self.apply_rgb(rgb))
    }
}

/// Run a per-pixel RGB mapping over an `Rgb8`/`Rgba8` frame in place, preserving
/// alpha. Values round-trip through `[0, 1]` floats.
fn apply_rgb_fn(frame: &mut Frame, f: impl Fn([f32; 3]) -> [f32; 3]) -> Result<()> {
    let bpp = match frame.format {
        PixelFormat::Rgb8 => 3,
        PixelFormat::Rgba8 => 4,
        other => {
            return Err(Error::Filter(format!("color grade needs Rgb8/Rgba8, got {other:?}")))
        }
    };
    for px in frame.data.chunks_exact_mut(bpp) {
        let inp = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        let out = f(inp);
        px[0] = (out[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        px[1] = (out[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        px[2] = (out[2].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    Ok(())
}

/// A per-clip color grade: an optional ASC-CDL primary grade. Extensible with a
/// LUT/curves later; kept small so it serialises cheaply into a project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ColorGrade {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cdl: Option<AscCdl>,
}

impl ColorGrade {
    /// True when the grade would leave the clip unchanged.
    pub fn is_identity(&self) -> bool {
        match &self.cdl {
            None => true,
            Some(cdl) => cdl.is_identity(),
        }
    }

    /// Convenience constructor from a CDL.
    pub fn from_cdl(cdl: AscCdl) -> Self {
        Self { cdl: Some(cdl) }
    }

    /// Apply the grade in place to a clip frame.
    pub fn apply_frame(&self, frame: &mut Frame) -> Result<()> {
        if let Some(cdl) = &self.cdl {
            cdl.apply_frame(frame)?;
        }
        Ok(())
    }
}
