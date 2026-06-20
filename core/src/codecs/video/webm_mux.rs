use std::io::{BufWriter, Write};
use oxideav_mkv::mux::open_webm;
use oxideav_core::{
    Muxer as OxMuxer, Packet as OxPacket, Rational,
    StreamInfo as OxStreamInfo, TimeBase, WriteSeek,
};
use oxideav_core::packet::PacketFlags;
use oxideav_core::stream::{CodecId as OxCodecId, CodecParameters};
use crate::{
    error::{Error, Result},
    traits::ContainerMuxer,
    video::{EncodedPacket, StreamInfo},
};

/// WebM muxer wrapping `oxideav-mkv`.
///
/// Writes AV1-in-WebM (DocType = "webm").
pub struct WebmMuxer {
    inner: Box<dyn OxMuxer>,
    time_base: (u64, u64),
}

impl WebmMuxer {
    /// Create a new muxer writing to `output`.
    ///
    /// `streams` describes the ferrox-core streams to include.
    /// `fps_num / fps_den` sets the container time base.
    pub fn new<W: Write + std::io::Seek + Send + 'static>(
        output: W,
        streams: &[StreamInfo],
        fps_num: u64,
        fps_den: u64,
    ) -> Result<Self> {
        let ox_streams: Vec<OxStreamInfo> = streams
            .iter()
            .map(|s| {
                let codec_id = OxCodecId::new(codec_id_string(&s.codec));
                let mut params = if s.is_video() {
                    let mut p = CodecParameters::video(codec_id);
                    p.width = Some(s.width);
                    p.height = Some(s.height);
                    p.frame_rate = Some(Rational::new(fps_num as i64, fps_den as i64));
                    p
                } else {
                    let mut p = CodecParameters::audio(codec_id);
                    p.sample_rate = Some(s.sample_rate);
                    p.channels = Some(s.channels as u16);
                    p
                };
                let _ = &mut params; // suppress unused mut warning
                OxStreamInfo {
                    index: s.index as u32,
                    time_base: TimeBase(Rational::new(fps_den as i64, fps_num as i64)),
                    duration: None,
                    start_time: None,
                    params,
                }
            })
            .collect();

        let boxed_writer: Box<dyn WriteSeek> = Box::new(BufWriter::new(output));
        let inner = open_webm(boxed_writer, &ox_streams)
            .map_err(|e| Error::Video(format!("webm muxer open: {e}")))?;

        Ok(Self { inner, time_base: (fps_num, fps_den) })
    }
}

impl ContainerMuxer for WebmMuxer {
    fn write_header(&mut self) -> Result<()> {
        self.inner.write_header()
            .map_err(|e| Error::Video(format!("webm write_header: {e}")))
    }

    fn write_packet(&mut self, packet: &EncodedPacket) -> Result<()> {
        let (fps_num, fps_den) = self.time_base;
        let time_base = TimeBase(Rational::new(fps_den as i64, fps_num as i64));
        let flags = PacketFlags {
            keyframe: packet.is_keyframe,
            header: false,
            corrupt: false,
            discard: false,
            unit_boundary: false,
        };
        let ox_pkt = OxPacket {
            stream_index: packet.stream_index as u32,
            time_base,
            pts: Some(packet.pts as i64),
            dts: Some(packet.pts as i64),
            duration: Some(packet.duration as i64),
            data: packet.data.clone(),
            flags,
        };
        self.inner.write_packet(&ox_pkt)
            .map_err(|e| Error::Video(format!("webm write_packet: {e}")))
    }

    fn write_trailer(&mut self) -> Result<()> {
        self.inner.write_trailer()
            .map_err(|e| Error::Video(format!("webm write_trailer: {e}")))
    }
}

/// Map ferrox `CodecId` to oxideav-core codec ID strings.
///
/// `oxideav-mkv` uses lowercase short IDs internally and converts them to
/// Matroska track codec IDs (e.g., "av1" → "V_AV1") when writing the
/// container. The WebM whitelist also checks against these short IDs.
fn codec_id_string(codec: &crate::video::CodecId) -> String {
    match codec {
        crate::video::CodecId::Av1 => "av1".to_owned(),
        crate::video::CodecId::Vp8 => "vp8".to_owned(),
        crate::video::CodecId::Vp9 => "vp9".to_owned(),
        crate::video::CodecId::H264 => "avc".to_owned(),
        crate::video::CodecId::Aac => "aac".to_owned(),
        crate::video::CodecId::Opus => "opus".to_owned(),
        crate::video::CodecId::Vorbis => "vorbis".to_owned(),
        crate::video::CodecId::Pcm => "pcm".to_owned(),
        crate::video::CodecId::Other(s) => s.to_ascii_lowercase(),
    }
}
