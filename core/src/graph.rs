use std::{fs::File, io::BufWriter, path::Path};
use tracing::{debug, info, instrument};
use crate::{
    audio::AudioFrame,
    error::{Error, Result},
    frame::Frame,
    registry::{AudioDecoderRegistry, AudioEncoderRegistry, DecoderRegistry, EncoderRegistry},
    traits::{AudioFilter, Filter},
};

/// A linear image processing graph: one decoder → N filters → one encoder.
pub struct Graph {
    filters: Vec<Box<dyn Filter>>,
    decoders: DecoderRegistry,
    encoders: EncoderRegistry,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
            decoders: DecoderRegistry::default(),
            encoders: EncoderRegistry::default(),
        }
    }

    pub fn add_filter<F: Filter + 'static>(&mut self, filter: F) {
        self.filters.push(Box::new(filter));
    }

    pub fn with_filter<F: Filter + 'static>(mut self, filter: F) -> Self {
        self.add_filter(filter);
        self
    }

    /// Decode `input`, run all filters, encode to `output`.
    #[instrument(skip(self), fields(input = %input.display(), output = %output.display()))]
    pub fn run(&self, input: &Path, output: &Path) -> Result<()> {
        let in_ext = ext_of(input)?;
        let out_ext = ext_of(output)?;

        let decoder = self.decoders.get(in_ext).ok_or_else(|| {
            Error::UnsupportedFormat(format!("no decoder for extension '{in_ext}'"))
        })?;
        let encoder = self.encoders.get(out_ext).ok_or_else(|| {
            Error::UnsupportedFormat(format!("no encoder for extension '{out_ext}'"))
        })?;

        info!(input = %input.display(), codec = in_ext, "decoding");
        let in_file = File::open(input)?;
        let mut reader = std::io::BufReader::new(in_file);
        let mut frame: Frame = decoder.decode_dyn(&mut reader)?;
        debug!(width = frame.width, height = frame.height, format = ?frame.format, "decoded frame");

        for (i, filter) in self.filters.iter().enumerate() {
            debug!(filter_index = i, "applying filter");
            frame = filter.process(frame)?;
            debug!(width = frame.width, height = frame.height, "frame after filter");
        }

        info!(output = %output.display(), codec = out_ext, "encoding");
        let out_file = File::create(output)?;
        let mut writer = BufWriter::new(out_file);
        encoder.encode_dyn(&frame, &mut writer)?;
        info!("pipeline complete");
        Ok(())
    }
}

impl Default for Graph {
    fn default() -> Self { Self::new() }
}

/// A linear audio processing graph: one audio decoder → N audio filters → one audio encoder.
pub struct AudioGraph {
    filters: Vec<Box<dyn AudioFilter>>,
    decoders: AudioDecoderRegistry,
    encoders: AudioEncoderRegistry,
}

impl AudioGraph {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
            decoders: AudioDecoderRegistry::default(),
            encoders: AudioEncoderRegistry::default(),
        }
    }

    pub fn add_filter<F: AudioFilter + 'static>(&mut self, filter: F) {
        self.filters.push(Box::new(filter));
    }

    pub fn with_filter<F: AudioFilter + 'static>(mut self, filter: F) -> Self {
        self.add_filter(filter);
        self
    }

    /// Decode `input`, run all audio filters, encode to `output`.
    #[instrument(skip(self), fields(input = %input.display(), output = %output.display()))]
    pub fn run(&self, input: &Path, output: &Path) -> Result<()> {
        let in_ext = ext_of(input)?;
        let out_ext = ext_of(output)?;

        let decoder = self.decoders.get(in_ext).ok_or_else(|| {
            Error::UnsupportedFormat(format!("no audio decoder for extension '{in_ext}'"))
        })?;
        let encoder = self.encoders.get(out_ext).ok_or_else(|| {
            Error::UnsupportedFormat(format!("no audio encoder for extension '{out_ext}'"))
        })?;

        info!(input = %input.display(), codec = in_ext, "decoding audio");
        let in_file = File::open(input)?;
        let mut reader = std::io::BufReader::new(in_file);
        let mut frame: AudioFrame = decoder.decode_audio_dyn(&mut reader)?;
        debug!(
            sample_rate = frame.sample_rate,
            channels = frame.channels,
            frames = frame.frame_count(),
            "decoded audio"
        );

        for (i, filter) in self.filters.iter().enumerate() {
            debug!(filter_index = i, "applying audio filter");
            frame = filter.process_audio(frame)?;
        }

        info!(output = %output.display(), codec = out_ext, "encoding audio");
        let out_file = File::create(output)?;
        let mut writer = BufWriter::new(out_file);
        encoder.encode_audio_dyn(&frame, &mut writer)?;
        info!("audio pipeline complete");
        Ok(())
    }
}

impl Default for AudioGraph {
    fn default() -> Self { Self::new() }
}

fn ext_of(path: &Path) -> Result<&str> {
    path.extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| Error::UnsupportedFormat(format!("no extension on '{}'", path.display())))
}
