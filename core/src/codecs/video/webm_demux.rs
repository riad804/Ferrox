use std::io::{Read, Seek};
use matroska_demuxer::{MatroskaFile, TrackType};
use crate::{
    error::{Error, Result},
    traits::ContainerDemuxer,
    video::{CodecId, Packet, StreamInfo, StreamKind},
};

/// Demuxes a Matroska (MKV) or WebM container.
///
/// Stream metadata is discovered at construction; packets arrive via
/// [`ContainerDemuxer::next_packet`].
///
/// # Codec coverage
///
/// Codec IDs are the standard Matroska/WebM CODEC_ID strings:
/// `V_VP8`, `V_VP9`, `A_OPUS`, `A_VORBIS`, etc.
/// Unrecognised IDs are passed through as [`CodecId::Other`].
pub struct WebmDemuxer<R: Read + Seek> {
    file: MatroskaFile<R>,
    streams: Vec<StreamInfo>,
    frame_buf: matroska_demuxer::Frame,
}

impl<R: Read + Seek + Send> WebmDemuxer<R> {
    pub fn open(reader: R) -> Result<Self> {
        let file = MatroskaFile::open(reader)
            .map_err(|e| Error::Video(format!("mkv/webm parse: {e}")))?;

        let streams: Vec<StreamInfo> = file
            .tracks()
            .iter()
            .enumerate()
            .map(|(idx, track)| {
                let kind = match track.track_type() {
                    TrackType::Video => StreamKind::Video,
                    TrackType::Audio => StreamKind::Audio,
                    TrackType::Subtitle => StreamKind::Subtitle,
                    _ => StreamKind::Other,
                };
                let codec = codec_id_from_mkv(track.codec_id());
                let (width, height, frame_rate) = track.video()
                    .map(|v| {
                        // frame_rate isn't stored in all WebM files; derive
                        // from the track's default_duration if available.
                        let fps = track.default_duration()
                            .map(|d| 1_000_000_000.0 / d.get() as f64)
                            .unwrap_or(0.0);
                        (v.pixel_width().get() as u32, v.pixel_height().get() as u32, fps)
                    })
                    .unwrap_or((0, 0, 0.0));
                let (sample_rate, channels) = track.audio()
                    .map(|a| (a.sampling_frequency() as u32, a.channels().get() as u16))
                    .unwrap_or((0, 0));
                let codec_private = track.codec_private()
                    .map(|b| b.to_vec())
                    .unwrap_or_default();

                StreamInfo {
                    index: idx,
                    kind,
                    codec,
                    width,
                    height,
                    frame_rate,
                    sample_rate,
                    channels,
                    codec_private,
                }
            })
            .collect();

        Ok(Self {
            file,
            streams,
            frame_buf: matroska_demuxer::Frame::default(),
        })
    }

    /// Return the 0-based stream index for a given 1-based Matroska track number.
    fn track_num_to_stream_index(&self, track_num: u64) -> Option<usize> {
        self.file
            .tracks()
            .iter()
            .position(|t| t.track_number().get() == track_num)
    }
}

impl<R: Read + Seek + Send> ContainerDemuxer for WebmDemuxer<R> {
    fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    fn next_packet(&mut self) -> Result<Option<(usize, Packet)>> {
        loop {
            let has_frame = self.file
                .next_frame(&mut self.frame_buf)
                .map_err(|e| Error::Video(format!("mkv next_frame: {e}")))?;

            if !has_frame { return Ok(None); }

            let track_num = self.frame_buf.track;
            let Some(stream_idx) = self.track_num_to_stream_index(track_num) else {
                continue; // unknown track — skip
            };

            let packet = Packet {
                data: self.frame_buf.data.clone(),
                pts: self.frame_buf.timestamp,
                duration: self.frame_buf.duration.unwrap_or(0),
                is_keyframe: self.frame_buf.is_keyframe.unwrap_or(false),
            };
            return Ok(Some((stream_idx, packet)));
        }
    }
}

fn codec_id_from_mkv(id: &str) -> CodecId {
    match id {
        "V_VP8"     => CodecId::Vp8,
        "V_VP9"     => CodecId::Vp9,
        "A_OPUS"    => CodecId::Opus,
        "A_VORBIS"  => CodecId::Vorbis,
        "A_PCM/INT/LIT" | "A_PCM/INT/BIG" | "A_PCM/FLOAT/IEEE" => CodecId::Pcm,
        other       => CodecId::Other(other.to_owned()),
    }
}
