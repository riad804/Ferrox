//! Audio types and the audio-editing engine.
//!
//! [`AudioFrame`] is the shared sample buffer (interleaved `f32` in [-1, 1]).
//! The editing layer is split across submodules:
//! - [`mixer`] — the multi-track mixer (`mix`), the audio analog of the video
//!   compositor's `compose_frame`, plus `render_audio` export.
//! - [`effects`] — pure-Rust DSP processors (pan, EQ, compressor, reverb, …).
//! - [`waveform`] — peak/RMS bucket generation for UI display.

pub mod effects;
pub mod mixer;
pub mod waveform;

/// Supported audio container/codec formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Flac,
    Vorbis,
}

/// A decoded audio buffer: interleaved `f32` samples, normalized to [-1.0, 1.0].
#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples: [L0, R0, L1, R1, …]
    pub samples: Vec<f32>,
}

impl AudioFrame {
    pub fn new(sample_rate: u32, channels: u16, samples: Vec<f32>) -> Self {
        Self { sample_rate, channels, samples }
    }

    /// Number of frames (one sample per channel per frame).
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 { 0 } else { self.samples.len() / self.channels as usize }
    }

    /// Duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 { return 0.0; }
        self.frame_count() as f64 / self.sample_rate as f64
    }
}
