use oxideav_vp8::decode_vp8;
use crate::{
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::VideoDecoder,
    video::{Packet, VideoFrame},
};

/// Pure-Rust VP8 decoder backed by `oxideav-vp8`.
///
/// # Limitations
///
/// `oxideav-vp8` currently supports **keyframes only**. Inter-frames
/// (P-frames) return [`Error::Video`] with an `Unsupported` message.
/// Full inter-frame support is tracked upstream.
pub struct Vp8Decoder;

impl VideoDecoder for Vp8Decoder {
    fn decode_packet(&mut self, packet: &Packet) -> Result<VideoFrame> {
        let decoded = decode_vp8(&packet.data)
            .map_err(|e| Error::Video(e.to_string()))?;

        // oxideav-vp8 emits planar YUV 4:2:0.  Pack into our Yuv420p Frame.
        let w = decoded.width;
        let h = decoded.height;
        let mut data = Vec::with_capacity(PixelFormat::Yuv420p.expected_data_len(w, h));
        data.extend_from_slice(&decoded.y);
        data.extend_from_slice(&decoded.u);
        data.extend_from_slice(&decoded.v);

        let frame = Frame::new(w, h, PixelFormat::Yuv420p, data);
        Ok(VideoFrame::new(frame, packet.pts, packet.duration, packet.is_keyframe))
    }
}
