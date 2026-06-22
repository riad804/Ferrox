//! Fuzz target: feed arbitrary bytes to the MP4 demuxer.
//!
//! Run with: cargo +nightly fuzz run fuzz_mp4_demuxer
#![no_main]
use libfuzzer_sys::fuzz_target;
use ferrox_core::{codecs::Mp4Demuxer, traits::ContainerDemuxer};

fuzz_target!(|data: &[u8]| {
    let cursor = std::io::Cursor::new(data);
    let size = data.len() as u64;
    if let Ok(mut demuxer) = Mp4Demuxer::open(cursor, size) {
        // Drain packets — must not panic.
        loop {
            match demuxer.next_packet() {
                Ok(Some(_)) => {}
                Ok(None)    => break,
                Err(_)      => break,
            }
        }
    }
});
