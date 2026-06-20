//! Text overlay filter using `ab_glyph` (pure-Rust font rasteriser).
//!
//! Renders `text` at pixel position `(x, y)` with the given font scale and
//! colour. Uses the bundled `ab_glyph::FontRef` with any TTF font bytes the
//! caller provides, or falls back to a minimal embedded bitmap font.

use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::Filter,
};

/// RGBA colour for drawn text.
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const RED:   Self = Self { r: 255, g: 0, b: 0, a: 255 };

    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self { Self { r, g, b, a } }
}

/// Renders text onto a frame using a TTF font loaded at construction time.
pub struct DrawTextFilter {
    text: String,
    x: i32,
    y: i32,
    scale: f32,
    color: Color,
    font_data: Vec<u8>,
}

impl DrawTextFilter {
    /// `font_data`: raw bytes of a TTF/OTF font file.
    pub fn new(
        text: impl Into<String>,
        x: i32,
        y: i32,
        scale: f32,
        color: Color,
        font_data: Vec<u8>,
    ) -> Self {
        Self { text: text.into(), x, y, scale, color, font_data }
    }
}

impl Filter for DrawTextFilter {
    fn process(&self, mut frame: Frame) -> Result<Frame> {
        match frame.format {
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => {}
            _ => return Err(Error::Filter(format!(
                "DrawTextFilter requires Rgb8 or Rgba8, got {:?}", frame.format
            ))),
        }

        use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

        let font = FontRef::try_from_slice(&self.font_data)
            .map_err(|e| Error::Filter(format!("font load error: {e}")))?;
        let scale = PxScale::from(self.scale);
        let scaled = font.as_scaled(scale);

        let bpp = frame.format.bytes_per_pixel();
        let stride = frame.width as usize * bpp;

        let mut cursor_x = self.x as f32;
        let baseline_y = self.y as f32 + scaled.ascent();
        let mut prev_glyph: Option<ab_glyph::GlyphId> = None;

        for ch in self.text.chars() {
            let glyph_id = font.glyph_id(ch);
            if let Some(prev) = prev_glyph {
                cursor_x += scaled.kern(prev, glyph_id);
            }
            let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(cursor_x, baseline_y));
            cursor_x += scaled.h_advance(glyph_id);
            prev_glyph = Some(glyph_id);

            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, cov| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    if px < 0 || py < 0 { return; }
                    let px = px as usize;
                    let py = py as usize;
                    if px >= frame.width as usize || py >= frame.height as usize { return; }
                    let idx = py * stride + px * bpp;
                    // Alpha-blend with coverage
                    let alpha = (cov * self.color.a as f32) as u16;
                    let blend = |dst: u8, src: u8| -> u8 {
                        ((dst as u16 * (255 - alpha) + src as u16 * alpha) / 255) as u8
                    };
                    frame.data[idx]     = blend(frame.data[idx],     self.color.r);
                    frame.data[idx + 1] = blend(frame.data[idx + 1], self.color.g);
                    frame.data[idx + 2] = blend(frame.data[idx + 2], self.color.b);
                    if bpp == 4 {
                        frame.data[idx + 3] = 255;
                    }
                });
            }
        }

        Ok(frame)
    }
}
