use crate::{audio::AudioFrame, error::Result, traits::AudioFilter};

/// Multiplies every sample by `gain`. Values > 1.0 amplify, < 1.0 attenuate.
pub struct VolumeFilter {
    pub gain: f32,
}

impl VolumeFilter {
    pub fn new(gain: f32) -> Self {
        Self { gain }
    }
}

impl AudioFilter for VolumeFilter {
    fn process_audio(&self, mut frame: AudioFrame) -> Result<AudioFrame> {
        for s in &mut frame.samples {
            *s = (*s * self.gain).clamp(-1.0, 1.0);
        }
        Ok(frame)
    }
}
