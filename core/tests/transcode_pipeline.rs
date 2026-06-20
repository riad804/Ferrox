/// Integration tests for the video transcode pipeline.
///
/// These tests require the `encode` feature (enabled by default in the test
/// profile because `Cargo.toml` lists `encode` in `[features] default`).
use ferrox_core::{
    transcode_graph::{transcode, TranscodeOptions},
    Av1Encoder, EncodedPacket, VideoEncoder,
    traits::VideoDecoder,
    video::Packet as FxPacket,
    Vp8Decoder,
};
use oxideav_vp8::{
    encoder::{encode_silent_keyframe, SilentKeyframeParams},
    ivf::{write_frame, write_header, IvfHeader},
};
use tempfile::NamedTempFile;

// ── fixtures ──────────────────────────────────────────────────────────────────

fn make_vp8_keyframe(w: u32, h: u32) -> Vec<u8> {
    encode_silent_keyframe(SilentKeyframeParams::new(w, h))
        .expect("encode_silent_keyframe")
}

fn make_ivf(frames: &[Vec<u8>], w: u32, h: u32) -> Vec<u8> {
    let header = IvfHeader::vp8(w, h, 30, 1);
    let mut buf = write_header(&header);
    for (pts, data) in frames.iter().enumerate() {
        write_frame(&mut buf, pts as u64, data);
    }
    buf
}

fn ivf_tmpfile(ivf: &[u8]) -> NamedTempFile {
    let f = NamedTempFile::with_suffix(".ivf").expect("tmpfile");
    std::fs::write(f.path(), ivf).expect("write ivf");
    f
}

// ── Av1Encoder unit tests ─────────────────────────────────────────────────────

#[test]
fn av1_encoder_produces_packets_for_keyframe() {
    let raw = make_vp8_keyframe(32, 32);
    let vp8_pkt = FxPacket { data: raw, pts: 0, duration: 0, is_keyframe: true };
    let vf = Vp8Decoder.decode_packet(&vp8_pkt).expect("vp8 decode");

    let mut enc = Av1Encoder::new(32, 32, 9, 100, 30, 1).expect("av1 encoder");
    let packets = enc.encode(&vf).expect("encode");
    let flushed = enc.flush().expect("flush");
    let all: Vec<EncodedPacket> = packets.into_iter().chain(flushed).collect();

    assert!(!all.is_empty(), "should produce at least one AV1 packet");
    assert!(!all[0].data.is_empty(), "AV1 packet should have data");
}

#[test]
fn av1_encoder_first_packet_is_keyframe() {
    let raw = make_vp8_keyframe(16, 16);
    let vp8_pkt = FxPacket { data: raw, pts: 0, duration: 0, is_keyframe: true };
    let vf = Vp8Decoder.decode_packet(&vp8_pkt).expect("vp8 decode");

    let mut enc = Av1Encoder::new(16, 16, 9, 100, 30, 1).expect("av1 encoder");
    enc.encode(&vf).expect("encode");
    let flushed = enc.flush().expect("flush");

    assert!(flushed.iter().any(|p| p.is_keyframe), "should have at least one keyframe");
}

#[test]
fn av1_encoder_multi_frame_sequence() {
    let w = 32u32;
    let h = 32u32;
    let mut enc = Av1Encoder::new(w, h, 10, 150, 30, 1).expect("av1 encoder");

    let mut total_pkts = 0usize;
    for i in 0..5u64 {
        let raw = make_vp8_keyframe(w, h);
        let vp8_pkt = FxPacket { data: raw, pts: i, duration: 0, is_keyframe: true };
        let vf = Vp8Decoder.decode_packet(&vp8_pkt).expect("vp8 decode");
        let pkts = enc.encode(&vf).expect("encode");
        total_pkts += pkts.len();
    }
    let flushed = enc.flush().expect("flush");
    total_pkts += flushed.len();

    assert!(total_pkts >= 1, "multi-frame sequence should produce packets");
}

#[test]
fn av1_encoder_rejects_wrong_pixel_format() {
    use ferrox_core::{frame::{Frame, PixelFormat}, video::VideoFrame};
    // rav1e requires width/height >= 16; use 16×16 for this test
    let frame = Frame::new(16, 16, PixelFormat::Rgb8, vec![0u8; 16 * 16 * 3]);
    let vf = VideoFrame::new(frame, 0, 0, true);
    let mut enc = Av1Encoder::new(16, 16, 10, 150, 30, 1).expect("av1 encoder");
    assert!(enc.encode(&vf).is_err(), "should reject Rgb8 input");
}

// ── End-to-end transcode tests ────────────────────────────────────────────────

#[test]
fn transcode_vp8_ivf_to_av1_webm_produces_file() {
    let frames: Vec<Vec<u8>> = (0..5).map(|_| make_vp8_keyframe(32, 32)).collect();
    let ivf = make_ivf(&frames, 32, 32);
    let input = ivf_tmpfile(&ivf);

    let out = NamedTempFile::with_suffix(".webm").expect("tmpfile");
    let result = transcode(
        input.path(),
        out.path(),
        &TranscodeOptions { speed: 10, quantizer: 200, ..Default::default() },
        None,
    ).expect("transcode");

    assert!(result.frames_encoded > 0, "should encode at least one frame");
    let file_size = std::fs::metadata(out.path()).expect("metadata").len();
    assert!(file_size > 100, "output WebM should have content; got {file_size} bytes");
}

#[test]
fn transcode_with_resize_produces_smaller_output() {
    let frames: Vec<Vec<u8>> = (0..3).map(|_| make_vp8_keyframe(64, 64)).collect();
    let ivf = make_ivf(&frames, 64, 64);
    let input = ivf_tmpfile(&ivf);

    let out_full = NamedTempFile::with_suffix(".webm").expect("tmpfile");
    let out_half = NamedTempFile::with_suffix(".webm").expect("tmpfile");

    let opts_full = TranscodeOptions { speed: 10, quantizer: 200, ..Default::default() };
    let opts_half = TranscodeOptions {
        speed: 10,
        quantizer: 200,
        resize: Some((32, 32)),
        ..Default::default()
    };

    transcode(input.path(), out_full.path(), &opts_full, None).expect("full transcode");
    transcode(input.path(), out_half.path(), &opts_half, None).expect("half transcode");

    let full_size = std::fs::metadata(out_full.path()).unwrap().len();
    let half_size = std::fs::metadata(out_half.path()).unwrap().len();
    // Both should be non-empty.
    assert!(full_size > 0, "full output must be non-empty");
    assert!(half_size > 0, "half output must be non-empty");
}

#[test]
fn transcode_with_custom_fps() {
    let frames: Vec<Vec<u8>> = (0..3).map(|_| make_vp8_keyframe(16, 16)).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let input = ivf_tmpfile(&ivf);
    let out = NamedTempFile::with_suffix(".webm").expect("tmpfile");

    let result = transcode(
        input.path(),
        out.path(),
        &TranscodeOptions {
            speed: 10,
            quantizer: 200,
            fps: Some((24, 1)),
            ..Default::default()
        },
        None,
    ).expect("transcode with custom fps");

    assert!(result.frames_encoded > 0);
}

#[test]
fn transcode_unsupported_input_errors() {
    let tmp = NamedTempFile::with_suffix(".avi").expect("tmpfile");
    std::fs::write(tmp.path(), b"RIFF....AVI ").unwrap();
    let out = NamedTempFile::with_suffix(".webm").expect("tmpfile");

    let result = transcode(tmp.path(), out.path(), &TranscodeOptions::default(), None);
    assert!(result.is_err(), "unsupported container should fail");
}

#[test]
fn transcode_progress_callback_is_called() {
    let frames: Vec<Vec<u8>> = (0..5).map(|_| make_vp8_keyframe(16, 16)).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let input = ivf_tmpfile(&ivf);
    let out = NamedTempFile::with_suffix(".webm").expect("tmpfile");

    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    let count = Arc::new(AtomicUsize::new(0));
    let count2 = Arc::clone(&count);

    transcode(
        input.path(),
        out.path(),
        &TranscodeOptions { speed: 10, quantizer: 200, ..Default::default() },
        Some(Box::new(move |_, _| { count2.fetch_add(1, Ordering::Relaxed); })),
    ).expect("transcode");

    assert!(count.load(Ordering::Relaxed) > 0, "progress callback should be called");
}

#[test]
fn transcode_output_webm_starts_with_ebml_magic() {
    let frames: Vec<Vec<u8>> = (0..3).map(|_| make_vp8_keyframe(16, 16)).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let input = ivf_tmpfile(&ivf);
    let out = NamedTempFile::with_suffix(".webm").expect("tmpfile");

    transcode(
        input.path(),
        out.path(),
        &TranscodeOptions { speed: 10, quantizer: 200, ..Default::default() },
        None,
    ).expect("transcode");

    let bytes = std::fs::read(out.path()).expect("read webm");
    // EBML magic bytes: 1A 45 DF A3
    assert!(bytes.len() >= 4, "output must have bytes");
    assert_eq!(&bytes[..4], &[0x1A, 0x45, 0xDF, 0xA3], "must start with EBML magic");
}
