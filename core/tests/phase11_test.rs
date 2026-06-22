/// Phase 11 integration tests: HDR VP9 pixel formats, H.264 High Profile
/// detection, and YUV conversion helpers.
///
/// Run with: cargo test -p ferrox-core --test phase11_test

// ── Pixel format tests (always compiled) ─────────────────────────────────────

#[cfg(feature = "image-codecs")]
mod pixel_formats {
    use ferrox_core::frame::{Frame, PixelFormat};

    #[test]
    fn yuv420p10_expected_len() {
        // 8x8: Y = 64 samples, UV = 4x4 = 16 each → 96 samples → 192 bytes (u16)
        assert_eq!(PixelFormat::Yuv420p10.expected_data_len(8, 8), 192);
    }

    #[test]
    fn yuv420p12_expected_len() {
        assert_eq!(PixelFormat::Yuv420p12.expected_data_len(8, 8), 192);
    }

    #[test]
    fn yuv422p_expected_len() {
        // 8x8: Y = 64, UV = 4x8 = 32 each → 128 bytes
        assert_eq!(PixelFormat::Yuv422p.expected_data_len(8, 8), 128);
    }

    #[test]
    fn yuv444p_expected_len() {
        // 8x8: each plane = 64 → 192 bytes
        assert_eq!(PixelFormat::Yuv444p.expected_data_len(8, 8), 192);
    }

    #[test]
    fn yuv420p_expected_len_unchanged() {
        assert_eq!(PixelFormat::Yuv420p.expected_data_len(8, 8), 96);
    }

    #[test]
    fn hdr_formats_flagged_correctly() {
        assert!(PixelFormat::Yuv420p10.is_hdr());
        assert!(PixelFormat::Yuv420p12.is_hdr());
        assert!(!PixelFormat::Yuv420p.is_hdr());
        assert!(!PixelFormat::Rgb8.is_hdr());
        assert!(!PixelFormat::Yuv422p.is_hdr());
        assert!(!PixelFormat::Yuv444p.is_hdr());
    }

    #[test]
    fn bytes_per_sample_correct() {
        assert_eq!(PixelFormat::Yuv420p10.bytes_per_sample(), 2);
        assert_eq!(PixelFormat::Yuv420p12.bytes_per_sample(), 2);
        assert_eq!(PixelFormat::Yuv420p.bytes_per_sample(), 1);
        assert_eq!(PixelFormat::Rgb8.bytes_per_sample(), 1);
    }
}

// ── YUV conversion helpers ────────────────────────────────────────────────────

#[cfg(feature = "video-codecs")]
mod yuv_conversion {
    use ferrox_core::{
        frame::{Frame, PixelFormat},
        any_yuv_to_rgb8, yuv420p_to_rgb8, yuv420p_hdr_to_rgb8,
        yuv422p_to_rgb8, yuv444p_to_rgb8,
    };

    fn gray_yuv420p(w: u32, h: u32) -> Frame {
        let w_uv = ((w + 1) / 2) as usize;
        let h_uv = ((h + 1) / 2) as usize;
        let mut data = vec![128u8; w as usize * h as usize]; // Y = 128 (mid-gray)
        data.extend(vec![128u8; w_uv * h_uv]); // U = neutral
        data.extend(vec![128u8; w_uv * h_uv]); // V = neutral
        Frame::new(w, h, PixelFormat::Yuv420p, data)
    }

    fn gray_yuv422p(w: u32, h: u32) -> Frame {
        let w_uv = ((w + 1) / 2) as usize;
        let mut data = vec![128u8; w as usize * h as usize];
        data.extend(vec![128u8; w_uv * h as usize]);
        data.extend(vec![128u8; w_uv * h as usize]);
        Frame::new(w, h, PixelFormat::Yuv422p, data)
    }

    fn gray_yuv444p(w: u32, h: u32) -> Frame {
        let n = w as usize * h as usize;
        let mut data = vec![128u8; n];
        data.extend(vec![128u8; n]);
        data.extend(vec![128u8; n]);
        Frame::new(w, h, PixelFormat::Yuv444p, data)
    }

    fn gray_yuv420p10(w: u32, h: u32) -> Frame {
        // 10-bit: Y=512 (mid-gray in 10-bit), UV=512 (neutral chroma)
        let w_uv = ((w + 1) / 2) as usize;
        let h_uv = ((h + 1) / 2) as usize;
        let y_n  = w as usize * h as usize;
        let uv_n = w_uv * h_uv;
        let mut data: Vec<u8> = Vec::with_capacity((y_n + 2 * uv_n) * 2);
        let sample: u16 = 512u16; // mid-gray at 10-bit
        for _ in 0..(y_n + 2 * uv_n) {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        Frame::new(w, h, PixelFormat::Yuv420p10, data)
    }

    #[test]
    fn yuv420p_gray_converts_to_gray_rgb() {
        let frame = gray_yuv420p(4, 4);
        let rgb = yuv420p_to_rgb8(&frame).unwrap();
        assert_eq!(rgb.format, PixelFormat::Rgb8);
        assert_eq!(rgb.data.len(), 4 * 4 * 3);
        // Y=128, U=V=0 → should be near mid-gray
        let avg: u32 = rgb.data.iter().map(|&b| b as u32).sum::<u32>() / rgb.data.len() as u32;
        assert!(avg > 100 && avg < 160, "expected ~gray output, got avg={avg}");
    }

    #[test]
    fn yuv422p_converts_correctly() {
        let frame = gray_yuv422p(4, 4);
        let rgb = yuv422p_to_rgb8(&frame).unwrap();
        assert_eq!(rgb.format, PixelFormat::Rgb8);
        assert_eq!(rgb.data.len(), 4 * 4 * 3);
    }

    #[test]
    fn yuv444p_converts_correctly() {
        let frame = gray_yuv444p(4, 4);
        let rgb = yuv444p_to_rgb8(&frame).unwrap();
        assert_eq!(rgb.format, PixelFormat::Rgb8);
        assert_eq!(rgb.data.len(), 4 * 4 * 3);
    }

    #[test]
    fn yuv420p10_converts_to_rgb8() {
        let frame = gray_yuv420p10(4, 4);
        let rgb = yuv420p_hdr_to_rgb8(&frame).unwrap();
        assert_eq!(rgb.format, PixelFormat::Rgb8);
        assert_eq!(rgb.data.len(), 4 * 4 * 3);
    }

    #[test]
    fn yuv420p12_converts_to_rgb8() {
        let w_uv = 2usize;
        let h_uv = 2usize;
        let y_n  = 4 * 4;
        let uv_n = w_uv * h_uv;
        let mut data: Vec<u8> = Vec::with_capacity((y_n + 2 * uv_n) * 2);
        let sample: u16 = 2048u16; // mid-gray at 12-bit
        for _ in 0..(y_n + 2 * uv_n) {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let frame = Frame::new(4, 4, PixelFormat::Yuv420p12, data);
        let rgb = yuv420p_hdr_to_rgb8(&frame).unwrap();
        assert_eq!(rgb.format, PixelFormat::Rgb8);
        assert_eq!(rgb.data.len(), 4 * 4 * 3);
    }

    #[test]
    fn any_yuv_to_rgb8_dispatches_all_formats() {
        assert!(any_yuv_to_rgb8(&gray_yuv420p(4, 4)).is_ok());
        assert!(any_yuv_to_rgb8(&gray_yuv422p(4, 4)).is_ok());
        assert!(any_yuv_to_rgb8(&gray_yuv444p(4, 4)).is_ok());
        assert!(any_yuv_to_rgb8(&gray_yuv420p10(4, 4)).is_ok());
        // Rgb8 passthrough
        let rgb_frame = Frame::new(2, 2, PixelFormat::Rgb8, vec![255u8; 12]);
        assert!(any_yuv_to_rgb8(&rgb_frame).is_ok());
    }

    #[test]
    fn wrong_format_returns_error() {
        // yuv420p_to_rgb8 with a non-Yuv420p frame must return Err.
        let rgb_frame = Frame::new(2, 2, PixelFormat::Rgb8, vec![0u8; 12]);
        assert!(yuv420p_to_rgb8(&rgb_frame).is_err());

        // yuv422p_to_rgb8 with a non-Yuv422p frame must return Err.
        let yuv_frame = gray_yuv420p(4, 4);
        // Relabel as Rgb8 to trigger the format check.
        let bad = Frame::new(4, 4, PixelFormat::Rgb8, yuv_frame.data);
        assert!(yuv422p_to_rgb8(&bad).is_err());
    }
}

// ── H.264 profile detection ───────────────────────────────────────────────────

#[cfg(feature = "h264")]
mod h264_profile {
    use ferrox_core::{H264Profile, detect_h264_profile};

    fn make_sps_annex_b(profile_idc: u8) -> Vec<u8> {
        // Minimal Annex B SPS NAL unit:
        // [start_code 4B] [nal_header 0x67=SPS] [profile_idc] [constraints] [level_idc]
        vec![
            0x00, 0x00, 0x00, 0x01, // start code
            0x67,                   // nal_unit_type=7 (SPS), nal_ref_idc=3
            profile_idc,
            0x40,                   // constraint_set flags
            0x28,                   // level_idc = 40
        ]
    }

    #[test]
    fn detects_baseline_profile() {
        let data = make_sps_annex_b(66);
        assert_eq!(detect_h264_profile(&data), Some(H264Profile::Baseline));
    }

    #[test]
    fn detects_main_profile() {
        let data = make_sps_annex_b(77);
        assert_eq!(detect_h264_profile(&data), Some(H264Profile::Main));
    }

    #[test]
    fn detects_high_profile() {
        let data = make_sps_annex_b(100);
        assert_eq!(detect_h264_profile(&data), Some(H264Profile::High));
    }

    #[test]
    fn detects_high10_profile() {
        let data = make_sps_annex_b(110);
        assert_eq!(detect_h264_profile(&data), Some(H264Profile::High10));
    }

    #[test]
    fn detects_high422_profile() {
        let data = make_sps_annex_b(122);
        assert_eq!(detect_h264_profile(&data), Some(H264Profile::High422));
    }

    #[test]
    fn detects_unknown_profile() {
        let data = make_sps_annex_b(42);
        assert_eq!(detect_h264_profile(&data), Some(H264Profile::Unknown(42)));
    }

    #[test]
    fn no_sps_returns_none() {
        // Just some random bytes with no SPS NAL.
        let data = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        assert_eq!(detect_h264_profile(&data), None);
    }

    #[test]
    fn empty_data_returns_none() {
        assert_eq!(detect_h264_profile(&[]), None);
    }

    #[test]
    fn h264_profile_from_idc_roundtrip() {
        for (idc, expected) in [
            (66u8,  H264Profile::Baseline),
            (77,    H264Profile::Main),
            (100,   H264Profile::High),
            (110,   H264Profile::High10),
            (122,   H264Profile::High422),
            (244,   H264Profile::High444),
        ] {
            assert_eq!(H264Profile::from_idc(idc), expected);
        }
    }
}

// ── H264OutputMode ────────────────────────────────────────────────────────────

#[cfg(feature = "h264")]
mod h264_output_mode {
    use ferrox_core::H264OutputMode;

    #[test]
    fn default_output_mode_is_rgb8() {
        assert_eq!(H264OutputMode::default(), H264OutputMode::Rgb8);
    }
}
