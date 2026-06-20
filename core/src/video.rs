use crate::frame::{Frame, PixelFormat};

/// A decoded video frame: an image plane + presentation timestamp.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Underlying pixel data (YUV420p or RGB8 depending on codec path).
    pub frame: Frame,
    /// Presentation timestamp in the container's timebase units.
    pub pts: u64,
    /// Frame duration in the container's timebase units (0 = unknown).
    pub duration: u64,
    /// True if this is a keyframe (IDR / intra-only).
    pub is_keyframe: bool,
}

impl VideoFrame {
    pub fn new(frame: Frame, pts: u64, duration: u64, is_keyframe: bool) -> Self {
        Self { frame, pts, duration, is_keyframe }
    }

    pub fn width(&self) -> u32 { self.frame.width }
    pub fn height(&self) -> u32 { self.frame.height }
    pub fn format(&self) -> PixelFormat { self.frame.format }
}

/// A raw compressed packet from a container track, ready for a codec decoder.
#[derive(Debug, Clone)]
pub struct Packet {
    /// Compressed payload bytes.
    pub data: Vec<u8>,
    /// Presentation timestamp in the container's timebase units.
    pub pts: u64,
    /// Duration in the container's timebase units.
    pub duration: u64,
    /// Whether this packet starts a decodable unit (keyframe / IDR).
    pub is_keyframe: bool,
}

/// High-level codec kind reported by a demuxer for each track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecId {
    /// VP8 video (WebM / IVF containers).
    Vp8,
    /// VP9 video (WebM container).
    Vp9,
    /// H.264 / AVC video (MP4 container).
    H264,
    /// AAC audio (MP4 container).
    Aac,
    /// Opus audio (WebM container).
    Opus,
    /// Vorbis audio (WebM container).
    Vorbis,
    /// PCM audio (WAV / raw).
    Pcm,
    /// Any codec this library does not yet recognise; the raw codec-id
    /// string from the container is preserved for diagnostics.
    Other(String),
}

impl std::fmt::Display for CodecId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vp8 => write!(f, "VP8"),
            Self::Vp9 => write!(f, "VP9"),
            Self::H264 => write!(f, "H264"),
            Self::Aac => write!(f, "AAC"),
            Self::Opus => write!(f, "Opus"),
            Self::Vorbis => write!(f, "Vorbis"),
            Self::Pcm => write!(f, "PCM"),
            Self::Other(s) => write!(f, "Other({s})"),
        }
    }
}

/// Broad category of a stream inside a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Other,
}

/// Metadata describing one track / stream inside a container.
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// Demuxer-assigned stream index (0-based).
    pub index: usize,
    /// Broad media kind.
    pub kind: StreamKind,
    /// Codec for this stream.
    pub codec: CodecId,
    /// Width in pixels (video only; 0 otherwise).
    pub width: u32,
    /// Height in pixels (video only; 0 otherwise).
    pub height: u32,
    /// Nominal frames-per-second (video only; 0.0 otherwise).
    pub frame_rate: f64,
    /// Sample rate in Hz (audio only; 0 otherwise).
    pub sample_rate: u32,
    /// Channel count (audio only; 0 otherwise).
    pub channels: u16,
    /// Codec-private / extradata bytes needed to initialise certain decoders.
    pub codec_private: Vec<u8>,
}

impl StreamInfo {
    pub fn is_video(&self) -> bool { self.kind == StreamKind::Video }
    pub fn is_audio(&self) -> bool { self.kind == StreamKind::Audio }
}
