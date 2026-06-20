use std::collections::HashMap;
use crate::{
    codecs::{FlacDecoder, JpegDecoder, JpegEncoder, Mp3Decoder, PngDecoder, PngEncoder, VorbisDecoder, WavDecoder, WavEncoder},
    traits::{DynAudioDecoder, DynAudioEncoder, DynDecoder, DynEncoder},
};

pub struct DecoderRegistry(HashMap<&'static str, Box<dyn DynDecoder>>);
pub struct EncoderRegistry(HashMap<&'static str, Box<dyn DynEncoder>>);

impl DecoderRegistry {
    pub fn new() -> Self { Self(HashMap::new()) }

    pub fn register(&mut self, ext: &'static str, decoder: Box<dyn DynDecoder>) {
        self.0.insert(ext, decoder);
    }

    pub fn get(&self, ext: &str) -> Option<&dyn DynDecoder> {
        self.0.get(ext).map(|d| d.as_ref())
    }
}

impl Default for DecoderRegistry {
    fn default() -> Self {
        let mut r = Self::new();
        r.register("png", Box::new(PngDecoder));
        r.register("jpg", Box::new(JpegDecoder));
        r.register("jpeg", Box::new(JpegDecoder));
        r
    }
}

impl EncoderRegistry {
    pub fn new() -> Self { Self(HashMap::new()) }

    pub fn register(&mut self, ext: &'static str, encoder: Box<dyn DynEncoder>) {
        self.0.insert(ext, encoder);
    }

    pub fn get(&self, ext: &str) -> Option<&dyn DynEncoder> {
        self.0.get(ext).map(|e| e.as_ref())
    }
}

impl Default for EncoderRegistry {
    fn default() -> Self {
        let mut r = Self::new();
        r.register("png", Box::new(PngEncoder));
        r.register("jpg", Box::new(JpegEncoder::default()));
        r.register("jpeg", Box::new(JpegEncoder::default()));
        r
    }
}

/// Register a decoder for one or more extensions.
///
/// ```ignore
/// register_decoder!(registry, "png" => PngDecoder, "jpg" => JpegDecoder);
/// ```
#[macro_export]
macro_rules! register_decoder {
    ($registry:expr, $( $ext:literal => $decoder:expr ),+ $(,)?) => {
        $( $registry.register($ext, Box::new($decoder)); )+
    };
}

pub struct AudioDecoderRegistry(HashMap<&'static str, Box<dyn DynAudioDecoder>>);
pub struct AudioEncoderRegistry(HashMap<&'static str, Box<dyn DynAudioEncoder>>);

impl AudioDecoderRegistry {
    pub fn new() -> Self { Self(HashMap::new()) }

    pub fn register(&mut self, ext: &'static str, decoder: Box<dyn DynAudioDecoder>) {
        self.0.insert(ext, decoder);
    }

    pub fn get(&self, ext: &str) -> Option<&dyn DynAudioDecoder> {
        self.0.get(ext).map(|d| d.as_ref())
    }
}

impl Default for AudioDecoderRegistry {
    fn default() -> Self {
        let mut r = Self::new();
        r.register("wav", Box::new(WavDecoder));
        r.register("mp3", Box::new(Mp3Decoder));
        r.register("flac", Box::new(FlacDecoder));
        r.register("ogg", Box::new(VorbisDecoder));
        r
    }
}

impl AudioEncoderRegistry {
    pub fn new() -> Self { Self(HashMap::new()) }

    pub fn register(&mut self, ext: &'static str, encoder: Box<dyn DynAudioEncoder>) {
        self.0.insert(ext, encoder);
    }

    pub fn get(&self, ext: &str) -> Option<&dyn DynAudioEncoder> {
        self.0.get(ext).map(|e| e.as_ref())
    }
}

impl Default for AudioEncoderRegistry {
    fn default() -> Self {
        let mut r = Self::new();
        r.register("wav", Box::new(WavEncoder));
        r
    }
}

/// Register an encoder for one or more extensions.
#[macro_export]
macro_rules! register_encoder {
    ($registry:expr, $( $ext:literal => $encoder:expr ),+ $(,)?) => {
        $( $registry.register($ext, Box::new($encoder)); )+
    };
}
