//! MP3 / AAC / Opus / generic audio decoder backed by `symphonia` (pure Rust).

use std::io::{Cursor, Read};
use symphonia::core::{
    audio::{AudioBufferRef, Signal},
    codecs::DecoderOptions,
    formats::FormatOptions,
    io::{MediaSourceStream, ReadOnlySource},
    meta::MetadataOptions,
    probe::Hint,
};
use crate::{audio::AudioFrame, error::{Error, Result}, traits::AudioDecoder};

// ── internal helper ───────────────────────────────────────────────────────────

/// Decode any format symphonia can probe. Buffers the whole reader into memory
/// so we can satisfy symphonia's `Send + 'static` bounds.
fn decode_via_symphonia(bytes: Vec<u8>, hint: Hint) -> Result<AudioFrame> {
    let cursor = Cursor::new(bytes);
    let source = ReadOnlySource::new(cursor);
    let mss = MediaSourceStream::new(Box::new(source), Default::default());

    let fmt_opts  = FormatOptions { enable_gapless: true, ..Default::default() };
    let meta_opts = MetadataOptions::default();
    let dec_opts  = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .map_err(|e| Error::Audio(format!("symphonia probe: {e}")))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Audio("no audio track found".into()))?
        .clone();

    let track_id    = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels    = track.codec_params.channels
        .map(|c| c.count() as u16)
        .unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .map_err(|e| Error::Audio(format!("symphonia make decoder: {e}")))?;

    let mut samples: Vec<f32> = Vec::new();

    loop {
        use symphonia::core::errors::Error as SE;
        let packet = match format.next_packet() {
            Ok(p)  => p,
            Err(SE::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SE::ResetRequired) => { decoder.reset(); continue; }
            Err(e) => return Err(Error::Audio(format!("next_packet: {e}"))),
        };

        if packet.track_id() != track_id { continue; }

        match decoder.decode(&packet) {
            Ok(buf) => append_samples(&buf, &mut samples),
            Err(SE::IoError(_))           => continue,
            Err(SE::DecodeError(e))       => { tracing::warn!("decode skip: {e}"); continue; }
            Err(e) => return Err(Error::Audio(format!("decode: {e}"))),
        }
    }

    if samples.is_empty() {
        return Err(Error::Audio("no audio samples decoded".into()));
    }
    Ok(AudioFrame::new(sample_rate, channels, samples))
}

/// Append interleaved f32 samples from any symphonia buffer variant.
fn append_samples(buf: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    match buf {
        AudioBufferRef::F32(b) => {
            let n = b.frames();
            let planes = b.planes();
            for i in 0..n {
                for plane in planes.planes() { out.push(plane[i]); }
            }
        }
        AudioBufferRef::S16(b) => {
            let n = b.frames();
            let planes = b.planes();
            for i in 0..n {
                for plane in planes.planes() { out.push(plane[i] as f32 / i16::MAX as f32); }
            }
        }
        AudioBufferRef::S32(b) => {
            let n = b.frames();
            let planes = b.planes();
            for i in 0..n {
                for plane in planes.planes() { out.push(plane[i] as f32 / i32::MAX as f32); }
            }
        }
        AudioBufferRef::U8(b) => {
            let n = b.frames();
            let planes = b.planes();
            for i in 0..n {
                for plane in planes.planes() { out.push((plane[i] as f32 - 128.0) / 128.0); }
            }
        }
        // S8, U16, U32, F64 — uncommon; convert via f64 path
        AudioBufferRef::F64(b) => {
            let n = b.frames();
            let planes = b.planes();
            for i in 0..n {
                for plane in planes.planes() { out.push(plane[i] as f32); }
            }
        }
        _ => {}
    }
}

// ── public decoders ───────────────────────────────────────────────────────────

/// MP3 decoder backed by `symphonia` (pure Rust, no C FFI).
pub struct Mp3Decoder;

impl AudioDecoder for Mp3Decoder {
    fn decode_audio<R: Read>(&self, mut reader: R) -> Result<AudioFrame> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        let mut hint = Hint::new();
        hint.mime_type("audio/mpeg");
        decode_via_symphonia(bytes, hint)
    }
}

/// Generic symphonia-backed decoder — auto-probes format from content.
///
/// Covers AAC (in M4A/ADTS), Opus-in-Ogg, and any other format symphonia
/// supports. Provide an extension hint for better detection.
pub struct SymphoniaDecoder {
    pub ext_hint: Option<String>,
}

impl SymphoniaDecoder {
    pub fn new() -> Self { Self { ext_hint: None } }
    pub fn with_ext(ext: impl Into<String>) -> Self { Self { ext_hint: Some(ext.into()) } }
}

impl Default for SymphoniaDecoder {
    fn default() -> Self { Self::new() }
}

impl AudioDecoder for SymphoniaDecoder {
    fn decode_audio<R: Read>(&self, mut reader: R) -> Result<AudioFrame> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        let mut hint = Hint::new();
        if let Some(ext) = &self.ext_hint {
            hint.with_extension(ext);
        }
        decode_via_symphonia(bytes, hint)
    }
}
