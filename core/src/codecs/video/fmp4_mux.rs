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
    video::{CodecId, EncodedPacket, StreamInfo},
};

// ── Atom / box writer helpers ─────────────────────────────────────────────────

/// Write a 4-byte big-endian u32.
#[inline]
pub(super) fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }

/// Write a 8-byte big-endian u64.
#[inline]
pub(super) fn be64(v: u64) -> [u8; 8] { v.to_be_bytes() }

/// Build a generic MP4 box: 4-byte length + 4-byte type + payload.
pub(super) fn make_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let total = 8 + payload.len();
    let mut b = Vec::with_capacity(total);
    b.extend_from_slice(&be32(total as u32));
    b.extend_from_slice(fourcc);
    b.extend_from_slice(payload);
    b
}

/// Make a full box (version + flags prefix).
pub(super) fn make_full_box(fourcc: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + payload.len());
    p.push(version);
    p.extend_from_slice(&(flags & 0x00FF_FFFF).to_be_bytes()[1..]);
    p.extend_from_slice(payload);
    make_box(fourcc, &p)
}

// ── ftyp ─────────────────────────────────────────────────────────────────────

pub(super) fn build_ftyp() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"iso5");  // major brand
    p.extend_from_slice(&be32(512)); // minor version
    p.extend_from_slice(b"iso5");
    p.extend_from_slice(b"iso6");
    p.extend_from_slice(b"mp41");
    make_box(b"ftyp", &p)
}

// ── mvhd ─────────────────────────────────────────────────────────────────────

pub(super) fn build_mvhd(timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be64(0u64)); // creation_time
    p.extend_from_slice(&be64(0u64)); // modification_time
    p.extend_from_slice(&be32(timescale));
    p.extend_from_slice(&be64(duration));
    p.extend_from_slice(&be32(0x0001_0000)); // rate = 1.0
    p.extend_from_slice(&[0x01, 0x00]); // volume = 1.0
    p.extend_from_slice(&[0u8; 10]); // reserved
    // unity matrix
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x4000_0000));
    p.extend_from_slice(&[0u8; 24]); // pre-defined
    p.extend_from_slice(&be32(0xFFFF_FFFE)); // next_track_id placeholder
    make_full_box(b"mvhd", 1, 0, &p)
}

// ── tkhd ─────────────────────────────────────────────────────────────────────

pub(super) fn build_tkhd(track_id: u32, duration: u64, width: u32, height: u32, is_audio: bool) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be64(0u64)); // creation_time
    p.extend_from_slice(&be64(0u64)); // modification_time
    p.extend_from_slice(&be32(track_id));
    p.extend_from_slice(&be32(0)); // reserved
    p.extend_from_slice(&be64(duration));
    p.extend_from_slice(&[0u8; 8]); // reserved
    p.extend_from_slice(&[0u8; 2]); // layer
    p.extend_from_slice(&[0u8; 2]); // alternate_group
    let vol: [u8; 2] = if is_audio { [0x01, 0x00] } else { [0u8; 2] };
    p.extend_from_slice(&vol);
    p.extend_from_slice(&[0u8; 2]); // reserved
    // unity matrix
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x0001_0000)); p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0));            p.extend_from_slice(&be32(0));
    p.extend_from_slice(&be32(0x4000_0000));
    // width/height as 16.16 fixed point (0 for audio)
    if is_audio {
        p.extend_from_slice(&[0u8; 8]);
    } else {
        p.extend_from_slice(&be32(width << 16));
        p.extend_from_slice(&be32(height << 16));
    }
    // flags=3: track_enabled | track_in_movie
    make_full_box(b"tkhd", 1, 3, &p)
}

// ── mdhd ─────────────────────────────────────────────────────────────────────

pub(super) fn build_mdhd(timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be64(0u64)); // creation_time
    p.extend_from_slice(&be64(0u64)); // modification_time
    p.extend_from_slice(&be32(timescale));
    p.extend_from_slice(&be64(duration));
    p.extend_from_slice(&[0x55, 0xC4]); // language = 'und' (ISO 639-2/T)
    p.extend_from_slice(&be32(0)); // pre_defined
    make_full_box(b"mdhd", 1, 0, &p)
}

// ── hdlr ─────────────────────────────────────────────────────────────────────

pub(super) fn build_hdlr(handler: &[u8; 4], name: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&be32(0)); // pre_defined
    p.extend_from_slice(handler);
    p.extend_from_slice(&[0u8; 12]); // reserved
    p.extend_from_slice(name);
    p.push(0); // null terminator
    make_full_box(b"hdlr", 0, 0, &p)
}

// ── sample entries ────────────────────────────────────────────────────────────

pub(super) fn build_avc1(width: u32, height: u32, avcc: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]); // reserved
    p.extend_from_slice(&be32(1)[2..]); // data_reference_index = 1 (u16)
    p.extend_from_slice(&[0u8; 16]); // pre_defined + reserved
    p.extend_from_slice(&be32(width)[2..]);  // width (u16)
    p.extend_from_slice(&be32(height)[2..]); // height (u16)
    p.extend_from_slice(&be32(0x0048_0000)); // horizresolution = 72dpi
    p.extend_from_slice(&be32(0x0048_0000)); // vertresolution
    p.extend_from_slice(&be32(0)); // reserved
    p.extend_from_slice(&be32(1)[2..]); // frame_count = 1 (u16)
    p.extend_from_slice(&[0u8; 32]); // compressorname
    p.extend_from_slice(&[0x00, 0x18]); // depth = 24
    p.extend_from_slice(&[0xFF, 0xFF]); // pre_defined = -1

    // avcC box
    let avcc_box = make_box(b"avcC", avcc);
    p.extend_from_slice(&avcc_box);
    make_box(b"avc1", &p)
}

pub(super) fn build_av01(width: u32, height: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]); // reserved
    p.extend_from_slice(&[0x00, 0x01]); // data_reference_index
    p.extend_from_slice(&[0u8; 16]);
    p.extend_from_slice(&be32(width)[2..]);
    p.extend_from_slice(&be32(height)[2..]);
    p.extend_from_slice(&be32(0x0048_0000));
    p.extend_from_slice(&be32(0x0048_0000));
    p.extend_from_slice(&be32(0));
    p.extend_from_slice(&[0x00, 0x01]);
    p.extend_from_slice(&[0u8; 32]);
    p.extend_from_slice(&[0x00, 0x18]);
    p.extend_from_slice(&[0xFF, 0xFF]);
    // Minimal av1C (sequence header placeholder — real encoders write this)
    let av1c = make_box(b"av1C", &[0x81, 0x04, 0x0C, 0x00]);
    p.extend_from_slice(&av1c);
    make_box(b"av01", &p)
}

fn build_mp4a(sample_rate: u32, channels: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]);
    p.extend_from_slice(&[0x00, 0x01]);
    p.extend_from_slice(&[0u8; 8]);
    p.extend_from_slice(&be32(channels)[2..]); // channelcount (u16)
    p.extend_from_slice(&[0x00, 0x10]); // samplesize = 16
    p.extend_from_slice(&[0u8; 4]);
    p.extend_from_slice(&be32(sample_rate << 16)); // samplerate 16.16
    // esds (minimal, describing AAC LC)
    let esds = build_esds(channels as u8, sample_rate);
    p.extend_from_slice(&esds);
    make_box(b"mp4a", &p)
}

fn build_esds(channels: u8, sample_rate: u32) -> Vec<u8> {
    // Encode sample_rate index for AAC
    let sri: u8 = match sample_rate {
        96000 => 0, 88200 => 1, 64000 => 2, 48000 => 3,
        44100 => 4, 32000 => 5, 24000 => 6, 22050 => 7,
        16000 => 8, 12000 => 9, 11025 => 10, 8000 => 11,
        _ => 4,
    };
    // AudioSpecificConfig: AAC-LC, sample_rate_index, channels
    let asc: [u8; 2] = [0x11 | ((sri >> 1) << 3), ((sri & 1) << 7) | (channels << 3)];
    // DecoderSpecificInfo tag (0x05)
    let mut dsi = vec![0x05u8, asc.len() as u8];
    dsi.extend_from_slice(&asc);
    // DecoderConfigDescriptor tag (0x04): objectTypeIndication=0x40 (AAC), streamType=0x15, bufferSize=0, maxBitrate/avgBitrate
    let mut dcd = vec![
        0x04u8, (13 + dsi.len()) as u8,
        0x40, // objectTypeIndication = Audio ISO/IEC 14496-3
        0x15, // streamType=0x05 (AudioStream) <<1 | upStream=0 | 1
        0x00, 0x00, 0x00, // bufferSizeDB
        0x00, 0x00, 0x00, 0x00, // maxBitrate
        0x00, 0x00, 0x00, 0x00, // avgBitrate
    ];
    dcd.extend_from_slice(&dsi);
    // SLConfigDescriptor (0x06): predefined=2
    let slcd = vec![0x06u8, 0x01, 0x02];
    // ES_Descriptor (0x03)
    let mut esd = vec![0x03u8, (3 + dcd.len() + slcd.len()) as u8, 0x00, 0x00, 0x00];
    esd.extend_from_slice(&dcd);
    esd.extend_from_slice(&slcd);
    make_full_box(b"esds", 0, 0, &esd)
}

// ── stsd / stts / stsc / stsz / stco (empty, for fragmented MP4) ─────────────

fn build_empty_stbl(sample_entry: Vec<u8>) -> Vec<u8> {
    let stsd = {
        let mut p = be32(1).to_vec(); // entry_count = 1
        p.extend_from_slice(&sample_entry);
        make_full_box(b"stsd", 0, 0, &p)
    };
    let stts = make_full_box(b"stts", 0, 0, &be32(0)); // entry_count=0
    let stsc = make_full_box(b"stsc", 0, 0, &be32(0));
    let stsz = {
        let mut p = Vec::new();
        p.extend_from_slice(&be32(0)); // sample_size = variable
        p.extend_from_slice(&be32(0)); // sample_count = 0
        make_full_box(b"stsz", 0, 0, &p)
    };
    let stco = make_full_box(b"stco", 0, 0, &be32(0));

    let mut stbl = Vec::new();
    stbl.extend_from_slice(&stsd);
    stbl.extend_from_slice(&stts);
    stbl.extend_from_slice(&stsc);
    stbl.extend_from_slice(&stsz);
    stbl.extend_from_slice(&stco);
    make_box(b"stbl", &stbl)
}

// ── Track descriptor ──────────────────────────────────────────────────────────

struct TrackInfo {
    track_id: u32,
    timescale: u32,
    codec: CodecId,
    // video only
    width: u32,
    height: u32,
    // audio only
    sample_rate: u32,
    channels: u32,
    // codec private (e.g. avcC for H.264)
    codec_private: Vec<u8>,
    // fragmentation state
    base_decode_time: u64,
    buffered: Vec<EncodedPacket>,
}

fn build_trak(ti: &TrackInfo, movie_duration: u64) -> Vec<u8> {
    let tkhd = build_tkhd(ti.track_id, movie_duration, ti.width, ti.height,
        matches!(ti.codec, CodecId::Aac | CodecId::Opus));

    let mdhd = build_mdhd(ti.timescale, movie_duration);
    let hdlr = if matches!(ti.codec, CodecId::Aac | CodecId::Opus) {
        build_hdlr(b"soun", b"SoundHandler")
    } else {
        build_hdlr(b"vide", b"VideoHandler")
    };

    let sample_entry = match &ti.codec {
        CodecId::H264 => build_avc1(ti.width, ti.height, &ti.codec_private),
        CodecId::Av1  => build_av01(ti.width, ti.height),
        CodecId::Aac  => build_mp4a(ti.sample_rate, ti.channels),
        _             => build_av01(ti.width, ti.height), // fallback
    };

    let stbl = build_empty_stbl(sample_entry);

    // dinf (data reference — self-contained)
    let url = make_full_box(b"url ", 0, 1, &[]);
    let dref = {
        let mut p = be32(1).to_vec();
        p.extend_from_slice(&url);
        make_full_box(b"dref", 0, 0, &p)
    };
    let dinf = make_box(b"dinf", &dref);

    // minf
    let nmhd_or_vmhd = if matches!(ti.codec, CodecId::Aac | CodecId::Opus) {
        make_full_box(b"smhd", 0, 0, &[0u8; 4]) // balance=0, reserved
    } else {
        make_full_box(b"vmhd", 0, 1, &[0u8; 8]) // graphicsMode=0, opcolor
    };
    let mut minf_payload = Vec::new();
    minf_payload.extend_from_slice(&nmhd_or_vmhd);
    minf_payload.extend_from_slice(&dinf);
    minf_payload.extend_from_slice(&stbl);
    let minf = make_box(b"minf", &minf_payload);

    // mdia
    let mut mdia_payload = Vec::new();
    mdia_payload.extend_from_slice(&mdhd);
    mdia_payload.extend_from_slice(&hdlr);
    mdia_payload.extend_from_slice(&minf);
    let mdia = make_box(b"mdia", &mdia_payload);

    // trak
    let mut trak_payload = Vec::new();
    trak_payload.extend_from_slice(&tkhd);
    trak_payload.extend_from_slice(&mdia);
    make_box(b"trak", &trak_payload)
}

// ── mvex (movie extends — declares that fragments will follow) ─────────────────

fn build_mvex(tracks: &[TrackInfo]) -> Vec<u8> {
    let mut payload = Vec::new();
    for t in tracks {
        let mut trex_payload = Vec::new();
        trex_payload.extend_from_slice(&be32(t.track_id));
        trex_payload.extend_from_slice(&be32(1)); // default_sample_description_index
        trex_payload.extend_from_slice(&be32(0)); // default_sample_duration
        trex_payload.extend_from_slice(&be32(0)); // default_sample_size
        trex_payload.extend_from_slice(&be32(0)); // default_sample_flags
        payload.extend_from_slice(&make_full_box(b"trex", 0, 0, &trex_payload));
    }
    make_box(b"mvex", &payload)
}

// ── moof + mdat ───────────────────────────────────────────────────────────────

fn build_moof_mdat(
    sequence_number: u32,
    track_id: u32,
    base_decode_time: u64,
    packets: &[EncodedPacket],
    timescale: u32,
    src_fps_num: u64,
    src_fps_den: u64,
) -> Vec<u8> {
    // Convert PTS units → track timescale units.
    let scale_pts = |pts: u64| -> u32 {
        if src_fps_num == 0 { return pts as u32; }
        (pts * src_fps_den * timescale as u64 / src_fps_num) as u32
    };

    // Build trun entries: [duration, size, flags, composition_offset?]
    let trun_flags = 0x0000_0F05u32; // data_offset + duration + size + flags + composition_offset
    let sample_flags_keyframe   = 0x0200_0000u32;
    let sample_flags_non_key    = 0x0100_0040u32;

    let mut trun_entries: Vec<u8> = Vec::new();
    let mut data_size: usize = 0;

    for (i, pkt) in packets.iter().enumerate() {
        let dur = if i + 1 < packets.len() {
            scale_pts(packets[i + 1].pts.saturating_sub(pkt.pts))
        } else {
            scale_pts(pkt.duration.max(1))
        };
        trun_entries.extend_from_slice(&be32(dur.max(1)));
        trun_entries.extend_from_slice(&be32(pkt.data.len() as u32));
        trun_entries.extend_from_slice(&be32(
            if pkt.is_keyframe { sample_flags_keyframe } else { sample_flags_non_key }
        ));
        trun_entries.extend_from_slice(&be32(0)); // composition_offset = 0
        data_size += pkt.data.len();
    }

    // trun box
    let sample_count = packets.len() as u32;
    let trun = {
        let mut p = Vec::new();
        p.extend_from_slice(&be32(sample_count));
        p.extend_from_slice(&be32(0)); // data_offset placeholder (patched below)
        p.extend_from_slice(&trun_entries);
        make_full_box(b"trun", 0, trun_flags, &p)
    };

    // tfhd
    let tfhd = {
        let mut p = Vec::new();
        p.extend_from_slice(&be32(track_id));
        make_full_box(b"tfhd", 0, 0x020000, &p) // default-base-is-moof flag
    };

    // tfdt (track fragment decode time)
    let tfdt = {
        let mut p = Vec::new();
        p.extend_from_slice(&be64(base_decode_time));
        make_full_box(b"tfdt", 1, 0, &p)
    };

    // traf
    let mut traf_payload = Vec::new();
    traf_payload.extend_from_slice(&tfhd);
    traf_payload.extend_from_slice(&tfdt);
    traf_payload.extend_from_slice(&trun);
    let traf = make_box(b"traf", &traf_payload);

    // mfhd
    let mfhd = make_full_box(b"mfhd", 0, 0, &be32(sequence_number));

    // moof
    let mut moof_payload = Vec::new();
    moof_payload.extend_from_slice(&mfhd);
    moof_payload.extend_from_slice(&traf);
    let mut moof = make_box(b"moof", &moof_payload);

    // Patch data_offset in trun: it is the offset from the start of moof to
    // the first byte of mdat payload = moof.len() + 8 (mdat header).
    let data_offset = (moof.len() + 8) as u32;
    // trun data_offset is at: moof_header(8) + mfhd(?) + traf_header(8) + tfhd(?) + tfdt(?) + trun_header(8) + full_box_prefix(4) + sample_count(4)
    // Easier: search for the trun box and patch by walking the structure.
    patch_trun_data_offset(&mut moof, data_offset);

    // mdat
    let mut mdat_payload: Vec<u8> = Vec::with_capacity(data_size);
    for pkt in packets { mdat_payload.extend_from_slice(&pkt.data); }
    let mdat = make_box(b"mdat", &mdat_payload);

    let mut out = moof;
    out.extend_from_slice(&mdat);
    out
}

/// Walk a `moof` box to find the `trun` full-box and patch its data_offset field.
fn patch_trun_data_offset(moof: &mut Vec<u8>, data_offset: u32) {
    let data = data_offset.to_be_bytes();
    // Find "trun" fourcc
    let Some(pos) = moof.windows(4).position(|w| w == b"trun") else { return };
    // After fourcc: 1 (version) + 3 (flags) + 4 (sample_count) = 8 bytes → data_offset at pos+4+8
    let off = pos + 4 + 8;
    if off + 4 <= moof.len() {
        moof[off..off + 4].copy_from_slice(&data);
    }
}

// ── HLS init segment helper ───────────────────────────────────────────────────

/// Build an fMP4 init segment (`ftyp` + `moov`) as raw bytes.
///
/// Used by the HLS segmenter to write the `#EXT-X-MAP` init file once, then
/// write each HLS segment as an independent `moof`+`mdat` pair (via
/// [`build_fmp4_segment`]).
pub fn build_fmp4_init(streams: &[StreamInfo]) -> Vec<u8> {
    let tracks: Vec<TrackInfo> = streams.iter().enumerate().map(|(i, s)| {
        let timescale = if s.is_video() { 90_000u32 } else { s.sample_rate.max(44100) };
        TrackInfo {
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
        }
    }).collect();

    let ftyp = build_ftyp();
    let mvhd = build_mvhd(90_000, 0);
    let mvex = build_mvex(&tracks);

    let mut moov_payload = mvhd;
    for ti in &tracks { moov_payload.extend_from_slice(&build_trak(ti, 0)); }
    moov_payload.extend_from_slice(&mvex);
    let moov = make_box(b"moov", &moov_payload);

    let mut out = ftyp;
    out.extend_from_slice(&moov);
    out
}

/// Build one fMP4 media segment (`moof`+`mdat`) for use in HLS.
///
/// `sequence_number` must increase monotonically across all segments.
/// `base_decode_time` is in the track timescale (90 000 Hz for video).
pub fn build_fmp4_segment(
    sequence_number: u32,
    track_id: u32,
    timescale: u32,
    base_decode_time: u64,
    packets: &[EncodedPacket],
    fps_num: u64,
    fps_den: u64,
) -> Vec<u8> {
    build_moof_mdat(sequence_number, track_id, base_decode_time, packets, timescale, fps_num, fps_den)
}

// ── FMp4Muxer ─────────────────────────────────────────────────────────────────

/// Pure-Rust fragmented MP4 muxer.
///
/// Implements [`ContainerMuxer`].
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
