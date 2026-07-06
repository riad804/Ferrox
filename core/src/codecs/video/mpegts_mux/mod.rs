//! Pure-Rust MPEG-TS (ISO 13818-1) muxer.
//!
//! Writes PAT + PMT + PES-packetised elementary streams into 188-byte
//! transport stream packets.  Supports:
//! - AV1 video  (stream_type 0x06, registered via `registration_descriptor`)
//! - H.264 video (stream_type 0x1B)
//! - AAC audio  (stream_type 0x0F, ADTS framing)
//! - Opus audio (stream_type 0x06, registered)
//!
//! This implementation is self-contained (no C bindings, no external crate).
//!
//! # Limitations
//! - Single program only (one PAT entry → one PMT).
//! - PCR is derived from video PTS (no separate PCR PID).
//! - No encryption, no stuffing beyond what TS padding requires.

mod ts;
use ts::*;

use std::io::Write;
use crate::{
    error::{Error, Result},
    traits::ContainerMuxer,
    video::{CodecId, EncodedPacket, StreamInfo},
};

struct ElemStream {
    pid: u16,
    codec: CodecId,
    stream_id: u8,   // PES stream_id byte
    cc: u8,          // continuity counter
}

/// Pure-Rust MPEG-TS muxer.
///
/// Implements [`ContainerMuxer`].
pub struct MpegTsMuxer<W: Write + Send> {
    writer: W,
    streams: Vec<ElemStream>,
    pat_cc:  u8,
    pmt_cc:  u8,
    pat_pmt_interval: u32, // write PAT+PMT every N video packets
    pkt_count: u32,
    pcr_pid: u16,
    timebase: (u64, u64), // (fps_num, fps_den)
}

impl<W: Write + Send> MpegTsMuxer<W> {
    /// Create a new MPEG-TS muxer.
    ///
    /// `streams` is the same slice used by [`WebmMuxer`].
    /// `fps_num / fps_den` is the video time base.
    pub fn new(writer: W, streams: &[StreamInfo], fps_num: u64, fps_den: u64) -> Result<Self> {
        let mut elem: Vec<ElemStream> = Vec::new();
        let mut pcr_pid = FIRST_ELEM;
        let mut first_video = true;

        for (i, s) in streams.iter().enumerate() {
            let pid = FIRST_ELEM + i as u16;
            let stream_id = if s.is_video() {
                if first_video { pcr_pid = pid; first_video = false; }
                0xE0
            } else {
                0xC0
            };
            elem.push(ElemStream {
                pid,
                codec: s.codec.clone(),
                stream_id,
                cc: 0,
            });
        }

        Ok(Self {
            writer,
            streams: elem,
            pat_cc: 0,
            pmt_cc: 0,
            pat_pmt_interval: 30,
            pkt_count: 0,
            pcr_pid,
            timebase: (fps_num, fps_den),
        })
    }

    fn write_pat_pmt(&mut self) -> Result<()> {
        let pat = build_pat();
        self.write_section_packets(PAT_PID, &mut self.pat_cc.clone(), &pat)?;
        self.pat_cc = self.pat_cc.wrapping_add(1) & 0x0F;

        let stream_specs: Vec<(u16, CodecId)> = self.streams
            .iter()
            .map(|s| (s.pid, s.codec.clone()))
            .collect();
        let pmt = build_pmt(&stream_specs, self.pcr_pid);
        let mut pmt_cc = self.pmt_cc;
        self.write_section_packets(PMT_PID, &mut pmt_cc, &pmt)?;
        self.pmt_cc = pmt_cc;
        Ok(())
    }

    fn write_section_packets(&mut self, pid: u16, cc: &mut u8, section: &[u8]) -> Result<()> {
        let mut offset = 0;
        let mut first = true;
        while offset < section.len() {
            let avail = TS_PACKET_SIZE - 4;
            let end = (offset + avail).min(section.len());
            write_ts_packet(&mut self.writer, pid, first, *cc, &section[offset..end])?;
            *cc = (*cc + 1) & 0x0F;
            offset = end;
            first = false;
        }
        Ok(())
    }

    fn pts_to_90khz(&self, pts: u64) -> u64 {
        // pts is in timebase units (1/fps), convert to 90 kHz clock
        let (num, den) = self.timebase;
        if num == 0 { return pts * 3000; }
        pts * den * 90_000 / num
    }
}

impl<W: Write + Send> ContainerMuxer for MpegTsMuxer<W> {
    fn write_header(&mut self) -> Result<()> {
        self.write_pat_pmt()
    }

    fn write_packet(&mut self, packet: &EncodedPacket) -> Result<()> {
        // Periodically re-emit PAT+PMT for seekability.
        if packet.is_keyframe || self.pkt_count % self.pat_pmt_interval == 0 {
            self.write_pat_pmt()?;
        }
        self.pkt_count += 1;

        let stream_idx = packet.stream_index;
        if stream_idx >= self.streams.len() {
            return Err(Error::Video(format!(
                "mpegts: stream index {stream_idx} out of range"
            )));
        }

        let pid       = self.streams[stream_idx].pid;
        let stream_id = self.streams[stream_idx].stream_id;
        let is_pcr    = pid == self.pcr_pid;

        let pts_90 = self.pts_to_90khz(packet.pts);
        let pes_hdr = build_pes_header(stream_id, pts_90, None, packet.data.len());

        // Concatenate PES header + payload, then chop into 188-byte packets.
        let mut payload = Vec::with_capacity(pes_hdr.len() + packet.data.len());
        payload.extend_from_slice(&pes_hdr);
        payload.extend_from_slice(&packet.data);

        let mut offset = 0;
        let mut first  = true;
        while offset < payload.len() {
            let cc = self.streams[stream_idx].cc;

            if first && is_pcr && packet.is_keyframe {
                let avail = TS_PACKET_SIZE - 12; // 4 hdr + 8 adaptation field
                let end = (offset + avail).min(payload.len());
                write_ts_packet_with_pcr(
                    &mut self.writer, pid, true, cc, pts_90,
                    &payload[offset..end],
                )?;
                offset = end;
            } else {
                let avail = TS_PACKET_SIZE - 4;
                let end = (offset + avail).min(payload.len());
                write_ts_packet(&mut self.writer, pid, first, cc, &payload[offset..end])?;
                offset = end;
            }

            self.streams[stream_idx].cc = (cc + 1) & 0x0F;
            first = false;
        }

        Ok(())
    }

    fn write_trailer(&mut self) -> Result<()> {
        self.writer.flush().map_err(Error::Io)
    }
}
