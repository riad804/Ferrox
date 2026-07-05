//! The editing **timeline** data model — the piece that turns ferrox from a
//! frame-processing library into a video *editing* engine.
//!
//! A [`Project`] holds ordered [`Track`]s, each holding [`Clip`]s placed on a
//! shared time axis (seconds). The model is fully `serde`-serialisable, so
//! `Project::to_json` / `Project::from_json` are project save/load.
//!
//! The [`crate::compositor`] consumes this model: `compose_frame(project, t)`
//! renders the output frame at any timeline position `t`. Both the (future)
//! real-time preview and the exporter share that single entry point.
//!
//! This is the M4 skeleton: still-image / solid sources with position, scale,
//! and opacity. Video-clip in/out media time, keyframed transforms, effect
//! stacks, transitions, and audio params layer on top of these types later.

use serde::{Deserialize, Serialize};

use crate::anim::{Curve, Keyframe};
use crate::audio::effects::AudioEffect;
use crate::audio::AudioFrame;
use crate::blend::BlendMode;
use crate::color::ColorGrade;
use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};
use crate::keyer::Keyer;
use crate::mask::Mask;

/// A per-clip 2-D transform applied when compositing onto the output canvas.
///
/// `x`/`y` are the top-left placement of the (scaled) clip in output pixels.
/// `scale` is a uniform factor (1.0 = original size). `opacity` is `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default = "one")]
    pub opacity: f32,
}

fn one() -> f32 {
    1.0
}

impl Default for Transform {
    fn default() -> Self {
        Self { x: 0, y: 0, scale: 1.0, opacity: 1.0 }
    }
}

impl Transform {
    /// A transform that places the clip at `(x, y)` at original size, opaque.
    pub fn at(x: i32, y: i32) -> Self {
        Self { x, y, ..Self::default() }
    }
}

/// Optional per-field keyframe animation for a clip's [`Transform`], evaluated
/// against **clip-local** time (seconds since the clip's `start`). Only the
/// fields with a curve are animated; the rest keep the static `transform`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipAnimation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Curve>,
}

impl ClipAnimation {
    /// True when no field is animated (used to omit it from serialised JSON).
    pub fn is_empty(&self) -> bool {
        self.x.is_none() && self.y.is_none() && self.scale.is_none() && self.opacity.is_none()
    }

    /// Override the animated fields of `tf` at clip-local time `local_t`.
    pub fn apply(&self, tf: &mut Transform, local_t: f64) {
        if let Some(c) = &self.x {
            tf.x = c.sample(local_t).round() as i32;
        }
        if let Some(c) = &self.y {
            tf.y = c.sample(local_t).round() as i32;
        }
        if let Some(c) = &self.scale {
            tf.scale = c.sample(local_t);
        }
        if let Some(c) = &self.opacity {
            tf.opacity = c.sample(local_t);
        }
    }

    /// Animate opacity 0→1 over the first `secs` seconds (fade in).
    pub fn fade_in(secs: f64) -> Self {
        Self {
            opacity: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(secs, 1.0)])),
            ..Self::default()
        }
    }

    /// Animate opacity 1→0 over the last `secs` seconds of a `clip_dur` clip (fade out).
    pub fn fade_out(clip_dur: f64, secs: f64) -> Self {
        Self {
            opacity: Some(Curve::keyed(vec![
                Keyframe::new((clip_dur - secs).max(0.0), 1.0),
                Keyframe::new(clip_dur, 0.0),
            ])),
            ..Self::default()
        }
    }
}

/// The visual source a clip draws from.
///
/// `Solid` is a dependency-free colour generator — ideal for backgrounds and
/// deterministic tests. `Image` decodes a PNG/JPEG file at composite time.
/// Video sources (with in/out media time) are added in a later milestone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipSource {
    /// A solid RGBA colour fill of the given size.
    Solid { width: u32, height: u32, r: u8, g: u8, b: u8, a: u8 },
    /// An image file, decoded to a frame when composited.
    Image { path: String },
}

impl ClipSource {
    /// Produce this source as an `Rgba8` [`Frame`].
    pub fn render(&self) -> Result<Frame> {
        match self {
            ClipSource::Solid { width, height, r, g, b, a } => {
                if *width == 0 || *height == 0 {
                    return Err(Error::InvalidDimensions { width: *width, height: *height });
                }
                let mut data = vec![0u8; (*width as usize) * (*height as usize) * 4];
                for px in data.chunks_exact_mut(4) {
                    px[0] = *r;
                    px[1] = *g;
                    px[2] = *b;
                    px[3] = *a;
                }
                Ok(Frame::new(*width, *height, PixelFormat::Rgba8, data))
            }
            ClipSource::Image { path } => {
                let bytes = std::fs::read(path)?;
                let frame = decode_image(&bytes)?;
                to_rgba8(frame)
            }
        }
    }
}

/// Decode PNG or JPEG bytes into a [`Frame`], reusing the core decoders.
fn decode_image(data: &[u8]) -> Result<Frame> {
    use crate::codecs::{jpeg::JpegDecoder, png::PngDecoder};
    use crate::traits::Decoder;
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        PngDecoder.decode(std::io::Cursor::new(data))
    } else if data.starts_with(&[0xFF, 0xD8]) {
        JpegDecoder.decode(std::io::Cursor::new(data))
    } else {
        Err(Error::UnsupportedFormat("clip image must be PNG or JPEG".into()))
    }
}

/// Normalise a decoded frame to `Rgba8` so the compositor has one code path.
fn to_rgba8(frame: Frame) -> Result<Frame> {
    match frame.format {
        PixelFormat::Rgba8 => Ok(frame),
        PixelFormat::Rgb8 => {
            let mut data = Vec::with_capacity((frame.width as usize) * (frame.height as usize) * 4);
            for px in frame.data.chunks_exact(3) {
                data.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgba8, data))
        }
        other => Err(Error::Filter(format!(
            "compositor sources must be Rgb8/Rgba8, got {other:?}"
        ))),
    }
}

/// A single placed clip on a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub source: ClipSource,
    /// Timeline start time, in seconds.
    pub start: f64,
    /// How long the clip is visible, in seconds (must be > 0 to ever show).
    pub duration: f64,
    #[serde(default)]
    pub transform: Transform,
    /// Optional keyframe animation overriding transform fields over time.
    #[serde(default, skip_serializing_if = "ClipAnimation::is_empty")]
    pub animation: ClipAnimation,
    /// Layer blend mode against the layers beneath.
    #[serde(default, skip_serializing_if = "BlendMode::is_normal")]
    pub blend: BlendMode,
    /// Per-clip color grade (applied before compositing).
    #[serde(default, skip_serializing_if = "ColorGrade::is_identity")]
    pub color: ColorGrade,
    /// Optional chroma-key applied before compositing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyer: Option<Keyer>,
    /// Optional vector mask multiplying the clip's alpha.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Mask>,
}

impl Clip {
    /// A clip of `source` visible for `[start, start + duration)` at `transform`.
    pub fn new(source: ClipSource, start: f64, duration: f64, transform: Transform) -> Self {
        Self {
            source,
            start,
            duration,
            transform,
            animation: ClipAnimation::default(),
            blend: BlendMode::default(),
            color: ColorGrade::default(),
            keyer: None,
            mask: None,
        }
    }

    /// Attach keyframe animation and return `self`.
    pub fn with_animation(mut self, animation: ClipAnimation) -> Self {
        self.animation = animation;
        self
    }

    /// Set the layer blend mode and return `self`.
    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        self.blend = blend;
        self
    }

    /// Set the per-clip color grade and return `self`.
    pub fn with_color(mut self, color: ColorGrade) -> Self {
        self.color = color;
        self
    }

    /// Attach a chroma-keyer and return `self`.
    pub fn with_keyer(mut self, keyer: Keyer) -> Self {
        self.keyer = Some(keyer);
        self
    }

    /// Attach a vector mask and return `self`.
    pub fn with_mask(mut self, mask: Mask) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Whether this clip is visible at timeline time `t` (seconds).
    pub fn is_active(&self, t: f64) -> bool {
        self.duration > 0.0 && t >= self.start && t < self.start + self.duration
    }

    /// The clip's transform at timeline time `t`, with any animation applied
    /// (evaluated at clip-local time `t - start`).
    pub fn effective_transform(&self, t: f64) -> Transform {
        let mut tf = self.transform;
        if !self.animation.is_empty() {
            self.animation.apply(&mut tf, t - self.start);
        }
        tf
    }
}

/// An ordered stack of clips. Lower track index = drawn first (further back).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub clips: Vec<Clip>,
}

impl Track {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a clip and return `self` for fluent construction.
    pub fn with_clip(mut self, clip: Clip) -> Self {
        self.clips.push(clip);
        self
    }
}

// ── Audio timeline ──────────────────────────────────────────────────────────

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

// ── Project ─────────────────────────────────────────────────────────────────

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
