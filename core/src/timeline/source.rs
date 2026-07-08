//! [`ClipSource`] — the visual source a clip draws from.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};

/// The visual source a clip draws from.
///
/// `Solid` is a dependency-free colour generator — ideal for backgrounds and
/// deterministic tests. `Image` decodes a PNG/JPEG file at composite time.
/// Video sources (with in/out media time) are added in a later milestone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipSource {
    /// A solid RGBA colour fill of the given size.
    Solid { width: u32, height: u32, r: u8, g: u8, b: u8, a: u8 },
    /// An image file, decoded to a frame when composited.
    Image { path: String },
}

impl ClipSource {
    /// Produce this source as an `Rgba8` [`Frame`].
    pub fn render(&self) -> Result<Frame> {
        match self {
            ClipSource::Solid { width, height, r, g, b, a } => {
                if *width == 0 || *height == 0 {
                    return Err(Error::InvalidDimensions { width: *width, height: *height });
                }
                let mut data = vec![0u8; (*width as usize) * (*height as usize) * 4];
                for px in data.chunks_exact_mut(4) {
                    px[0] = *r;
                    px[1] = *g;
                    px[2] = *b;
                    px[3] = *a;
                }
                Ok(Frame::new(*width, *height, PixelFormat::Rgba8, data))
            }
            ClipSource::Image { path } => {
                let bytes = std::fs::read(path)?;
                let frame = decode_image(&bytes)?;
                to_rgba8(frame)
            }
        }
    }
}

/// Decode PNG or JPEG bytes into a [`Frame`], reusing the core decoders.
fn decode_image(data: &[u8]) -> Result<Frame> {
    use crate::codecs::{jpeg::JpegDecoder, png::PngDecoder};
    use crate::traits::Decoder;
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        PngDecoder.decode(std::io::Cursor::new(data))
    } else if data.starts_with(&[0xFF, 0xD8]) {
        JpegDecoder.decode(std::io::Cursor::new(data))
    } else {
        Err(Error::UnsupportedFormat("clip image must be PNG or JPEG".into()))
    }
}

/// Normalise a decoded frame to `Rgba8` so the compositor has one code path.
fn to_rgba8(frame: Frame) -> Result<Frame> {
    match frame.format {
        PixelFormat::Rgba8 => Ok(frame),
        PixelFormat::Rgb8 => {
            let mut data = Vec::with_capacity((frame.width as usize) * (frame.height as usize) * 4);
            for px in frame.data.chunks_exact(3) {
                data.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgba8, data))
        }
        other => Err(Error::Filter(format!(
            "compositor sources must be Rgb8/Rgba8, got {other:?}"
        ))),
    }
}
