//! Envelope-follower dynamics: compressor, limiter, and noise gate.

use crate::audio::AudioFrame;
use crate::error::Result;
use crate::traits::AudioFilter;

use super::dsp::db_to_linear;

/// Shared envelope-follower dynamics processor. A channel-linked detector drives
/// one gain applied to all channels, so stereo imaging is preserved.
pub(super) struct Dynamics {
    pub(super) threshold_db: f32,
    pub(super) ratio: f32,
    pub(super) attack_ms: f32,
    pub(super) release_ms: f32,
    pub(super) makeup_db: f32,
    /// `true` = downward expander/gate (attenuate *below* threshold);
    /// `false` = compressor (attenuate *above* threshold).
    pub(super) gate: bool,
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
