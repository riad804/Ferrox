//! Shared DSP helpers used across the effect processors.

use crate::audio::AudioFrame;

/// Convert decibels to a linear amplitude factor.
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Split interleaved samples into per-channel planes.
pub(super) fn deinterleave(frame: &AudioFrame) -> Vec<Vec<f32>> {
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
pub(super) fn interleave(planes: &[Vec<f32>], sample_rate: u32) -> AudioFrame {
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
