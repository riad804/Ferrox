use crate::{audio::AudioFrame, frame::Frame};
#[cfg(feature = "video-codecs")]
use crate::video::VideoFrame;

/// A unified media frame that can carry image, audio, or video data.
#[derive(Debug, Clone)]
pub enum MediaFrame {
    Image(Frame),
    Audio(AudioFrame),
    #[cfg(feature = "video-codecs")]
    Video(VideoFrame),
}

impl MediaFrame {
    pub fn as_image(&self) -> Option<&Frame> {
        if let Self::Image(f) = self { Some(f) } else { None }
    }

    pub fn as_audio(&self) -> Option<&AudioFrame> {
        if let Self::Audio(f) = self { Some(f) } else { None }
    }

    #[cfg(feature = "video-codecs")]
    pub fn as_video(&self) -> Option<&VideoFrame> {
        if let Self::Video(f) = self { Some(f) } else { None }
    }

    pub fn into_image(self) -> Option<Frame> {
        if let Self::Image(f) = self { Some(f) } else { None }
    }

    pub fn into_audio(self) -> Option<AudioFrame> {
        if let Self::Audio(f) = self { Some(f) } else { None }
    }

    #[cfg(feature = "video-codecs")]
    pub fn into_video(self) -> Option<VideoFrame> {
        if let Self::Video(f) = self { Some(f) } else { None }
    }

    pub fn media_type(&self) -> MediaType {
        match self {
            Self::Image(_) => MediaType::Image,
            Self::Audio(_) => MediaType::Audio,
            #[cfg(feature = "video-codecs")]
            Self::Video(_) => MediaType::Video,
        }
    }
}

/// Discriminant for [`MediaFrame`] — useful for routing in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Audio,
    #[cfg(feature = "video-codecs")]
    Video,
}
