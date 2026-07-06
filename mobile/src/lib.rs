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


mod functions;
mod session;

pub use functions::*;
pub use session::{ImageSession, RawImage, RgbaColor};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::frame_to_png;

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
