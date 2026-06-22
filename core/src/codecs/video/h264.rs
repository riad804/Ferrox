//! H.264 video decoder backed by `OpenH264` (BSD-licensed C library by Cisco).
//!
//! Enabled by the `h264` feature flag; disabled by default.
//!
//! OpenH264 supports **Baseline, Main, and High** profiles (including High
//! Profile features: 8×8 DCT, CABAC, B-frames).  High Profile detection is
//! performed by parsing the SPS NAL unit embedded in the bitstream.
//!
//! # Usage
//!
//! ```toml
//! ferrox-core = { path = "…", features = ["h264"] }
//! ```
//!
//! # Output formats
//!
//! Two output modes are available via [`H264OutputMode`]:
//!
//! - `Rgb8` (default) — RGB pixel data, ready for PNG encoding.
//! - `Yuv420p` — raw YUV 4:2:0 planar data, for pipelines that need
//!   lossless chroma access or AV1 re-encoding.
//!
//! # Annex B vs. AVCC
//!
//! MP4 containers store H.264 in **AVCC** format (4-byte length-prefixed NAL
//! units). Most raw streams use **Annex B** (start-code prefixed).
//! Both are handled transparently.

use openh264::decoder::{Decoder, DecoderConfig};
use openh264::formats::YUVSource;
use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::VideoDecoder,
    video::{Packet, VideoFrame},
};

// ── Output mode ───────────────────────────────────────────────────────────────

/// Controls the pixel format that [`H264Decoder`] produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum H264OutputMode {
    /// Decode to RGB8 (3 bytes per pixel). Default.
    #[default]
    Rgb8,
    /// Decode to planar YUV 4:2:0 (8-bit). Suitable for AV1 re-encoding.
    Yuv420p,
}

// ── Profile detection ─────────────────────────────────────────────────────────

/// H.264 profile parsed from a SPS NAL unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264Profile {
    Baseline,
    Main,
    High,
    High10,
    High422,
    High444,
    Unknown(u8),
}

impl H264Profile {
    /// Parse the profile_idc byte from a SPS NAL (after the NAL type byte).
    pub fn from_idc(idc: u8) -> Self {
        match idc {
            66  => Self::Baseline,
            77  => Self::Main,
            100 => Self::High,
            110 => Self::High10,
            122 => Self::High422,
            244 => Self::High444,
            x   => Self::Unknown(x),
        }
    }
}

/// Detect the H.264 profile from Annex B or AVCC bitstream data.
///
/// Returns `None` if no SPS NAL unit is found.
pub fn detect_h264_profile(data: &[u8]) -> Option<H264Profile> {
    let annex_b = ensure_annex_b(data);
    // Scan for SPS NAL units (nal_unit_type == 7 = 0x67).
    let mut i = 0;
    while i + 4 < annex_b.len() {
        if annex_b[i..i + 4] == [0x00, 0x00, 0x00, 0x01] {
            let nal_type = annex_b.get(i + 4).copied().unwrap_or(0) & 0x1F;
            if nal_type == 7 {
                // SPS NAL: [forbidden_zero_bit(1) | nal_ref_idc(2) | nal_unit_type(5)] | profile_idc | ...
                let profile_idc = annex_b.get(i + 5).copied()?;
                return Some(H264Profile::from_idc(profile_idc));
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    None
}

// ── Decoder ───────────────────────────────────────────────────────────────────

/// H.264 video decoder backed by OpenH264.
///
/// Supports Baseline, Main, and High profiles.
pub struct H264Decoder {
    inner: Decoder,
    output_mode: H264OutputMode,
}

impl H264Decoder {
    /// Create a new decoder producing RGB8 output (default).
    pub fn new() -> Result<Self> {
        Self::with_output_mode(H264OutputMode::Rgb8)
    }

    /// Create a decoder with a specific output pixel format.
    pub fn with_output_mode(output_mode: H264OutputMode) -> Result<Self> {
        let inner = Decoder::new()
            .map_err(|e| Error::Video(format!("openh264 init: {e}")))?;
        Ok(Self { inner, output_mode })
    }

    /// Create a decoder with explicit OpenH264 config and output mode.
    pub fn with_config(cfg: DecoderConfig, output_mode: H264OutputMode) -> Result<Self> {
        let api = openh264::OpenH264API::from_source();
        let inner = Decoder::with_api_config(api, cfg)
            .map_err(|e| Error::Video(format!("openh264 init: {e}")))?;
        Ok(Self { inner, output_mode })
    }

    /// The output pixel format this decoder produces.
    pub fn output_mode(&self) -> H264OutputMode { self.output_mode }
}

impl VideoDecoder for H264Decoder {
    fn decode_packet(&mut self, packet: &Packet) -> Result<VideoFrame> {
        let data = ensure_annex_b(&packet.data);

        let yuv = match self.inner.decode(&data)
            .map_err(|e| Error::Video(format!("openh264 decode: {e}")))?
        {
            Some(y) => y,
            None => return Err(Error::Video(
                "openh264: no frame produced (SPS/PPS-only packet or buffered frame)".into()
            )),
        };

        let (w, h) = yuv.dimensions();
        let w = w as u32;
        let h = h as u32;

        let frame = match self.output_mode {
            H264OutputMode::Rgb8 => {
                let mut rgb = vec![0u8; (w * h * 3) as usize];
                yuv.write_rgb8(&mut rgb);
                Frame::new(w, h, PixelFormat::Rgb8, rgb)
            }
            H264OutputMode::Yuv420p => {
                // Extract YUV planes from openh264's YUVBuffer.
                let y_data  = yuv.y_with_stride();
                let cb_data = yuv.u_with_stride();
                let cr_data = yuv.v_with_stride();

                let y_stride  = yuv.strides_yuv().0;
                let cb_stride = yuv.strides_yuv().1;

                let w_uv = (w as usize + 1) / 2;
                let h_uv = (h as usize + 1) / 2;

                let mut data = Vec::with_capacity(
                    w as usize * h as usize + 2 * w_uv * h_uv
                );
                // Copy Y rows.
                for row in 0..h as usize {
                    let start = row * y_stride;
                    data.extend_from_slice(&y_data[start..start + w as usize]);
                }
                // Copy Cb rows.
                for row in 0..h_uv {
                    let start = row * cb_stride;
                    data.extend_from_slice(&cb_data[start..start + w_uv]);
                }
                // Copy Cr rows.
                for row in 0..h_uv {
                    let start = row * cb_stride;
                    data.extend_from_slice(&cr_data[start..start + w_uv]);
                }
                Frame::new(w, h, PixelFormat::Yuv420p, data)
            }
        };

        Ok(VideoFrame::new(frame, packet.pts, packet.duration, packet.is_keyframe))
    }
}

// ── Annex B conversion ────────────────────────────────────────────────────────

/// Ensure NAL data is in Annex B format (start-code prefixed).
///
/// Already-Annex-B packets are returned as-is.
/// AVCC packets (4-byte length prefix) are converted.
pub fn ensure_annex_b(data: &[u8]) -> Vec<u8> {
    if data.starts_with(&[0x00, 0x00, 0x00, 0x01])
        || data.starts_with(&[0x00, 0x00, 0x01])
    {
        return data.to_vec();
    }

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

    if out.is_empty() { data.to_vec() } else { out }
}
