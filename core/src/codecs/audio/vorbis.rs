use std::io::{Cursor, Read};
use lewton::inside_ogg::OggStreamReader;
use crate::{
    audio::AudioFrame,
    error::{Error, Result},
    traits::AudioDecoder,
};

pub struct VorbisDecoder;

impl AudioDecoder for VorbisDecoder {
    fn decode_audio<R: Read>(&self, mut reader: R) -> Result<AudioFrame> {
        // OggStreamReader requires Seek, so buffer into memory first.
        let mut raw = Vec::new();
        reader.read_to_end(&mut raw)?;
        let mut ogg = OggStreamReader::new(Cursor::new(raw))
            .map_err(|e| Error::Audio(e.to_string()))?;

        let sample_rate = ogg.ident_hdr.audio_sample_rate;
        let channels = ogg.ident_hdr.audio_channels as u16;
        let mut samples: Vec<f32> = Vec::new();

        loop {
            match ogg.read_dec_packet_itl() {
                Ok(Some(pkt)) => {
                    for s in pkt {
                        samples.push(s as f32 / i16::MAX as f32);
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(Error::Audio(e.to_string())),
            }
        }

        Ok(AudioFrame::new(sample_rate, channels, samples))
    }
}
