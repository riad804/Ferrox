//! Pure-Rust fragmented MP4 (fMP4 / ISO 14496-12) muxer.
//!
//! Produces a streaming-friendly MP4 file:
//!
//!   `ftyp` → `moov` (movie header, no sample data) → one or more `moof`+`mdat` fragments
//!
//! Each call to [`FMp4Muxer::write_packet`] accumulates packets; each call
//! may flush a fragment when enough data has been buffered.  Calling
//! [`FMp4Muxer::write_trailer`] flushes any remaining packets.
//!
//! Supports: AV1 video, H.264 video, AAC audio.
//!
//! # Limitations
//! - Single video + optional single audio track only.
//! - No edit lists, no chapter tracks, no encryption.
//! - Timescale is fixed at 90 000 Hz for video, 44 100 Hz for audio.

use std::io::{Seek, Write};
use crate::{
    error::{Error, Result},
    traits::ContainerMuxer,
    video::{EncodedPacket, StreamInfo},
};

pub(super) mod boxes;
mod fragments;
use boxes::*;
use fragments::*;
pub use fragments::{build_fmp4_init, build_fmp4_segment};

pub struct FMp4Muxer<W: Write + Seek + Send> {
    writer: W,
    tracks: Vec<TrackInfo>,
    sequence_number: u32,
    fps_num: u64,
    fps_den: u64,
    /// Flush a new fragment after this many packets (per track).
    fragment_size: usize,
}

impl<W: Write + Seek + Send> FMp4Muxer<W> {
    /// Create a new fMP4 muxer.
    ///
    /// `streams` is the same slice used by [`WebmMuxer`].
    pub fn new(writer: W, streams: &[StreamInfo], fps_num: u64, fps_den: u64) -> Result<Self> {
        let mut tracks: Vec<TrackInfo> = Vec::new();
        for (i, s) in streams.iter().enumerate() {
            let timescale = if s.is_video() { 90_000u32 } else { s.sample_rate.max(44100) };
            tracks.push(TrackInfo {
                track_id: (i + 1) as u32,
                timescale,
                codec: s.codec.clone(),
                width: s.width,
                height: s.height,
                sample_rate: s.sample_rate,
                channels: s.channels as u32,
                codec_private: s.codec_private.clone(),
                base_decode_time: 0,
                buffered: Vec::new(),
            });
        }
        Ok(Self {
            writer,
            tracks,
            sequence_number: 1,
            fps_num,
            fps_den,
            fragment_size: 30,
        })
    }

    fn flush_fragment(&mut self, track_idx: usize) -> Result<()> {
        if self.tracks[track_idx].buffered.is_empty() { return Ok(()); }

        let pkts: Vec<EncodedPacket> = self.tracks[track_idx].buffered.drain(..).collect();
        let tid = self.tracks[track_idx].track_id;
        let bdt = self.tracks[track_idx].base_decode_time;
        let ts  = self.tracks[track_idx].timescale;
        let seq = self.sequence_number;
        self.sequence_number += 1;

        // Advance base_decode_time by total duration of flushed packets.
        let total_dur: u64 = pkts.iter().map(|p| p.duration.max(1)).sum();
        let scaled_dur = if self.fps_num > 0 {
            total_dur * self.fps_den * ts as u64 / self.fps_num
        } else {
            total_dur
        };
        self.tracks[track_idx].base_decode_time = bdt + scaled_dur;

        let fragment = build_moof_mdat(seq, tid, bdt, &pkts, ts, self.fps_num, self.fps_den);
        self.writer.write_all(&fragment).map_err(Error::Io)
    }

    fn flush_all(&mut self) -> Result<()> {
        for i in 0..self.tracks.len() {
            self.flush_fragment(i)?;
        }
        Ok(())
    }
}

impl<W: Write + Seek + Send> ContainerMuxer for FMp4Muxer<W> {
    fn write_header(&mut self) -> Result<()> {
        // ftyp
        let ftyp = build_ftyp();
        self.writer.write_all(&ftyp).map_err(Error::Io)?;

        // moov
        let movie_timescale = 90_000u32;
        let movie_duration  = 0u64; // unknown upfront for live/fragmented

        let mvhd = build_mvhd(movie_timescale, movie_duration);
        let mvex = {
            build_mvex(&self.tracks)
        };

        let mut moov_payload = mvhd;

        for ti in &self.tracks {
            moov_payload.extend_from_slice(&build_trak(ti, movie_duration));
        }
        moov_payload.extend_from_slice(&mvex);

        let moov = make_box(b"moov", &moov_payload);
        self.writer.write_all(&moov).map_err(Error::Io)
    }

    fn write_packet(&mut self, packet: &EncodedPacket) -> Result<()> {
        let idx = packet.stream_index;
        if idx >= self.tracks.len() {
            return Err(Error::Video(format!(
                "fmp4: stream index {idx} out of range"
            )));
        }

        self.tracks[idx].buffered.push(packet.clone());

        if self.tracks[idx].buffered.len() >= self.fragment_size {
            self.flush_fragment(idx)?;
        }
        Ok(())
    }

    fn write_trailer(&mut self) -> Result<()> {
        self.flush_all()?;
        self.writer.flush().map_err(Error::Io)
    }
}
