//! VP9 video decoder backed by `libdav1d` (BSD-licensed C library).
//!
//! Enabled by the `vp9` feature flag; disabled by default so that pure-Rust
//! builds remain the default.
//!
//! `libdav1d` is the reference VP9/AV1 decoder used by VLC, Firefox, and
//! Chrome. It is a single C library with no transitive C dependencies beyond
//! libc and is available on all major platforms via package managers.
//!
//! # Usage
//!
//! Enable the feature and use `Vp9Decoder` anywhere a `VideoDecoder` is
//! accepted:
//!
//! ```toml
//! # Cargo.toml
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
/// One instance can decode a complete stream sequentially. Instantiate once
/// and call [`VideoDecoder::decode_packet`] for each compressed packet.
pub struct Vp9Decoder {
    inner: Decoder,
}

impl Vp9Decoder {
    /// Create a new decoder with default settings (single-threaded, no grain).
    pub fn new() -> Result<Self> {
        let mut settings = Settings::new();
        // Use all available threads for performance.
        settings.set_n_threads(0); // 0 = auto-detect
        let inner = Decoder::with_settings(&settings)
            .map_err(|e| Error::Video(format!("dav1d init: {e}")))?;
        Ok(Self { inner })
    }
}

impl VideoDecoder for Vp9Decoder {
    fn decode_packet(&mut self, packet: &Packet) -> Result<VideoFrame> {
        // Feed the compressed data to dav1d.
        self.inner
            .send_data(packet.data.clone(), None, None, None)
            .map_err(|e| Error::Video(format!("dav1d send_data: {e}")))?;

        // Retrieve the decoded picture.
        let pic = self.inner
            .get_picture()
            .map_err(|e| Error::Video(format!("dav1d get_picture: {e}")))?;

        let w = pic.width();
        let h = pic.height();

        // dav1d outputs YUV420p for standard VP9 streams.
        // We support 8-bit YUV420 (the overwhelming common case for web video).
        if pic.bit_depth() != 8 {
            return Err(Error::Video(format!(
                "VP9: {}-bit depth not yet supported (only 8-bit)",
                pic.bit_depth()
            )));
        }
        if pic.pixel_layout() != PixelLayout::I420 {
            return Err(Error::Video(format!(
                "VP9: pixel layout {:?} not supported (only YUV420/I420)",
                pic.pixel_layout()
            )));
        }

        let yuv = extract_yuv420(&pic, w, h)?;
        let frame = Frame::new(w, h, PixelFormat::Yuv420p, yuv);
        Ok(VideoFrame::new(frame, packet.pts, packet.duration, packet.is_keyframe))
    }
}

/// Copy Y/Cb/Cr planes from a dav1d `Picture` into a flat YUV420p buffer.
///
/// dav1d planes may have padding (stride > width), so we copy row-by-row.
fn extract_yuv420(pic: &dav1d::Picture, w: u32, h: u32) -> Result<Vec<u8>> {
    let w = w as usize;
    let h = h as usize;
    let uv_w = (w + 1) / 2;
    let uv_h = (h + 1) / 2;

    let mut data = Vec::with_capacity(w * h + 2 * uv_w * uv_h);

    for component in [PlanarImageComponent::Y, PlanarImageComponent::U, PlanarImageComponent::V] {
        let plane  = pic.plane(component);
        let stride = pic.stride(component) as usize;
        let (plane_w, plane_h) = match component {
            PlanarImageComponent::Y => (w, h),
            _                       => (uv_w, uv_h),
        };
        let bytes: &[u8] = plane.as_ref();
        for row in 0..plane_h {
            let start = row * stride;
            data.extend_from_slice(&bytes[start..start + plane_w]);
        }
    }

    Ok(data)
}
