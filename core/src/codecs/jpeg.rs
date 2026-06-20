use std::io::{Cursor, Read, Write};
use image::{ImageDecoder, ImageEncoder};
use image::codecs::jpeg::{JpegDecoder as ImgJpegDecoder, JpegEncoder as ImgJpegEncoder};

use crate::{
    error::Result,
    frame::{Frame, PixelFormat},
    traits::{Decoder, Encoder},
};

pub struct JpegDecoder;

/// `quality` is 1–100 (default 85).
pub struct JpegEncoder {
    pub quality: u8,
}

impl Default for JpegEncoder {
    fn default() -> Self { Self { quality: 85 } }
}

impl Decoder for JpegDecoder {
    fn decode<R: Read>(&self, mut reader: R) -> Result<Frame> {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        let cursor = Cursor::new(buf);

        let dec = ImgJpegDecoder::new(cursor)?;
        let (width, height) = dec.dimensions();
        let color = dec.color_type();

        let (format, bpp): (PixelFormat, usize) = match color {
            image::ColorType::Rgb8 => (PixelFormat::Rgb8, 3),
            image::ColorType::L8 => (PixelFormat::Gray8, 1),
            _ => (PixelFormat::Rgb8, 3),
        };
        let mut data = vec![0u8; bpp * width as usize * height as usize];
        dec.read_image(&mut data)?;
        Ok(Frame::new(width, height, format, data))
    }
}

impl Encoder for JpegEncoder {
    fn encode<W: Write>(&self, frame: &Frame, writer: W) -> Result<()> {
        let color = match frame.format {
            PixelFormat::Rgb8 => image::ColorType::Rgb8,
            PixelFormat::Gray8 => image::ColorType::L8,
            _ => return Err(crate::error::Error::UnsupportedPixelFormat),
        };
        let enc = ImgJpegEncoder::new_with_quality(writer, self.quality);
        enc.write_image(&frame.data, frame.width, frame.height, color.into())?;
        Ok(())
    }
}
