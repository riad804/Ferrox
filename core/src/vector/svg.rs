//! [`SvgImage`] — parse and rasterise SVG via `resvg`/`usvg`/`tiny-skia`.

use resvg::tiny_skia;
use resvg::usvg;

use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};

use super::VectorRenderer;

/// A parsed SVG document, rasterisable to any size.
pub struct SvgImage {
    tree: usvg::Tree,
}

impl SvgImage {
    /// Parse SVG bytes (UTF-8 XML). Text elements need fonts registered in the
    /// options; shape-only SVG parses with defaults.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let opt = usvg::Options::default();
        let tree = usvg::Tree::from_data(data, &opt).map_err(|e| Error::Filter(format!("svg parse: {e}")))?;
        Ok(Self { tree })
    }

}

impl std::str::FromStr for SvgImage {
    type Err = Error;
    fn from_str(svg: &str) -> Result<Self> {
        Self::from_bytes(svg.as_bytes())
    }
}

impl VectorRenderer for SvgImage {
    fn intrinsic_size(&self) -> (u32, u32) {
        let s = self.tree.size();
        (s.width().ceil() as u32, s.height().ceil() as u32)
    }

    fn render(&self, width: u32, height: u32, _t: f64) -> Result<Frame> {
        let (w, h) = (width.max(1), height.max(1));
        let mut pixmap = tiny_skia::Pixmap::new(w, h)
            .ok_or(Error::InvalidDimensions { width: w, height: h })?;

        // Scale the SVG's intrinsic size to fill the requested raster.
        let size = self.tree.size();
        let transform = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
        resvg::render(&self.tree, transform, &mut pixmap.as_mut());

        // tiny-skia stores premultiplied RGBA; convert to straight-alpha Rgba8.
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for px in pixmap.pixels() {
            let a = px.alpha();
            let (r, g, b) = if a == 0 {
                (0, 0, 0)
            } else {
                (unpremultiply(px.red(), a), unpremultiply(px.green(), a), unpremultiply(px.blue(), a))
            };
            data.extend_from_slice(&[r, g, b, a]);
        }
        Ok(Frame::new(w, h, PixelFormat::Rgba8, data))
    }
}

/// Recover a straight-alpha channel value from a premultiplied one.
fn unpremultiply(c: u8, a: u8) -> u8 {
    ((c as u16 * 255 + a as u16 / 2) / a as u16).min(255) as u8
}
