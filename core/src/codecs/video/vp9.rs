//! VP9 video decoder backed by `libdav1d` (BSD-licensed C library).
//!
//! Enabled by the `vp9` feature flag; disabled by default so that pure-Rust
//! builds remain the default.
//!
//! # Supported formats
//!
//! | Bit depth | Layout | ferrox PixelFormat |
//! |-----------|--------|--------------------|
//! | 8-bit     | YUV 4:2:0 | `Yuv420p`      |
//! | 10-bit    | YUV 4:2:0 | `Yuv420p10`    |
//! | 12-bit    | YUV 4:2:0 | `Yuv420p12`    |
//! | 8-bit     | YUV 4:2:2 | `Yuv422p`      |
//! | 8-bit     | YUV 4:4:4 | `Yuv444p`      |
//!
//! 10/12-bit samples are stored as **little-endian u16** values (the low
//! 10 or 12 bits are significant; the upper bits are zero-padded).
//!
//! # Usage
//!
//! ```toml
//! ferrox-core = { path = "…", features = ["vp9"] }
//! ```
//!
//! ```no_run
//! # #[cfg(feature = "vp9")] {
//! use ferrox_core::codecs::video::Vp9Decoder;
//! use ferrox_core::traits::VideoDecoder;
//! let mut dec = Vp9Decoder::new().unwrap();
//! # }
//! ```

use dav1d::{Decoder, Settings, PixelLayout, PlanarImageComponent};
use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::VideoDecoder,
    video::{Packet, VideoFrame},
};

/// VP9 video decoder backed by `libdav1d`.
///
/// Supports 8/10/12-bit YUV 4:2:0 (SDR and HDR) as well as 4:2:2 and 4:4:4
/// chroma subsampling.  One instance decodes a complete stream sequentially.
pub struct Vp9Decoder {
    inner: Decoder,
}

impl Vp9Decoder {
    /// Create a new decoder (auto-detects thread count).
    pub fn new() -> Result<Self> {
        let mut settings = Settings::new();
        settings.set_n_threads(0); // 0 = use all available cores
        let inner = Decoder::with_settings(&settings)
            .map_err(|e| Error::Video(format!("dav1d init: {e}")))?;
        Ok(Self { inner })
    }
}

impl VideoDecoder for Vp9Decoder {
    fn decode_packet(&mut self, packet: &Packet) -> Result<VideoFrame> {
        self.inner
            .send_data(packet.data.clone(), None, None, None)
            .map_err(|e| Error::Video(format!("dav1d send_data: {e}")))?;

        let pic = self.inner
            .get_picture()
            .map_err(|e| Error::Video(format!("dav1d get_picture: {e}")))?;

        let w = pic.width();
        let h = pic.height();
        let depth  = pic.bit_depth();
        let layout = pic.pixel_layout();

        let pixel_format = pixel_format_for(depth, layout)?;
        let data = extract_planes(&pic, w, h, layout, depth)?;

        let frame = Frame::new(w, h, pixel_format, data);
        Ok(VideoFrame::new(frame, packet.pts, packet.duration, packet.is_keyframe))
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Map dav1d (bit_depth, PixelLayout) → ferrox PixelFormat.
fn pixel_format_for(depth: usize, layout: PixelLayout) -> Result<PixelFormat> {
    match (depth, layout) {
        (8,  PixelLayout::I420) => Ok(PixelFormat::Yuv420p),
        (10, PixelLayout::I420) => Ok(PixelFormat::Yuv420p10),
        (12, PixelLayout::I420) => Ok(PixelFormat::Yuv420p12),
        (8,  PixelLayout::I422) => Ok(PixelFormat::Yuv422p),
        (8,  PixelLayout::I444) => Ok(PixelFormat::Yuv444p),
        (d,  l) => Err(Error::Video(format!(
            "VP9: unsupported {d}-bit {l:?} — only 8/10/12-bit I420, 8-bit I422/I444 are supported"
        ))),
    }
}

/// Copy all planes from a dav1d `Picture` into a flat buffer,
/// row-by-row to skip stride padding.
///
/// For 8-bit formats each sample is one `u8`.
/// For 10/12-bit formats each sample is two bytes (little-endian u16).
fn extract_planes(
    pic: &dav1d::Picture,
    w: u32,
    h: u32,
    layout: PixelLayout,
    depth: usize,
) -> Result<Vec<u8>> {
    let w = w as usize;
    let h = h as usize;
    let bps = if depth > 8 { 2usize } else { 1usize };

    // Chroma dimensions per layout.
    let (uv_w, uv_h) = match layout {
        PixelLayout::I420 => ((w + 1) / 2, (h + 1) / 2),
        PixelLayout::I422 => ((w + 1) / 2, h),
        PixelLayout::I444 => (w, h),
        PixelLayout::I400 => (0, 0), // monochrome, no chroma
    };

    let total = (w * h + 2 * uv_w * uv_h) * bps;
    let mut data: Vec<u8> = Vec::with_capacity(total);

    let components: &[PlanarImageComponent] = if layout == PixelLayout::I400 {
        &[PlanarImageComponent::Y]
    } else {
        &[PlanarImageComponent::Y, PlanarImageComponent::U, PlanarImageComponent::V]
    };

    for &comp in components {
        let plane  = pic.plane(comp);
        let stride = pic.stride(comp) as usize; // in bytes (even for 10/12-bit)
        let (plane_w, plane_h) = match comp {
            PlanarImageComponent::Y => (w, h),
            _                       => (uv_w, uv_h),
        };
        let bytes: &[u8] = plane.as_ref();
        // Each row is `plane_w * bps` bytes of payload; the rest is padding.
        let row_bytes = plane_w * bps;
        for row in 0..plane_h {
            let start = row * stride;
            data.extend_from_slice(&bytes[start..start + row_bytes]);
        }
    }

    Ok(data)
}
