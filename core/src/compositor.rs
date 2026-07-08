//! The **compositor** — renders a [`Project`] timeline to a single output frame.
//!
//! [`compose_frame`] is the one entry point shared by the (future) real-time
//! preview and the exporter: given a project and a time `t` (seconds), it
//! produces the composited `Rgba8` output frame. It walks tracks bottom-to-top
//! (z-order), and for every clip active at `t` renders its source, applies the
//! clip's transform (scale + position), and alpha-composites it over the canvas
//! using the Porter–Duff "over" operator with the clip's opacity.
//!
//! This is the M4 skeleton: nearest-neighbour scale, CPU compositing. The
//! design intentionally keeps `compose_frame` as the single seam so the scale
//! path can move to Lanczos/`ResizeFilter` and the whole composite can move to
//! the `gpu` (wgpu) path for real-time preview without changing callers.

use crate::blend::composite_over;
use crate::color::Lut3D;
use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};
use crate::timeline::Project;

/// Render the project's output frame at timeline time `t` (seconds).
///
/// The returned frame is always `Rgba8`, `project.width × project.height`, with
/// an opaque background. Clips are drawn in track order (track 0 first / behind).
pub fn compose_frame(project: &Project, t: f64) -> Result<Frame> {
    compose_frame_graded(project, t, None)
}

/// Like [`compose_frame`], but applies an output 3D LUT to the final composite
/// ("color grade after blending"). Kept WASM-pure — the caller supplies an
/// already-loaded [`Lut3D`] so no file I/O happens in the engine.
pub fn compose_frame_graded(project: &Project, t: f64, output_lut: Option<&Lut3D>) -> Result<Frame> {
    if project.width == 0 || project.height == 0 {
        return Err(Error::InvalidDimensions { width: project.width, height: project.height });
    }

    let mut canvas = solid_canvas(project.width, project.height, project.background);

    for track in &project.tracks {
        for clip in &track.clips {
            if !clip.is_active(t) {
                continue;
            }
            let tf = clip.effective_transform(t);
            if tf.scale <= 0.0 {
                continue; // fully collapsed (e.g. start of a zoom-in) → invisible
            }

            // Per-clip pipeline: source → keyer → color grade → scale → mask → blend.
            let mut src = clip.source.render()?;
            if let Some(keyer) = &clip.keyer {
                keyer.apply_frame(&mut src)?;
            }
            if !clip.color.is_identity() {
                clip.color.apply_frame(&mut src)?;
            }
            let mut src = scale_nearest(&src, tf.scale);
            if let Some(mask) = &clip.mask {
                mask.apply_frame(&mut src)?;
            }
            composite_over(&mut canvas, &src, tf.x, tf.y, tf.opacity, clip.blend);
        }
    }

    if let Some(lut) = output_lut {
        lut.apply_frame(&mut canvas)?;
    }
    Ok(canvas)
}

/// An opaque `Rgba8` canvas filled with `bg` (RGB).
fn solid_canvas(width: u32, height: u32, bg: [u8; 3]) -> Frame {
    let mut data = vec![0u8; (width as usize) * (height as usize) * 4];
    for px in data.chunks_exact_mut(4) {
        px[0] = bg[0];
        px[1] = bg[1];
        px[2] = bg[2];
        px[3] = 255;
    }
    Frame::new(width, height, PixelFormat::Rgba8, data)
}

/// Nearest-neighbour uniform scale of an `Rgba8` frame.
///
/// Returns the source unchanged for scale ≈ 1.0 or non-positive scale, so the
/// common (unscaled) path is copy-free and bit-exact.
fn scale_nearest(src: &Frame, scale: f32) -> Frame {
    if scale <= 0.0 || (scale - 1.0).abs() < f32::EPSILON {
        return src.clone();
    }
    let nw = (((src.width as f32) * scale).round() as u32).max(1);
    let nh = (((src.height as f32) * scale).round() as u32).max(1);
    let mut data = vec![0u8; (nw as usize) * (nh as usize) * 4];
    for oy in 0..nh {
        let sy = ((oy as f32 / scale) as u32).min(src.height - 1);
        for ox in 0..nw {
            let sx = ((ox as f32 / scale) as u32).min(src.width - 1);
            let si = ((sy * src.width + sx) * 4) as usize;
            let di = ((oy * nw + ox) * 4) as usize;
            data[di..di + 4].copy_from_slice(&src.data[si..si + 4]);
        }
    }
    Frame::new(nw, nh, PixelFormat::Rgba8, data)
}

// Compositing now lives in [`crate::blend::composite_over`] (shared with the
// render backends).
