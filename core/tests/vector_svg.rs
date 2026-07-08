//! Phase 13 vector graphics: SVG parsing + rasterisation to correct pixels,
//! intrinsic size, arbitrary output size, and error handling.
#![cfg(feature = "svg")]

use ferrox_core::vector::{SvgImage, VectorRenderer};
use std::str::FromStr;
use ferrox_core::PixelFormat;

fn px(f: &ferrox_core::Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * f.width + x) * 4) as usize;
    [f.data[i], f.data[i + 1], f.data[i + 2], f.data[i + 3]]
}

const RED_SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
  <rect x="0" y="0" width="10" height="10" fill="rgb(255,0,0)"/>
</svg>"#;

#[test]
fn parses_and_reports_intrinsic_size() {
    let svg = SvgImage::from_str(RED_SQUARE).unwrap();
    assert_eq!(svg.intrinsic_size(), (10, 10));
}

#[test]
fn rasterizes_solid_fill_to_correct_pixels() {
    let svg = SvgImage::from_str(RED_SQUARE).unwrap();
    let frame = svg.render(10, 10, 0.0).unwrap();
    assert_eq!((frame.width, frame.height), (10, 10));
    assert_eq!(frame.format, PixelFormat::Rgba8);
    // Interior pixel is opaque red (allow a little AA slack at edges via center).
    assert_eq!(px(&frame, 5, 5), [255, 0, 0, 255]);
}

#[test]
fn renders_at_requested_resolution() {
    let svg = SvgImage::from_str(RED_SQUARE).unwrap();
    let frame = svg.render(40, 20, 0.0).unwrap();
    assert_eq!((frame.width, frame.height), (40, 20), "scales to requested size");
    assert_eq!(px(&frame, 20, 10), [255, 0, 0, 255]);
}

#[test]
fn transparent_regions_have_zero_alpha() {
    // A small circle centered in a larger canvas → corners are transparent.
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
      <circle cx="10" cy="10" r="4" fill="rgb(0,0,255)"/>
    </svg>"#;
    let frame = SvgImage::from_str(svg).unwrap().render(20, 20, 0.0).unwrap();
    assert_eq!(px(&frame, 0, 0)[3], 0, "corner is transparent");
    assert_eq!(px(&frame, 10, 10), [0, 0, 255, 255], "center is opaque blue");
}

#[test]
fn invalid_svg_is_rejected() {
    assert!(SvgImage::from_bytes(b"not xml at all <<<").is_err());
}
