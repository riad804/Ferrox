//! Video-specific frame filters: pad and overlay.

use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::Filter,
};

// ── Pad ───────────────────────────────────────────────────────────────────────

/// Add a solid-colour border around a frame to reach `out_width × out_height`.
///
/// The original frame is centred. Background colour defaults to black.
pub struct PadFilter {
    pub out_width: u32,
    pub out_height: u32,
    pub bg_r: u8,
    pub bg_g: u8,
    pub bg_b: u8,
}

impl PadFilter {
    pub fn new(out_width: u32, out_height: u32) -> Self {
        Self { out_width, out_height, bg_r: 0, bg_g: 0, bg_b: 0 }
    }

    pub fn with_color(mut self, r: u8, g: u8, b: u8) -> Self {
        self.bg_r = r; self.bg_g = g; self.bg_b = b; self
    }
}

impl Filter for PadFilter {
    fn process(&self, frame: Frame) -> Result<Frame> {
        match frame.format {
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => {}
            _ => return Err(Error::Filter(format!(
                "PadFilter requires Rgb8 or Rgba8, got {:?}", frame.format
            ))),
        }
        if frame.width > self.out_width || frame.height > self.out_height {
            return Err(Error::Filter(format!(
                "PadFilter: source {}×{} exceeds output {}×{}",
                frame.width, frame.height, self.out_width, self.out_height
            )));
        }

        let bpp = frame.format.bytes_per_pixel();
        let (ow, oh) = (self.out_width as usize, self.out_height as usize);
        let ox = (self.out_width - frame.width) / 2;
        let oy = (self.out_height - frame.height) / 2;

        let mut out = vec![0u8; ow * oh * bpp];
        // fill background
        for px in out.chunks_exact_mut(bpp) {
            px[0] = self.bg_r;
            px[1] = self.bg_g;
            px[2] = self.bg_b;
            if bpp == 4 { px[3] = 255; }
        }
        // blit source into output
        let src_stride = frame.width as usize * bpp;
        let dst_stride = ow * bpp;
        for row in 0..frame.height as usize {
            let dst_start = (oy as usize + row) * dst_stride + ox as usize * bpp;
            let src_start = row * src_stride;
            out[dst_start..dst_start + src_stride]
                .copy_from_slice(&frame.data[src_start..src_start + src_stride]);
        }
        Ok(Frame::new(self.out_width, self.out_height, frame.format, out))
    }
}

// ── Overlay ───────────────────────────────────────────────────────────────────

/// Place an overlay frame on top of a base frame at pixel offset `(x, y)`.
///
/// Supports alpha blending when both frames are Rgba8, otherwise performs
/// direct copy (nearest-pixel, no interpolation). The output size equals the
/// base frame size.
pub struct OverlayFilter {
    pub overlay: Frame,
    pub x: i32,
    pub y: i32,
}

impl OverlayFilter {
    pub fn new(overlay: Frame, x: i32, y: i32) -> Self {
        Self { overlay, x, y }
    }
}

impl Filter for OverlayFilter {
    fn process(&self, mut base: Frame) -> Result<Frame> {
        match base.format {
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => {}
            _ => return Err(Error::Filter(format!(
                "OverlayFilter: base must be Rgb8/Rgba8, got {:?}", base.format
            ))),
        }
        match self.overlay.format {
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => {}
            _ => return Err(Error::Filter(format!(
                "OverlayFilter: overlay must be Rgb8/Rgba8, got {:?}", self.overlay.format
            ))),
        }

        let base_bpp = base.format.bytes_per_pixel();
        let ov_bpp   = self.overlay.format.bytes_per_pixel();
        let base_stride = base.width as usize * base_bpp;
        let ov_stride   = self.overlay.width as usize * ov_bpp;

        for oy in 0..self.overlay.height as i32 {
            let by = self.y + oy;
            if by < 0 || by >= base.height as i32 { continue; }
            for ox in 0..self.overlay.width as i32 {
                let bx = self.x + ox;
                if bx < 0 || bx >= base.width as i32 { continue; }

                let ov_idx = oy as usize * ov_stride + ox as usize * ov_bpp;
                let b_idx  = by as usize * base_stride + bx as usize * base_bpp;

                let alpha = if ov_bpp == 4 { self.overlay.data[ov_idx + 3] } else { 255u8 };
                let inv = 255u16 - alpha as u16;
                let blend = |dst: u8, src: u8| -> u8 {
                    ((dst as u16 * inv + src as u16 * alpha as u16) / 255) as u8
                };
                base.data[b_idx]     = blend(base.data[b_idx],     self.overlay.data[ov_idx]);
                base.data[b_idx + 1] = blend(base.data[b_idx + 1], self.overlay.data[ov_idx + 1]);
                base.data[b_idx + 2] = blend(base.data[b_idx + 2], self.overlay.data[ov_idx + 2]);
                if base_bpp == 4 {
                    base.data[b_idx + 3] = 255;
                }
            }
        }
        Ok(base)
    }
}
