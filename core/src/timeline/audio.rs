//! Audio timeline types: [`AudioClipSource`], [`AudioClip`], [`AudioTrack`], [`Fade`].

use serde::{Deserialize, Serialize};

use crate::audio::effects::AudioEffect;
use crate::audio::AudioFrame;
use crate::error::{Error, Result};

/// The audio a clip draws from.
///
/// `Samples`/`Silence` are dependency-free and deterministic (ideal for tests
/// and generated tones); `File` decodes an audio file via the audio-decoder
/// registry at mix time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioClipSource {
    /// An audio file (wav/mp3/flac/ogg/aac/m4a/opus), decoded when mixed.
    File { path: String },
    /// Inline interleaved `f32` samples.
    Samples { sample_rate: u32, channels: u16, samples: Vec<f32> },
    /// `duration` seconds of silence at the given format.
    Silence { sample_rate: u32, channels: u16, duration: f64 },
}

impl AudioClipSource {
    /// Produce this source as an [`AudioFrame`].
    pub fn render(&self) -> Result<AudioFrame> {
        match self {
            AudioClipSource::Samples { sample_rate, channels, samples } => {
                Ok(AudioFrame::new(*sample_rate, *channels, samples.clone()))
            }
            AudioClipSource::Silence { sample_rate, channels, duration } => {
                let frames = (duration.max(0.0) * *sample_rate as f64).round() as usize;
                let samples = vec![0.0f32; frames * (*channels).max(1) as usize];
                Ok(AudioFrame::new(*sample_rate, *channels, samples))
            }
            AudioClipSource::File { path } => decode_audio_file(path),
        }
    }
}

/// Decode an audio file to an [`AudioFrame`] using the audio-decoder registry.
fn decode_audio_file(path: &str) -> Result<AudioFrame> {
    use crate::registry::AudioDecoderRegistry;
    let p = std::path::Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| Error::UnsupportedFormat(format!("no extension on '{path}'")))?;
    let registry = AudioDecoderRegistry::default();
    let decoder = registry
        .get(ext)
        .ok_or_else(|| Error::UnsupportedFormat(format!("no audio decoder for '{ext}'")))?;
    let file = std::fs::File::open(p)?;
    let mut reader = std::io::BufReader::new(file);
    decoder.decode_audio_dyn(&mut reader)
}

/// Linear fade in/out envelope for a clip, in seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Fade {
    #[serde(default)]
    pub in_secs: f64,
    #[serde(default)]
    pub out_secs: f64,
}

/// A single placed audio clip on an audio track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub source: AudioClipSource,
    /// Timeline start time, in seconds.
    pub start: f64,
    /// Trim: seconds into the source where playback begins.
    #[serde(default)]
    pub in_offset: f64,
    /// How long the clip plays, in seconds.
    pub duration: f64,
    /// Clip gain in decibels (0 = unity).
    #[serde(default)]
    pub gain_db: f32,
    /// Stereo pan, -1.0 (left) … +1.0 (right).
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub fade: Fade,
    /// Ordered clip effect stack.
    #[serde(default)]
    pub effects: Vec<AudioEffect>,
}

impl AudioClip {
    /// A clip of `source` playing for `[start, start + duration)` seconds.
    pub fn new(source: AudioClipSource, start: f64, duration: f64) -> Self {
        Self {
            source,
            start,
            in_offset: 0.0,
            duration,
            gain_db: 0.0,
            pan: 0.0,
            fade: Fade::default(),
            effects: Vec::new(),
        }
    }

    pub fn with_in_offset(mut self, in_offset: f64) -> Self {
        self.in_offset = in_offset;
        self
    }
    pub fn with_gain_db(mut self, gain_db: f32) -> Self {
        self.gain_db = gain_db;
        self
    }
    pub fn with_pan(mut self, pan: f32) -> Self {
        self.pan = pan;
        self
    }
    pub fn with_fade(mut self, in_secs: f64, out_secs: f64) -> Self {
        self.fade = Fade { in_secs, out_secs };
        self
    }
    pub fn with_effects(mut self, effects: Vec<AudioEffect>) -> Self {
        self.effects = effects;
        self
    }

    /// Whether this clip plays at timeline time `t` (seconds).
    pub fn is_active(&self, t: f64) -> bool {
        self.duration > 0.0 && t >= self.start && t < self.start + self.duration
    }

    /// Timeline end time (seconds).
    pub fn end(&self) -> f64 {
        self.start + self.duration.max(0.0)
    }
}

/// A stack of audio clips mixed together with a track gain.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    #[serde(default)]
    pub clips: Vec<AudioClip>,
    /// Track gain in decibels (0 = unity).
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub muted: bool,
}

impl AudioTrack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_clip(mut self, clip: AudioClip) -> Self {
        self.clips.push(clip);
        self
    }

    pub fn with_gain_db(mut self, gain_db: f32) -> Self {
        self.gain_db = gain_db;
        self
    }

    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }
}
