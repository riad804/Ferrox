//! WASM bindings for ferrox-core (feature = `wasm`).
//!
//! Exposes a JavaScript-callable API via `wasm-bindgen`.  All functions
//! receive and return `Uint8Array` / `String` so they work from any JS
//! environment: browsers, Node.js, Deno, and WASI runtimes.
//!
//! # Supported operations
//!
//! | Function | Input | Output |
//! |----------|-------|--------|
//! | `decode_vp8_to_png` | VP8 keyframe bytes | PNG bytes |
//! | `decode_image_to_png` | PNG/JPEG bytes | PNG bytes |
//! | `resize_image` | PNG/JPEG bytes + w + h | PNG bytes |
//! | `apply_filter` | PNG/JPEG bytes + filter expr | PNG bytes |
//! | `probe_image` | PNG/JPEG bytes | JSON string |
//!
//! # Usage (from JS/TS)
//!
//! ```js
//! import init, { decode_vp8_to_png, resize_image } from './ferrox_core.js';
//! await init();
//! const png = decode_vp8_to_png(vp8Bytes);
//! const small = resize_image(pngBytes, 320, 240);
//! ```

use wasm_bindgen::prelude::*;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Convert a ferrox [`crate::Error`] to a `JsValue` (becomes a JS Error).
fn to_js(e: crate::Error) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Encode a [`crate::Frame`] as PNG bytes.
fn frame_to_png(frame: crate::Frame) -> Result<Vec<u8>, crate::Error> {
    use crate::codecs::png::PngEncoder;
    use crate::traits::Encoder;
    let mut buf = Vec::new();
    PngEncoder.encode(&frame, &mut buf)?;
    Ok(buf)
}

/// Decode PNG or JPEG bytes into a [`crate::Frame`].
fn decode_image(data: &[u8]) -> Result<crate::Frame, crate::Error> {
    use crate::error::Error;
    // Try PNG first, fall back to JPEG.
    use crate::codecs::{png::PngDecoder, jpeg::JpegDecoder};
    use crate::traits::Decoder;
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        PngDecoder.decode(std::io::Cursor::new(data))
    } else if data.starts_with(&[0xFF, 0xD8]) {
        JpegDecoder.decode(std::io::Cursor::new(data))
    } else {
        Err(Error::UnsupportedFormat("expected PNG or JPEG input".into()))
    }
}

// ── VP8 ───────────────────────────────────────────────────────────────────────

/// Decode a single VP8 keyframe and return it as PNG bytes.
///
/// Throws a JS Error on decode failure.
#[wasm_bindgen]
pub fn decode_vp8_to_png(vp8_data: &[u8]) -> Result<Vec<u8>, JsValue> {
    use crate::codecs::video::Vp8Decoder;
    use crate::traits::VideoDecoder;
    use crate::video::Packet;
    use crate::demux_graph::yuv420p_to_rgb8;

    let packet = Packet {
        data: vp8_data.to_vec(),
        pts: 0,
        duration: 0,
        is_keyframe: true,
    };

    let mut decoder = Vp8Decoder;
    let vf = decoder.decode_packet(&packet).map_err(to_js)?;

    // VP8 decoder outputs Yuv420p; convert to Rgb8 for PNG encoding.
    let rgb = yuv420p_to_rgb8(&vf.frame).map_err(to_js)?;
    frame_to_png(rgb).map_err(to_js)
}

// ── Image decode ──────────────────────────────────────────────────────────────

/// Decode PNG or JPEG bytes and re-encode as PNG.
///
/// Useful for normalising any image input to PNG in the browser.
#[wasm_bindgen]
pub fn decode_image_to_png(image_data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let frame = decode_image(image_data).map_err(to_js)?;
    frame_to_png(frame).map_err(to_js)
}

// ── Resize ────────────────────────────────────────────────────────────────────

/// Resize a PNG or JPEG image to `width × height` and return PNG bytes.
///
/// Uses Lanczos3 resampling.
#[wasm_bindgen]
pub fn resize_image(image_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, JsValue> {
    use crate::filters::ResizeFilter;
    use crate::traits::Filter;

    let frame = decode_image(image_data).map_err(to_js)?;
    let resized = ResizeFilter::new(width, height).process(frame).map_err(to_js)?;
    frame_to_png(resized).map_err(to_js)
}

// ── Filter graph ──────────────────────────────────────────────────────────────

/// Apply a ferrox filtergraph expression to a PNG or JPEG image.
///
/// Expression examples: `"blur=2.0"`, `"grayscale"`,
/// `"scale=640:480,brightness=20,contrast=1.2"`.
///
/// Returns PNG bytes.
#[wasm_bindgen]
pub fn apply_filter(image_data: &[u8], filter_expr: &str) -> Result<Vec<u8>, JsValue> {
    use crate::filter_graph::FilterGraph;

    let frame = decode_image(image_data).map_err(to_js)?;
    let out = FilterGraph::parse_and_run(frame, filter_expr).map_err(to_js)?;
    frame_to_png(out).map_err(to_js)
}

// ── Probe ─────────────────────────────────────────────────────────────────────

/// Return basic metadata about a PNG or JPEG image as a JSON string.
///
/// ```json
/// {"width":1920,"height":1080,"format":"png","channels":3}
/// ```
#[wasm_bindgen]
pub fn probe_image(image_data: &[u8]) -> Result<String, JsValue> {
    let frame = decode_image(image_data).map_err(to_js)?;

    let fmt = if image_data.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpeg" };
    let channels = match frame.format {
        crate::frame::PixelFormat::Rgb8     => 3,
        crate::frame::PixelFormat::Rgba8    => 4,
        crate::frame::PixelFormat::Yuv420p  => 3,
        _                                   => 0,
    };

    Ok(format!(
        r#"{{"width":{w},"height":{h},"format":"{fmt}","channels":{channels}}}"#,
        w = frame.width,
        h = frame.height,
    ))
}

// ── GIF ───────────────────────────────────────────────────────────────────────

/// Decode an animated GIF and return a flat array of PNG-encoded frames,
/// prefixed by a 4-byte big-endian frame count.
///
/// Layout: `[u32 frame_count][u32 len_0][bytes_0][u32 len_1][bytes_1]…`
#[cfg(feature = "gif-support")]
#[wasm_bindgen]
pub fn decode_gif_frames(gif_data: &[u8]) -> Result<Vec<u8>, JsValue> {
    use crate::codecs::gif::decode_gif;

    let frames = decode_gif(gif_data).map_err(to_js)?;
    let n = frames.len() as u32;

    let mut out = Vec::new();
    out.extend_from_slice(&n.to_be_bytes());
    for gf in &frames {
        let png = frame_to_png(gf.frame.clone()).map_err(to_js)?;
        out.extend_from_slice(&(png.len() as u32).to_be_bytes());
        out.extend_from_slice(&png);
    }
    Ok(out)
}

// ── Blur convenience ──────────────────────────────────────────────────────────

/// Gaussian-style blur a PNG or JPEG image.
///
/// `sigma` controls the blur radius (e.g. `2.0` for a gentle blur).
#[wasm_bindgen]
pub fn blur_image(image_data: &[u8], sigma: f32) -> Result<Vec<u8>, JsValue> {
    use crate::filters::BlurFilter;
    use crate::traits::Filter;

    let frame = decode_image(image_data).map_err(to_js)?;
    let blurred = BlurFilter::new(sigma).process(frame).map_err(to_js)?;
    frame_to_png(blurred).map_err(to_js)
}

// ── Grayscale convenience ─────────────────────────────────────────────────────

/// Convert a PNG or JPEG image to grayscale and return PNG bytes.
#[wasm_bindgen]
pub fn grayscale_image(image_data: &[u8]) -> Result<Vec<u8>, JsValue> {
    use crate::filters::GrayscaleFilter;
    use crate::traits::Filter;

    let frame = decode_image(image_data).map_err(to_js)?;
    let gray = GrayscaleFilter.process(frame).map_err(to_js)?;
    frame_to_png(gray).map_err(to_js)
}
