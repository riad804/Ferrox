//! MP3 encoder backed by `libmp3lame` (LGPL).
//!
//! Enabled by the `mp3-encode` feature flag; disabled by default so the
//! default build remains free of LGPL and C dependencies.
//!
//! # System library requirement
//!
//! Install `libmp3lame` before building with this feature:
//!
//! ```sh
//! # Linux
//! apt-get install libmp3lame-dev
//! # macOS
//! brew install lame
//! # Windows (vcpkg)
//! vcpkg install mp3lame
//! ```
//!
//! # Usage
//!
//! ```toml
//! ferrox-core = { path = "…", features = ["mp3-encode"] }
//! ```
//!
//! ```no_run
//! # #[cfg(feature = "mp3-encode")] {
//! use std::fs::File;
//! use ferrox_core::codecs::audio::Mp3Encoder;
//! use ferrox_core::traits::AudioEncoder;
//! // frame: AudioFrame obtained from decoding
//! # }
//! ```

use std::io::Write;
use mp3lame_encoder::{Builder, DualPcm, FlushNoGap, Mode, MonoPcm, Quality};
use crate::{
    audio::AudioFrame,
    error::{Error, Result},
    traits::AudioEncoder,
};

/// MP3 encoder quality preset.
#[derive(Debug, Clone, Copy)]
pub enum Mp3Quality {
    /// Highest quality, slowest encoding.
    Best,
    /// Good quality, fast encoding (default).
    Good,
    /// Lowest quality, fastest encoding.
    Fast,
}

impl Default for Mp3Quality {
    fn default() -> Self { Self::Good }
}

/// Options for the MP3 encoder.
#[derive(Debug, Clone)]
pub struct Mp3Options {
    /// Target bitrate in kbps (e.g. 128, 192, 320). `None` = VBR mode.
    pub bitrate_kbps: Option<u32>,
    /// Encoding quality preset.
    pub quality: Mp3Quality,
}

impl Default for Mp3Options {
    fn default() -> Self {
        Self { bitrate_kbps: Some(192), quality: Mp3Quality::Good }
    }
}

/// MP3 encoder backed by libmp3lame.
///
/// Implements [`AudioEncoder`]; register it under `"mp3"` in
/// [`AudioEncoderRegistry`](crate::registry::AudioEncoderRegistry).
#[derive(Debug, Default, Clone)]
pub struct Mp3Encoder {
    pub opts: Mp3Options,
}

impl Mp3Encoder {
    pub fn new() -> Self { Self::default() }

    pub fn with_opts(opts: Mp3Options) -> Self { Self { opts } }

    /// Convenience constructor for a specific CBR bitrate.
    pub fn cbr(bitrate_kbps: u32) -> Self {
        Self { opts: Mp3Options { bitrate_kbps: Some(bitrate_kbps), ..Default::default() } }
    }
}

impl AudioEncoder for Mp3Encoder {
    fn encode_audio<W: Write>(&self, frame: &AudioFrame, mut writer: W) -> Result<()> {
        if frame.samples.is_empty() {
            return Err(Error::Audio("cannot encode empty audio frame to MP3".into()));
        }

        let lame_quality = match self.opts.quality {
            Mp3Quality::Best => Quality::Best,
            Mp3Quality::Good => Quality::Good,
            Mp3Quality::Fast => Quality::Worst,
        };

        let mut builder = Builder::new()
            .ok_or_else(|| Error::Audio("failed to initialise libmp3lame encoder".into()))?;

        builder.set_sample_rate(frame.sample_rate)
            .map_err(|e| Error::Audio(format!("lame set_sample_rate: {e:?}")))?;
        builder.set_num_channels(frame.channels as u8)
            .map_err(|e| Error::Audio(format!("lame set_num_channels: {e:?}")))?;
        builder.set_quality(lame_quality)
            .map_err(|e| Error::Audio(format!("lame set_quality: {e:?}")))?;

        if let Some(kbps) = self.opts.bitrate_kbps {
            let br = kbps_to_bitrate(kbps);
            builder.set_brate(br)
                .map_err(|e| Error::Audio(format!("lame set_brate: {e:?}")))?;
        } else {
            // VBR mode
            builder.set_vbr_quality(lame_quality)
                .map_err(|e| Error::Audio(format!("lame set_vbr_quality: {e:?}")))?;
        }

        // Choose stereo mode based on channel count.
        let mode = match frame.channels {
            1 => Mode::Mono,
            _ => Mode::Stereo,
        };
        builder.set_mode(mode)
            .map_err(|e| Error::Audio(format!("lame set_mode: {e:?}")))?;

        let mut enc = builder.build()
            .map_err(|e| Error::Audio(format!("lame build: {e:?}")))?;

        // Convert f32 interleaved → i16 interleaved.
        let i16_samples: Vec<i16> = frame.samples.iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();

        // libmp3lame requires the output buffer to have at least
        // ceil(1.25 * num_samples + 7200) bytes of spare capacity.
        let n_samples = i16_samples.len();
        let min_cap = (n_samples as f64 * 1.25) as usize + 7200;

        // Encode all samples in one pass + flush.
        let mut mp3_buf: Vec<u8> = Vec::with_capacity(min_cap);

        match frame.channels {
            1 => {
                enc.encode_to_vec(MonoPcm(&i16_samples), &mut mp3_buf)
                    .map_err(|e| Error::Audio(format!("lame encode (mono): {e:?}")))?;
            }
            2 => {
                // De-interleave into L/R.
                let (left, right): (Vec<i16>, Vec<i16>) = i16_samples
                    .chunks_exact(2)
                    .map(|c| (c[0], c[1]))
                    .unzip();
                enc.encode_to_vec(DualPcm { left: &left, right: &right }, &mut mp3_buf)
                    .map_err(|e| Error::Audio(format!("lame encode (stereo): {e:?}")))?;
            }
            ch => {
                // Down-mix to stereo by averaging pairs of channels.
                let stereo = downmix_to_stereo(&i16_samples, ch as usize);
                let (left, right): (Vec<i16>, Vec<i16>) = stereo
                    .chunks_exact(2)
                    .map(|c| (c[0], c[1]))
                    .unzip();
                enc.encode_to_vec(DualPcm { left: &left, right: &right }, &mut mp3_buf)
                    .map_err(|e| Error::Audio(format!("lame encode (downmix): {e:?}")))?;
            }
        }

        // Flush remaining encoder delay frames (needs at least 7200 bytes spare).
        mp3_buf.reserve(7200);
        enc.flush_to_vec::<FlushNoGap>(&mut mp3_buf)
            .map_err(|e| Error::Audio(format!("lame flush: {e:?}")))?;

        writer.write_all(&mp3_buf)
            .map_err(Error::Io)?;
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn kbps_to_bitrate(kbps: u32) -> mp3lame_encoder::Bitrate {
    use mp3lame_encoder::Bitrate::*;
    match kbps {
        ..=40   => Kbps40,
        ..=48   => Kbps48,
        ..=64   => Kbps64,
        ..=80   => Kbps80,
        ..=96   => Kbps96,
        ..=112  => Kbps112,
        ..=128  => Kbps128,
        ..=160  => Kbps160,
        ..=192  => Kbps192,
        ..=224  => Kbps224,
        ..=256  => Kbps256,
        _       => Kbps320,
    }
}

/// Down-mix an N-channel interleaved i16 stream to stereo by averaging.
fn downmix_to_stereo(samples: &[i16], channels: usize) -> Vec<i16> {
    if channels == 0 { return Vec::new(); }
    samples.chunks_exact(channels).flat_map(|frame| {
        let half = channels / 2;
        let left: i32  = frame[..half.max(1)].iter().map(|&s| s as i32).sum::<i32>()
            / half.max(1) as i32;
        let right: i32 = frame[half..].iter().map(|&s| s as i32).sum::<i32>()
            / (channels - half).max(1) as i32;
        [left.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
         right.clamp(i16::MIN as i32, i16::MAX as i32) as i16]
    }).collect()
}
