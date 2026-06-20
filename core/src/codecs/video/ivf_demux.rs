use std::io::{Read, Seek, SeekFrom};
use oxideav_vp8::ivf::{
    parse_frame_header, parse_header, IVF_FRAME_HEADER_LEN, IVF_HEADER_LEN, IVF_VP8_FOURCC,
};
use crate::{
    error::{Error, Result},
    traits::ContainerDemuxer,
    video::{CodecId, Packet, StreamInfo, StreamKind},
};

/// Demuxes a raw IVF container (DKIF magic, VP8 or VP9 payload).
///
/// IVF is the lightweight test container used by libvpx tooling.
/// It holds exactly one video stream — no audio.
pub struct IvfDemuxer<R: Read + Seek> {
    reader: R,
    streams: Vec<StreamInfo>,
    /// Byte offset of the next frame header inside the file.
    next_offset: u64,
}

impl<R: Read + Seek + Send> IvfDemuxer<R> {
    pub fn open(mut reader: R) -> Result<Self> {
        let mut hdr_buf = [0u8; IVF_HEADER_LEN];
        reader.read_exact(&mut hdr_buf)
            .map_err(|e| Error::Video(format!("ivf: read header: {e}")))?;

        let hdr = parse_header(&hdr_buf)
            .map_err(|e| Error::Video(format!("ivf: {e}")))?;

        let codec = if hdr.fourcc == IVF_VP8_FOURCC {
            CodecId::Vp8
        } else if hdr.fourcc == *b"VP90" {
            CodecId::Vp9
        } else {
            CodecId::Other(String::from_utf8_lossy(&hdr.fourcc).into_owned())
        };

        let fps = if hdr.framerate_den > 0 {
            hdr.framerate_num as f64 / hdr.framerate_den as f64
        } else {
            0.0
        };

        let stream = StreamInfo {
            index: 0,
            kind: StreamKind::Video,
            codec,
            width: hdr.width,
            height: hdr.height,
            frame_rate: fps,
            sample_rate: 0,
            channels: 0,
            codec_private: Vec::new(),
        };

        Ok(Self {
            reader,
            streams: vec![stream],
            next_offset: IVF_HEADER_LEN as u64,
        })
    }
}

impl<R: Read + Seek + Send> ContainerDemuxer for IvfDemuxer<R> {
    fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    fn next_packet(&mut self) -> Result<Option<(usize, Packet)>> {
        self.reader.seek(SeekFrom::Start(self.next_offset))
            .map_err(|e| Error::Video(format!("ivf seek: {e}")))?;

        let mut fhdr_buf = [0u8; IVF_FRAME_HEADER_LEN];
        match self.reader.read_exact(&mut fhdr_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(Error::Video(format!("ivf frame header: {e}"))),
        }

        let fhdr = parse_frame_header(&fhdr_buf)
            .map_err(|e| Error::Video(format!("ivf: {e}")))?;

        let mut data = vec![0u8; fhdr.size as usize];
        self.reader.read_exact(&mut data)
            .map_err(|e| Error::Video(format!("ivf frame payload: {e}")))?;

        self.next_offset += IVF_FRAME_HEADER_LEN as u64 + fhdr.size as u64;

        // IVF has no keyframe flag in its header; for VP8 the low bit of
        // byte 0 of the payload is 0 for keyframes (per RFC 6386 §9.1).
        let is_keyframe = data.first().map(|b| b & 1 == 0).unwrap_or(false);

        let packet = Packet {
            data,
            pts: fhdr.pts,
            duration: 0,
            is_keyframe,
        };
        Ok(Some((0, packet)))
    }
}
