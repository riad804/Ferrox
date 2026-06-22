//! H.264 video decoder backed by `OpenH264` (BSD-licensed C library by Cisco).
//!
//! Enabled by the `h264` feature flag; disabled by default.
//!
//! OpenH264 supports Baseline, Main, and High profiles — covers essentially all
//! web and mobile H.264 content. Cisco provides pre-built `.so`/`.dll` binaries
//! on their CDN under a royalty-free patent grant.
//!
//! # Usage
//!
//! ```toml
//! ferrox-core = { path = "…", features = ["h264"] }
//! ```
//!
//! ```no_run
//! # #[cfg(feature = "h264")] {
//! use ferrox_core::codecs::video::H264Decoder;
//! use ferrox_core::traits::VideoDecoder;
//! let mut dec = H264Decoder::new().unwrap();
//! # }
//! ```
//!
//! # Annex B vs. AVCC
//!
//! MP4 containers store H.264 in **AVCC** format (4-byte length-prefixed NAL
//! units). Most raw streams and WebM use **Annex B** (start-code prefixed).
//!
//! This decoder handles both: packets starting with `\x00\x00\x00\x01` or
//! `\x00\x00\x01` are forwarded as-is (Annex B); otherwise the packet is
//! assumed AVCC and converted by replacing the length prefix with a start code.

use openh264::decoder::{Decoder, DecoderConfig};
use openh264::formats::YUVSource;
use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::VideoDecoder,
    video::{Packet, VideoFrame},
};

/// H.264 video decoder backed by OpenH264.
pub struct H264Decoder {
    inner: Decoder,
}

impl H264Decoder {
    /// Create a new decoder with default OpenH264 settings.
    pub fn new() -> Result<Self> {
        let inner = Decoder::new()
            .map_err(|e| Error::Video(format!("openh264 init: {e}")))?;
        Ok(Self { inner })
    }

    /// Create a decoder with explicit config.
    pub fn with_config(cfg: DecoderConfig) -> Result<Self> {
        let api = openh264::OpenH264API::from_source();
        let inner = Decoder::with_api_config(api, cfg)
            .map_err(|e| Error::Video(format!("openh264 init: {e}")))?;
        Ok(Self { inner })
    }
}

impl VideoDecoder for H264Decoder {
    fn decode_packet(&mut self, packet: &Packet) -> Result<VideoFrame> {
        let data = ensure_annex_b(&packet.data);

        let yuv = match self.inner.decode(&data)
            .map_err(|e| Error::Video(format!("openh264 decode: {e}")))?
        {
            Some(y) => y,
            // OpenH264 returned no frame for this packet (e.g. SPS/PPS-only).
            None => return Err(Error::Video(
                "openh264: no frame produced for this NAL unit (SPS/PPS/buffered)".into()
            )),
        };

        let (w, h) = yuv.dimensions();
        let w = w as u32;
        let h = h as u32;

        // openh264 gives us a convenient write_rgb8 method — use it to get
        // Rgb8 output directly, avoiding manual YUV→RGB conversion.
        let mut rgb = vec![0u8; (w * h * 3) as usize];
        yuv.write_rgb8(&mut rgb);

        let frame = Frame::new(w, h, PixelFormat::Rgb8, rgb);
        Ok(VideoFrame::new(frame, packet.pts, packet.duration, packet.is_keyframe))
    }
}

// ── Annex B conversion ────────────────────────────────────────────────────────

/// Ensure NAL data is in Annex B format (start-code prefixed).
///
/// If the packet already starts with a 3- or 4-byte start code it is returned
/// as-is. Otherwise it is assumed to be AVCC (length-prefixed) and converted.
fn ensure_annex_b(data: &[u8]) -> Vec<u8> {
    // Already Annex B?
    if data.starts_with(&[0x00, 0x00, 0x00, 0x01])
        || data.starts_with(&[0x00, 0x00, 0x01])
    {
        return data.to_vec();
    }

    // AVCC → Annex B: replace 4-byte length fields with start codes.
    let start_code: &[u8] = &[0x00, 0x00, 0x00, 0x01];
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut pos = 0;

    while pos + 4 <= data.len() {
        let nal_len = u32::from_be_bytes([
            data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
        ]) as usize;
        pos += 4;

        if pos + nal_len > data.len() { break; }

        out.extend_from_slice(start_code);
        out.extend_from_slice(&data[pos..pos + nal_len]);
        pos += nal_len;
    }

    if out.is_empty() {
        // Could not parse as AVCC — forward raw and let the decoder decide.
        data.to_vec()
    } else {
        out
    }
}
