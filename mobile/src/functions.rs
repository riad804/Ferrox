//! Stateless `#[uniffi::export]` free functions (decode/resize/filter/probe/…).

use ferrox_core as fx;
use crate::session::{decode_image_frame, frame_to_png};
use crate::FerroxError;

#[uniffi::export]
pub fn decode_image_to_png(image_data: Vec<u8>) -> Result<Vec<u8>, FerroxError> {
    frame_to_png(decode_image_frame(&image_data)?)
}

/// Resize a PNG or JPEG image to `width × height`; returns PNG bytes (Lanczos3).
#[uniffi::export]
pub fn resize_image(image_data: Vec<u8>, width: u32, height: u32) -> Result<Vec<u8>, FerroxError> {
    use fx::traits::Filter;
    use fx::ResizeFilter;
    let frame = decode_image_frame(&image_data)?;
    let resized = ResizeFilter::new(width, height).process(frame)?;
    frame_to_png(resized)
}

/// Apply a ferrox filtergraph expression to a PNG/JPEG image; returns PNG bytes.
///
/// Examples: `"blur=2.0"`, `"grayscale"`,
/// `"scale=640:480,brightness=20,contrast=1.2"`.
#[uniffi::export]
pub fn apply_filter(image_data: Vec<u8>, filter_expr: String) -> Result<Vec<u8>, FerroxError> {
    use fx::FilterGraph;
    let frame = decode_image_frame(&image_data)?;
    let out = FilterGraph::parse_and_run(frame, &filter_expr)?;
    frame_to_png(out)
}

/// Gaussian-style blur a PNG/JPEG image. `sigma` controls radius (e.g. `2.0`).
#[uniffi::export]
pub fn blur_image(image_data: Vec<u8>, sigma: f32) -> Result<Vec<u8>, FerroxError> {
    use fx::traits::Filter;
    use fx::BlurFilter;
    let frame = decode_image_frame(&image_data)?;
    let blurred = BlurFilter::new(sigma).process(frame)?;
    frame_to_png(blurred)
}

/// Convert a PNG/JPEG image to grayscale; returns PNG bytes.
#[uniffi::export]
pub fn grayscale_image(image_data: Vec<u8>) -> Result<Vec<u8>, FerroxError> {
    use fx::traits::Filter;
    use fx::GrayscaleFilter;
    let frame = decode_image_frame(&image_data)?;
    let gray = GrayscaleFilter.process(frame)?;
    frame_to_png(gray)
}

/// Return basic metadata about a PNG/JPEG image as a JSON string:
/// `{"width":1920,"height":1080,"format":"png","channels":3}`.
#[uniffi::export]
pub fn probe_image(image_data: Vec<u8>) -> Result<String, FerroxError> {
    use fx::PixelFormat;
    let frame = decode_image_frame(&image_data)?;
    let fmt = if image_data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else {
        "jpeg"
    };
    let channels = match frame.format {
        PixelFormat::Rgb8 | PixelFormat::Yuv420p => 3,
        PixelFormat::Rgba8 => 4,
        PixelFormat::Gray8 => 1,
        PixelFormat::GrayA8 => 2,
        _ => 0,
    };
    Ok(format!(
        r#"{{"width":{w},"height":{h},"format":"{fmt}","channels":{channels}}}"#,
        w = frame.width,
        h = frame.height,
    ))
}

// ── Video (VP8) ───────────────────────────────────────────────────────────────

/// Decode a single VP8 keyframe and return it as PNG bytes.
#[uniffi::export]
pub fn decode_vp8_to_png(vp8_data: Vec<u8>) -> Result<Vec<u8>, FerroxError> {
    use fx::demux_graph::yuv420p_to_rgb8;
    use fx::traits::VideoDecoder;
    use fx::video::Packet;
    use fx::Vp8Decoder;

    let packet = Packet {
        data: vp8_data,
        pts: 0,
        duration: 0,
        is_keyframe: true,
    };
    let mut decoder = Vp8Decoder;
    let vf = decoder
        .decode_packet(&packet)
        .map_err(|e| FerroxError::Decode {
            message: e.to_string(),
        })?;
    let rgb = yuv420p_to_rgb8(&vf.frame)?;
    frame_to_png(rgb)
}

// ── GIF ───────────────────────────────────────────────────────────────────────

/// One decoded GIF frame: PNG bytes plus its display delay in milliseconds.
#[derive(uniffi::Record)]
pub struct GifPngFrame {
    pub png: Vec<u8>,
    pub delay_ms: u32,
}

/// Decode an animated GIF into a list of PNG-encoded frames.
///
/// Unlike the WASM build (which returns a packed byte blob), this returns a
/// typed list so Kotlin/Swift get `List<GifPngFrame>` / `[GifPngFrame]`.
#[uniffi::export]
pub fn decode_gif_frames(gif_data: Vec<u8>) -> Result<Vec<GifPngFrame>, FerroxError> {
    use fx::decode_gif;
    let frames = decode_gif(std::io::Cursor::new(&gif_data))?;
    frames
        .into_iter()
        .map(|gf| {
            // delay is stored in centiseconds (1/100 s) in the GIF spec.
            let delay_ms = gf.delay_cs as u32 * 10;
            Ok(GifPngFrame {
                png: frame_to_png(gf.frame)?,
                delay_ms,
            })
        })
        .collect()
}

// ── Meta ───────────────────────────────────────────────────────────────────────

/// The ferrox-mobile crate version (semver string).
#[uniffi::export]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── tests ───────────────────────────────────────────────────────────────────────

