//! GIF decoder and encoder using the `gif` crate + `color_quant` for palette
//! quantisation (both pure Rust).
//!
//! Animated GIFs are decoded into a `Vec<GifFrame>` containing an Rgb8
//! [`Frame`] and the inter-frame delay in centiseconds.

use std::io::{Read, Write};
use crate::{error::{Error, Result}, frame::{Frame, PixelFormat}};

/// A single GIF frame with its display delay.
#[derive(Debug, Clone)]
pub struct GifFrame {
    /// Decoded pixels in Rgb8 format.
    pub frame: Frame,
    /// Frame delay in centiseconds (1/100 s). 0 means "as fast as possible".
    pub delay_cs: u16,
}

impl GifFrame {
    /// Delay as a floating-point number of seconds.
    pub fn delay_secs(&self) -> f64 {
        self.delay_cs as f64 / 100.0
    }
}

// ── Decoder ───────────────────────────────────────────────────────────────────

/// Decode all frames from a GIF byte stream.
pub fn decode_gif<R: Read>(reader: R) -> Result<Vec<GifFrame>> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(reader)
        .map_err(|e| Error::UnsupportedFormat(format!("gif decode error: {e}")))?;

    let screen_w = decoder.width() as u32;
    let screen_h = decoder.height() as u32;

    // Maintain a compositing canvas in RGBA.
    let mut canvas = vec![0u8; screen_w as usize * screen_h as usize * 4];
    let mut frames: Vec<GifFrame> = Vec::new();

    while let Some(gif_frame) = decoder.read_next_frame()
        .map_err(|e| Error::UnsupportedFormat(format!("gif frame error: {e}")))?
    {
        let delay_cs = gif_frame.delay;
        let fw = gif_frame.width  as usize;
        let fh = gif_frame.height as usize;
        let fx = gif_frame.left   as usize;
        let fy = gif_frame.top    as usize;

        // Blit this frame's RGBA buffer onto the canvas.
        let buf = &gif_frame.buffer;
        for row in 0..fh {
            for col in 0..fw {
                let src = (row * fw + col) * 4;
                let alpha = buf[src + 3];
                if alpha == 0 { continue; }
                let dst = ((fy + row) * screen_w as usize + (fx + col)) * 4;
                if dst + 3 >= canvas.len() { continue; }
                canvas[dst]     = buf[src];
                canvas[dst + 1] = buf[src + 1];
                canvas[dst + 2] = buf[src + 2];
                canvas[dst + 3] = 255;
            }
        }

        // Convert composited canvas RGBA → Rgb8.
        let rgb: Vec<u8> = canvas.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        frames.push(GifFrame {
            frame: Frame::new(screen_w, screen_h, PixelFormat::Rgb8, rgb),
            delay_cs,
        });
    }

    if frames.is_empty() {
        return Err(Error::UnsupportedFormat("GIF contained no frames".into()));
    }
    Ok(frames)
}

// ── Encoder ───────────────────────────────────────────────────────────────────

/// Options for GIF encoding.
#[derive(Debug, Clone)]
pub struct GifEncodeOptions {
    /// Number of colours in the palette (2–256, must be power-of-two or will
    /// be rounded up).
    pub palette_size: usize,
    /// Loop count: 0 = infinite loop, N = loop N times.
    pub repeat: gif::Repeat,
    /// Default frame delay in centiseconds when a frame doesn't specify one.
    pub default_delay_cs: u16,
}

impl Default for GifEncodeOptions {
    fn default() -> Self {
        Self {
            palette_size: 256,
            repeat: gif::Repeat::Infinite,
            default_delay_cs: 10,
        }
    }
}

/// Encode a sequence of `GifFrame`s into GIF byte stream.
pub fn encode_gif<W: Write>(
    writer: W,
    frames: &[GifFrame],
    opts: &GifEncodeOptions,
) -> Result<()> {
    if frames.is_empty() {
        return Err(Error::Filter("encode_gif: no frames provided".into()));
    }

    let w = frames[0].frame.width as u16;
    let h = frames[0].frame.height as u16;

    let mut encoder = gif::Encoder::new(writer, w, h, &[])
        .map_err(|e| Error::Filter(format!("gif encoder init: {e}")))?;
    encoder.set_repeat(opts.repeat)
        .map_err(|e| Error::Filter(format!("gif set_repeat: {e}")))?;

    let palette_size = opts.palette_size.clamp(2, 256);

    for gif_frame in frames {
        let fw = gif_frame.frame.width;
        let fh = gif_frame.frame.height;
        // color_quant requires RGBA (4-byte pixels).
        let rgba: Vec<u8> = match gif_frame.frame.format {
            PixelFormat::Rgb8 => gif_frame.frame.data.chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255u8]).collect(),
            PixelFormat::Rgba8 => gif_frame.frame.data.clone(),
            _ => return Err(Error::Filter(format!(
                "encode_gif: frame must be Rgb8/Rgba8, got {:?}", gif_frame.frame.format
            ))),
        };

        // Quantise to palette.
        let quantiser = color_quant::NeuQuant::new(10, palette_size, &rgba);
        let palette: Vec<u8> = quantiser.color_map_rgb();
        let indices: Vec<u8> = rgba.chunks_exact(4)
            .map(|p| quantiser.index_of(p) as u8)
            .collect();

        let mut frame = gif::Frame::default();
        frame.width   = fw as u16;
        frame.height  = fh as u16;
        frame.delay   = if gif_frame.delay_cs > 0 { gif_frame.delay_cs } else { opts.default_delay_cs };
        frame.palette = Some(palette);
        frame.buffer  = std::borrow::Cow::Owned(indices);

        encoder.write_frame(&frame)
            .map_err(|e| Error::Filter(format!("gif write_frame: {e}")))?;
    }

    Ok(())
}
