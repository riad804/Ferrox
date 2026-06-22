pub mod flac;
pub mod mp3;
pub mod vorbis;
pub mod wav;
#[cfg(feature = "mp3-encode")]
pub mod mp3_enc;
#[cfg(feature = "opus-encode")]
pub mod opus_enc;

pub use flac::FlacDecoder;
pub use mp3::{Mp3Decoder, SymphoniaDecoder};
pub use vorbis::VorbisDecoder;
pub use wav::{WavDecoder, WavEncoder};
#[cfg(feature = "mp3-encode")]
pub use mp3_enc::{Mp3Encoder, Mp3Options, Mp3Quality};
#[cfg(feature = "opus-encode")]
pub use opus_enc::{OpusEncoder, OpusOptions, OpusApplication};
