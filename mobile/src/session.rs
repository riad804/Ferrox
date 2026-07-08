//! The stateful `ImageSession` and image helpers exposed over UniFFI.

use ferrox_core as fx;
use crate::FerroxError;

pub(crate) fn frame_to_png(frame: fx::Frame) -> Result<Vec<u8>, FerroxError> {
    use fx::codecs::png::PngEncoder;
    use fx::traits::Encoder;
    let mut buf = Vec::new();
    PngEncoder
        .encode(&frame, &mut buf)
        .map_err(|e| FerroxError::Encode {
            message: e.to_string(),
        })?;
    Ok(buf)
}

/// Decode PNG or JPEG bytes into a [`fx::Frame`].
pub(crate) fn decode_image_frame(data: &[u8]) -> Result<fx::Frame, FerroxError> {
    use fx::codecs::{jpeg::JpegDecoder, png::PngDecoder};
    use fx::traits::Decoder;
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Ok(PngDecoder.decode(std::io::Cursor::new(data))?)
    } else if data.starts_with(&[0xFF, 0xD8]) {
        Ok(JpegDecoder.decode(std::io::Cursor::new(data))?)
    } else {
        Err(FerroxError::Unsupported {
            message: "expected PNG or JPEG input".into(),
        })
    }
}

// ── ImageSession (stateful editor) ─────────────────────────────────────────────

/// A mutable, in-memory image being edited.
///
/// This is the primitive a photo editor should use: decode **once**, apply a
/// chain of edits in memory (each is a cheap in-place op), then `export*` at the
/// end. Avoids the decode→op→re-encode round-trip per tap that the standalone
/// functions incur.
///
/// Thread-safe: the frame lives behind a mutex so it can be shared as an
/// `Arc<ImageSession>` across the FFI boundary.
///
/// ```kotlin
/// val s = ImageSession(jpegBytes)
/// s.brightness(20); s.contrast(1.2f); s.cropCenterSquare()
/// val out = s.exportJpeg(90u)   // ByteArray
/// ```
#[derive(uniffi::Object)]
pub struct ImageSession {
    frame: std::sync::Mutex<fx::Frame>,
}

// Non-exported helpers (generics aren't allowed in a `#[uniffi::export] impl`).
impl ImageSession {
    fn map_frame<F>(&self, f: F) -> Result<(), FerroxError>
    where
        F: FnOnce(fx::Frame) -> Result<fx::Frame, fx::Error>,
    {
        let mut guard = self.frame.lock().unwrap();
        // Take the frame out, transform, put it back. Replace with a 1x1 stub
        // first so the mutex never holds junk if `f` returns an error.
        let current = std::mem::replace(
            &mut *guard,
            fx::Frame::new(1, 1, fx::PixelFormat::Rgb8, vec![0, 0, 0]),
        );
        *guard = f(current)?;
        Ok(())
    }
}

#[uniffi::export]
impl ImageSession {
    /// Decode PNG/JPEG bytes into a new editing session.
    #[uniffi::constructor]
    pub fn new(image_data: Vec<u8>) -> Result<std::sync::Arc<Self>, FerroxError> {
        let frame = decode_image_frame(&image_data)?;
        Ok(std::sync::Arc::new(Self {
            frame: std::sync::Mutex::new(frame),
        }))
    }

    /// Current width in pixels.
    pub fn width(&self) -> u32 {
        self.frame.lock().unwrap().width
    }

    /// Current height in pixels.
    pub fn height(&self) -> u32 {
        self.frame.lock().unwrap().height
    }

    // ── adjustments ──────────────────────────────────────────────────────────

    /// Add `delta` (−255..=255) to every channel.
    pub fn brightness(&self, delta: i32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::BrightnessFilter::new(delta).process(fr))
    }

    /// Scale contrast around mid-grey. `1.0` = unchanged, `>1` = more contrast.
    pub fn contrast(&self, factor: f32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::ContrastFilter::new(factor).process(fr))
    }

    /// Scale saturation. `1.0` = unchanged, `0.0` = grayscale, `>1` = vivid.
    pub fn saturation(&self, factor: f32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::SaturationFilter::new(factor).process(fr))
    }

    /// Convert to grayscale.
    pub fn grayscale(&self) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::GrayscaleFilter.process(fr))
    }

    /// Invert colors.
    pub fn negate(&self) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::NegateFilter.process(fr))
    }

    /// Gaussian blur; `sigma` controls radius (e.g. `2.0`).
    pub fn blur(&self, sigma: f32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::BlurFilter::new(sigma).process(fr))
    }

    // ── geometry ─────────────────────────────────────────────────────────────

    /// Resize to exact `width × height` (Lanczos3).
    pub fn resize(&self, width: u32, height: u32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::ResizeFilter::new(width, height).process(fr))
    }

    /// Crop to a rectangle.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::CropFilter::new(x, y, width, height).process(fr))
    }

    /// Convenience: crop the largest centered square.
    pub fn crop_center_square(&self) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| {
            let side = fr.width.min(fr.height);
            let x = (fr.width - side) / 2;
            let y = (fr.height - side) / 2;
            fx::CropFilter::new(x, y, side, side).process(fr)
        })
    }

    /// Rotate clockwise by 90, 180, or 270 degrees. Other values error.
    pub fn rotate(&self, degrees_cw: u32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        let rotate = match degrees_cw % 360 {
            90 => fx::RotateFilter::cw90(),
            180 => fx::RotateFilter::cw180(),
            270 => fx::RotateFilter::cw270(),
            0 => return Ok(()),
            other => {
                return Err(FerroxError::Filter {
                    message: format!("rotate supports 0/90/180/270, got {other}"),
                })
            }
        };
        self.map_frame(|fr| rotate.process(fr))
    }

    /// Mirror horizontally (left↔right).
    pub fn flip_horizontal(&self) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::FlipFilter::horizontal().process(fr))
    }

    /// Mirror vertically (top↔bottom).
    pub fn flip_vertical(&self) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::FlipFilter::vertical().process(fr))
    }

    /// Shrink to fit within `max_width × max_height`, preserving aspect ratio.
    pub fn thumbnail(&self, max_width: u32, max_height: u32) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        self.map_frame(|fr| fx::ThumbnailFilter::new(max_width, max_height).process(fr))
    }

    // ── text ─────────────────────────────────────────────────────────────────

    /// Draw `text` at (`x`,`y`) using a caller-supplied TTF/OTF font.
    ///
    /// `scale` is the pixel height; `color` is RGBA. The app supplies font bytes
    /// (e.g. a bundled .ttf) so the SDK carries no font dependency.
    pub fn draw_text(
        &self,
        text: String,
        x: i32,
        y: i32,
        scale: f32,
        color: RgbaColor,
        font_data: Vec<u8>,
    ) -> Result<(), FerroxError> {
        use fx::traits::Filter;
        use fx::{DrawTextFilter, TextColor};
        let c = TextColor::new(color.r, color.g, color.b, color.a);
        self.map_frame(|fr| DrawTextFilter::new(text, x, y, scale, c, font_data).process(fr))
    }

    // ── export ───────────────────────────────────────────────────────────────

    /// Encode the current state as PNG bytes.
    pub fn export_png(&self) -> Result<Vec<u8>, FerroxError> {
        let frame = self.frame.lock().unwrap().clone();
        frame_to_png(frame)
    }

    /// Encode the current state as JPEG bytes. `quality` is 1–100.
    pub fn export_jpeg(&self, quality: u8) -> Result<Vec<u8>, FerroxError> {
        use fx::codecs::jpeg::JpegEncoder;
        use fx::traits::Encoder;
        let frame = self.frame.lock().unwrap().clone();
        let mut buf = Vec::new();
        JpegEncoder { quality }
            .encode(&frame, &mut buf)
            .map_err(|e| FerroxError::Encode {
                message: e.to_string(),
            })?;
        Ok(buf)
    }

    /// Raw RGBA8 pixels of the current state, for handing to a platform bitmap
    /// (`Bitmap.copyPixelsFromBuffer` / `CGImage`) without a PNG round-trip.
    pub fn to_rgba8(&self) -> Result<RawImage, FerroxError> {
        let frame = self.frame.lock().unwrap().clone();
        // Re-use the resize-into-self identity? No — convert format via PNG-free
        // path: most filters output Rgb8/Rgba8 already. Normalise to Rgba8.
        let (w, h) = (frame.width, frame.height);
        let rgba = to_rgba8_bytes(&frame)?;
        Ok(RawImage {
            width: w,
            height: h,
            pixels: rgba,
        })
    }
}

/// RGBA color, 0–255 per channel.
#[derive(uniffi::Record)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Raw decoded pixels in tightly-packed RGBA8 (row-major, no padding).
#[derive(uniffi::Record)]
pub struct RawImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Convert an Rgb8/Rgba8/Gray8 frame to tightly-packed RGBA8.
fn to_rgba8_bytes(frame: &fx::Frame) -> Result<Vec<u8>, FerroxError> {
    use fx::PixelFormat;
    let px = (frame.width as usize) * (frame.height as usize);
    let out = match frame.format {
        PixelFormat::Rgba8 => frame.data.clone(),
        PixelFormat::Rgb8 => {
            let mut o = Vec::with_capacity(px * 4);
            for c in frame.data.chunks_exact(3) {
                o.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
            o
        }
        PixelFormat::Gray8 => {
            let mut o = Vec::with_capacity(px * 4);
            for &g in &frame.data {
                o.extend_from_slice(&[g, g, g, 255]);
            }
            o
        }
        other => {
            return Err(FerroxError::Unsupported {
                message: format!("to_rgba8 expects Rgb8/Rgba8/Gray8, got {other:?}"),
            })
        }
    };
    Ok(out)
}

// ── Image (stateless one-shot helpers) ─────────────────────────────────────────

