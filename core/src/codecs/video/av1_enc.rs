use rav1e::{Config, Context, EncoderConfig, EncoderStatus};
use rav1e::color::ChromaSampling;
use crate::{
    error::{Error, Result},
    frame::PixelFormat,
    traits::VideoEncoder,
    video::{EncodedPacket, VideoFrame},
};

/// AV1 encoder backed by `rav1e`.
///
/// Accepts YUV420p [`VideoFrame`]s and produces AV1 OBU bitstream packets.
/// Speed ranges from 0 (slowest/best quality) to 10 (fastest/lowest quality).
pub struct Av1Encoder {
    ctx: Context<u8>,
    frame_index: u64,
}

impl Av1Encoder {
    /// Create a new encoder for frames of size `width × height`.
    ///
    /// `speed` maps directly to rav1e's speed preset (0–10).
    /// `quantizer` is the base quantizer (0 = lossless, 255 = worst).
    /// `fps_num / fps_den` is the nominal frame rate.
    pub fn new(width: u32, height: u32, speed: u8, quantizer: usize, fps_num: u64, fps_den: u64) -> Result<Self> {
        let mut cfg = EncoderConfig::with_speed_preset(speed);
        cfg.width = width as usize;
        cfg.height = height as usize;
        cfg.chroma_sampling = ChromaSampling::Cs420;
        cfg.quantizer = quantizer;
        // time_base: num = frame_duration_units, den = timescale
        // For 30 fps: num=1, den=30  (one frame = 1/30 sec)
        cfg.time_base.num = fps_den;
        cfg.time_base.den = fps_num;

        let enc_cfg = Config::new().with_encoder_config(cfg);
        let ctx: Context<u8> = enc_cfg.new_context()
            .map_err(|e| Error::Video(format!("rav1e config error: {e}")))?;

        Ok(Self { ctx, frame_index: 0 })
    }

    fn collect_ready_packets(&mut self) -> Result<Vec<EncodedPacket>> {
        let mut out = Vec::new();
        loop {
            match self.ctx.receive_packet() {
                Ok(pkt) => {
                    let is_keyframe = pkt.frame_type.all_intra();
                    out.push(EncodedPacket {
                        data: pkt.data,
                        pts: pkt.input_frameno,
                        duration: 1,
                        is_keyframe,
                        stream_index: 0,
                    });
                }
                Err(EncoderStatus::LimitReached) | Err(EncoderStatus::NeedMoreData) => break,
                Err(EncoderStatus::Encoded) => continue,
                Err(e) => return Err(Error::Video(format!("rav1e encode error: {e:?}"))),
            }
        }
        Ok(out)
    }
}

impl VideoEncoder for Av1Encoder {
    fn encode(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>> {
        if frame.format() != PixelFormat::Yuv420p {
            return Err(Error::Video(format!(
                "Av1Encoder expects Yuv420p input, got {:?}",
                frame.format()
            )));
        }

        let w = frame.width() as usize;
        let h = frame.height() as usize;
        let uv_w = (w + 1) / 2;
        let uv_h = (h + 1) / 2;

        let y_end = w * h;
        let u_end = y_end + uv_w * uv_h;
        let y_plane = &frame.frame.data[..y_end];
        let u_plane = &frame.frame.data[y_end..u_end];
        let v_plane = &frame.frame.data[u_end..];

        let mut rav1e_frame = self.ctx.new_frame();
        // stride == width for packed planar data
        rav1e_frame.planes[0].copy_from_raw_u8(y_plane, w, 1);
        rav1e_frame.planes[1].copy_from_raw_u8(u_plane, uv_w, 1);
        rav1e_frame.planes[2].copy_from_raw_u8(v_plane, uv_w, 1);

        self.ctx
            .send_frame(rav1e_frame)
            .map_err(|e| Error::Video(format!("rav1e send_frame: {e:?}")))?;

        self.frame_index += 1;
        self.collect_ready_packets()
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>> {
        self.ctx.flush();
        let mut out = Vec::new();
        loop {
            match self.ctx.receive_packet() {
                Ok(pkt) => {
                    let is_keyframe = pkt.frame_type.all_intra();
                    out.push(EncodedPacket {
                        data: pkt.data,
                        pts: pkt.input_frameno,
                        duration: 1,
                        is_keyframe,
                        stream_index: 0,
                    });
                }
                Err(EncoderStatus::LimitReached) => break,
                Err(EncoderStatus::Encoded) => continue,
                Err(EncoderStatus::NeedMoreData) => break,
                Err(e) => return Err(Error::Video(format!("rav1e flush error: {e:?}"))),
            }
        }
        Ok(out)
    }

    fn codec_name(&self) -> &str {
        "AV1"
    }
}
