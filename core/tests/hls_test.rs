/// Integration tests for the HLS segmenter and M3U8 parser.
use ferrox_core::{
    hls_segment, parse_m3u8,
    HlsOptions, HlsSegmentFormat,
};
use oxideav_vp8::{
    encoder::{encode_silent_keyframe, SilentKeyframeParams},
    ivf::{write_frame, write_header, IvfHeader},
};
use tempfile::{NamedTempFile, TempDir};

fn make_vp8_keyframe(w: u32, h: u32) -> Vec<u8> {
    encode_silent_keyframe(SilentKeyframeParams::new(w, h))
        .expect("encode_silent_keyframe")
}

fn make_ivf(frames: &[Vec<u8>], w: u32, h: u32) -> Vec<u8> {
    let hdr = IvfHeader::vp8(w, h, 30, 1);
    let mut buf = write_header(&hdr);
    for (pts, data) in frames.iter().enumerate() {
        write_frame(&mut buf, pts as u64, data);
    }
    buf
}

fn ivf_tmpfile(data: &[u8]) -> NamedTempFile {
    let f = NamedTempFile::with_suffix(".ivf").expect("tmpfile");
    std::fs::write(f.path(), data).expect("write");
    f
}

// ── M3U8 parser ───────────────────────────────────────────────────────────────

#[test]
fn m3u8_parse_minimal() {
    let data = b"#EXTM3U\n\
                 #EXT-X-VERSION:3\n\
                 #EXT-X-TARGETDURATION:10\n\
                 #EXT-X-MEDIA-SEQUENCE:0\n\
                 #EXTINF:10.000000,\n\
                 seg000.webm\n\
                 #EXT-X-ENDLIST\n";
    let pl = parse_m3u8(data).expect("parse");
    assert_eq!(pl.version, 3);
    assert_eq!(pl.target_duration, 10);
    assert_eq!(pl.segments.len(), 1);
    assert_eq!(pl.segments[0].uri, "seg000.webm");
    assert!((pl.segments[0].duration_secs - 10.0).abs() < 0.01);
    assert!(pl.is_ended);
}

#[test]
fn m3u8_parse_multi_segment() {
    let data = b"#EXTM3U\n\
                 #EXT-X-TARGETDURATION:12\n\
                 #EXTINF:10.500000,\nseg000.webm\n\
                 #EXTINF:11.200000,\nseg001.webm\n\
                 #EXT-X-ENDLIST\n";
    let pl = parse_m3u8(data).expect("parse");
    assert_eq!(pl.segments.len(), 2);
    assert_eq!(pl.segments[1].uri, "seg001.webm");
}

#[test]
fn m3u8_parse_missing_header_errors() {
    let data = b"#EXT-X-VERSION:3\nseg000.webm\n";
    assert!(parse_m3u8(data).is_err());
}

#[test]
fn m3u8_parse_invalid_utf8_errors() {
    let data = b"#EXTM3U\n\xff\xfe";
    assert!(parse_m3u8(data).is_err());
}

#[test]
fn m3u8_parse_unknown_tags_ignored() {
    let data = b"#EXTM3U\n\
                 #EXT-X-UNKNOWN-TAG:value\n\
                 #EXTINF:5.0,\nseg.webm\n\
                 #EXT-X-ENDLIST\n";
    let pl = parse_m3u8(data).expect("parse");
    assert_eq!(pl.segments.len(), 1);
}

// ── HLS segmenter ─────────────────────────────────────────────────────────────

#[test]
fn hls_segment_produces_playlist_and_segments() {
    // 9 keyframes → should produce at least 1 segment at 30 fps / 10s target.
    let kf = make_vp8_keyframe(16, 16);
    let frames: Vec<Vec<u8>> = (0..9).map(|_| kf.clone()).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let src = ivf_tmpfile(&ivf);

    let out_dir = TempDir::new().expect("tmpdir");
    let opts = HlsOptions {
        segment_duration_secs: 10.0,
        output_dir: out_dir.path().to_path_buf(),
        playlist_name: "index.m3u8".into(),
        segment_prefix: "seg".into(),
        format: HlsSegmentFormat::WebM,
        speed: 10,
        quantizer: 200,
    };

    let result = hls_segment(src.path(), &opts).expect("hls_segment");

    // Playlist must exist and be parseable.
    assert!(result.playlist_path.exists(), "playlist file should exist");
    let playlist_bytes = std::fs::read(&result.playlist_path).expect("read playlist");
    let pl = parse_m3u8(&playlist_bytes).expect("parse playlist");

    // At least one segment with a positive duration.
    assert!(!pl.segments.is_empty(), "playlist should have at least one segment");
    assert!(pl.is_ended, "playlist should end with #EXT-X-ENDLIST");

    // Every segment URI should reference an existing file.
    for seg_entry in &pl.segments {
        let seg_path = out_dir.path().join(&seg_entry.uri);
        assert!(seg_path.exists(), "segment file {} should exist", seg_entry.uri);
        assert!(seg_path.metadata().unwrap().len() > 0, "segment file should not be empty");
    }

    assert!(result.total_frames > 0);
}

#[test]
fn hls_segment_m3u8_roundtrip() {
    let kf = make_vp8_keyframe(16, 16);
    let frames: Vec<Vec<u8>> = (0..5).map(|_| kf.clone()).collect();
    let ivf = make_ivf(&frames, 16, 16);
    let src = ivf_tmpfile(&ivf);

    let out_dir = TempDir::new().expect("tmpdir");
    let opts = HlsOptions {
        segment_duration_secs: 2.0,
        output_dir: out_dir.path().to_path_buf(),
        playlist_name: "out.m3u8".into(),
        segment_prefix: "chunk".into(),
        format: HlsSegmentFormat::WebM,
        speed: 10,
        quantizer: 200,
    };

    let result = hls_segment(src.path(), &opts).expect("hls_segment");
    let playlist_bytes = std::fs::read(&result.playlist_path).unwrap();
    let pl = parse_m3u8(&playlist_bytes).unwrap();

    // Each segment in SegmentInfo should correspond to a playlist entry.
    assert_eq!(pl.segments.len(), result.segments.len());
    for (pi, si) in pl.segments.iter().zip(result.segments.iter()) {
        assert!((pi.duration_secs - si.duration_secs).abs() < 0.01);
    }
}
