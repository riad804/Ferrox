//! Pure-Rust audio DSP effects. Every processor implements [`AudioFilter`], so
//! it works standalone, in an [`crate::graph::AudioGraph`], or as a clip effect
//! in the [`crate::audio::mixer`].
//!
//! [`AudioEffect`] is a serialisable parameter enum (part of a saved project);
//! [`AudioEffect::build`] turns it into the corresponding processor, and
//! [`apply_effects`] runs an ordered effect stack over a frame.
//!
//! All processors read `sample_rate` from the frame at process time and hold
//! their filter state locally for the duration of one `process_audio` call
//! (each clip is processed as one whole buffer), so no interior mutability is
//! needed and `&self` stays immutable.

use serde::{Deserialize, Serialize};

use crate::audio::AudioFrame;
use crate::error::Result;
use crate::traits::AudioFilter;

// ── helpers ────────────────────────────────────────────────────────────────

/// Convert decibels to a linear amplitude factor.
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Split interleaved samples into per-channel planes.
fn deinterleave(frame: &AudioFrame) -> Vec<Vec<f32>> {
    let ch = frame.channels.max(1) as usize;
    let n = frame.frame_count();
    let mut planes = vec![vec![0.0f32; n]; ch];
    for i in 0..n {
        for (c, plane) in planes.iter_mut().enumerate() {
            plane[i] = frame.samples[i * ch + c];
        }
    }
    planes
}

/// Re-interleave per-channel planes into an [`AudioFrame`].
fn interleave(planes: &[Vec<f32>], sample_rate: u32) -> AudioFrame {
    let ch = planes.len().max(1);
    let n = planes.first().map(|p| p.len()).unwrap_or(0);
    let mut samples = vec![0.0f32; n * ch];
    for i in 0..n {
        for (c, plane) in planes.iter().enumerate() {
            samples[i * ch + c] = plane[i];
        }
    }
    AudioFrame::new(sample_rate, ch as u16, samples)
}

// ── biquad (shared by EQ) ───────────────────────────────────────────────────

/// The kind of a parametric-EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqKind {
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

/// One parametric-EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub kind: EqKind,
    pub freq_hz: f32,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default = "default_q")]
    pub q: f32,
}

fn default_q() -> f32 {
    0.707
}

/// A normalised biquad (transposed direct-form II) with RBJ-cookbook coefficients.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn new(band: &EqBand, sample_rate: u32) -> Self {
        let fs = sample_rate.max(1) as f32;
        let f0 = band.freq_hz.clamp(1.0, fs / 2.0 - 1.0);
        let w0 = std::f32::consts::TAU * f0 / fs;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let q = band.q.max(1e-4);
        let alpha = sin_w0 / (2.0 * q);
        let a = 10f32.powf(band.gain_db / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match band.kind {
            EqKind::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos_w0,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos_w0,
                1.0 - alpha / a,
            ),
            EqKind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 - s),
                    (a + 1.0) + (a - 1.0) * cos_w0 + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
                    (a + 1.0) + (a - 1.0) * cos_w0 - s,
                )
            }
            EqKind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 - s),
                    (a + 1.0) - (a - 1.0) * cos_w0 + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
                    (a + 1.0) - (a - 1.0) * cos_w0 - s,
                )
            }
            EqKind::LowPass => (
                (1.0 - cos_w0) / 2.0,
                1.0 - cos_w0,
                (1.0 - cos_w0) / 2.0,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
            EqKind::HighPass => (
                (1.0 + cos_w0) / 2.0,
                -(1.0 + cos_w0),
                (1.0 + cos_w0) / 2.0,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
        };

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

// ── gain / pan ──────────────────────────────────────────────────────────────

/// Apply a fixed gain in decibels.
pub struct GainFilter {
    pub db: f32,
}

impl AudioFilter for GainFilter {
    fn process_audio(&self, mut frame: AudioFrame) -> Result<AudioFrame> {
        let g = db_to_linear(self.db);
        for s in &mut frame.samples {
            *s = (*s * g).clamp(-1.0, 1.0);
        }
        Ok(frame)
    }
}

/// Constant-power stereo pan. `pan` is -1.0 (hard left) … +1.0 (hard right).
/// A no-op on non-stereo frames.
pub struct PanFilter {
    pub pan: f32,
}

impl AudioFilter for PanFilter {
    fn process_audio(&self, mut frame: AudioFrame) -> Result<AudioFrame> {
        if frame.channels != 2 {
            return Ok(frame);
        }
        let angle = (self.pan.clamp(-1.0, 1.0) + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
        let (lg, rg) = (angle.cos(), angle.sin());
        for f in frame.samples.chunks_exact_mut(2) {
            f[0] *= lg;
            f[1] *= rg;
        }
        Ok(frame)
    }
}

// ── normalize ───────────────────────────────────────────────────────────────

/// Scale the whole buffer so its peak (or RMS) hits `target_db` dBFS.
pub struct NormalizeFilter {
    pub target_db: f32,
    pub rms: bool,
}

impl AudioFilter for NormalizeFilter {
    fn process_audio(&self, mut frame: AudioFrame) -> Result<AudioFrame> {
        if frame.samples.is_empty() {
            return Ok(frame);
        }
        let measured = if self.rms {
            let sum_sq: f32 = frame.samples.iter().map(|s| s * s).sum();
            (sum_sq / frame.samples.len() as f32).sqrt()
        } else {
            frame.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
        };
        if measured <= f32::EPSILON {
            return Ok(frame); // silence — nothing to normalise
        }
        let gain = db_to_linear(self.target_db) / measured;
        for s in &mut frame.samples {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
        Ok(frame)
    }
}

// ── parametric EQ ───────────────────────────────────────────────────────────

/// Multi-band parametric EQ built from [`EqBand`]s (cascaded per channel).
pub struct EqFilter {
    pub bands: Vec<EqBand>,
}

impl AudioFilter for EqFilter {
    fn process_audio(&self, frame: AudioFrame) -> Result<AudioFrame> {
        if self.bands.is_empty() {
            return Ok(frame);
        }
        let mut planes = deinterleave(&frame);
        for plane in &mut planes {
            for band in &self.bands {
                let mut bq = Biquad::new(band, frame.sample_rate);
                for s in plane.iter_mut() {
                    *s = bq.process(*s);
                }
            }
        }
        Ok(interleave(&planes, frame.sample_rate))
    }
}

// ── dynamics: compressor / limiter / gate ───────────────────────────────────

/// Shared envelope-follower dynamics processor. A channel-linked detector drives
/// one gain applied to all channels, so stereo imaging is preserved.
struct Dynamics {
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
    /// `true` = downward expander/gate (attenuate *below* threshold);
    /// `false` = compressor (attenuate *above* threshold).
    gate: bool,
}

impl AudioFilter for Dynamics {
    fn process_audio(&self, mut frame: AudioFrame) -> Result<AudioFrame> {
        let ch = frame.channels.max(1) as usize;
        let n = frame.frame_count();
        if n == 0 {
            return Ok(frame);
        }
        let fs = frame.sample_rate.max(1) as f32;
        let att = time_coef(self.attack_ms, fs);
        let rel = time_coef(self.release_ms, fs);
        let makeup = db_to_linear(self.makeup_db);
        let mut env = 0.0f32;

        for i in 0..n {
            // Channel-linked peak detector.
            let mut level = 0.0f32;
            for c in 0..ch {
                level = level.max(frame.samples[i * ch + c].abs());
            }
            let coef = if level > env { att } else { rel };
            env = coef * env + (1.0 - coef) * level;

            let env_db = 20.0 * (env + 1e-9).log10();
            let reduction_db = if self.gate {
                // Below threshold → attenuate toward silence by the ratio.
                if env_db < self.threshold_db {
                    (env_db - self.threshold_db) * (self.ratio - 1.0)
                } else {
                    0.0
                }
            } else {
                // Above threshold → compress by the ratio.
                if env_db > self.threshold_db {
                    (self.threshold_db - env_db) * (1.0 - 1.0 / self.ratio)
                } else {
                    0.0
                }
            };
            let g = db_to_linear(reduction_db) * makeup;
            for c in 0..ch {
                let idx = i * ch + c;
                frame.samples[idx] = (frame.samples[idx] * g).clamp(-1.0, 1.0);
            }
        }
        Ok(frame)
    }
}

/// Smoothing coefficient for a one-pole envelope with the given time constant.
fn time_coef(ms: f32, fs: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms * 0.001 * fs)).exp()
    }
}

// ── delay / echo ────────────────────────────────────────────────────────────

/// A feedback delay line. `out = x + delayed * mix`; the buffer stores
/// `x + delayed * feedback`, giving repeating, decaying echoes.
pub struct DelayFilter {
    pub time_ms: f32,
    pub feedback: f32,
    pub mix: f32,
}

impl AudioFilter for DelayFilter {
    fn process_audio(&self, frame: AudioFrame) -> Result<AudioFrame> {
        let fs = frame.sample_rate.max(1) as f32;
        let len = ((self.time_ms * 0.001 * fs).round() as usize).max(1);
        let fb = self.feedback.clamp(0.0, 0.99);
        let mut planes = deinterleave(&frame);
        for plane in &mut planes {
            let mut buf = vec![0.0f32; len];
            let mut idx = 0usize;
            for s in plane.iter_mut() {
                let delayed = buf[idx];
                buf[idx] = *s + delayed * fb;
                *s = (*s + delayed * self.mix).clamp(-1.0, 1.0);
                idx = (idx + 1) % len;
            }
        }
        Ok(interleave(&planes, frame.sample_rate))
    }
}

// ── reverb (Freeverb) ───────────────────────────────────────────────────────

const COMB_TUNINGS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNINGS: [usize; 4] = [556, 441, 341, 225];

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
    feedback: f32,
    damp: f32,
}

impl Comb {
    fn new(len: usize, feedback: f32, damp: f32) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0, store: 0.0, feedback, damp }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * (1.0 - self.damp) + self.store * self.damp;
        self.buf[self.idx] = x + self.store * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0 }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let bufout = self.buf[self.idx];
        let out = -x + bufout;
        self.buf[self.idx] = x + bufout * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

/// Schroeder/Freeverb-style reverb. `room_size`/`damping`/`wet`/`dry` are 0..1.
pub struct ReverbFilter {
    pub room_size: f32,
    pub damping: f32,
    pub wet: f32,
    pub dry: f32,
}

impl AudioFilter for ReverbFilter {
    fn process_audio(&self, frame: AudioFrame) -> Result<AudioFrame> {
        let fs = frame.sample_rate.max(1) as f32;
        let scale = fs / 44100.0;
        let feedback = self.room_size.clamp(0.0, 1.0) * 0.28 + 0.7;
        let damp = self.damping.clamp(0.0, 1.0) * 0.4;
        const INPUT_GAIN: f32 = 0.015;

        let mut planes = deinterleave(&frame);
        for plane in &mut planes {
            let mut combs: Vec<Comb> = COMB_TUNINGS
                .iter()
                .map(|&t| Comb::new(((t as f32) * scale) as usize, feedback, damp))
                .collect();
            let mut allpasses: Vec<Allpass> = ALLPASS_TUNINGS
                .iter()
                .map(|&t| Allpass::new(((t as f32) * scale) as usize))
                .collect();

            for s in plane.iter_mut() {
                let input = *s * INPUT_GAIN;
                let mut out = 0.0f32;
                for comb in &mut combs {
                    out += comb.process(input);
                }
                for ap in &mut allpasses {
                    out = ap.process(out);
                }
                *s = (*s * self.dry + out * self.wet).clamp(-1.0, 1.0);
            }
        }
        Ok(interleave(&planes, frame.sample_rate))
    }
}

// ── effect parameters (serialisable) + factory ──────────────────────────────

/// A serialisable audio effect — the data form stored in a project's effect
/// stack. Build it into a runnable processor with [`AudioEffect::build`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioEffect {
    Gain { db: f32 },
    Pan { pan: f32 },
    Normalize {
        target_db: f32,
        #[serde(default)]
        rms: bool,
    },
    Eq { bands: Vec<EqBand> },
    Compressor {
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        #[serde(default)]
        makeup_db: f32,
    },
    Limiter { ceiling_db: f32 },
    Reverb {
        room_size: f32,
        damping: f32,
        wet: f32,
        dry: f32,
    },
    Delay {
        time_ms: f32,
        feedback: f32,
        mix: f32,
    },
    /// Noise gate / downward expander — attenuates signal *below* threshold.
    NoiseGate {
        threshold_db: f32,
        attack_ms: f32,
        release_ms: f32,
    },
}

impl AudioEffect {
    /// Instantiate the runnable DSP processor for this effect.
    pub fn build(&self) -> Box<dyn AudioFilter> {
        match self.clone() {
            AudioEffect::Gain { db } => Box::new(GainFilter { db }),
            AudioEffect::Pan { pan } => Box::new(PanFilter { pan }),
            AudioEffect::Normalize { target_db, rms } => {
                Box::new(NormalizeFilter { target_db, rms })
            }
            AudioEffect::Eq { bands } => Box::new(EqFilter { bands }),
            AudioEffect::Compressor { threshold_db, ratio, attack_ms, release_ms, makeup_db } => {
                Box::new(Dynamics {
                    threshold_db,
                    ratio: ratio.max(1.0),
                    attack_ms,
                    release_ms,
                    makeup_db,
                    gate: false,
                })
            }
            AudioEffect::Limiter { ceiling_db } => Box::new(Dynamics {
                threshold_db: ceiling_db,
                ratio: 1000.0,
                attack_ms: 1.0,
                release_ms: 50.0,
                makeup_db: 0.0,
                gate: false,
            }),
            AudioEffect::Reverb { room_size, damping, wet, dry } => {
                Box::new(ReverbFilter { room_size, damping, wet, dry })
            }
            AudioEffect::Delay { time_ms, feedback, mix } => {
                Box::new(DelayFilter { time_ms, feedback, mix })
            }
            AudioEffect::NoiseGate { threshold_db, attack_ms, release_ms } => Box::new(Dynamics {
                threshold_db,
                ratio: 4.0,
                attack_ms,
                release_ms,
                makeup_db: 0.0,
                gate: true,
            }),
        }
    }

    /// Apply this single effect to a frame.
    pub fn apply(&self, frame: AudioFrame) -> Result<AudioFrame> {
        self.build().process_audio(frame)
    }
}

/// Run an ordered effect stack over a frame (front to back).
pub fn apply_effects(mut frame: AudioFrame, effects: &[AudioEffect]) -> Result<AudioFrame> {
    for fx in effects {
        frame = fx.build().process_audio(frame)?;
    }
    Ok(frame)
}
