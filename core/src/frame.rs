/// Pixel layout of raw frame data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8-bit red, green, blue — 3 bytes per pixel, row-major.
    Rgb8,
    /// 8-bit red, green, blue, alpha — 4 bytes per pixel, row-major.
    Rgba8,
    /// 8-bit grayscale — 1 byte per pixel, row-major.
    Gray8,
    /// 8-bit grayscale + alpha — 2 bytes per pixel, row-major.
    GrayA8,
    /// Planar YUV 4:2:0 (Y plane full-res, U/V planes half-res).
    Yuv420p,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
            Self::Gray8 => 1,
            Self::GrayA8 => 2,
            // YUV420p is planar; stride depends on component — use expected_data_len.
            Self::Yuv420p => 0,
        }
    }

    /// Expected byte length of a packed/planar buffer for the given dimensions.
    pub fn expected_data_len(self, width: u32, height: u32) -> usize {
        let (w, h) = (width as usize, height as usize);
        match self {
            Self::Yuv420p => w * h + 2 * ((w + 1) / 2) * ((h + 1) / 2),
            other => other.bytes_per_pixel() * w * h,
        }
    }
}

/// A single decoded image frame held in CPU memory.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Self {
        Self { width, height, format, data }
    }

    /// Stride in bytes for a single row (packed formats only).
    pub fn row_stride(&self) -> usize {
        self.format.bytes_per_pixel() * self.width as usize
    }
}
