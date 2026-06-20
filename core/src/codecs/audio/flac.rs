use std::io::Read;
use claxon::FlacReader;
use crate::{
    audio::AudioFrame,
    error::{Error, Result},
    traits::AudioDecoder,
};

pub struct FlacDecoder;

impl AudioDecoder for FlacDecoder {
    fn decode_audio<R: Read>(&self, reader: R) -> Result<AudioFrame> {
        let mut flac = FlacReader::new(reader)
            .map_err(|e| Error::Audio(e.to_string()))?;
        let info = flac.streaminfo();
        let sample_rate = info.sample_rate;
        let channels = info.channels as u16;
        let bits = info.bits_per_sample;
        let max = (1i64 << (bits - 1)) as f32;

        let mut samples = Vec::new();
        let mut blocks = flac.blocks();
        let mut buf = Vec::new();

        loop {
            match blocks.read_next_or_eof(buf) {
                Ok(Some(block)) => {
                    let n_ch = block.channels() as usize;
                    let n_samples = block.len() as usize;
                    // claxon gives planar; interleave
                    for i in 0..n_samples {
                        for ch in 0..n_ch {
                            let s = block.sample(ch as u32, i as u32);
                            samples.push(s as f32 / max);
                        }
                    }
                    buf = block.into_buffer();
                }
                Ok(None) => break,
                Err(e) => return Err(Error::Audio(e.to_string())),
            }
        }

        Ok(AudioFrame::new(sample_rate, channels, samples))
    }
}
