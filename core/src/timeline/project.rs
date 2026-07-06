//! [`Project`] — the root editing document (canvas + tracks + audio tracks).

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

use super::audio::AudioTrack;
use super::clip::Track;

/// A complete editing project: output canvas + z-ordered visual tracks + a
/// stack of audio tracks mixed to `sample_rate`/`channels`.
///
/// `tracks[0]` is the bottom visual layer; higher indices composite on top.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub width: u32,
    pub height: u32,
    /// Frames per second (used by the exporter/preview; not needed to compose a
    /// single frame, but part of the persisted project).
    #[serde(default = "default_fps")]
    pub fps: f64,
    /// Opaque canvas background colour, RGB.
    #[serde(default)]
    pub background: [u8; 3],
    #[serde(default)]
    pub tracks: Vec<Track>,
    /// Output audio sample rate (Hz) the mixer renders to.
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// Output audio channel count the mixer renders to.
    #[serde(default = "default_channels")]
    pub channels: u16,
    #[serde(default)]
    pub audio_tracks: Vec<AudioTrack>,
}

fn default_fps() -> f64 {
    30.0
}

fn default_sample_rate() -> u32 {
    48_000
}

fn default_channels() -> u16 {
    2
}

impl Project {
    /// A new empty project with the given output canvas and fps. Audio defaults
    /// to 48 kHz stereo (override via [`Project::with_audio_format`]).
    pub fn new(width: u32, height: u32, fps: f64) -> Self {
        Self {
            width,
            height,
            fps,
            background: [0, 0, 0],
            tracks: Vec::new(),
            sample_rate: default_sample_rate(),
            channels: default_channels(),
            audio_tracks: Vec::new(),
        }
    }

    /// Set the output audio sample rate and channel count.
    pub fn with_audio_format(mut self, sample_rate: u32, channels: u16) -> Self {
        self.sample_rate = sample_rate;
        self.channels = channels;
        self
    }

    /// Append an audio track and return `self`.
    pub fn with_audio_track(mut self, track: AudioTrack) -> Self {
        self.audio_tracks.push(track);
        self
    }

    /// Total audio duration in seconds — the end of the last-ending audio clip.
    pub fn audio_duration(&self) -> f64 {
        self.audio_tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.end())
            .fold(0.0, f64::max)
    }

    /// Set the canvas background colour (RGB).
    pub fn with_background(mut self, r: u8, g: u8, b: u8) -> Self {
        self.background = [r, g, b];
        self
    }

    /// Append a track (composited above existing tracks) and return `self`.
    pub fn with_track(mut self, track: Track) -> Self {
        self.tracks.push(track);
        self
    }

    /// Total project duration in seconds — the end of the last-ending clip.
    pub fn duration(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.start + c.duration.max(0.0))
            .fold(0.0, f64::max)
    }

    /// Serialise the project to pretty JSON (save).
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| Error::Filter(format!("project serialise failed: {e}")))
    }

    /// Deserialise a project from JSON (load).
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| Error::Filter(format!("project parse failed: {e}")))
    }
}
