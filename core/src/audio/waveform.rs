//! Waveform peak/RMS bucket generation for UI display (filmstrip-style audio
//! overview). Channels are mono-summed; the sample range is split into `buckets`
//! equal spans, each reporting its min, max and RMS.

use serde::{Deserialize, Serialize};

use crate::audio::AudioFrame;

/// Summary of one horizontal pixel column of a waveform display.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WaveformBucket {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
}

/// Downsample `frame` into `buckets` min/max/rms summaries (mono-summed).
///
/// Returns an empty vec for `buckets == 0`; buckets with no samples are zero.
pub fn generate_waveform(frame: &AudioFrame, buckets: usize) -> Vec<WaveformBucket> {
    if buckets == 0 {
        return Vec::new();
    }
    let ch = frame.channels.max(1) as usize;
    let n = frame.frame_count();
    let mut out = Vec::with_capacity(buckets);

    for b in 0..buckets {
        let s = b * n / buckets;
        let e = ((b + 1) * n / buckets).max(s);
        let (mut min, mut max, mut sum_sq, mut count) = (f32::MAX, f32::MIN, 0.0f32, 0usize);
        for f in s..e {
            let mut v = 0.0f32;
            for c in 0..ch {
                v += frame.samples[f * ch + c];
            }
            v /= ch as f32;
            min = min.min(v);
            max = max.max(v);
            sum_sq += v * v;
            count += 1;
        }
        if count == 0 {
            out.push(WaveformBucket { min: 0.0, max: 0.0, rms: 0.0 });
        } else {
            out.push(WaveformBucket {
                min,
                max,
                rms: (sum_sq / count as f32).sqrt(),
            });
        }
    }
    out
}
