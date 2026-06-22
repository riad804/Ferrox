//! Opus encoder backed by `libopus` (BSD-3, Xiph).
//!
//! Enabled by the `opus-encode` feature flag; disabled by default.
//!
//! The output is a raw Opus bitstream wrapped in an **Ogg container**
//! (`.opus` files per RFC 7845). Opus inside WebM/MKV is also possible
//! but requires a container muxer; raw Ogg Opus is the most universally
//! compatible standalone format.
//!
//! # System library requirement
//!
//! ```sh
//! # Linux
//! apt-get install libopus-dev
//! # macOS
//! brew install opus
//! # Windows (vcpkg)
//! vcpkg install opus
//! ```
//!
//! # Usage
//!
//! ```toml
//! ferrox-core = { path = "…", features = ["opus-encode"] }
//! ```
//!
//! ```no_run
//! # #[cfg(feature = "opus-encode")] {
//! use ferrox_core::codecs::audio::OpusEncoder;
//! use ferrox_core::traits::AudioEncoder;
//! # }
//! ```
//!
//! # Frame-size requirement
//!
//! Opus processes audio in fixed-size frames of 2.5–60 ms. This encoder
//! uses 20 ms frames (960 samples at 48 kHz). Input at other sample rates
//! is resampled to 48 kHz automatically using rubato.

use std::io::Write;
use audiopus::{
    coder::Encoder as OpusInner,
    Application, Channels, SampleRate,
};
use crate::{
    audio::AudioFrame,
    error::{Error, Result},
    filters::ResampleFilter,
    traits::{AudioEncoder, AudioFilter},
};

// Opus frame sizes at 48 kHz (samples per channel).
const OPUS_SAMPLE_RATE: u32 = 48_000;
const FRAME_SIZE: usize = 960; // 20 ms at 48 kHz

/// Opus encoding application type.
#[derive(Debug, Clone, Copy, Default)]
pub enum OpusApplication {
    /// General-purpose audio (music, podcasts). Default.
    #[default]
    Audio,
    /// Optimised for voice/speech.
    Voip,
    /// Lowest latency (restricted).
    LowDelay,
}

/// Options for the Opus encoder.
#[derive(Debug, Clone)]
pub struct OpusOptions {
    /// Bitrate in bits per second (e.g. 64_000, 128_000). Default: 128 kbps.
    pub bitrate_bps: i32,
    /// Application type hint.
    pub application: OpusApplication,
}

impl Default for OpusOptions {
    fn default() -> Self {
        Self { bitrate_bps: 128_000, application: OpusApplication::Audio }
    }
}

/// Opus encoder producing Ogg-wrapped `.opus` output.
#[derive(Debug, Default, Clone)]
pub struct OpusEncoder {
    pub opts: OpusOptions,
}

impl OpusEncoder {
    pub fn new() -> Self { Self::default() }

    pub fn with_opts(opts: OpusOptions) -> Self { Self { opts } }

    pub fn with_bitrate(bitrate_bps: i32) -> Self {
        Self { opts: OpusOptions { bitrate_bps, ..Default::default() } }
    }
}

impl AudioEncoder for OpusEncoder {
    fn encode_audio<W: Write>(&self, frame: &AudioFrame, mut writer: W) -> Result<()> {
        if frame.samples.is_empty() {
            return Err(Error::Audio("cannot encode empty audio frame to Opus".into()));
        }

        // Resample to 48 kHz if needed — Opus only accepts 8/12/16/24/48 kHz.
        let frame48 = if frame.sample_rate != OPUS_SAMPLE_RATE {
            let f = ResampleFilter::new(OPUS_SAMPLE_RATE);
            f.process_audio(frame.clone())?
        } else {
            frame.clone()
        };

        // Clamp to mono/stereo — Opus only supports 1 or 2 channels.
        let (channels, pcm) = match frame48.channels {
            1 => (Channels::Mono, frame48.samples.clone()),
            2 => (Channels::Stereo, frame48.samples.clone()),
            n => {
                // Down-mix to stereo.
                let stereo = downmix_to_stereo_f32(&frame48.samples, n as usize);
                (Channels::Stereo, stereo)
            }
        };

        let app = match self.opts.application {
            OpusApplication::Audio    => Application::Audio,
            OpusApplication::Voip     => Application::Voip,
            OpusApplication::LowDelay => Application::LowDelay,
        };

        let mut enc = OpusInner::new(SampleRate::Hz48000, channels, app)
            .map_err(|e| Error::Audio(format!("opus encoder init: {e}")))?;

        enc.set_bitrate(audiopus::Bitrate::BitsPerSecond(self.opts.bitrate_bps))
            .map_err(|e| Error::Audio(format!("opus set_bitrate: {e}")))?;

        let ch_count = channels as usize;
        let frame_samples = FRAME_SIZE * ch_count; // interleaved samples per frame

        // Encode all frames into raw Opus packets.
        let mut packets: Vec<Vec<u8>> = Vec::new();
        let mut out_buf = vec![0u8; 4096];

        for chunk in pcm.chunks(frame_samples) {
            // Zero-pad the last chunk if it's short.
            let input: Vec<f32> = if chunk.len() < frame_samples {
                let mut padded = chunk.to_vec();
                padded.resize(frame_samples, 0.0);
                padded
            } else {
                chunk.to_vec()
            };

            let n = enc.encode_float(&input, &mut out_buf)
                .map_err(|e| Error::Audio(format!("opus encode_float: {e}")))?;
            packets.push(out_buf[..n].to_vec());
        }

        // Write Ogg container wrapping the raw Opus packets.
        write_ogg_opus(
            &mut writer,
            &packets,
            frame48.sample_rate,
            frame48.channels.min(2),
            FRAME_SIZE as u64,
        )
    }
}

// ── Ogg Opus container writer ─────────────────────────────────────────────────
//
// Implements RFC 7845 (Ogg Opus) — enough to produce a valid file readable
// by ffmpeg, VLC, and browser MediaSource. No external ogg crate needed.

fn write_ogg_opus<W: Write>(
    writer: &mut W,
    packets: &[Vec<u8>],
    _sample_rate: u32,
    channels: u16,
    frame_size: u64,
) -> Result<()> {
    // Serial number (arbitrary, non-zero).
    let serial: u32 = 0x4F_50_55_53; // "OPUS"

    let mut granule_pos: u64 = 0;
    let mut seq: u32 = 0;

    // ── Page 0: OpusHead ──────────────────────────────────────────────────────
    let opus_head = build_opus_head(channels as u8, OPUS_SAMPLE_RATE);
    write_ogg_page(writer, serial, 0, 0, seq, true, false, &[opus_head.as_slice()])
        .map_err(Error::Io)?;
    seq += 1;

    // ── Page 1: OpusTags ─────────────────────────────────────────────────────
    let opus_tags = build_opus_tags("ferrox");
    write_ogg_page(writer, serial, 0, seq, seq, false, false, &[opus_tags.as_slice()])
        .map_err(Error::Io)?;
    seq += 1;

    // ── Audio pages: up to 255 packets per page ───────────────────────────────
    // RFC 7845 §4: granule position = sample count encoded so far (at 48 kHz).
    for chunk in packets.chunks(255) {
        granule_pos += frame_size * chunk.len() as u64;
        let slices: Vec<&[u8]> = chunk.iter().map(|p| p.as_slice()).collect();
        let is_last = std::ptr::eq(chunk as *const _, packets.chunks(255).last().unwrap() as *const _);
        write_ogg_page(writer, serial, granule_pos, seq, seq, false, is_last, &slices)
            .map_err(Error::Io)?;
        seq += 1;
    }

    Ok(())
}

/// Build the OpusHead binary header (RFC 7845 §5.1).
fn build_opus_head(channels: u8, input_sample_rate: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(19);
    h.extend_from_slice(b"OpusHead");
    h.push(1);                                          // version
    h.push(channels);                                   // channel count
    h.extend_from_slice(&3840u16.to_le_bytes());        // pre-skip (typical for 48 kHz)
    h.extend_from_slice(&input_sample_rate.to_le_bytes()); // original sample rate
    h.extend_from_slice(&0i16.to_le_bytes());           // output gain
    h.push(0);                                          // channel mapping family 0
    h
}

/// Build the OpusTags binary header (RFC 7845 §5.2).
fn build_opus_tags(vendor: &str) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(b"OpusTags");
    t.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    t.extend_from_slice(vendor.as_bytes());
    t.extend_from_slice(&0u32.to_le_bytes()); // 0 user comments
    t
}

/// Write one Ogg page (RFC 3533).
fn write_ogg_page<W: Write>(
    w: &mut W,
    serial: u32,
    granule_pos: u64,
    seq_no: u32,
    _page_seq: u32,
    is_bos: bool,
    is_eos: bool,
    packets: &[&[u8]],
) -> std::io::Result<()> {
    // Build segment table (lacing).
    let mut lacing: Vec<u8> = Vec::new();
    let mut body: Vec<u8>   = Vec::new();

    for &pkt in packets {
        let mut remaining = pkt.len();
        loop {
            let seg = remaining.min(255);
            lacing.push(seg as u8);
            remaining -= seg;
            if seg < 255 { break; }
        }
        body.extend_from_slice(pkt);
    }

    let mut header = Vec::with_capacity(27 + lacing.len());
    header.extend_from_slice(b"OggS");
    header.push(0);            // stream structure version
    let mut flags: u8 = 0;
    if is_bos { flags |= 0x02; }
    if is_eos { flags |= 0x04; }
    header.push(flags);
    header.extend_from_slice(&granule_pos.to_le_bytes());
    header.extend_from_slice(&serial.to_le_bytes());
    header.extend_from_slice(&seq_no.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
    header.push(lacing.len() as u8);
    header.extend_from_slice(&lacing);

    // Compute CRC over header + body.
    let crc = ogg_crc32(&header, &body);
    header[22..26].copy_from_slice(&crc.to_le_bytes());

    w.write_all(&header)?;
    w.write_all(&body)?;
    Ok(())
}

/// Ogg CRC-32 (RFC 3533 §6.3 — generator polynomial 0x04C11DB7, init 0).
fn ogg_crc32(header: &[u8], body: &[u8]) -> u32 {
    // Pre-computed lookup table for the Ogg CRC-32 polynomial.
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, entry) in t.iter_mut().enumerate() {
            let mut crc = (i as u32) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ 0x04C1_1DB7
                } else {
                    crc << 1
                };
            }
            *entry = crc;
        }
        t
    });

    let mut crc: u32 = 0;
    for &b in header.iter().chain(body.iter()) {
        let idx = ((crc >> 24) ^ b as u32) as usize;
        crc = (crc << 8) ^ table[idx];
    }
    crc
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn downmix_to_stereo_f32(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 { return Vec::new(); }
    samples.chunks_exact(channels).flat_map(|frame| {
        let half = channels / 2;
        let left: f32  = frame[..half.max(1)].iter().copied().sum::<f32>()
            / half.max(1) as f32;
        let right: f32 = frame[half..].iter().copied().sum::<f32>()
            / (channels - half).max(1) as f32;
        [left, right]
    }).collect()
}
