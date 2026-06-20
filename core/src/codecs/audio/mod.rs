pub mod flac;
pub mod mp3;
pub mod vorbis;
pub mod wav;

pub use flac::FlacDecoder;
pub use mp3::Mp3Decoder;
pub use vorbis::VorbisDecoder;
pub use wav::{WavDecoder, WavEncoder};
