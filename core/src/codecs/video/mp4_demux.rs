use std::io::{BufReader, Read, Seek};
use mp4::{ChannelConfig, Mp4Reader, TrackType};
use crate::{
    error::{Error, Result},
    traits::ContainerDemuxer,
    video::{CodecId, Packet, StreamInfo, StreamKind},
};

/// Demuxes an ISO BMFF / MP4 container using the `mp4` crate.
///
/// Streams are discovered at construction time; packets are delivered
/// one sample at a time via [`ContainerDemuxer::next_packet`].
///
/// # Codec coverage
///
/// The `mp4` crate parses the container and returns raw NAL / codec
/// bytes per sample.  No in-process decoding is performed here.
/// Supported codecs for metadata inspection: H.264 (AVC), H.265 (HEVC),
/// VP9, and AAC audio.  Other codecs are reported as [`CodecId::Other`].
pub struct Mp4Demuxer<R: Read + Seek> {
    reader: Mp4Reader<BufReader<R>>,
    streams: Vec<StreamInfo>,
    /// Per-stream: (track_id, next_sample_id, total_samples).
    cursors: Vec<(u32, u32, u32)>,
    /// Flat iteration index across all streams (round-robins).
    current_stream: usize,
}

impl<R: Read + Seek + Send> Mp4Demuxer<R> {
    /// Open a container from any `Read + Seek` source.
    pub fn open(reader: R, size: u64) -> Result<Self> {
        let mp4 = Mp4Reader::read_header(BufReader::new(reader), size)
            .map_err(|e| Error::Video(format!("mp4 parse: {e}")))?;

        let mut streams = Vec::new();
        let mut cursors = Vec::new();

        for (idx, (track_id, track)) in mp4.tracks().iter().enumerate() {
            let kind = match track.track_type() {
                Ok(TrackType::Video) => StreamKind::Video,
                Ok(TrackType::Audio) => StreamKind::Audio,
                Ok(TrackType::Subtitle) => StreamKind::Subtitle,
                Err(_) => StreamKind::Other,
            };

            let codec = match track.media_type() {
                Ok(mt) => codec_id_from_mp4_media_type(mt),
                Err(_) => CodecId::Other("unknown".into()),
            };

            let info = StreamInfo {
                index: idx,
                kind,
                codec,
                width: track.width() as u32,
                height: track.height() as u32,
                frame_rate: track.frame_rate(),
                sample_rate: track.sample_freq_index()
                    .map(|sfi| sfi.freq())
                    .unwrap_or(0),
                channels: track.channel_config()
                    .map(|cc| channel_config_count(cc))
                    .unwrap_or(0),
                codec_private: Vec::new(),
            };

            let sample_count = track.sample_count();
            streams.push(info);
            cursors.push((*track_id, 1u32, sample_count));
        }

        Ok(Self { reader: mp4, streams, cursors, current_stream: 0 })
    }
}

impl<R: Read + Seek + Send> ContainerDemuxer for Mp4Demuxer<R> {
    fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    fn next_packet(&mut self) -> Result<Option<(usize, Packet)>> {
        // Iterate streams round-robin; skip exhausted ones.
        let n = self.cursors.len();
        if n == 0 { return Ok(None); }

        for attempt in 0..n {
            let si = (self.current_stream + attempt) % n;
            let (track_id, ref mut sample_id, total) = self.cursors[si];
            if *sample_id > total { continue; }

            let sid = *sample_id;
            *sample_id += 1;
            self.current_stream = (si + 1) % n;

            let sample = self.reader
                .read_sample(track_id, sid)
                .map_err(|e| Error::Video(format!("mp4 read_sample: {e}")))?;

            if let Some(s) = sample {
                let timescale = self.streams[si].sample_rate.max(
                    (self.streams[si].frame_rate as u32).max(1)
                ).max(1) as u64;
                let _ = timescale; // pts is already in container units
                let packet = Packet {
                    data: s.bytes.to_vec(),
                    pts: s.start_time,
                    duration: s.duration as u64,
                    is_keyframe: s.is_sync,
                };
                return Ok(Some((si, packet)));
            }
        }
        Ok(None)
    }
}

fn channel_config_count(cc: ChannelConfig) -> u16 {
    match cc {
        ChannelConfig::Mono    => 1,
        ChannelConfig::Stereo  => 2,
        ChannelConfig::Three   => 3,
        ChannelConfig::Four    => 4,
        ChannelConfig::Five    => 5,
        ChannelConfig::FiveOne => 6,
        ChannelConfig::SevenOne => 8,
    }
}

fn codec_id_from_mp4_media_type(mt: mp4::MediaType) -> CodecId {
    match mt {
        mp4::MediaType::H264 => CodecId::H264,
        mp4::MediaType::H265 => CodecId::Other("H265".into()),
        mp4::MediaType::VP9  => CodecId::Vp9,
        mp4::MediaType::AAC  => CodecId::Aac,
        mp4::MediaType::TTXT => CodecId::Other("TTXT".into()),
    }
}
