use crate::traits::Format;

pub struct PngFormat;
pub struct JpegFormat;

impl Format for PngFormat {
    fn name(&self) -> &str { "PNG" }
    fn extensions(&self) -> &[&str] { &["png"] }
    fn mime_type(&self) -> &str { "image/png" }
}

impl Format for JpegFormat {
    fn name(&self) -> &str { "JPEG" }
    fn extensions(&self) -> &[&str] { &["jpg", "jpeg"] }
    fn mime_type(&self) -> &str { "image/jpeg" }
}
