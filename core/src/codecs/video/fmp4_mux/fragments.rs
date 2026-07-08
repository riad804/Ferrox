//! Fragment (moof/mdat) + track/moov box construction for fMP4.

use super::boxes::*;
use crate::video::{CodecId, EncodedPacket, StreamInfo};

pub(super) fn build_empty_stbl(sample_entry: Vec<u8>) -> Vec<u8> {
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

pub(super) struct TrackInfo {
    pub(super) track_id: u32,
    pub(super) timescale: u32,
    pub(super) codec: CodecId,
    // video only
    pub(super) width: u32,
    pub(super) height: u32,
    // audio only
    pub(super) sample_rate: u32,
    pub(super) channels: u32,
    // codec private (e.g. avcC for H.264)
    pub(super) codec_private: Vec<u8>,
    // fragmentation state
    pub(super) base_decode_time: u64,
    pub(super) buffered: Vec<EncodedPacket>,
}

pub(super) fn build_trak(ti: &TrackInfo, movie_duration: u64) -> Vec<u8> {
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

pub(super) fn build_mvex(tracks: &[TrackInfo]) -> Vec<u8> {
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

pub(super) fn build_moof_mdat(
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
pub(super) fn patch_trun_data_offset(moof: &mut Vec<u8>, data_offset: u32) {
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

