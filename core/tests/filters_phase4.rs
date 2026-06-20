//! Phase 4 filter tests: pixel filters, compositing filters, FilterGraph, GIF.

use ferrox_core::{
    filter_graph::FilterGraph,
    filters::{
        BlurFilter, BrightnessFilter, ContrastFilter, CropFilter,
        FlipAxis, FlipFilter, GrayscaleFilter, NegateFilter,
        OverlayFilter, PadFilter, ResizeFilter, RotateFilter,
        SaturationFilter, ThumbnailFilter,
    },
    frame::{Frame, PixelFormat},
    traits::Filter,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn rgb_frame(w: u32, h: u32, r: u8, g: u8, b: u8) -> Frame {
    let data = (0..w * h).flat_map(|_| [r, g, b]).collect();
    Frame::new(w, h, PixelFormat::Rgb8, data)
}

fn rgba_frame(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> Frame {
    let data = (0..w * h).flat_map(|_| [r, g, b, a]).collect();
    Frame::new(w, h, PixelFormat::Rgba8, data)
}

// ── BrightnessFilter ──────────────────────────────────────────────────────────

#[test]
fn brightness_increases_rgb() {
    let frame = rgb_frame(4, 4, 100, 100, 100);
    let out = BrightnessFilter::new(50).process(frame).unwrap();
    for px in out.data.chunks_exact(3) {
        assert_eq!(px[0], 150);
    }
}

#[test]
fn brightness_clamps_at_255() {
    let frame = rgb_frame(4, 4, 250, 250, 250);
    let out = BrightnessFilter::new(100).process(frame).unwrap();
    for px in out.data.chunks_exact(3) {
        assert_eq!(px[0], 255);
    }
}

#[test]
fn brightness_negative_darkens() {
    let frame = rgb_frame(4, 4, 100, 100, 100);
    let out = BrightnessFilter::new(-50).process(frame).unwrap();
    for px in out.data.chunks_exact(3) {
        assert_eq!(px[0], 50);
    }
}

// ── ContrastFilter ────────────────────────────────────────────────────────────

#[test]
fn contrast_identity_factor_one() {
    let frame = rgb_frame(4, 4, 128, 128, 128);
    let out = ContrastFilter::new(1.0).process(frame).unwrap();
    for px in out.data.chunks_exact(3) {
        assert_eq!(px[0], 128);
    }
}

#[test]
fn contrast_zero_flattens_to_midgray() {
    let frame = rgb_frame(4, 4, 200, 200, 200);
    let out = ContrastFilter::new(0.0).process(frame).unwrap();
    for px in out.data.chunks_exact(3) {
        assert_eq!(px[0], 128);
    }
}

// ── SaturationFilter ──────────────────────────────────────────────────────────

#[test]
fn saturation_zero_produces_gray() {
    let frame = rgb_frame(4, 4, 200, 100, 50);
    let out = SaturationFilter::new(0.0).process(frame).unwrap();
    let r = out.data[0];
    let g = out.data[1];
    let b = out.data[2];
    // All three channels should be approximately equal (luma).
    assert!((r as i32 - g as i32).abs() <= 2, "r={r} g={g}");
    assert!((g as i32 - b as i32).abs() <= 2, "g={g} b={b}");
}

// ── NegateFilter ─────────────────────────────────────────────────────────────

#[test]
fn negate_inverts_channels() {
    let frame = rgb_frame(4, 4, 100, 150, 200);
    let out = NegateFilter.process(frame).unwrap();
    let px = &out.data[..3];
    assert_eq!((px[0], px[1], px[2]), (155, 105, 55));
}

// ── GrayscaleFilter ───────────────────────────────────────────────────────────

#[test]
fn grayscale_makes_channels_equal() {
    let frame = rgb_frame(4, 4, 200, 100, 50);
    let out = GrayscaleFilter.process(frame).unwrap();
    for px in out.data.chunks_exact(3) {
        assert_eq!(px[0], px[1], "R/G mismatch in grayscale output");
        assert_eq!(px[1], px[2], "G/B mismatch in grayscale output");
    }
}

#[test]
fn grayscale_on_gray_is_noop() {
    let frame = rgb_frame(4, 4, 128, 128, 128);
    let out = GrayscaleFilter.process(frame.clone()).unwrap();
    assert_eq!(out.data, frame.data);
}

// ── FlipFilter ────────────────────────────────────────────────────────────────

#[test]
fn flip_horizontal_swaps_left_right() {
    // 2×1 image: left pixel = red, right pixel = blue
    let data: Vec<u8> = vec![255, 0, 0,   0, 0, 255];
    let frame = Frame::new(2, 1, PixelFormat::Rgb8, data);
    let out = FlipFilter::horizontal().process(frame).unwrap();
    assert_eq!(&out.data[..3], &[0, 0, 255]);   // was right, now left
    assert_eq!(&out.data[3..6], &[255, 0, 0]);  // was left, now right
}

#[test]
fn flip_vertical_swaps_top_bottom() {
    // 1×2 image: top = red, bottom = blue
    let data: Vec<u8> = vec![255, 0, 0,   0, 0, 255];
    let frame = Frame::new(1, 2, PixelFormat::Rgb8, data);
    let out = FlipFilter::vertical().process(frame).unwrap();
    assert_eq!(&out.data[..3], &[0, 0, 255]);
    assert_eq!(&out.data[3..6], &[255, 0, 0]);
}

// ── RotateFilter ──────────────────────────────────────────────────────────────

#[test]
fn rotate_180_is_double_flip() {
    let frame = rgb_frame(8, 6, 200, 100, 50);
    let r180 = RotateFilter::cw180().process(frame.clone()).unwrap();
    assert_eq!(r180.width, 8);
    assert_eq!(r180.height, 6);
    assert_eq!(r180.data.len(), frame.data.len());
}

#[test]
fn rotate_90_swaps_dimensions() {
    let frame = rgb_frame(8, 4, 200, 100, 50);
    let r90 = RotateFilter::cw90().process(frame).unwrap();
    assert_eq!(r90.width, 4);
    assert_eq!(r90.height, 8);
}

// ── CropFilter ────────────────────────────────────────────────────────────────

#[test]
fn crop_extracts_correct_dimensions() {
    let frame = rgb_frame(100, 80, 128, 128, 128);
    let out = CropFilter::new(10, 10, 50, 40).process(frame).unwrap();
    assert_eq!(out.width, 50);
    assert_eq!(out.height, 40);
}

#[test]
fn crop_out_of_bounds_errors() {
    let frame = rgb_frame(50, 50, 128, 128, 128);
    let result = CropFilter::new(40, 40, 20, 20).process(frame);
    assert!(result.is_err(), "out-of-bounds crop should error");
}

// ── BlurFilter ────────────────────────────────────────────────────────────────

#[test]
fn blur_preserves_dimensions() {
    let frame = rgb_frame(64, 64, 200, 100, 50);
    let out = BlurFilter::new(2.0).process(frame.clone()).unwrap();
    assert_eq!(out.width, 64);
    assert_eq!(out.height, 64);
    assert_eq!(out.data.len(), frame.data.len());
}

// ── ThumbnailFilter ───────────────────────────────────────────────────────────

#[test]
fn thumbnail_reduces_size() {
    let frame = rgb_frame(200, 100, 128, 128, 128);
    let out = ThumbnailFilter::new(100, 50).process(frame).unwrap();
    assert!(out.width <= 100 && out.height <= 50);
}

#[test]
fn thumbnail_crop_to_fit_is_exact() {
    let frame = rgb_frame(200, 100, 128, 128, 128);
    let out = ThumbnailFilter::new(64, 64).with_crop().process(frame).unwrap();
    assert_eq!(out.width, 64);
    assert_eq!(out.height, 64);
}

// ── PadFilter ────────────────────────────────────────────────────────────────

#[test]
fn pad_produces_correct_size() {
    let frame = rgb_frame(100, 80, 200, 0, 0);
    let out = PadFilter::new(200, 160).process(frame).unwrap();
    assert_eq!(out.width, 200);
    assert_eq!(out.height, 160);
}

#[test]
fn pad_fills_border_with_bg_color() {
    // A 1×1 red frame padded to 3×3 with white background.
    let frame = Frame::new(1, 1, PixelFormat::Rgb8, vec![255, 0, 0]);
    let out = PadFilter::new(3, 3).with_color(255, 255, 255).process(frame).unwrap();
    // Top-left corner should be white (background).
    assert_eq!(&out.data[..3], &[255, 255, 255], "top-left should be bg color");
    // Centre pixel should be red (the original frame).
    let centre_idx = (1 * 3 + 1) * 3;
    assert_eq!(&out.data[centre_idx..centre_idx + 3], &[255, 0, 0], "centre should be red");
}

#[test]
fn pad_source_larger_than_output_errors() {
    let frame = rgb_frame(100, 100, 0, 0, 0);
    assert!(PadFilter::new(50, 50).process(frame).is_err());
}

// ── OverlayFilter ─────────────────────────────────────────────────────────────

#[test]
fn overlay_rgb_replaces_pixels() {
    let base = rgb_frame(10, 10, 0, 0, 0);
    let ovl  = rgb_frame(4, 4, 255, 255, 255);
    let out  = OverlayFilter::new(ovl, 3, 3).process(base).unwrap();
    // Pixel at (3,3) in the base should now be white.
    let idx = (3 * 10 + 3) * 3;
    assert_eq!(&out.data[idx..idx + 3], &[255, 255, 255]);
}

#[test]
fn overlay_partially_out_of_bounds_is_clipped() {
    let base = rgb_frame(10, 10, 0, 0, 0);
    let ovl  = rgb_frame(4, 4, 255, 255, 255);
    // Overlay placed so it overhangs the right/bottom edges.
    let out  = OverlayFilter::new(ovl, 8, 8).process(base).unwrap();
    assert_eq!(out.width, 10);
    assert_eq!(out.height, 10);
}

#[test]
fn overlay_rgba_alpha_blends() {
    let base = rgb_frame(4, 4, 0, 0, 0);
    // Semi-transparent white overlay.
    let ovl  = rgba_frame(4, 4, 255, 255, 255, 128);
    let out  = OverlayFilter::new(ovl, 0, 0).process(base).unwrap();
    // Blended: ~50% of 255 ≈ 128 (allow ±2 rounding).
    let r = out.data[0];
    assert!(r >= 126 && r <= 130, "expected ~128, got {r}");
}

// ── FilterGraph ───────────────────────────────────────────────────────────────

#[test]
fn filter_graph_linear_chain() {
    let mut graph = FilterGraph::new();
    graph.add_filter("bright", BrightnessFilter::new(20));
    graph.add_filter("gray",   GrayscaleFilter);
    graph.connect("bright", "gray");

    let frame = rgb_frame(8, 8, 100, 100, 100);
    let out = graph.run(frame, "bright", "gray").unwrap();
    // After brightness +20: 120. After grayscale: still 120 (equal channels).
    assert_eq!(out.data[0], 120);
}

#[test]
fn filter_graph_run_all_applies_all_nodes() {
    let mut graph = FilterGraph::new();
    graph.add_filter("b1", BrightnessFilter::new(10));
    graph.add_filter("b2", BrightnessFilter::new(10));

    let frame = rgb_frame(4, 4, 100, 100, 100);
    let out = graph.run_all(frame).unwrap();
    assert_eq!(out.data[0], 120);
}

#[test]
fn filter_graph_unknown_node_errors() {
    let graph = FilterGraph::new();
    let frame = rgb_frame(4, 4, 128, 128, 128);
    assert!(graph.run(frame, "missing", "also_missing").is_err());
}

#[test]
fn filter_graph_parse_and_run_grayscale() {
    let frame = rgb_frame(8, 8, 200, 100, 50);
    let out = FilterGraph::parse_and_run(frame, "grayscale").unwrap();
    assert_eq!(out.data[0], out.data[1]);
    assert_eq!(out.data[1], out.data[2]);
}

#[test]
fn filter_graph_parse_and_run_chain() {
    let frame = rgb_frame(8, 8, 100, 100, 100);
    let out = FilterGraph::parse_and_run(frame, "brightness=30,grayscale").unwrap();
    assert_eq!(out.data[0], 130);
}

#[test]
fn filter_graph_parse_scale() {
    let frame = rgb_frame(64, 64, 128, 128, 128);
    let out = FilterGraph::parse_and_run(frame, "scale=32:16").unwrap();
    assert_eq!(out.width, 32);
    assert_eq!(out.height, 16);
}

#[test]
fn filter_graph_parse_unknown_token_errors() {
    let frame = rgb_frame(8, 8, 128, 128, 128);
    assert!(FilterGraph::parse_and_run(frame, "foobar").is_err());
}

// ── GIF codec ─────────────────────────────────────────────────────────────────

#[cfg(feature = "gif-support")]
mod gif_tests {
    use ferrox_core::{decode_gif, encode_gif, GifEncodeOptions, GifFrame};
    use ferrox_core::frame::{Frame, PixelFormat};
    use std::io::Cursor;

    fn make_test_gif() -> Vec<u8> {
        // Encode a minimal 2-frame GIF programmatically with the gif crate.
        let mut buf = Vec::new();
        let w: u16 = 4;
        let h: u16 = 4;
        let mut enc = gif::Encoder::new(&mut buf, w, h, &[]).unwrap();
        enc.set_repeat(gif::Repeat::Infinite).unwrap();

        for color in [128u8, 200u8] {
            let palette = vec![color, color, color];  // single-color palette
            let indices = vec![0u8; (w * h) as usize];
            let mut frame = gif::Frame::default();
            frame.width   = w;
            frame.height  = h;
            frame.delay   = 10;
            frame.palette = Some(palette);
            frame.buffer  = std::borrow::Cow::Owned(indices);
            enc.write_frame(&frame).unwrap();
        }
        drop(enc);
        buf
    }

    #[test]
    fn decode_gif_returns_frames() {
        let gif_bytes = make_test_gif();
        let frames = decode_gif(Cursor::new(gif_bytes)).unwrap();
        assert_eq!(frames.len(), 2, "should decode 2 frames");
        assert_eq!(frames[0].frame.width, 4);
        assert_eq!(frames[0].frame.height, 4);
    }

    #[test]
    fn decode_gif_frames_are_rgb8() {
        let gif_bytes = make_test_gif();
        let frames = decode_gif(Cursor::new(gif_bytes)).unwrap();
        for f in &frames {
            assert_eq!(f.frame.format, PixelFormat::Rgb8);
        }
    }

    #[test]
    fn encode_gif_roundtrip_produces_valid_bytes() {
        let gif_frames: Vec<GifFrame> = (0..3).map(|_| GifFrame {
            frame: Frame::new(8, 8, PixelFormat::Rgb8, vec![128u8; 8 * 8 * 3]),
            delay_cs: 10,
        }).collect();
        let mut out = Vec::new();
        encode_gif(&mut out, &gif_frames, &GifEncodeOptions::default()).unwrap();
        // GIF magic bytes: GIF89a or GIF87a
        assert!(out.len() > 6);
        assert_eq!(&out[..3], b"GIF");
    }

    #[test]
    fn encode_then_decode_roundtrip() {
        let orig_frames: Vec<GifFrame> = vec![
            GifFrame { frame: Frame::new(8, 8, PixelFormat::Rgb8, vec![200u8; 8*8*3]), delay_cs: 5 },
            GifFrame { frame: Frame::new(8, 8, PixelFormat::Rgb8, vec![100u8; 8*8*3]), delay_cs: 5 },
        ];
        let mut encoded = Vec::new();
        encode_gif(&mut encoded, &orig_frames, &GifEncodeOptions::default()).unwrap();
        let decoded = decode_gif(Cursor::new(encoded)).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].frame.width, 8);
    }

    #[test]
    fn encode_gif_empty_frames_errors() {
        let mut out = Vec::new();
        assert!(encode_gif(&mut out, &[], &GifEncodeOptions::default()).is_err());
    }
}
