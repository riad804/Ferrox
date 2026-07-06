//! Level/spectral filters: gain, pan, normalize, and parametric EQ.

use crate::audio::AudioFrame;
use crate::error::Result;
use crate::traits::AudioFilter;

use super::biquad::{Biquad, EqBand};
use super::dsp::{db_to_linear, deinterleave, interleave};

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
