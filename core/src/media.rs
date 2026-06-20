use crate::{audio::AudioFrame, frame::Frame};

/// A unified media frame that can carry either image or audio data.
#[derive(Debug, Clone)]
pub enum MediaFrame {
    Image(Frame),
    Audio(AudioFrame),
}

impl MediaFrame {
    pub fn as_image(&self) -> Option<&Frame> {
        if let Self::Image(f) = self { Some(f) } else { None }
    }

    pub fn as_audio(&self) -> Option<&AudioFrame> {
        if let Self::Audio(f) = self { Some(f) } else { None }
    }

    pub fn into_image(self) -> Option<Frame> {
        if let Self::Image(f) = self { Some(f) } else { None }
    }

    pub fn into_audio(self) -> Option<AudioFrame> {
        if let Self::Audio(f) = self { Some(f) } else { None }
    }

    pub fn media_type(&self) -> MediaType {
        match self {
            Self::Image(_) => MediaType::Image,
            Self::Audio(_) => MediaType::Audio,
        }
    }
}

/// Discriminant for [`MediaFrame`] — useful for routing in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Audio,
}
