use image::{imageops::FilterType, DynamicImage, RgbImage, RgbaImage, GrayImage, GrayAlphaImage};
use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::Filter,
};

/// Resampling algorithm used by [`ResizeFilter`].
#[derive(Debug, Clone, Copy, Default)]
pub enum ResizeAlgorithm {
    Nearest,
    #[default]
    Lanczos3,
    CatmullRom,
    Gaussian,
    Triangle,
}

impl From<ResizeAlgorithm> for FilterType {
    fn from(a: ResizeAlgorithm) -> Self {
        match a {
            ResizeAlgorithm::Nearest => FilterType::Nearest,
            ResizeAlgorithm::Lanczos3 => FilterType::Lanczos3,
            ResizeAlgorithm::CatmullRom => FilterType::CatmullRom,
            ResizeAlgorithm::Gaussian => FilterType::Gaussian,
            ResizeAlgorithm::Triangle => FilterType::Triangle,
        }
    }
}

/// Scales a frame to `target_width × target_height`, preserving pixel format.
pub struct ResizeFilter {
    pub target_width: u32,
    pub target_height: u32,
    pub algorithm: ResizeAlgorithm,
}

impl ResizeFilter {
    pub fn new(target_width: u32, target_height: u32) -> Self {
        Self { target_width, target_height, algorithm: ResizeAlgorithm::default() }
    }

    pub fn with_algorithm(mut self, algorithm: ResizeAlgorithm) -> Self {
        self.algorithm = algorithm;
        self
    }
}

impl Filter for ResizeFilter {
    fn process(&self, frame: Frame) -> Result<Frame> {
        if self.target_width == 0 || self.target_height == 0 {
            return Err(Error::InvalidDimensions {
                width: self.target_width,
                height: self.target_height,
            });
        }

        let filter: FilterType = self.algorithm.into();
        let (tw, th) = (self.target_width, self.target_height);

        let resized: DynamicImage = match frame.format {
            PixelFormat::Rgb8 => {
                let img = RgbImage::from_raw(frame.width, frame.height, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGB8 buffer".into()))?;
                DynamicImage::ImageRgb8(img).resize_exact(tw, th, filter)
            }
            PixelFormat::Rgba8 => {
                let img = RgbaImage::from_raw(frame.width, frame.height, frame.data)
                    .ok_or_else(|| Error::Filter("invalid RGBA8 buffer".into()))?;
                DynamicImage::ImageRgba8(img).resize_exact(tw, th, filter)
            }
            PixelFormat::Gray8 => {
                let img = GrayImage::from_raw(frame.width, frame.height, frame.data)
                    .ok_or_else(|| Error::Filter("invalid Gray8 buffer".into()))?;
                DynamicImage::ImageLuma8(img).resize_exact(tw, th, filter)
            }
            PixelFormat::GrayA8 => {
                let img = GrayAlphaImage::from_raw(frame.width, frame.height, frame.data)
                    .ok_or_else(|| Error::Filter("invalid GrayA8 buffer".into()))?;
                DynamicImage::ImageLumaA8(img).resize_exact(tw, th, filter)
            }
            PixelFormat::Yuv420p
            | PixelFormat::Yuv420p10 | PixelFormat::Yuv420p12
            | PixelFormat::Yuv422p   | PixelFormat::Yuv444p => {
                return Err(Error::Filter(
                    "ResizeFilter does not support planar YUV formats; convert to RGB8 first".into(),
                ))
            }
        };

        let (out_fmt, out_data) = dynamic_to_frame_data(resized, frame.format);
        Ok(Frame::new(tw, th, out_fmt, out_data))
    }
}

fn dynamic_to_frame_data(img: DynamicImage, original: PixelFormat) -> (PixelFormat, Vec<u8>) {
    match original {
        PixelFormat::Rgb8 => (PixelFormat::Rgb8, img.into_rgb8().into_raw()),
        PixelFormat::Rgba8 => (PixelFormat::Rgba8, img.into_rgba8().into_raw()),
        PixelFormat::Gray8 => (PixelFormat::Gray8, img.into_luma8().into_raw()),
        PixelFormat::GrayA8 => (PixelFormat::GrayA8, img.into_luma_alpha8().into_raw()),
        _ => unreachable!(),
    }
}
