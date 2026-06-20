use rubato::{FftFixedIn, Resampler};
use crate::{audio::AudioFrame, error::{Error, Result}, traits::AudioFilter};

/// Resamples audio to a target sample rate using Rubato's FFT-based resampler.
pub struct ResampleFilter {
    pub target_rate: u32,
}

impl ResampleFilter {
    pub fn new(target_rate: u32) -> Self {
        Self { target_rate }
    }
}

impl AudioFilter for ResampleFilter {
    fn process_audio(&self, frame: AudioFrame) -> Result<AudioFrame> {
        if frame.sample_rate == self.target_rate {
            return Ok(frame);
        }

        let channels = frame.channels as usize;
        let in_rate = frame.sample_rate as usize;
        let out_rate = self.target_rate as usize;

        // De-interleave
        let n_frames = frame.frame_count();
        let planar: Vec<Vec<f32>> = (0..channels)
            .map(|ch| {
                (0..n_frames)
                    .map(|i| frame.samples[i * channels + ch])
                    .collect()
            })
            .collect();

        // Chunk size for FFT resampler
        let chunk = 1024usize;
        let mut resampler = FftFixedIn::<f32>::new(in_rate, out_rate, chunk, 2, channels)
            .map_err(|e| Error::Audio(format!("resample init: {e}")))?;

        let mut out_planar: Vec<Vec<f32>> = vec![Vec::new(); channels];
        let mut pos = 0usize;

        while pos + chunk <= n_frames {
            let chunk_in: Vec<Vec<f32>> = planar.iter().map(|ch| ch[pos..pos + chunk].to_vec()).collect();
            let chunk_out = resampler
                .process(&chunk_in, None)
                .map_err(|e| Error::Audio(format!("resample: {e}")))?;
            for (ch, buf) in chunk_out.into_iter().enumerate() {
                out_planar[ch].extend(buf);
            }
            pos += chunk;
        }

        // Flush tail
        if pos < n_frames {
            let tail_len = n_frames - pos;
            let padded: Vec<Vec<f32>> = planar.iter().map(|ch| {
                let mut v = ch[pos..].to_vec();
                v.resize(chunk, 0.0);
                v
            }).collect();
            let chunk_out = resampler
                .process(&padded, None)
                .map_err(|e| Error::Audio(format!("resample flush: {e}")))?;
            let expected_out = (tail_len * out_rate + in_rate - 1) / in_rate;
            for (ch, buf) in chunk_out.into_iter().enumerate() {
                let take = expected_out.min(buf.len());
                out_planar[ch].extend_from_slice(&buf[..take]);
            }
        }

        // Re-interleave
        let out_frames = out_planar[0].len();
        let mut samples = Vec::with_capacity(out_frames * channels);
        for i in 0..out_frames {
            for ch in 0..channels {
                samples.push(out_planar[ch][i]);
            }
        }

        Ok(AudioFrame::new(self.target_rate, frame.channels, samples))
    }
}
