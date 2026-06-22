//! Fuzz target: feed arbitrary bytes to the M3U8 playlist parser.
//!
//! Run with: cargo +nightly fuzz run fuzz_m3u8_parser
#![no_main]
use libfuzzer_sys::fuzz_target;
use ferrox_core::parse_m3u8;

fuzz_target!(|data: &[u8]| {
    // Parser must not panic on any input.
    let _ = parse_m3u8(data);
});
