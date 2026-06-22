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
    /// Planar YUV 4:2:0, 8-bit (Y full-res, U/V half-res).
    Yuv420p,
    /// Planar YUV 4:2:0, 10-bit packed as little-endian u16 per sample.
    /// Data layout: Y plane (w*h u16s) + U plane + V plane (each w/2*h/2 u16s).
    Yuv420p10,
    /// Planar YUV 4:2:0, 12-bit packed as little-endian u16 per sample.
    Yuv420p12,
    /// Planar YUV 4:2:2, 8-bit (Y full-res, U/V half horizontal res).
    Yuv422p,
    /// Planar YUV 4:4:4, 8-bit (all planes full-res).
    Yuv444p,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb8    => 3,
            Self::Rgba8   => 4,
            Self::Gray8   => 1,
            Self::GrayA8  => 2,
            // Planar formats: use expected_data_len instead.
            Self::Yuv420p | Self::Yuv420p10 | Self::Yuv420p12
            | Self::Yuv422p | Self::Yuv444p => 0,
        }
    }

    /// Bytes per sample (1 for 8-bit, 2 for 10/12-bit packed in u16).
    pub fn bytes_per_sample(self) -> usize {
        match self {
            Self::Yuv420p10 | Self::Yuv420p12 => 2,
            _ => 1,
        }
    }

    /// Expected byte length of a packed/planar buffer for the given dimensions.
    pub fn expected_data_len(self, width: u32, height: u32) -> usize {
        let (w, h) = (width as usize, height as usize);
        let bps = self.bytes_per_sample();
        match self {
            Self::Yuv420p | Self::Yuv420p10 | Self::Yuv420p12 => {
                let uv_w = (w + 1) / 2;
                let uv_h = (h + 1) / 2;
                (w * h + 2 * uv_w * uv_h) * bps
            }
            Self::Yuv422p => {
                let uv_w = (w + 1) / 2;
                (w * h + 2 * uv_w * h) * bps
            }
            Self::Yuv444p => w * h * 3 * bps,
            other => other.bytes_per_pixel() * w * h,
        }
    }

    /// True for any HDR (>8-bit) format.
    pub fn is_hdr(self) -> bool {
        matches!(self, Self::Yuv420p10 | Self::Yuv420p12)
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
