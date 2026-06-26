//! Native mobile SDK bindings for ferrox (feature-equivalent to `core/src/wasm.rs`).
//!
//! Exposes a UniFFI-generated API consumable as idiomatic **Kotlin** (Android)
//! and **Swift** (iOS). Every function takes/returns byte buffers (`Vec<u8>`
//! ⇄ Kotlin `ByteArray` / Swift `Data`) and `String`, and surfaces failures as
//! a [`FerroxError`] that becomes a Kotlin exception / Swift `throws`.
//!
//! # Supported operations
//!
//! | Function                | Input                     | Output      |
//! |-------------------------|---------------------------|-------------|
//! | `decode_image_to_png`   | PNG/JPEG bytes            | PNG bytes   |
//! | `resize_image`          | image bytes + w + h       | PNG bytes   |
//! | `apply_filter`          | image bytes + filter expr | PNG bytes   |
//! | `blur_image`            | image bytes + sigma       | PNG bytes   |
//! | `grayscale_image`       | image bytes               | PNG bytes   |
//! | `probe_image`           | image bytes               | JSON string |
//! | `decode_vp8_to_png`     | VP8 keyframe bytes        | PNG bytes   |
//! | `decode_gif_frames`     | GIF bytes                 | PNG frames  |
//! | `version`               | —                         | String      |
//!
//! # Kotlin
//! ```kotlin
//! import ferrox.*
//! val small: ByteArray = resizeImage(jpegBytes, 320u, 240u)
//! ```
//!
//! # Swift
//! ```swift
//! import Ferrox
//! let small: Data = try resizeImage(data: jpegBytes, width: 320, height: 240)
//! ```

use ferrox_core as fx;

uniffi::setup_scaffolding!();

// ── Error ───────────────────────────────────────────────────────────────────

/// Errors surfaced across the FFI boundary.
///
/// Becomes a checked Kotlin exception (`FerroxException`) and a Swift `Error`
/// you handle with `try`/`catch`.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FerroxError {
    #[error("decode failed: {message}")]
    Decode { message: String },
    #[error("encode failed: {message}")]
    Encode { message: String },
    #[error("unsupported input: {message}")]
    Unsupported { message: String },
    #[error("filter error: {message}")]
    Filter { message: String },
    #[error("processing error: {message}")]
    Processing { message: String },
}

impl From<fx::Error> for FerroxError {
    fn from(e: fx::Error) -> Self {
        match e {
            fx::Error::UnsupportedFormat(m) => FerroxError::Unsupported { message: m },
            fx::Error::UnsupportedPixelFormat => FerroxError::Unsupported {
                message: "unsupported pixel format".into(),
            },
            fx::Error::Filter(m) => FerroxError::Filter { message: m },
            fx::Error::Image(_) | fx::Error::Io(_) => FerroxError::Decode {
                message: e.to_string(),
            },
            other => FerroxError::Processing {
                message: other.to_string(),
            },
        }
    }
}

// ── helpers (ported from wasm.rs) ────────────────────────────────────────────

/// Encode a [`fx::Frame`] as PNG bytes.
fn frame_to_png(frame: fx::Frame) -> Result<Vec<u8>, FerroxError> {
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
fn decode_image_frame(data: &[u8]) -> Result<fx::Frame, FerroxError> {
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

/// Decode PNG or JPEG bytes and re-encode as PNG (input normalisation).
#[uniffi::export]
pub fn decode_image_to_png(image_data: Vec<u8>) -> Result<Vec<u8>, FerroxError> {
    frame_to_png(decode_image_frame(&image_data)?)
}

/// Resize a PNG or JPEG image to `width × height`; returns PNG bytes (Lanczos3).
#[uniffi::export]
pub fn resize_image(image_data: Vec<u8>, width: u32, height: u32) -> Result<Vec<u8>, FerroxError> {
    use fx::traits::Filter;
    use fx::ResizeFilter;
    let frame = decode_image_frame(&image_data)?;
    let resized = ResizeFilter::new(width, height).process(frame)?;
    frame_to_png(resized)
}

/// Apply a ferrox filtergraph expression to a PNG/JPEG image; returns PNG bytes.
///
/// Examples: `"blur=2.0"`, `"grayscale"`,
/// `"scale=640:480,brightness=20,contrast=1.2"`.
#[uniffi::export]
pub fn apply_filter(image_data: Vec<u8>, filter_expr: String) -> Result<Vec<u8>, FerroxError> {
    use fx::FilterGraph;
    let frame = decode_image_frame(&image_data)?;
    let out = FilterGraph::parse_and_run(frame, &filter_expr)?;
    frame_to_png(out)
}

/// Gaussian-style blur a PNG/JPEG image. `sigma` controls radius (e.g. `2.0`).
#[uniffi::export]
pub fn blur_image(image_data: Vec<u8>, sigma: f32) -> Result<Vec<u8>, FerroxError> {
    use fx::traits::Filter;
    use fx::BlurFilter;
    let frame = decode_image_frame(&image_data)?;
    let blurred = BlurFilter::new(sigma).process(frame)?;
    frame_to_png(blurred)
}

/// Convert a PNG/JPEG image to grayscale; returns PNG bytes.
#[uniffi::export]
pub fn grayscale_image(image_data: Vec<u8>) -> Result<Vec<u8>, FerroxError> {
    use fx::traits::Filter;
    use fx::GrayscaleFilter;
    let frame = decode_image_frame(&image_data)?;
    let gray = GrayscaleFilter.process(frame)?;
    frame_to_png(gray)
}

/// Return basic metadata about a PNG/JPEG image as a JSON string:
/// `{"width":1920,"height":1080,"format":"png","channels":3}`.
#[uniffi::export]
pub fn probe_image(image_data: Vec<u8>) -> Result<String, FerroxError> {
    use fx::PixelFormat;
    let frame = decode_image_frame(&image_data)?;
    let fmt = if image_data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else {
        "jpeg"
    };
    let channels = match frame.format {
        PixelFormat::Rgb8 | PixelFormat::Yuv420p => 3,
        PixelFormat::Rgba8 => 4,
        PixelFormat::Gray8 => 1,
        PixelFormat::GrayA8 => 2,
        _ => 0,
    };
    Ok(format!(
        r#"{{"width":{w},"height":{h},"format":"{fmt}","channels":{channels}}}"#,
        w = frame.width,
        h = frame.height,
    ))
}

// ── Video (VP8) ───────────────────────────────────────────────────────────────

/// Decode a single VP8 keyframe and return it as PNG bytes.
#[uniffi::export]
pub fn decode_vp8_to_png(vp8_data: Vec<u8>) -> Result<Vec<u8>, FerroxError> {
    use fx::demux_graph::yuv420p_to_rgb8;
    use fx::traits::VideoDecoder;
    use fx::video::Packet;
    use fx::Vp8Decoder;

    let packet = Packet {
        data: vp8_data,
        pts: 0,
        duration: 0,
        is_keyframe: true,
    };
    let mut decoder = Vp8Decoder;
    let vf = decoder
        .decode_packet(&packet)
        .map_err(|e| FerroxError::Decode {
            message: e.to_string(),
        })?;
    let rgb = yuv420p_to_rgb8(&vf.frame)?;
    frame_to_png(rgb)
}

// ── GIF ───────────────────────────────────────────────────────────────────────

/// One decoded GIF frame: PNG bytes plus its display delay in milliseconds.
#[derive(uniffi::Record)]
pub struct GifPngFrame {
    pub png: Vec<u8>,
    pub delay_ms: u32,
}

/// Decode an animated GIF into a list of PNG-encoded frames.
///
/// Unlike the WASM build (which returns a packed byte blob), this returns a
/// typed list so Kotlin/Swift get `List<GifPngFrame>` / `[GifPngFrame]`.
#[uniffi::export]
pub fn decode_gif_frames(gif_data: Vec<u8>) -> Result<Vec<GifPngFrame>, FerroxError> {
    use fx::decode_gif;
    let frames = decode_gif(std::io::Cursor::new(&gif_data))?;
    frames
        .into_iter()
        .map(|gf| {
            // delay is stored in centiseconds (1/100 s) in the GIF spec.
            let delay_ms = gf.delay_cs as u32 * 10;
            Ok(GifPngFrame {
                png: frame_to_png(gf.frame)?,
                delay_ms,
            })
        })
        .collect()
}

// ── Meta ───────────────────────────────────────────────────────────────────────

/// The ferrox-mobile crate version (semver string).
#[uniffi::export]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── tests ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a small RGB8 PNG (16×12, simple gradient) as a test fixture.
    fn fixture_png() -> Vec<u8> {
        let (w, h) = (16u32, 12u32);
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                data.push((x * 16) as u8);
                data.push((y * 20) as u8);
                data.push(128);
            }
        }
        let frame = fx::Frame::new(w, h, fx::PixelFormat::Rgb8, data);
        frame_to_png(frame).unwrap()
    }

    #[test]
    fn session_reports_dimensions() {
        let s = ImageSession::new(fixture_png()).unwrap();
        assert_eq!(s.width(), 16);
        assert_eq!(s.height(), 12);
    }

    #[test]
    fn chained_edits_apply_in_place() {
        let s = ImageSession::new(fixture_png()).unwrap();
        s.brightness(20).unwrap();
        s.contrast(1.2).unwrap();
        s.saturation(0.8).unwrap();
        s.grayscale().unwrap();
        // dimensions unchanged by color ops
        assert_eq!(s.width(), 16);
        assert_eq!(s.height(), 12);
        // still exports to a valid PNG
        let png = s.export_png().unwrap();
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn geometry_changes_dimensions() {
        let s = ImageSession::new(fixture_png()).unwrap();
        s.crop_center_square().unwrap();
        assert_eq!(s.width(), s.height());
        assert_eq!(s.width(), 12); // min(16,12)

        s.rotate(90).unwrap(); // square → square
        assert_eq!(s.width(), 12);

        s.resize(8, 8).unwrap();
        assert_eq!(s.width(), 8);
        assert_eq!(s.height(), 8);
    }

    #[test]
    fn rotate_rejects_bad_angle() {
        let s = ImageSession::new(fixture_png()).unwrap();
        let err = s.rotate(45).unwrap_err();
        matches!(err, FerroxError::Filter { .. });
    }

    #[test]
    fn thumbnail_preserves_aspect_within_bounds() {
        let s = ImageSession::new(fixture_png()).unwrap(); // 16×12 (4:3)
        s.thumbnail(8, 8).unwrap();
        assert!(s.width() <= 8 && s.height() <= 8);
        // wider than tall, so width should hit the bound
        assert_eq!(s.width(), 8);
    }

    #[test]
    fn export_jpeg_produces_jpeg_magic() {
        let s = ImageSession::new(fixture_png()).unwrap();
        let jpg = s.export_jpeg(90).unwrap();
        assert!(jpg.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn to_rgba8_is_tightly_packed() {
        let s = ImageSession::new(fixture_png()).unwrap();
        let raw = s.to_rgba8().unwrap();
        assert_eq!(raw.width, 16);
        assert_eq!(raw.height, 12);
        assert_eq!(raw.pixels.len(), (16 * 12 * 4) as usize);
        // opaque alpha for an Rgb8 source
        assert_eq!(raw.pixels[3], 255);
    }

    #[test]
    fn flip_keeps_dimensions() {
        let s = ImageSession::new(fixture_png()).unwrap();
        s.flip_horizontal().unwrap();
        s.flip_vertical().unwrap();
        assert_eq!(s.width(), 16);
        assert_eq!(s.height(), 12);
    }

    #[test]
    fn standalone_helpers_still_work() {
        let png = fixture_png();
        let small = resize_image(png.clone(), 8, 6).unwrap();
        assert!(small.starts_with(&[0x89, b'P', b'N', b'G']));
        let meta = probe_image(png).unwrap();
        assert!(meta.contains("\"width\":16"));
    }
}
