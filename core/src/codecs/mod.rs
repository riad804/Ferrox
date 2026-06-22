pub mod audio;
pub mod jpeg;
pub mod png;
#[cfg(feature = "gif-support")]
pub mod gif;
#[cfg(feature = "video-codecs")]
pub mod video;

pub use audio::{FlacDecoder, Mp3Decoder, SymphoniaDecoder, VorbisDecoder, WavDecoder, WavEncoder};
#[cfg(feature = "mp3-encode")]
pub use audio::{Mp3Encoder, Mp3Options, Mp3Quality};
#[cfg(feature = "opus-encode")]
pub use audio::{OpusEncoder, OpusOptions, OpusApplication};
pub use jpeg::{JpegDecoder, JpegEncoder};
pub use png::{PngDecoder, PngEncoder};
#[cfg(feature = "video-codecs")]
pub use video::{Mp4Demuxer, Vp8Decoder, WebmDemuxer};
