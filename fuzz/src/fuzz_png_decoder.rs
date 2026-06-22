//! Fuzz target: feed arbitrary bytes to the PNG decoder.
//!
//! Run with: cargo +nightly fuzz run fuzz_png_decoder
//!
//! The decoder must not panic or produce undefined behaviour on any input.
//! Errors (invalid PNG, truncated, etc.) are acceptable return values.
#![no_main]
use libfuzzer_sys::fuzz_target;
use ferrox_core::{codecs::PngDecoder, traits::DynDecoder};

fuzz_target!(|data: &[u8]| {
    // Decode should never panic — only return Ok or Err.
    let _ = PngDecoder.decode_dyn(&mut std::io::Cursor::new(data));
});
