//! The serialisable [`AudioEffect`] parameter enum + its processor factory.

use serde::{Deserialize, Serialize};

use crate::audio::AudioFrame;
use crate::error::Result;
use crate::traits::AudioFilter;

use super::biquad::EqBand;
use super::dynamics::Dynamics;
use super::filters::{EqFilter, GainFilter, NormalizeFilter, PanFilter};
use super::spatial::{DelayFilter, ReverbFilter};

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
