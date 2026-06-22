//! Fuzz target: feed arbitrary bytes to the MP3/symphonia decoder.
//!
//! Run with: cargo +nightly fuzz run fuzz_mp3_decoder
#![no_main]
use libfuzzer_sys::fuzz_target;
use ferrox_core::{Mp3Decoder, traits::AudioDecoder};

fuzz_target!(|data: &[u8]| {
    let _ = Mp3Decoder.decode_audio(std::io::Cursor::new(data));
});
