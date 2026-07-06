//! Colorspace conversions between planar YUV and packed RGB.

use crate::error::{Error, Result};
use crate::frame::{Frame, PixelFormat};

pub fn any_yuv_to_rgb8(frame: &Frame) -> Result<Frame> {
    match frame.format {
        PixelFormat::Rgb8    => Ok(frame.clone()),
        PixelFormat::Yuv420p => yuv420p_to_rgb8(frame),
        PixelFormat::Yuv420p10 | PixelFormat::Yuv420p12 => yuv420p_hdr_to_rgb8(frame),
        PixelFormat::Yuv422p => yuv422p_to_rgb8(frame),
        PixelFormat::Yuv444p => yuv444p_to_rgb8(frame),
        fmt => Err(Error::Filter(format!("unsupported pixel format for RGB conversion: {fmt:?}"))),
    }
}

/// BT.601 full-range YUV420p (8-bit) → packed RGB8.
pub fn yuv420p_to_rgb8(frame: &Frame) -> Result<Frame> {
    if frame.format != PixelFormat::Yuv420p {
        return Err(Error::Filter("yuv420p_to_rgb8 expects Yuv420p input".into()));
    }
    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = (w + 1) / 2;
    let uv_h = (h + 1) / 2;
    let y_plane = &frame.data[..w * h];
    let u_plane = &frame.data[w * h..w * h + uv_w * uv_h];
    let v_plane = &frame.data[w * h + uv_w * uv_h..];

    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col] as f32;
            let u = u_plane[(row / 2) * uv_w + col / 2] as f32 - 128.0;
            let v = v_plane[(row / 2) * uv_w + col / 2] as f32 - 128.0;
            yuv_to_rgb_pixel(&mut rgb, row * w + col, y, u, v);
        }
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// Convert an `Rgb8`/`Rgba8` frame to packed planar `Yuv420p` (Y then U then V),
/// using full-range BT.601 (JPEG) coefficients — the exact inverse of
/// [`yuv420p_to_rgb8`]. Chroma is 4:2:0 subsampled by averaging each 2×2 block.
///
/// This is the encode-side companion to the decoders' YUV→RGB path: the
/// compositor renders RGBA, and the exporter feeds `rav1e` YUV420p.
pub fn rgb8_to_yuv420p(frame: &Frame) -> Result<Frame> {
    let bpp = match frame.format {
        PixelFormat::Rgb8 => 3usize,
        PixelFormat::Rgba8 => 4usize,
        other => {
            return Err(Error::Filter(format!("rgb8_to_yuv420p expects Rgb8/Rgba8, got {other:?}")))
        }
    };
    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = w.div_ceil(2);
    let uv_h = h.div_ceil(2);

    let mut y_plane = vec![0u8; w * h];
    let mut u_plane = vec![0u8; uv_w * uv_h];
    let mut v_plane = vec![0u8; uv_w * uv_h];

    // Luma for every pixel.
    for row in 0..h {
        for col in 0..w {
            let i = (row * w + col) * bpp;
            let r = frame.data[i] as f32;
            let g = frame.data[i + 1] as f32;
            let b = frame.data[i + 2] as f32;
            y_plane[row * w + col] = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0) as u8;
        }
    }

    // Chroma: average each 2×2 block (clamped at edges).
    for by in 0..uv_h {
        for bx in 0..uv_w {
            let (mut cb, mut cr, mut n) = (0.0f32, 0.0f32, 0.0f32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let row = (by * 2 + dy).min(h - 1);
                    let col = (bx * 2 + dx).min(w - 1);
                    let i = (row * w + col) * bpp;
                    let r = frame.data[i] as f32;
                    let g = frame.data[i + 1] as f32;
                    let b = frame.data[i + 2] as f32;
                    cb += -0.168736 * r - 0.331264 * g + 0.5 * b + 128.0;
                    cr += 0.5 * r - 0.418688 * g - 0.081312 * b + 128.0;
                    n += 1.0;
                }
            }
            u_plane[by * uv_w + bx] = (cb / n).round().clamp(0.0, 255.0) as u8;
            v_plane[by * uv_w + bx] = (cr / n).round().clamp(0.0, 255.0) as u8;
        }
    }

    let mut data = y_plane;
    data.extend_from_slice(&u_plane);
    data.extend_from_slice(&v_plane);
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Yuv420p, data))
}

/// BT.2020 10/12-bit YUV420p → 8-bit RGB8 (tone-mapped by bit-shift).
pub fn yuv420p_hdr_to_rgb8(frame: &Frame) -> Result<Frame> {
    let depth = match frame.format {
        PixelFormat::Yuv420p10 => 10u32,
        PixelFormat::Yuv420p12 => 12u32,
        _ => return Err(Error::Filter("yuv420p_hdr_to_rgb8 expects Yuv420p10/12".into())),
    };
    let shift = depth - 8; // shift down to 8-bit range
    let bias  = (1u16 << (depth - 1)) as f32; // 512 for 10-bit, 2048 for 12-bit

    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = (w + 1) / 2;
    let uv_h = (h + 1) / 2;

    // Samples are little-endian u16.
    let read_u16 = |data: &[u8], idx: usize| -> f32 {
        let b0 = data[idx * 2] as u16;
        let b1 = data[idx * 2 + 1] as u16;
        ((b0 | (b1 << 8)) >> shift) as f32
    };

    let y_end  = w * h * 2;
    let u_end  = y_end + uv_w * uv_h * 2;
    let y_plane = &frame.data[..y_end];
    let u_plane = &frame.data[y_end..u_end];
    let v_plane = &frame.data[u_end..];

    let bias8 = (bias as u32 >> shift) as f32;

    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let y = read_u16(y_plane, row * w + col);
            let u = read_u16(u_plane, (row / 2) * uv_w + col / 2) - bias8;
            let v = read_u16(v_plane, (row / 2) * uv_w + col / 2) - bias8;
            yuv_to_rgb_pixel(&mut rgb, row * w + col, y, u, v);
        }
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// BT.601 YUV422p (8-bit) → RGB8.
pub fn yuv422p_to_rgb8(frame: &Frame) -> Result<Frame> {
    if frame.format != PixelFormat::Yuv422p {
        return Err(Error::Filter("yuv422p_to_rgb8 expects Yuv422p input".into()));
    }
    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = (w + 1) / 2;
    let y_plane = &frame.data[..w * h];
    let u_plane = &frame.data[w * h..w * h + uv_w * h];
    let v_plane = &frame.data[w * h + uv_w * h..];

    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col] as f32;
            let u = u_plane[row * uv_w + col / 2] as f32 - 128.0;
            let v = v_plane[row * uv_w + col / 2] as f32 - 128.0;
            yuv_to_rgb_pixel(&mut rgb, row * w + col, y, u, v);
        }
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// BT.601 YUV444p (8-bit) → RGB8.
pub fn yuv444p_to_rgb8(frame: &Frame) -> Result<Frame> {
    if frame.format != PixelFormat::Yuv444p {
        return Err(Error::Filter("yuv444p_to_rgb8 expects Yuv444p input".into()));
    }
    let w = frame.width as usize;
    let h = frame.height as usize;
    let n = w * h;
    let y_plane = &frame.data[..n];
    let u_plane = &frame.data[n..2 * n];
    let v_plane = &frame.data[2 * n..];

    let mut rgb = vec![0u8; n * 3];
    for i in 0..n {
        let y = y_plane[i] as f32;
        let u = u_plane[i] as f32 - 128.0;
        let v = v_plane[i] as f32 - 128.0;
        yuv_to_rgb_pixel(&mut rgb, i, y, u, v);
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// BT.601 YUV → RGB conversion for one pixel, written into `rgb` at `pixel_idx`.
#[inline(always)]
fn yuv_to_rgb_pixel(rgb: &mut [u8], pixel_idx: usize, y: f32, u: f32, v: f32) {
    let r = (y + 1.402 * v).clamp(0.0, 255.0) as u8;
    let g = (y - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8;
    let b = (y + 1.772 * u).clamp(0.0, 255.0) as u8;
    let off = pixel_idx * 3;
    rgb[off]     = r;
    rgb[off + 1] = g;
    rgb[off + 2] = b;
}
