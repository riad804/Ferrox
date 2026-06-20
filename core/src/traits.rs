use std::io::{Read, Write};
use crate::{Frame, Result, audio::AudioFrame, media::MediaFrame};
#[cfg(feature = "video-codecs")]
use crate::video::{Packet, StreamInfo, VideoFrame};

/// Identifies a media format.
pub trait Format: Send + Sync {
    fn name(&self) -> &str;
    fn extensions(&self) -> &[&str];
    fn mime_type(&self) -> &str;
}

// ── Image codecs ─────────────────────────────────────────────────────────────

/// Generic (statically-dispatched) image decoder — prefer this in concrete code.
pub trait Decoder: Send + Sync {
    fn decode<R: Read>(&self, reader: R) -> Result<Frame>;
}

/// Generic (statically-dispatched) image encoder — prefer this in concrete code.
pub trait Encoder: Send + Sync {
    fn encode<W: Write>(&self, frame: &Frame, writer: W) -> Result<()>;
}

/// Object-safe image decoder used by the registry and graph.
pub trait DynDecoder: Send + Sync {
    fn decode_dyn(&self, reader: &mut dyn Read) -> Result<Frame>;
}

/// Object-safe image encoder used by the registry and graph.
pub trait DynEncoder: Send + Sync {
    fn encode_dyn(&self, frame: &Frame, writer: &mut dyn Write) -> Result<()>;
}

impl<T: Decoder> DynDecoder for T {
    fn decode_dyn(&self, reader: &mut dyn Read) -> Result<Frame> {
        self.decode(reader)
    }
}

impl<T: Encoder> DynEncoder for T {
    fn encode_dyn(&self, frame: &Frame, writer: &mut dyn Write) -> Result<()> {
        self.encode(frame, writer)
    }
}

// ── Audio codecs ─────────────────────────────────────────────────────────────

/// Generic (statically-dispatched) audio decoder.
pub trait AudioDecoder: Send + Sync {
    fn decode_audio<R: Read>(&self, reader: R) -> Result<AudioFrame>;
}

/// Generic (statically-dispatched) audio encoder.
pub trait AudioEncoder: Send + Sync {
    fn encode_audio<W: Write>(&self, frame: &AudioFrame, writer: W) -> Result<()>;
}

/// Object-safe audio decoder used by the registry and graph.
pub trait DynAudioDecoder: Send + Sync {
    fn decode_audio_dyn(&self, reader: &mut dyn Read) -> Result<AudioFrame>;
}

/// Object-safe audio encoder used by the registry and graph.
pub trait DynAudioEncoder: Send + Sync {
    fn encode_audio_dyn(&self, frame: &AudioFrame, writer: &mut dyn Write) -> Result<()>;
}

impl<T: AudioDecoder> DynAudioDecoder for T {
    fn decode_audio_dyn(&self, reader: &mut dyn Read) -> Result<AudioFrame> {
        self.decode_audio(reader)
    }
}

impl<T: AudioEncoder> DynAudioEncoder for T {
    fn encode_audio_dyn(&self, frame: &AudioFrame, writer: &mut dyn Write) -> Result<()> {
        self.encode_audio(frame, writer)
    }
}

// ── Filters ───────────────────────────────────────────────────────────────────

/// Transforms an image [`Frame`], returning a (possibly new) frame.
pub trait Filter: Send + Sync {
    fn process(&self, frame: Frame) -> Result<Frame>;
}

/// Transforms an [`AudioFrame`], returning a (possibly new) audio frame.
pub trait AudioFilter: Send + Sync {
    fn process_audio(&self, frame: AudioFrame) -> Result<AudioFrame>;
}

/// Transforms any [`MediaFrame`] — used by the unified graph.
pub trait MediaFilter: Send + Sync {
    fn process_media(&self, frame: MediaFrame) -> Result<MediaFrame>;
}

// ── Video codecs ──────────────────────────────────────────────────────────────

/// Decodes a single compressed [`Packet`] into a [`VideoFrame`].
///
/// Stateless decoders (keyframe-only) can be `Send + Sync`; stateful
/// (inter-frame) decoders hold reference frames internally.
#[cfg(feature = "video-codecs")]
pub trait VideoDecoder: Send {
    fn decode_packet(&mut self, packet: &Packet) -> Result<VideoFrame>;
}

/// Demuxes a container into stream metadata and raw packets.
///
/// The demuxer takes ownership of a `Read + Seek` source; callers pull
/// packets via [`ContainerDemuxer::next_packet`] until `None` is returned.
#[cfg(feature = "video-codecs")]
pub trait ContainerDemuxer: Send {
    /// Return metadata for every stream in the container.
    fn streams(&self) -> &[StreamInfo];

    /// Pull the next packet from any stream.
    ///
    /// Returns `Ok(None)` at end of file / stream.
    fn next_packet(&mut self) -> Result<Option<(usize, Packet)>>;
}
