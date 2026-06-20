use std::io::{Cursor, Read, Write};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use crate::{
    audio::AudioFrame,
    error::{Error, Result},
    traits::{AudioDecoder, AudioEncoder},
};

pub struct WavDecoder;
pub struct WavEncoder;

impl AudioDecoder for WavDecoder {
    fn decode_audio<R: Read>(&self, reader: R) -> Result<AudioFrame> {
        let mut wav = WavReader::new(reader)
            .map_err(|e| Error::Audio(e.to_string()))?;
        let spec = wav.spec();

        let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
            (SampleFormat::Float, 32) => wav
                .samples::<f32>()
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| Error::Audio(e.to_string()))?,
            (SampleFormat::Int, bits) => {
                let max = (1i64 << (bits - 1)) as f32;
                match bits {
                    8 => wav
                        .samples::<i8>()
                        .map(|s| s.map(|v| v as f32 / 128.0))
                        .collect::<std::result::Result<_, _>>()
                        .map_err(|e| Error::Audio(e.to_string()))?,
                    16 => wav
                        .samples::<i16>()
                        .map(|s| s.map(|v| v as f32 / max))
                        .collect::<std::result::Result<_, _>>()
                        .map_err(|e| Error::Audio(e.to_string()))?,
                    24 | 32 => wav
                        .samples::<i32>()
                        .map(|s| s.map(|v| v as f32 / max))
                        .collect::<std::result::Result<_, _>>()
                        .map_err(|e| Error::Audio(e.to_string()))?,
                    b => return Err(Error::Audio(format!("unsupported bit depth: {b}"))),
                }
            }
            (fmt, bits) => {
                return Err(Error::Audio(format!("unsupported WAV format: {fmt:?}/{bits}bit")))
            }
        };

        Ok(AudioFrame::new(spec.sample_rate, spec.channels, samples))
    }
}

impl AudioEncoder for WavEncoder {
    fn encode_audio<W: Write>(&self, frame: &AudioFrame, mut writer: W) -> Result<()> {
        let spec = WavSpec {
            channels: frame.channels,
            sample_rate: frame.sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        // hound requires Seek; buffer into memory then flush.
        let mut buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        {
            let mut wav = WavWriter::new(&mut buf, spec)
                .map_err(|e| Error::Audio(e.to_string()))?;
            for &s in &frame.samples {
                wav.write_sample(s).map_err(|e| Error::Audio(e.to_string()))?;
            }
            wav.finalize().map_err(|e| Error::Audio(e.to_string()))?;
        }
        writer.write_all(buf.get_ref())?;
        Ok(())
    }
}
