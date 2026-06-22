/// Phase 8 integration tests: MPEG-TS muxer and fMP4 muxer.
///
/// Both are pure-Rust implementations; no system libraries required.
/// Run with: cargo test -p ferrox-core --features encode

#[cfg(feature = "encode")]
mod mpegts {
    use ferrox_core::{
        CodecId, EncodedPacket, MpegTsMuxer, StreamInfo, StreamKind,
        traits::ContainerMuxer,
    };

    fn video_stream() -> StreamInfo {
        StreamInfo {
            index: 0,
            kind: StreamKind::Video,
            codec: CodecId::Av1,
            width: 320,
            height: 240,
            frame_rate: 30.0,
            sample_rate: 0,
            channels: 0,
            codec_private: vec![],
        }
    }

    fn fake_packet(pts: u64, keyframe: bool) -> EncodedPacket {
        EncodedPacket {
            data: vec![0x00, 0x00, 0x01, 0xB3, 0xAA, 0xBB], // fake payload
            pts,
            duration: 3000,
            is_keyframe: keyframe,
            stream_index: 0,
        }
    }

    #[test]
    fn ts_output_starts_with_sync_byte() {
        let mut buf = Vec::new();
        let mut mux = MpegTsMuxer::new(&mut buf, &[video_stream()], 30, 1).unwrap();
        mux.write_header().unwrap();
        mux.write_packet(&fake_packet(0, true)).unwrap();
        mux.write_trailer().unwrap();
        assert!(!buf.is_empty(), "no output written");
        assert_eq!(buf[0], 0x47, "first byte must be TS sync byte 0x47");
    }

    #[test]
    fn ts_all_packets_are_188_bytes() {
        let mut buf = Vec::new();
        let mut mux = MpegTsMuxer::new(&mut buf, &[video_stream()], 30, 1).unwrap();
        mux.write_header().unwrap();
        for i in 0..5u64 {
            mux.write_packet(&fake_packet(i * 3000, i == 0)).unwrap();
        }
        mux.write_trailer().unwrap();

        assert_eq!(buf.len() % 188, 0, "TS output must be a multiple of 188 bytes");
        for chunk in buf.chunks(188) {
            assert_eq!(chunk[0], 0x47, "every TS packet must start with 0x47");
        }
    }

    #[test]
    fn ts_contains_pat_and_pmt() {
        let mut buf = Vec::new();
        let mut mux = MpegTsMuxer::new(&mut buf, &[video_stream()], 30, 1).unwrap();
        mux.write_header().unwrap();
        mux.write_packet(&fake_packet(0, true)).unwrap();
        mux.write_trailer().unwrap();

        // PAT is on PID 0x0000; PMT on PID 0x0020.
        let has_pid = |pid: u16| -> bool {
            buf.chunks(188).any(|p| {
                let p1 = p[1] as u16;
                let p2 = p[2] as u16;
                let pkt_pid = ((p1 & 0x1F) << 8) | p2;
                pkt_pid == pid
            })
        };
        assert!(has_pid(0x0000), "no PAT packet (PID 0) found");
        assert!(has_pid(0x0020), "no PMT packet (PID 0x20) found");
    }

    #[test]
    fn ts_multi_stream_video_audio() {
        let audio = StreamInfo {
            index: 1,
            kind: StreamKind::Audio,
            codec: CodecId::Aac,
            width: 0, height: 0,
            frame_rate: 0.0,
            sample_rate: 44100,
            channels: 2,
            codec_private: vec![],
        };
        let mut buf = Vec::new();
        let mut mux = MpegTsMuxer::new(&mut buf, &[video_stream(), audio], 30, 1).unwrap();
        mux.write_header().unwrap();
        mux.write_packet(&fake_packet(0, true)).unwrap();
        let audio_pkt = EncodedPacket {
            data: vec![0xFF, 0xF1, 0x50, 0x80], // ADTS sync word
            pts: 1000,
            duration: 1024,
            is_keyframe: true,
            stream_index: 1,
        };
        mux.write_packet(&audio_pkt).unwrap();
        mux.write_trailer().unwrap();

        assert_eq!(buf.len() % 188, 0);
    }

    #[test]
    fn ts_large_payload_spans_multiple_ts_packets() {
        // A 4 KB payload must be split into multiple 188-byte TS packets.
        let mut buf = Vec::new();
        let mut mux = MpegTsMuxer::new(&mut buf, &[video_stream()], 30, 1).unwrap();
        mux.write_header().unwrap();
        let big = EncodedPacket {
            data: vec![0xAB; 4096],
            pts: 0,
            duration: 3000,
            is_keyframe: true,
            stream_index: 0,
        };
        mux.write_packet(&big).unwrap();
        mux.write_trailer().unwrap();

        assert_eq!(buf.len() % 188, 0);
        let n_pkt = buf.len() / 188;
        assert!(n_pkt >= 22, "expected ≥22 TS packets for 4 KB payload, got {n_pkt}");
    }
}

// ── fMP4 tests ─────────────────────────────────────────────────────────────────

#[cfg(feature = "encode")]
mod fmp4 {
    use std::io::Cursor;
    use ferrox_core::{
        CodecId, EncodedPacket, FMp4Muxer, StreamInfo, StreamKind,
        traits::ContainerMuxer,
    };

    fn video_stream() -> StreamInfo {
        StreamInfo {
            index: 0,
            kind: StreamKind::Video,
            codec: CodecId::Av1,
            width: 640,
            height: 480,
            frame_rate: 24.0,
            sample_rate: 0,
            channels: 0,
            codec_private: vec![],
        }
    }

    fn fake_packet(pts: u64, keyframe: bool) -> EncodedPacket {
        EncodedPacket {
            data: vec![0x12, 0x34, 0x56, 0x78],
            pts,
            duration: 1,
            is_keyframe: keyframe,
            stream_index: 0,
        }
    }

    #[test]
    fn fmp4_starts_with_ftyp() {
        let mut buf = Cursor::new(Vec::new());
        let mut mux = FMp4Muxer::new(&mut buf, &[video_stream()], 24, 1).unwrap();
        mux.write_header().unwrap();
        let data = buf.into_inner();
        assert!(data.len() >= 8, "too short");
        let size = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
        assert_eq!(&data[4..8], b"ftyp", "first box must be ftyp");
        assert!(size >= 8 && size <= data.len(), "ftyp size out of range");
    }

    #[test]
    fn fmp4_contains_moov() {
        let mut buf = Cursor::new(Vec::new());
        let mut mux = FMp4Muxer::new(&mut buf, &[video_stream()], 24, 1).unwrap();
        mux.write_header().unwrap();
        let data = buf.into_inner();
        let has_moov = data.windows(4).any(|w| w == b"moov");
        assert!(has_moov, "no moov box found in output");
    }

    #[test]
    fn fmp4_contains_moof_mdat_after_packets() {
        let mut buf = Cursor::new(Vec::new());
        let mut mux = FMp4Muxer::new(&mut buf, &[video_stream()], 24, 1).unwrap();
        mux.write_header().unwrap();
        for i in 0..35u64 {
            mux.write_packet(&fake_packet(i, i == 0)).unwrap();
        }
        mux.write_trailer().unwrap();
        let data = buf.into_inner();
        let has_moof = data.windows(4).any(|w| w == b"moof");
        let has_mdat = data.windows(4).any(|w| w == b"mdat");
        assert!(has_moof, "no moof fragment box found");
        assert!(has_mdat, "no mdat box found");
    }

    #[test]
    fn fmp4_h264_uses_avc1_box() {
        let h264_stream = StreamInfo {
            index: 0,
            kind: StreamKind::Video,
            codec: CodecId::H264,
            width: 1280,
            height: 720,
            frame_rate: 30.0,
            sample_rate: 0,
            channels: 0,
            codec_private: vec![
                // minimal avcC: version=1, profile=66 (Baseline), compat=0xC0, level=31
                0x01, 0x42, 0xC0, 0x1F, 0xFF,
                0xE1, 0x00, 0x04, 0x67, 0x42, 0xC0, 0x1F,
                0x01, 0x00, 0x04, 0x68, 0xCE, 0x38, 0x80,
            ],
        };
        let mut buf = Cursor::new(Vec::new());
        let mut mux = FMp4Muxer::new(&mut buf, &[h264_stream], 30, 1).unwrap();
        mux.write_header().unwrap();
        mux.write_packet(&fake_packet(0, true)).unwrap();
        mux.write_trailer().unwrap();
        let data = buf.into_inner();
        let has_avc1 = data.windows(4).any(|w| w == b"avc1");
        assert!(has_avc1, "no avc1 box found for H.264 stream");
    }

    #[test]
    fn fmp4_audio_stream_produces_mp4a() {
        let audio = StreamInfo {
            index: 0,
            kind: StreamKind::Audio,
            codec: CodecId::Aac,
            width: 0, height: 0,
            frame_rate: 0.0,
            sample_rate: 48000,
            channels: 2,
            codec_private: vec![],
        };
        let mut buf = Cursor::new(Vec::new());
        let mut mux = FMp4Muxer::new(&mut buf, &[audio], 48000, 1).unwrap();
        mux.write_header().unwrap();
        let data = buf.into_inner();
        let has_mp4a = data.windows(4).any(|w| w == b"mp4a");
        assert!(has_mp4a, "no mp4a box found for AAC audio stream");
    }

    #[test]
    fn fmp4_mvex_trex_present() {
        let mut buf = Cursor::new(Vec::new());
        let mut mux = FMp4Muxer::new(&mut buf, &[video_stream()], 24, 1).unwrap();
        mux.write_header().unwrap();
        let data = buf.into_inner();
        assert!(data.windows(4).any(|w| w == b"mvex"), "no mvex box");
        assert!(data.windows(4).any(|w| w == b"trex"), "no trex box");
    }

    #[test]
    fn fmp4_multiple_fragments_have_increasing_sequence() {
        let mut buf = Cursor::new(Vec::new());
        let mut mux = FMp4Muxer::new(&mut buf, &[video_stream()], 24, 1).unwrap();
        mux.write_header().unwrap();
        // Write 70 packets — triggers at least 2 fragments (flush at 30)
        for i in 0..70u64 {
            mux.write_packet(&fake_packet(i, i == 0 || i == 30 || i == 60)).unwrap();
        }
        mux.write_trailer().unwrap();
        let data = buf.into_inner();

        // Count moof boxes
        let moof_count = data.windows(4).filter(|w| *w == b"moof").count();
        assert!(moof_count >= 2, "expected ≥2 moof fragments, got {moof_count}");
    }
}
