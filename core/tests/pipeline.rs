use std::io::Cursor;
use image::{codecs::jpeg::JpegDecoder, ImageDecoder};
use mediaforge_core::{filters::ResizeFilter, Graph};
use tempfile::NamedTempFile;

/// Build a minimal valid 64×48 RGB8 PNG in memory without any file I/O.
fn make_test_png(width: u32, height: u32) -> Vec<u8> {
    use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
    let pixels: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                [
                    (x * 255 / width) as u8,
                    (y * 255 / height) as u8,
                    128u8,
                ]
            })
        })
        .collect();
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(&pixels, width, height, ColorType::Rgb8.into())
        .expect("encode test PNG");
    buf
}

#[test]
fn decode_png_resize_encode_jpeg_dimensions() {
    let src_w = 64u32;
    let src_h = 48u32;
    let dst_w = 20u32;
    let dst_h = 15u32;

    // Write source PNG to a temp file (Graph works with paths).
    let src = NamedTempFile::with_suffix(".png").unwrap();
    std::fs::write(src.path(), make_test_png(src_w, src_h)).unwrap();

    let dst = NamedTempFile::with_suffix(".jpg").unwrap();

    let graph = Graph::new().with_filter(ResizeFilter::new(dst_w, dst_h));
    graph.run(src.path(), dst.path()).expect("pipeline failed");

    // Verify dimensions of the written JPEG.
    let jpeg_bytes = std::fs::read(dst.path()).unwrap();
    let dec = JpegDecoder::new(Cursor::new(jpeg_bytes)).unwrap();
    let (out_w, out_h) = dec.dimensions();
    assert_eq!(out_w, dst_w, "JPEG width mismatch");
    assert_eq!(out_h, dst_h, "JPEG height mismatch");
}

#[test]
fn decode_png_encode_jpeg_no_resize() {
    let src_w = 32u32;
    let src_h = 32u32;

    let src = NamedTempFile::with_suffix(".png").unwrap();
    std::fs::write(src.path(), make_test_png(src_w, src_h)).unwrap();

    let dst = NamedTempFile::with_suffix(".jpg").unwrap();

    Graph::new().run(src.path(), dst.path()).expect("convert failed");

    let jpeg_bytes = std::fs::read(dst.path()).unwrap();
    let dec = JpegDecoder::new(Cursor::new(jpeg_bytes)).unwrap();
    let (out_w, out_h) = dec.dimensions();
    assert_eq!(out_w, src_w);
    assert_eq!(out_h, src_h);
}

#[test]
fn resize_then_png_roundtrip() {
    let src = NamedTempFile::with_suffix(".png").unwrap();
    std::fs::write(src.path(), make_test_png(64, 64)).unwrap();

    let dst = NamedTempFile::with_suffix(".png").unwrap();

    Graph::new()
        .with_filter(ResizeFilter::new(8, 8))
        .run(src.path(), dst.path())
        .expect("png→png resize failed");

    let img = image::open(dst.path()).expect("re-open resized PNG");
    assert_eq!(img.width(), 8);
    assert_eq!(img.height(), 8);
}

#[test]
fn unsupported_extension_returns_error() {
    let src = NamedTempFile::with_suffix(".bmp").unwrap();
    let dst = NamedTempFile::with_suffix(".png").unwrap();
    let result = Graph::new().run(src.path(), dst.path());
    assert!(result.is_err(), "expected error for unsupported extension");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("bmp"), "error should mention the extension: {msg}");
}
