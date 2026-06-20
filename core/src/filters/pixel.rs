use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::Filter,
};

fn require_rgb(frame: &Frame, op: &str) -> Result<()> {
    match frame.format {
        PixelFormat::Rgb8 | PixelFormat::Rgba8 => Ok(()),
        _ => Err(Error::Filter(format!(
            "{op} requires Rgb8 or Rgba8 input, got {:?}",
            frame.format
        ))),
    }
}

// ── Brightness ────────────────────────────────────────────────────────────────

/// Adjust brightness by adding `delta` to each RGB channel (clamped to 0–255).
pub struct BrightnessFilter {
    pub delta: i32,
}

impl BrightnessFilter {
    pub fn new(delta: i32) -> Self { Self { delta } }
}

impl Filter for BrightnessFilter {
    fn process(&self, mut frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "BrightnessFilter")?;
        for px in frame.data.chunks_exact_mut(frame.format.bytes_per_pixel()) {
            px[0] = (px[0] as i32 + self.delta).clamp(0, 255) as u8;
            px[1] = (px[1] as i32 + self.delta).clamp(0, 255) as u8;
            px[2] = (px[2] as i32 + self.delta).clamp(0, 255) as u8;
        }
        Ok(frame)
    }
}

// ── Contrast ──────────────────────────────────────────────────────────────────

/// Adjust contrast by `factor` (1.0 = no change, >1.0 = more contrast).
pub struct ContrastFilter {
    pub factor: f32,
}

impl ContrastFilter {
    pub fn new(factor: f32) -> Self { Self { factor } }
}

impl Filter for ContrastFilter {
    fn process(&self, mut frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "ContrastFilter")?;
        let f = self.factor;
        for px in frame.data.chunks_exact_mut(frame.format.bytes_per_pixel()) {
            px[0] = ((f * (px[0] as f32 - 128.0)) + 128.0).clamp(0.0, 255.0) as u8;
            px[1] = ((f * (px[1] as f32 - 128.0)) + 128.0).clamp(0.0, 255.0) as u8;
            px[2] = ((f * (px[2] as f32 - 128.0)) + 128.0).clamp(0.0, 255.0) as u8;
        }
        Ok(frame)
    }
}

// ── Saturation ────────────────────────────────────────────────────────────────

/// Adjust saturation by `factor` (0.0 = grayscale, 1.0 = no change, >1.0 = more vivid).
pub struct SaturationFilter {
    pub factor: f32,
}

impl SaturationFilter {
    pub fn new(factor: f32) -> Self { Self { factor } }
}

impl Filter for SaturationFilter {
    fn process(&self, mut frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "SaturationFilter")?;
        let f = self.factor;
        for px in frame.data.chunks_exact_mut(frame.format.bytes_per_pixel()) {
            let r = px[0] as f32;
            let g = px[1] as f32;
            let b = px[2] as f32;
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            px[0] = (luma + f * (r - luma)).clamp(0.0, 255.0) as u8;
            px[1] = (luma + f * (g - luma)).clamp(0.0, 255.0) as u8;
            px[2] = (luma + f * (b - luma)).clamp(0.0, 255.0) as u8;
        }
        Ok(frame)
    }
}

// ── Negate ────────────────────────────────────────────────────────────────────

/// Invert all RGB channels (255 - value). Alpha is preserved.
pub struct NegateFilter;

impl Filter for NegateFilter {
    fn process(&self, mut frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "NegateFilter")?;
        let bpp = frame.format.bytes_per_pixel();
        for px in frame.data.chunks_exact_mut(bpp) {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
            // leave alpha (px[3]) unchanged
        }
        Ok(frame)
    }
}

// ── Grayscale ─────────────────────────────────────────────────────────────────

/// Convert to grayscale using luminosity weights. Output remains Rgb8/Rgba8.
pub struct GrayscaleFilter;

impl Filter for GrayscaleFilter {
    fn process(&self, mut frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "GrayscaleFilter")?;
        let bpp = frame.format.bytes_per_pixel();
        for px in frame.data.chunks_exact_mut(bpp) {
            let luma = (0.2126 * px[0] as f32
                + 0.7152 * px[1] as f32
                + 0.0722 * px[2] as f32) as u8;
            px[0] = luma;
            px[1] = luma;
            px[2] = luma;
        }
        Ok(frame)
    }
}

// ── Flip ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum FlipAxis { Horizontal, Vertical }

/// Flip the frame horizontally or vertically.
pub struct FlipFilter {
    pub axis: FlipAxis,
}

impl FlipFilter {
    pub fn horizontal() -> Self { Self { axis: FlipAxis::Horizontal } }
    pub fn vertical() -> Self { Self { axis: FlipAxis::Vertical } }
}

impl Filter for FlipFilter {
    fn process(&self, frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "FlipFilter")?;
        use image::{DynamicImage, RgbImage, RgbaImage};
        let (w, h) = (frame.width, frame.height);
        let flipped = match frame.format {
            PixelFormat::Rgb8 => {
                let img = RgbImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGB8 buffer".into()))?;
                let d = DynamicImage::ImageRgb8(img);
                match self.axis {
                    FlipAxis::Horizontal => d.fliph().into_rgb8().into_raw(),
                    FlipAxis::Vertical => d.flipv().into_rgb8().into_raw(),
                }
            }
            PixelFormat::Rgba8 => {
                let img = RgbaImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGBA8 buffer".into()))?;
                let d = DynamicImage::ImageRgba8(img);
                match self.axis {
                    FlipAxis::Horizontal => d.fliph().into_rgba8().into_raw(),
                    FlipAxis::Vertical => d.flipv().into_rgba8().into_raw(),
                }
            }
            _ => unreachable!(),
        };
        Ok(Frame::new(w, h, frame.format, flipped))
    }
}

// ── Rotate ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum Rotation { Cw90, Cw180, Cw270 }

/// Rotate the frame by 90°, 180°, or 270° clockwise.
pub struct RotateFilter {
    pub rotation: Rotation,
}

impl RotateFilter {
    pub fn cw90() -> Self { Self { rotation: Rotation::Cw90 } }
    pub fn cw180() -> Self { Self { rotation: Rotation::Cw180 } }
    pub fn cw270() -> Self { Self { rotation: Rotation::Cw270 } }
}

impl Filter for RotateFilter {
    fn process(&self, frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "RotateFilter")?;
        use image::{DynamicImage, RgbImage, RgbaImage};
        let (w, h) = (frame.width, frame.height);
        let (nw, nh, data) = match frame.format {
            PixelFormat::Rgb8 => {
                let img = RgbImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGB8 buffer".into()))?;
                let d = DynamicImage::ImageRgb8(img);
                let rotated = match self.rotation {
                    Rotation::Cw90  => d.rotate90(),
                    Rotation::Cw180 => d.rotate180(),
                    Rotation::Cw270 => d.rotate270(),
                };
                let out = rotated.into_rgb8();
                (out.width(), out.height(), out.into_raw())
            }
            PixelFormat::Rgba8 => {
                let img = RgbaImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGBA8 buffer".into()))?;
                let d = DynamicImage::ImageRgba8(img);
                let rotated = match self.rotation {
                    Rotation::Cw90  => d.rotate90(),
                    Rotation::Cw180 => d.rotate180(),
                    Rotation::Cw270 => d.rotate270(),
                };
                let out = rotated.into_rgba8();
                (out.width(), out.height(), out.into_raw())
            }
            _ => unreachable!(),
        };
        Ok(Frame::new(nw, nh, frame.format, data))
    }
}

// ── Crop ──────────────────────────────────────────────────────────────────────

/// Crop a rectangular region from the frame.
pub struct CropFilter {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropFilter {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }
}

impl Filter for CropFilter {
    fn process(&self, frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "CropFilter")?;
        if self.x + self.width > frame.width || self.y + self.height > frame.height {
            return Err(Error::Filter(format!(
                "crop region ({},{} {}×{}) exceeds frame {}×{}",
                self.x, self.y, self.width, self.height, frame.width, frame.height
            )));
        }
        use image::{DynamicImage, GenericImageView, RgbImage, RgbaImage};
        let (w, h) = (frame.width, frame.height);
        let cropped = match frame.format {
            PixelFormat::Rgb8 => {
                let img = RgbImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGB8 buffer".into()))?;
                DynamicImage::ImageRgb8(img)
                    .crop_imm(self.x, self.y, self.width, self.height)
                    .into_rgb8().into_raw()
            }
            PixelFormat::Rgba8 => {
                let img = RgbaImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGBA8 buffer".into()))?;
                DynamicImage::ImageRgba8(img)
                    .crop_imm(self.x, self.y, self.width, self.height)
                    .into_rgba8().into_raw()
            }
            _ => unreachable!(),
        };
        Ok(Frame::new(self.width, self.height, frame.format, cropped))
    }
}

// ── Blur ──────────────────────────────────────────────────────────────────────

/// Gaussian blur with `sigma` radius (uses the `image` crate's built-in blur).
pub struct BlurFilter {
    pub sigma: f32,
}

impl BlurFilter {
    pub fn new(sigma: f32) -> Self { Self { sigma } }
}

impl Filter for BlurFilter {
    fn process(&self, frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "BlurFilter")?;
        use image::{DynamicImage, RgbImage, RgbaImage};
        let (w, h) = (frame.width, frame.height);
        let blurred: Vec<u8> = match frame.format {
            PixelFormat::Rgb8 => {
                let img = RgbImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGB8 buffer".into()))?;
                DynamicImage::ImageRgb8(img)
                    .blur(self.sigma).into_rgb8().into_raw()
            }
            PixelFormat::Rgba8 => {
                let img = RgbaImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGBA8 buffer".into()))?;
                DynamicImage::ImageRgba8(img)
                    .blur(self.sigma).into_rgba8().into_raw()
            }
            _ => unreachable!(),
        };
        Ok(Frame::new(w, h, frame.format, blurred))
    }
}

// ── Thumbnail ─────────────────────────────────────────────────────────────────

/// Scale the frame to fit within `max_width × max_height`, preserving aspect ratio.
/// If `crop_to_fit` is true, the output will be exactly `max_width × max_height`
/// with the excess cropped from the centre.
pub struct ThumbnailFilter {
    pub max_width: u32,
    pub max_height: u32,
    pub crop_to_fit: bool,
}

impl ThumbnailFilter {
    pub fn new(max_width: u32, max_height: u32) -> Self {
        Self { max_width, max_height, crop_to_fit: false }
    }

    pub fn with_crop(mut self) -> Self { self.crop_to_fit = true; self }
}

impl Filter for ThumbnailFilter {
    fn process(&self, frame: Frame) -> Result<Frame> {
        require_rgb(&frame, "ThumbnailFilter")?;
        use image::{DynamicImage, imageops::FilterType, RgbImage, RgbaImage};
        let (w, h) = (frame.width, frame.height);
        let (tw, th) = (self.max_width, self.max_height);

        let dyn_img: DynamicImage = match frame.format {
            PixelFormat::Rgb8 => {
                let img = RgbImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGB8 buffer".into()))?;
                DynamicImage::ImageRgb8(img)
            }
            PixelFormat::Rgba8 => {
                let img = RgbaImage::from_raw(w, h, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGBA8 buffer".into()))?;
                DynamicImage::ImageRgba8(img)
            }
            _ => unreachable!(),
        };

        let scaled = if self.crop_to_fit {
            // Scale so the smallest dimension fits, then crop excess.
            let scale = (tw as f32 / w as f32).max(th as f32 / h as f32);
            let sw = (w as f32 * scale) as u32;
            let sh = (h as f32 * scale) as u32;
            let resized = dyn_img.resize_exact(sw, sh, FilterType::Lanczos3);
            let ox = (sw.saturating_sub(tw)) / 2;
            let oy = (sh.saturating_sub(th)) / 2;
            resized.crop_imm(ox, oy, tw, th)
        } else {
            dyn_img.resize(tw, th, FilterType::Lanczos3)
        };

        let (ow, oh) = (scaled.width(), scaled.height());
        let (fmt, data) = match frame.format {
            PixelFormat::Rgb8  => (PixelFormat::Rgb8,  scaled.into_rgb8().into_raw()),
            PixelFormat::Rgba8 => (PixelFormat::Rgba8, scaled.into_rgba8().into_raw()),
            _ => unreachable!(),
        };
        Ok(Frame::new(ow, oh, fmt, data))
    }
}
