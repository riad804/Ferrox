/// Phase 9 integration tests: fMP4 and MPEG-TS HLS segment formats.
///
/// These tests verify the M3U8 structure and segment bytes without running a
/// full encode pipeline (which requires VP8 fixture files).
///
/// Run with: cargo test -p ferrox-core --test hls_phase9_test

#[cfg(feature = "encode")]
mod hls_format {
    use ferrox_core::hls::{HlsSegmentFormat, HlsOptions, parse_m3u8};
    use ferrox_core::{build_fmp4_init, build_fmp4_segment, CodecId, EncodedPacket, StreamInfo, StreamKind};

    fn av1_stream() -> StreamInfo {
        StreamInfo {
            index: 0,
            kind: StreamKind::Video,
            codec: CodecId::Av1,
            width: 640,
            height: 360,
            frame_rate: 30.0,
            sample_rate: 0,
            channels: 0,
            codec_private: vec![],
        }
    }

    fn fake_pkts(n: u64) -> Vec<EncodedPacket> {
        (0..n).map(|i| EncodedPacket {
            data: vec![0xAB, 0xCD, 0xEF],
            pts: i,
            duration: 1,
            is_keyframe: i == 0,
            stream_index: 0,
        }).collect()
    }

    // ── fMP4 init segment ─────────────────────────────────────────────────────

    #[test]
    fn fmp4_init_starts_with_ftyp() {
        let init = build_fmp4_init(&[av1_stream()]);
        assert!(init.len() >= 8);
        assert_eq!(&init[4..8], b"ftyp", "init segment must start with ftyp box");
    }

    #[test]
    fn fmp4_init_contains_moov_and_mvex() {
        let init = build_fmp4_init(&[av1_stream()]);
        assert!(init.windows(4).any(|w| w == b"moov"), "no moov in init segment");
        assert!(init.windows(4).any(|w| w == b"mvex"), "no mvex in init segment");
        assert!(init.windows(4).any(|w| w == b"trex"), "no trex in init segment");
    }

    #[test]
    fn fmp4_init_has_no_moof_or_mdat() {
        let init = build_fmp4_init(&[av1_stream()]);
        assert!(!init.windows(4).any(|w| w == b"moof"), "init must not contain moof");
        assert!(!init.windows(4).any(|w| w == b"mdat"), "init must not contain mdat");
    }

    // ── fMP4 media segment ────────────────────────────────────────────────────

    #[test]
    fn fmp4_segment_contains_moof_and_mdat() {
        let pkts = fake_pkts(10);
        let seg = build_fmp4_segment(1, 1, 90_000, 0, &pkts, 30, 1);
        assert!(seg.windows(4).any(|w| w == b"moof"), "segment must contain moof");
        assert!(seg.windows(4).any(|w| w == b"mdat"), "segment must contain mdat");
    }

    #[test]
    fn fmp4_segment_has_no_ftyp_or_moov() {
        let pkts = fake_pkts(5);
        let seg = build_fmp4_segment(1, 1, 90_000, 0, &pkts, 30, 1);
        assert!(!seg.windows(4).any(|w| w == b"ftyp"), "media segment must not have ftyp");
        assert!(!seg.windows(4).any(|w| w == b"moov"), "media segment must not have moov");
    }

    #[test]
    fn fmp4_segment_sequence_in_mfhd() {
        let pkts = fake_pkts(3);
        // sequence=42; mfhd contains sequence_number as big-endian u32
        let seg = build_fmp4_segment(42, 1, 90_000, 0, &pkts, 30, 1);
        // find "mfhd" + version(4) + sequence(4)
        let pos = seg.windows(4).position(|w| w == b"mfhd").expect("no mfhd");
        let seq_bytes = &seg[pos + 4 + 4..pos + 4 + 8];
        let seq = u32::from_be_bytes(seq_bytes.try_into().unwrap());
        assert_eq!(seq, 42, "mfhd sequence_number mismatch");
    }

    #[test]
    fn fmp4_bdt_advances_between_segments() {
        let pkts = fake_pkts(30);
        let seg1 = build_fmp4_segment(1, 1, 90_000, 0, &pkts, 30, 1);
        let seg2 = build_fmp4_segment(2, 1, 90_000, 90_000, &pkts, 30, 1);
        // tfdt contains base_decode_time; find "tfdt" and check its value
        let bdt_from_seg = |seg: &[u8], expected: u64| {
            let pos = seg.windows(4).position(|w| w == b"tfdt").expect("no tfdt");
            // tfdt full box: version(1)+flags(3) + decode_time (8 bytes for v1)
            let bdt = u64::from_be_bytes(seg[pos + 4 + 4..pos + 4 + 12].try_into().unwrap());
            assert_eq!(bdt, expected, "tfdt mismatch");
        };
        bdt_from_seg(&seg1, 0);
        bdt_from_seg(&seg2, 90_000);
    }

    // ── HLS options: format field ─────────────────────────────────────────────

    #[test]
    fn hls_options_default_is_fmp4() {
        let opts = HlsOptions::default();
        assert_eq!(opts.format, HlsSegmentFormat::FMp4);
    }

    #[test]
    fn hls_options_default_segment_duration_is_6s() {
        let opts = HlsOptions::default();
        assert!((opts.segment_duration_secs - 6.0).abs() < 0.01);
    }

    #[test]
    fn fmp4_format_uses_version_6() {
        assert_eq!(HlsSegmentFormat::FMp4.hls_version(), 6);
    }

    #[test]
    fn webm_and_ts_formats_use_version_3() {
        assert_eq!(HlsSegmentFormat::WebM.hls_version(), 3);
        assert_eq!(HlsSegmentFormat::MpegTs.hls_version(), 3);
    }

    // ── M3U8 parser: EXT-X-MAP ────────────────────────────────────────────────

    #[test]
    fn parse_m3u8_reads_ext_x_map() {
        let m3u8 = b"#EXTM3U\n\
                     #EXT-X-VERSION:6\n\
                     #EXT-X-TARGETDURATION:6\n\
                     #EXT-X-MEDIA-SEQUENCE:0\n\
                     #EXT-X-MAP:URI=\"seginit.mp4\"\n\
                     #EXTINF:6.000000,\n\
                     seg000.mp4\n\
                     #EXT-X-ENDLIST\n";
        let pl = parse_m3u8(m3u8).unwrap();
        assert_eq!(pl.version, 6);
        assert_eq!(pl.init_segment_uri.as_deref(), Some("seginit.mp4"));
        assert_eq!(pl.segments.len(), 1);
        assert_eq!(pl.segments[0].uri, "seg000.mp4");
    }

    #[test]
    fn parse_m3u8_no_map_for_webm_playlist() {
        let m3u8 = b"#EXTM3U\n\
                     #EXT-X-VERSION:3\n\
                     #EXT-X-TARGETDURATION:10\n\
                     #EXT-X-MEDIA-SEQUENCE:0\n\
                     #EXTINF:10.000000,\n\
                     seg000.webm\n\
                     #EXT-X-ENDLIST\n";
        let pl = parse_m3u8(m3u8).unwrap();
        assert!(pl.init_segment_uri.is_none());
        assert_eq!(pl.segments[0].uri, "seg000.webm");
    }

    #[test]
    fn fmp4_segment_extension_is_mp4() {
        assert_eq!(HlsSegmentFormat::FMp4.extension(), "mp4");
    }

    #[test]
    fn ts_segment_extension_is_ts() {
        assert_eq!(HlsSegmentFormat::MpegTs.extension(), "ts");
    }

    #[test]
    fn webm_segment_extension_is_webm() {
        assert_eq!(HlsSegmentFormat::WebM.extension(), "webm");
    }
}
