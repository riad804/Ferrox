//! Time-based spatial effects: delay/echo and Freeverb-style reverb.

use crate::audio::AudioFrame;
use crate::error::Result;
use crate::traits::AudioFilter;

use super::dsp::{deinterleave, interleave};

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
