use std::io::Cursor;
use ferrox_core::{
    demux_graph::{extract_frames, ContainerKind},
    frame::PixelFormat,
    traits::{ContainerDemuxer, VideoDecoder},
    video::{CodecId, StreamKind},
    IvfDemuxer, Vp8Decoder,
};
use oxideav_vp8::{
    encoder::{encode_silent_keyframe, SilentKeyframeParams},
    ivf::{write_frame, write_header, IvfHeader},
};
use tempfile::NamedTempFile;

// ── fixture helpers ───────────────────────────────────────────────────────────

/// Build a raw VP8 keyframe bitstream for a `w × h` silent frame.
fn make_vp8_keyframe(w: u32, h: u32) -> Vec<u8> {
    encode_silent_keyframe(SilentKeyframeParams::new(w, h))
        .expect("encode_silent_keyframe")
}

/// Wrap VP8 frames in a minimal IVF container.
fn make_ivf(frames: &[Vec<u8>], w: u32, h: u32) -> Vec<u8> {
    let header = IvfHeader::vp8(w, h, 30, 1);
    let mut buf = write_header(&header);
    for (pts, data) in frames.iter().enumerate() {
        write_frame(&mut buf, pts as u64, data);
    }
    buf
}

/// Write an IVF blob to a `.ivf` temp file.
fn ivf_to_tmpfile(ivf: &[u8]) -> NamedTempFile {
    let f = NamedTempFile::with_suffix(".ivf").expect("tmpfile");
    std::fs::write(f.path(), ivf).expect("write ivf");
    f
}

// ── VP8 decoder ──────────────────────────────────────────────────────────────

#[test]
fn vp8_decode_silent_keyframe_dimensions() {
    let raw = make_vp8_keyframe(64, 48);
    let packet = ferrox_core::video::Packet {
        data: raw, pts: 0, duration: 0, is_keyframe: true,
    };
    let vf = Vp8Decoder.decode_packet(&packet).expect("vp8 decode");
    assert_eq!(vf.width(), 64);
    assert_eq!(vf.height(), 48);
    assert_eq!(vf.format(), PixelFormat::Yuv420p);
    assert_eq!(vf.pts, 0);
    assert!(vf.is_keyframe);
}

#[test]
fn vp8_decode_various_sizes() {
    for (w, h) in [(16, 16), (32, 32), (128, 96), (320, 240)] {
        let raw = make_vp8_keyframe(w, h);
        let packet = ferrox_core::video::Packet {
            data: raw, pts: 0, duration: 0, is_keyframe: true,
        };
        let vf = Vp8Decoder.decode_packet(&packet).expect("vp8 decode");
        assert_eq!(vf.width(), w, "width mismatch for {w}×{h}");
        assert_eq!(vf.height(), h, "height mismatch for {w}×{h}");
        let expected_len = PixelFormat::Yuv420p.expected_data_len(w, h);
        assert_eq!(vf.frame.data.len(), expected_len);
    }
}

#[test]
fn vp8_decode_error_on_empty_packet() {
    let packet = ferrox_core::video::Packet {
        data: vec![], pts: 0, duration: 0, is_keyframe: true,
    };
    assert!(Vp8Decoder.decode_packet(&packet).is_err());
}

#[test]
fn vp8_decode_error_on_garbage() {
    let packet = ferrox_core::video::Packet {
        data: vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00],
        pts: 0, duration: 0, is_keyframe: false,
    };
    assert!(Vp8Decoder.decode_packet(&packet).is_err());
}

// ── IVF demuxer ──────────────────────────────────────────────────────────────

#[test]
fn ivf_demuxer_reports_vp8_stream() {
    let frame = make_vp8_keyframe(32, 32);
    let ivf = make_ivf(&[frame], 32, 32);
    let demuxer = IvfDemuxer::open(Cursor::new(ivf)).expect("demuxer");
    let streams = demuxer.streams();

    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].codec, CodecId::Vp8);
    assert_eq!(streams[0].kind, StreamKind::Video);
    assert_eq!(streams[0].width, 32);
    assert_eq!(streams[0].height, 32);
}

#[test]
fn ivf_demuxer_yields_one_packet_per_frame() {
    let frames: Vec<Vec<u8>> = (0..3).map(|_| make_vp8_keyframe(16, 16)).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let mut demuxer = IvfDemuxer::open(Cursor::new(ivf)).expect("demuxer");

    let mut count = 0usize;
    while let Some((idx, pkt)) = demuxer.next_packet().expect("next_packet") {
        assert_eq!(idx, 0);
        assert!(!pkt.data.is_empty());
        count += 1;
    }
    assert_eq!(count, 3);
}

#[test]
fn ivf_demuxer_packet_pts_ascending() {
    let frames: Vec<Vec<u8>> = (0..5).map(|_| make_vp8_keyframe(16, 16)).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let mut demuxer = IvfDemuxer::open(Cursor::new(ivf)).expect("demuxer");

    let mut pts_values = Vec::new();
    while let Some((_, pkt)) = demuxer.next_packet().expect("next_packet") {
        pts_values.push(pkt.pts);
    }
    let mut sorted = pts_values.clone();
    sorted.sort();
    assert_eq!(pts_values, sorted, "PTS should be non-decreasing");
}

#[test]
fn ivf_demuxer_marks_vp8_keyframes() {
    let frame = make_vp8_keyframe(16, 16);
    let ivf = make_ivf(&[frame], 16, 16);
    let mut demuxer = IvfDemuxer::open(Cursor::new(ivf)).expect("demuxer");
    let (_, pkt) = demuxer.next_packet().expect("ok").expect("packet");
    assert!(pkt.is_keyframe, "VP8 keyframe should be marked as keyframe");
}

#[test]
fn ivf_demuxer_returns_error_on_bad_magic() {
    let garbage = b"NOTDKIF\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let result = IvfDemuxer::open(Cursor::new(garbage.as_ref()));
    assert!(result.is_err());
}

// ── end-to-end: IVF → VP8 decode → PNG strip ─────────────────────────────────

#[test]
fn extract_frames_vp8_ivf_writes_pngs() {
    let frames: Vec<Vec<u8>> = (0..5).map(|_| make_vp8_keyframe(32, 32)).collect();
    let ivf = make_ivf(&frames, 32, 32);
    let input = ivf_to_tmpfile(&ivf);

    let out_dir = tempfile::tempdir().expect("tmpdir");
    let pattern = out_dir.path().join("frame_%03d.png").to_str().unwrap().to_owned();

    let result = extract_frames(input.path(), &pattern, 3).expect("extract_frames");

    assert_eq!(result.frame_paths.len(), 3, "should write 3 PNGs");
    for p in &result.frame_paths {
        assert!(p.exists(), "PNG must exist: {}", p.display());
        let bytes = std::fs::read(p).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "must be valid PNG");
    }
}

#[test]
fn extract_frames_png_dimensions_match_source() {
    let w = 64u32;
    let h = 48u32;
    let frame = make_vp8_keyframe(w, h);
    let ivf = make_ivf(&[frame], w, h);
    let input = ivf_to_tmpfile(&ivf);

    let out_dir = tempfile::tempdir().expect("tmpdir");
    let pattern = out_dir.path().join("f_%d.png").to_str().unwrap().to_owned();

    let result = extract_frames(input.path(), &pattern, 1).expect("extract");
    assert_eq!(result.frame_paths.len(), 1);

    let img = image::open(&result.frame_paths[0]).expect("open png");
    assert_eq!(img.width(), w);
    assert_eq!(img.height(), h);
}

#[test]
fn extract_frames_count_cap() {
    let frames: Vec<Vec<u8>> = (0..10).map(|_| make_vp8_keyframe(16, 16)).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let input = ivf_to_tmpfile(&ivf);

    let out_dir = tempfile::tempdir().expect("tmpdir");
    let pattern = out_dir.path().join("f_%d.png").to_str().unwrap().to_owned();

    let result = extract_frames(input.path(), &pattern, 4).expect("extract");
    assert_eq!(result.frame_paths.len(), 4, "must cap at requested count");
}

#[test]
fn extract_frames_output_path_zero_padded() {
    let frame = make_vp8_keyframe(16, 16);
    let ivf = make_ivf(&[frame], 16, 16);
    let input = ivf_to_tmpfile(&ivf);

    let out_dir = tempfile::tempdir().expect("tmpdir");
    let pattern = out_dir.path().join("thumb_%03d.png").to_str().unwrap().to_owned();

    let result = extract_frames(input.path(), &pattern, 1).expect("extract");
    let name = result.frame_paths[0].file_name().unwrap().to_str().unwrap();
    assert_eq!(name, "thumb_000.png");
}

#[test]
fn extract_frames_unsupported_container_errors() {
    let tmp = NamedTempFile::with_suffix(".avi").expect("tmpfile");
    std::fs::write(tmp.path(), b"RIFF....AVI ").unwrap();
    let result = extract_frames(tmp.path(), "f_%d.png", 1);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("avi") || msg.contains("container") || msg.contains("unrecognised"),
        "error should mention the bad extension: {msg}"
    );
}

// ── ContainerKind detection ───────────────────────────────────────────────────

#[test]
fn container_kind_from_extension() {
    use std::path::Path;
    assert_eq!(ContainerKind::from_path(Path::new("a.mp4")),  Some(ContainerKind::Mp4));
    assert_eq!(ContainerKind::from_path(Path::new("a.m4v")),  Some(ContainerKind::Mp4));
    assert_eq!(ContainerKind::from_path(Path::new("a.mkv")),  Some(ContainerKind::Mkv));
    assert_eq!(ContainerKind::from_path(Path::new("a.webm")), Some(ContainerKind::Mkv));
    assert_eq!(ContainerKind::from_path(Path::new("a.ivf")),  Some(ContainerKind::Ivf));
    assert_eq!(ContainerKind::from_path(Path::new("a.avi")),  None);
    assert_eq!(ContainerKind::from_path(Path::new("noext")),  None);
}

// ── YUV→RGB conversion sanity ─────────────────────────────────────────────────

#[test]
fn decoded_frame_pixel_data_is_non_empty() {
    // A silent VP8 keyframe at DC_PRED / all-zero-coeff decodes to
    // a uniform grey-ish field in YUV; verifying the buffer is non-empty
    // and that the PNG round-trip works is sufficient here.
    let raw = make_vp8_keyframe(16, 16);
    let packet = ferrox_core::video::Packet {
        data: raw, pts: 0, duration: 0, is_keyframe: true,
    };
    let vf = Vp8Decoder.decode_packet(&packet).unwrap();
    assert!(!vf.frame.data.is_empty());
    // Y plane should have width*height bytes
    let y_len = (vf.width() * vf.height()) as usize;
    assert!(vf.frame.data.len() >= y_len);
}
