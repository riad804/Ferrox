use std::io::Read;
use minimp3::{Decoder as Mp3Inner, Error as Mp3Error};
use crate::{
    audio::AudioFrame,
    error::{Error, Result},
    traits::AudioDecoder,
};

pub struct Mp3Decoder;

impl AudioDecoder for Mp3Decoder {
    fn decode_audio<R: Read>(&self, mut reader: R) -> Result<AudioFrame> {
        let mut raw = Vec::new();
        reader.read_to_end(&mut raw)?;

        let mut decoder = Mp3Inner::new(std::io::Cursor::new(raw));
        let mut samples: Vec<f32> = Vec::new();
        let mut sample_rate = 0u32;
        let mut channels = 0u16;

        loop {
            match decoder.next_frame() {
                Ok(frame) => {
                    if sample_rate == 0 {
                        sample_rate = frame.sample_rate as u32;
                        channels = frame.channels as u16;
                    }
                    for s in frame.data {
                        samples.push(s as f32 / i16::MAX as f32);
                    }
                }
                Err(Mp3Error::Eof) => break,
                Err(Mp3Error::SkippedData) => continue,
                Err(e) => return Err(Error::Audio(e.to_string())),
            }
        }

        if sample_rate == 0 {
            return Err(Error::Audio("no MP3 frames found".into()));
        }

        Ok(AudioFrame::new(sample_rate, channels, samples))
    }
}
