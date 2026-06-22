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

use std::io::Write;
use crate::{
    error::{Error, Result},
    traits::ContainerMuxer,
    video::{CodecId, EncodedPacket, StreamInfo, StreamKind},
};

// ── Constants ─────────────────────────────────────────────────────────────────

const TS_PACKET_SIZE: usize = 188;
const SYNC_BYTE: u8 = 0x47;

const PAT_PID:    u16 = 0x0000;
const PMT_PID:    u16 = 0x0020;
const FIRST_ELEM: u16 = 0x0100; // PIDs for elementary streams start here

const PROGRAM_NUM: u16 = 1;

/// MPEG-TS stream_type byte for each supported codec.
fn stream_type(codec: &CodecId) -> u8 {
    match codec {
        CodecId::H264  => 0x1B,
        CodecId::Aac   => 0x0F,
        CodecId::Av1   => 0x06, // private_data + registration descriptor
        CodecId::Opus  => 0x06,
        _              => 0x06,
    }
}

/// 4-byte registration_descriptor format_identifier for private streams.
fn format_id(codec: &CodecId) -> Option<[u8; 4]> {
    match codec {
        CodecId::Av1  => Some(*b"AV01"),
        CodecId::Opus => Some(*b"Opus"),
        _             => None,
    }
}

// ── CRC-32/MPEG ───────────────────────────────────────────────────────────────

fn crc32_mpeg(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        for i in (0..8).rev() {
            let bit = ((byte >> (7 - i)) & 1) as u32;
            let msb = (crc >> 31) & 1;
            crc <<= 1;
            if bit ^ msb != 0 { crc ^= 0x04C1_1DB7; }
        }
    }
    crc
}

// ── TS packet builder ─────────────────────────────────────────────────────────

/// Write a single 188-byte TS packet into `out`.
///
/// - `pid`: 13-bit packet identifier
/// - `payload_unit_start`: sets PUSI flag, inserts pointer_field=0 prefix
/// - `cc`: continuity counter (0–15)
/// - `payload`: up to 184 bytes of payload (rest is filled with 0xFF stuffing)
fn write_ts_packet<W: Write>(
    out: &mut W,
    pid: u16,
    payload_unit_start: bool,
    cc: u8,
    payload: &[u8],
) -> Result<()> {
    let mut pkt = [0xFFu8; TS_PACKET_SIZE];
    pkt[0] = SYNC_BYTE;

    let pusi = if payload_unit_start { 1u8 } else { 0u8 };
    pkt[1] = (pusi << 6) | ((pid >> 8) as u8 & 0x1F);
    pkt[2] = (pid & 0xFF) as u8;
    // no adaptation field (0b01), CC in lower nibble
    pkt[3] = 0x10 | (cc & 0x0F);

    let avail = TS_PACKET_SIZE - 4;
    let copy = payload.len().min(avail);
    pkt[4..4 + copy].copy_from_slice(&payload[..copy]);
    // bytes [4+copy..] already 0xFF (stuffing)

    out.write_all(&pkt).map_err(Error::Io)
}

/// Same as `write_ts_packet` but with an adaptation field carrying PCR.
fn write_ts_packet_with_pcr<W: Write>(
    out: &mut W,
    pid: u16,
    payload_unit_start: bool,
    cc: u8,
    pcr_90khz: u64,
    payload: &[u8],
) -> Result<()> {
    let mut pkt = [0xFFu8; TS_PACKET_SIZE];
    pkt[0] = SYNC_BYTE;

    let pusi = if payload_unit_start { 1u8 } else { 0u8 };
    pkt[1] = (pusi << 6) | ((pid >> 8) as u8 & 0x1F);
    pkt[2] = (pid & 0xFF) as u8;
    // adaptation field present + payload present (0b11)
    pkt[3] = 0x30 | (cc & 0x0F);

    // Adaptation field: length=7, flags=PCR_present (0x10), 6 PCR bytes
    let af_len: usize = 7;
    pkt[4] = af_len as u8;
    pkt[5] = 0x10; // PCR_flag

    let base = pcr_90khz / 300;
    let ext  = (pcr_90khz % 300) as u16;
    pkt[6]  = ((base >> 25) & 0xFF) as u8;
    pkt[7]  = ((base >> 17) & 0xFF) as u8;
    pkt[8]  = ((base >>  9) & 0xFF) as u8;
    pkt[9]  = ((base >>  1) & 0xFF) as u8;
    pkt[10] = (((base & 1) as u8) << 7) | 0x7E | ((ext >> 8) as u8 & 1);
    pkt[11] = (ext & 0xFF) as u8;

    let header_end = 4 + 1 + af_len; // 12
    let avail = TS_PACKET_SIZE - header_end;
    let copy  = payload.len().min(avail);
    pkt[header_end..header_end + copy].copy_from_slice(&payload[..copy]);

    out.write_all(&pkt).map_err(Error::Io)
}

// ── PAT ───────────────────────────────────────────────────────────────────────

fn build_pat() -> Vec<u8> {
    let mut section: Vec<u8> = Vec::new();
    section.push(0x00); // pointer_field = 0
    section.push(0x00); // table_id = PAT
    // section_syntax_indicator=1, '0'=0, reserved=11, section_length TBD
    section.push(0xB0);
    section.push(0x00); // length placeholder [3]
    // transport_stream_id
    section.push(0x00); section.push(0x01);
    // reserved=11, version=0, current_next=1
    section.push(0xC1);
    // section_number, last_section_number
    section.push(0x00); section.push(0x00);

    // Program entry: program_num + PMT_PID
    section.push((PROGRAM_NUM >> 8) as u8);
    section.push((PROGRAM_NUM & 0xFF) as u8);
    section.push(0xE0 | ((PMT_PID >> 8) as u8 & 0x1F));
    section.push((PMT_PID & 0xFF) as u8);

    // Fill length (excludes first 3 bytes of section, includes CRC)
    let section_len = (section.len() - 3 + 4) as u16;
    section[2] = 0xB0 | ((section_len >> 8) as u8 & 0x0F);
    section[3] = (section_len & 0xFF) as u8;

    let crc = crc32_mpeg(&section[1..]);
    section.extend_from_slice(&crc.to_be_bytes());
    section
}

// ── PMT ───────────────────────────────────────────────────────────────────────

fn build_pmt(streams: &[(u16, CodecId)], pcr_pid: u16) -> Vec<u8> {
    let mut info: Vec<u8> = Vec::new();
    for (pid, codec) in streams {
        let st = stream_type(codec);
        let mut es_info: Vec<u8> = Vec::new();
        if let Some(fid) = format_id(codec) {
            // registration_descriptor (tag=0x05)
            es_info.push(0x05);
            es_info.push(4);
            es_info.extend_from_slice(&fid);
        }
        info.push(st);
        info.push(0xE0 | ((*pid >> 8) as u8 & 0x1F));
        info.push((*pid & 0xFF) as u8);
        let il = es_info.len() as u16;
        info.push(0xF0 | ((il >> 8) as u8 & 0x0F));
        info.push((il & 0xFF) as u8);
        info.extend_from_slice(&es_info);
    }

    let mut section: Vec<u8> = Vec::new();
    section.push(0x00); // pointer_field
    section.push(0x02); // table_id = PMT
    section.push(0xB0);
    section.push(0x00); // length placeholder [3]
    section.push((PROGRAM_NUM >> 8) as u8);
    section.push((PROGRAM_NUM & 0xFF) as u8);
    section.push(0xC1); // version=0, current_next=1
    section.push(0x00); section.push(0x00); // section numbers
    // PCR_PID
    section.push(0xE0 | ((pcr_pid >> 8) as u8 & 0x1F));
    section.push((pcr_pid & 0xFF) as u8);
    // program_info_length = 0
    section.push(0xF0); section.push(0x00);
    section.extend_from_slice(&info);

    let section_len = (section.len() - 3 + 4) as u16;
    section[2] = 0xB0 | ((section_len >> 8) as u8 & 0x0F);
    section[3] = (section_len & 0xFF) as u8;

    let crc = crc32_mpeg(&section[1..]);
    section.extend_from_slice(&crc.to_be_bytes());
    section
}

// ── PES packetiser ────────────────────────────────────────────────────────────

fn build_pes_header(stream_id: u8, pts_90khz: u64, dts_90khz: Option<u64>, payload_len: usize) -> Vec<u8> {
    let has_dts = dts_90khz.is_some();
    let pts_dts_flags: u8 = if has_dts { 0xC0 } else { 0x80 };
    let header_data_len: u8 = if has_dts { 10 } else { 5 };

    // PES packet length: 0 means unbounded (for video); set for audio.
    let pes_pkt_len = if payload_len + 3 + header_data_len as usize > 0xFFFF {
        0u16
    } else {
        (3 + header_data_len as usize + payload_len) as u16
    };

    let mut h = Vec::with_capacity(14);
    // start code prefix
    h.extend_from_slice(&[0x00, 0x00, 0x01, stream_id]);
    h.push((pes_pkt_len >> 8) as u8);
    h.push((pes_pkt_len & 0xFF) as u8);
    // marker bits + flags
    h.push(0x80);
    h.push(pts_dts_flags);
    h.push(header_data_len);
    write_pts_dts(&mut h, pts_90khz, if has_dts { 0x31 } else { 0x21 });
    if let Some(dts) = dts_90khz {
        write_pts_dts(&mut h, dts, 0x11);
    }
    h
}

fn write_pts_dts(buf: &mut Vec<u8>, ts: u64, marker_hi: u8) {
    buf.push(marker_hi | (((ts >> 30) & 0x07) as u8) << 1 | 0x01);
    buf.push(((ts >> 22) & 0xFF) as u8);
    buf.push((((ts >> 15) & 0x7F) as u8) << 1 | 0x01);
    buf.push(((ts >> 7) & 0xFF) as u8);
    buf.push(((ts & 0x7F) as u8) << 1 | 0x01);
}

// ── MpegTsMuxer ──────────────────────────────────────────────────────────────

struct ElemStream {
    pid: u16,
    codec: CodecId,
    stream_id: u8,   // PES stream_id byte
    cc: u8,          // continuity counter
}

/// Pure-Rust MPEG-TS muxer.
///
/// Implements [`ContainerMuxer`].
pub struct MpegTsMuxer<W: Write> {
    writer: W,
    streams: Vec<ElemStream>,
    pat_cc:  u8,
    pmt_cc:  u8,
    pat_pmt_interval: u32, // write PAT+PMT every N video packets
    pkt_count: u32,
    pcr_pid: u16,
    timebase: (u64, u64), // (fps_num, fps_den)
}

impl<W: Write> MpegTsMuxer<W> {
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

impl<W: Write> ContainerMuxer for MpegTsMuxer<W> {
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
