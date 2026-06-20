use std::io::{Cursor, Read, Write};
use image::{ImageDecoder, ImageEncoder};
use image::codecs::png::{PngDecoder as ImgPngDecoder, PngEncoder as ImgPngEncoder};

use crate::{
    error::Result,
    frame::{Frame, PixelFormat},
    traits::{Decoder, Encoder},
};

pub struct PngDecoder;
pub struct PngEncoder;

impl Decoder for PngDecoder {
    fn decode<R: Read>(&self, mut reader: R) -> Result<Frame> {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        let cursor = Cursor::new(buf);

        let dec = ImgPngDecoder::new(cursor)?;
        let (width, height) = dec.dimensions();
        let color = dec.color_type();

        let (format, bpp): (PixelFormat, usize) = color_to_pixel_format(color);
        let mut data = vec![0u8; bpp * width as usize * height as usize];
        dec.read_image(&mut data)?;
        Ok(Frame::new(width, height, format, data))
    }
}

impl Encoder for PngEncoder {
    fn encode<W: Write>(&self, frame: &Frame, writer: W) -> Result<()> {
        let color = pixel_format_to_color(frame.format)?;
        let enc = ImgPngEncoder::new(writer);
        enc.write_image(&frame.data, frame.width, frame.height, color.into())?;
        Ok(())
    }
}

fn color_to_pixel_format(color: image::ColorType) -> (PixelFormat, usize) {
    match color {
        image::ColorType::Rgb8 => (PixelFormat::Rgb8, 3),
        image::ColorType::Rgba8 => (PixelFormat::Rgba8, 4),
        image::ColorType::L8 => (PixelFormat::Gray8, 1),
        image::ColorType::La8 => (PixelFormat::GrayA8, 2),
        _ => (PixelFormat::Rgb8, 3),
    }
}

fn pixel_format_to_color(fmt: PixelFormat) -> Result<image::ColorType> {
    match fmt {
        PixelFormat::Rgb8 => Ok(image::ColorType::Rgb8),
        PixelFormat::Rgba8 => Ok(image::ColorType::Rgba8),
        PixelFormat::Gray8 => Ok(image::ColorType::L8),
        PixelFormat::GrayA8 => Ok(image::ColorType::La8),
        _ => Err(crate::error::Error::UnsupportedPixelFormat),
    }
}
