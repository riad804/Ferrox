use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("image codec error: {0}")]
    Image(#[from] image::ImageError),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("unsupported pixel format for this codec")]
    UnsupportedPixelFormat,

    #[error("audio codec error: {0}")]
    Audio(String),

    #[error("video error: {0}")]
    Video(String),

    #[error("filter error: {0}")]
    Filter(String),

    #[error("invalid dimensions: width={width}, height={height}")]
    InvalidDimensions { width: u32, height: u32 },
}

pub type Result<T> = std::result::Result<T, Error>;
