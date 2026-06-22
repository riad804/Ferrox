/// Phase 10: WASM API smoke tests.
///
/// These tests run on the host (not in a browser) and verify that the
/// functions exposed by the `wasm` module work correctly with real image data.
/// They test the same logic that wasm-bindgen exports to JavaScript.
///
/// Run with: cargo test -p ferrox-core --test wasm_api_test

// The wasm module's logic is plain Rust — we can call the inner helpers
// directly on the host without wasm-bindgen being involved.

#[cfg(feature = "image-codecs")]
mod image_api {
    use ferrox_core::{
        codecs::{png::PngEncoder, png::PngDecoder, jpeg::JpegDecoder},
        filters::{BlurFilter, GrayscaleFilter, ResizeFilter},
        filter_graph::FilterGraph,
        frame::{Frame, PixelFormat},
        traits::{Decoder, Encoder, Filter},
    };

    // ── helpers ───────────────────────────────────────────────────────────────

    fn tiny_rgb_frame(w: u32, h: u32) -> Frame {
        let data: Vec<f32> = (0..w * h * 3)
            .map(|i| ((i % 256) as f32) / 255.0)
            .collect();
        let bytes: Vec<u8> = data.iter().map(|&f| (f * 255.0) as u8).collect();
        Frame::new(w, h, PixelFormat::Rgb8, bytes)
    }

    fn encode_png(frame: &Frame) -> Vec<u8> {
        let mut buf = Vec::new();
        PngEncoder.encode(frame, &mut buf).expect("png encode");
        buf
    }

    fn is_png(data: &[u8]) -> bool {
        data.starts_with(&[0x89, b'P', b'N', b'G'])
    }

    // ── decode_image_to_png equivalent ────────────────────────────────────────

    #[test]
    fn png_round_trips_through_decode_encode() {
        let frame = tiny_rgb_frame(8, 8);
        let png_in = encode_png(&frame);

        let decoded = PngDecoder.decode(std::io::Cursor::new(&png_in)).unwrap();
        let mut png_out = Vec::new();
        PngEncoder.encode(&decoded, &mut png_out).unwrap();

        assert!(is_png(&png_out), "output must be PNG");
        assert_eq!(decoded.width, 8);
        assert_eq!(decoded.height, 8);
    }

    // ── resize_image equivalent ───────────────────────────────────────────────

    #[test]
    fn resize_produces_correct_dimensions() {
        let frame = tiny_rgb_frame(16, 16);
        let resized = ResizeFilter::new(4, 4).process(frame).unwrap();
        assert_eq!(resized.width, 4);
        assert_eq!(resized.height, 4);

        let mut png = Vec::new();
        PngEncoder.encode(&resized, &mut png).unwrap();
        assert!(is_png(&png));
    }

    #[test]
    fn resize_upscales_correctly() {
        let frame = tiny_rgb_frame(4, 4);
        let big = ResizeFilter::new(64, 64).process(frame).unwrap();
        assert_eq!(big.width, 64);
        assert_eq!(big.height, 64);
    }

    // ── blur_image equivalent ─────────────────────────────────────────────────

    #[test]
    fn blur_does_not_change_dimensions() {
        let frame = tiny_rgb_frame(16, 16);
        let blurred = BlurFilter::new(1.5).process(frame).unwrap();
        assert_eq!(blurred.width, 16);
        assert_eq!(blurred.height, 16);

        let mut png = Vec::new();
        PngEncoder.encode(&blurred, &mut png).unwrap();
        assert!(is_png(&png));
    }

    // ── grayscale_image equivalent ────────────────────────────────────────────

    #[test]
    fn grayscale_produces_valid_frame() {
        let frame = tiny_rgb_frame(8, 8);
        let gray = GrayscaleFilter.process(frame).unwrap();
        assert_eq!(gray.width, 8);
        assert_eq!(gray.height, 8);
        // After grayscale all R=G=B per pixel; data must be non-empty.
        assert!(!gray.data.is_empty());
    }

    // ── apply_filter / FilterGraph equivalent ─────────────────────────────────

    #[test]
    fn filter_graph_blur_then_grayscale() {
        let frame = tiny_rgb_frame(8, 8);
        let out = FilterGraph::parse_and_run(frame, "blur=1.0,grayscale").unwrap();
        assert_eq!(out.width, 8);
        assert_eq!(out.height, 8);
    }

    #[test]
    fn filter_graph_scale() {
        let frame = tiny_rgb_frame(32, 32);
        let out = FilterGraph::parse_and_run(frame, "scale=8:8").unwrap();
        assert_eq!(out.width, 8);
        assert_eq!(out.height, 8);
    }

    #[test]
    fn filter_graph_brightness_contrast() {
        let frame = tiny_rgb_frame(8, 8);
        let out = FilterGraph::parse_and_run(frame, "brightness=10,contrast=1.2").unwrap();
        assert_eq!(out.width, 8);
        assert_eq!(out.height, 8);
    }

    // ── probe_image equivalent ────────────────────────────────────────────────

    #[test]
    fn probe_image_returns_correct_dimensions() {
        let frame = tiny_rgb_frame(20, 30);
        let png_data = encode_png(&frame);

        let decoded = PngDecoder.decode(std::io::Cursor::new(&png_data)).unwrap();
        assert_eq!(decoded.width, 20);
        assert_eq!(decoded.height, 30);
    }
}

// ── VP8 → PNG (via demux_graph helpers) ───────────────────────────────────────

#[cfg(all(feature = "video-codecs", feature = "image-codecs"))]
mod vp8_api {
    use ferrox_core::{
        codecs::video::Vp8Decoder,
        codecs::png::PngEncoder,
        demux_graph::yuv420p_to_rgb8,
        traits::{Encoder, VideoDecoder},
        video::Packet,
    };
    use oxideav_vp8::{
        encoder::{encode_silent_keyframe, SilentKeyframeParams},
    };

    fn make_vp8_keyframe(w: u32, h: u32) -> Vec<u8> {
        encode_silent_keyframe(SilentKeyframeParams::new(w, h))
            .expect("encode_silent_keyframe")
    }

    fn is_png(data: &[u8]) -> bool {
        data.starts_with(&[0x89, b'P', b'N', b'G'])
    }

    #[test]
    fn vp8_keyframe_decodes_to_png() {
        let vp8 = make_vp8_keyframe(16, 16);
        let packet = Packet {
            data: vp8,
            pts: 0,
            duration: 0,
            is_keyframe: true,
        };
        let mut decoder = Vp8Decoder;
        let vf = decoder.decode_packet(&packet).expect("vp8 decode");
        let rgb = yuv420p_to_rgb8(&vf.frame).expect("yuv420p_to_rgb8");
        let mut png_out = Vec::new();
        PngEncoder.encode(&rgb, &mut png_out).expect("png encode");
        assert!(is_png(&png_out), "output must be PNG");
        assert!(png_out.len() > 64, "PNG too small");
    }

    #[test]
    fn vp8_decode_returns_correct_size() {
        let vp8 = make_vp8_keyframe(32, 24);
        let packet = Packet {
            data: vp8,
            pts: 0,
            duration: 0,
            is_keyframe: true,
        };
        let mut decoder = Vp8Decoder;
        let vf = decoder.decode_packet(&packet).expect("vp8 decode");
        assert_eq!(vf.width(), 32);
        assert_eq!(vf.height(), 24);
    }

    #[test]
    fn vp8_yuv_to_rgb_has_correct_byte_count() {
        let vp8 = make_vp8_keyframe(8, 8);
        let packet = Packet { data: vp8, pts: 0, duration: 0, is_keyframe: true };
        let vf = Vp8Decoder.decode_packet(&packet).unwrap();
        let rgb = yuv420p_to_rgb8(&vf.frame).unwrap();
        // Rgb8: w * h * 3 bytes
        assert_eq!(rgb.data.len() as u32, rgb.width * rgb.height * 3);
    }
}
