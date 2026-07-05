//! Progressive (non-fragmented) **MP4 muxer** — `ftyp` + `moov` (with real
//! `stbl` sample tables) + `mdat`. Unlike the fragmented [`super::FMp4Muxer`]
//! (`moof`/`trun`, for streaming/HLS), this produces a plain MP4 that general
//! players and demuxers (including this crate's own `Mp4Demuxer`) parse
//! directly — the right choice for file export.
//!
//! Video-only, single track (AV1 or H.264). Samples are buffered in memory and
//! written on [`Mp4Muxer::finish`]; chunk offsets are back-patched once the
//! `moov` size is known.

use std::io::Write;

use crate::error::{Error, Result};
use crate::video::{CodecId, EncodedPacket, StreamInfo};

use super::fmp4_mux::{
    be32, build_av01, build_avc1, build_ftyp, build_hdlr, build_mdhd, build_mvhd, build_tkhd,
    make_box, make_full_box,
};

struct SampleMeta {
    size: u32,
    is_keyframe: bool,
}

/// A progressive MP4 muxer for a single video track.
pub struct Mp4Muxer<W: Write> {
    writer: W,
    stream: StreamInfo,
    fps_num: u64,
    fps_den: u64,
    samples: Vec<SampleMeta>,
    mdat: Vec<u8>,
}

impl<W: Write> Mp4Muxer<W> {
    /// Create a muxer for `stream` (must be a video stream) at `fps_num/fps_den`.
    pub fn new(writer: W, stream: StreamInfo, fps_num: u64, fps_den: u64) -> Result<Self> {
        if !stream.is_video() {
            return Err(Error::Video("Mp4Muxer: expected a video stream".into()));
        }
        Ok(Self { writer, stream, fps_num: fps_num.max(1), fps_den: fps_den.max(1), samples: Vec::new(), mdat: Vec::new() })
    }

    /// Buffer one encoded video packet as a sample.
    pub fn write_packet(&mut self, pkt: &EncodedPacket) -> Result<()> {
        self.samples.push(SampleMeta { size: pkt.data.len() as u32, is_keyframe: pkt.is_keyframe });
        self.mdat.extend_from_slice(&pkt.data);
        Ok(())
    }

    /// Finalize: write `ftyp` + `moov` + `mdat`.
    pub fn finish(mut self) -> Result<()> {
        let n = self.samples.len() as u32;
        let timescale = self.fps_num as u32;
        let delta = self.fps_den as u32;
        let duration = n as u64 * self.fps_den;
        let (w, h) = (self.stream.width, self.stream.height);

        let sample_entry = match self.stream.codec {
            CodecId::Av1 => build_av01(w, h),
            CodecId::H264 => build_avc1(w, h, &self.stream.codec_private),
            ref other => return Err(Error::Video(format!("Mp4Muxer: unsupported codec {other}"))),
        };

        // ── sample tables ──
        let stsd = {
            let mut p = be32(1).to_vec();
            p.extend_from_slice(&sample_entry);
            make_full_box(b"stsd", 0, 0, &p)
        };
        let stts = {
            let mut p = be32(1).to_vec();
            p.extend_from_slice(&be32(n));
            p.extend_from_slice(&be32(delta));
            make_full_box(b"stts", 0, 0, &p)
        };
        let stss = {
            let mut idx: Vec<u32> = self
                .samples
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_keyframe)
                .map(|(i, _)| i as u32 + 1)
                .collect();
            if idx.is_empty() && n > 0 {
                idx.push(1); // guarantee at least one sync sample
            }
            let mut p = be32(idx.len() as u32).to_vec();
            for i in &idx {
                p.extend_from_slice(&be32(*i));
            }
            make_full_box(b"stss", 0, 0, &p)
        };
        let stsc = {
            // one chunk holding all samples
            let mut p = be32(1).to_vec();
            p.extend_from_slice(&be32(1)); // first_chunk
            p.extend_from_slice(&be32(n)); // samples_per_chunk
            p.extend_from_slice(&be32(1)); // sample_description_index
            make_full_box(b"stsc", 0, 0, &p)
        };
        let stsz = {
            let mut p = be32(0).to_vec(); // sample_size = 0 → per-sample sizes follow
            p.extend_from_slice(&be32(n));
            for s in &self.samples {
                p.extend_from_slice(&be32(s.size));
            }
            make_full_box(b"stsz", 0, 0, &p)
        };

        // `stco` size is fixed regardless of the offset value, so build the moov
        // once with a placeholder to measure it, then again with the real offset.
        let build_moov = |chunk_offset: u32| -> Vec<u8> {
            let stco = {
                let mut p = be32(1).to_vec();
                p.extend_from_slice(&be32(chunk_offset));
                make_full_box(b"stco", 0, 0, &p)
            };
            let mut stbl = stsd.clone();
            stbl.extend_from_slice(&stts);
            stbl.extend_from_slice(&stss);
            stbl.extend_from_slice(&stsc);
            stbl.extend_from_slice(&stsz);
            stbl.extend_from_slice(&stco);
            let stbl = make_box(b"stbl", &stbl);

            let vmhd = make_full_box(b"vmhd", 0, 1, &[0u8; 8]);
            let dinf = {
                let url = make_full_box(b"url ", 0, 1, &[]); // self-contained
                let mut dref_p = be32(1).to_vec();
                dref_p.extend_from_slice(&url);
                let dref = make_full_box(b"dref", 0, 0, &dref_p);
                make_box(b"dinf", &dref)
            };
            let mut minf_p = vmhd;
            minf_p.extend_from_slice(&dinf);
            minf_p.extend_from_slice(&stbl);
            let minf = make_box(b"minf", &minf_p);

            let mut mdia_p = build_mdhd(timescale, duration);
            mdia_p.extend_from_slice(&build_hdlr(b"vide", b"VideoHandler"));
            mdia_p.extend_from_slice(&minf);
            let mdia = make_box(b"mdia", &mdia_p);

            let mut trak_p = build_tkhd(1, duration, w, h, false);
            trak_p.extend_from_slice(&mdia);
            let trak = make_box(b"trak", &trak_p);

            let mut moov_p = build_mvhd(timescale, duration);
            moov_p.extend_from_slice(&trak);
            make_box(b"moov", &moov_p)
        };

        let ftyp = build_ftyp();
        let moov_len = build_moov(0).len();
        // mdat payload begins after ftyp + moov + the 8-byte mdat box header.
        let chunk_offset = (ftyp.len() + moov_len + 8) as u32;
        let moov = build_moov(chunk_offset);

        self.writer.write_all(&ftyp)?;
        self.writer.write_all(&moov)?;
        // mdat, written as header + payload to avoid an extra full copy.
        let mdat_total = (8 + self.mdat.len()) as u32;
        self.writer.write_all(&be32(mdat_total))?;
        self.writer.write_all(b"mdat")?;
        self.writer.write_all(&self.mdat)?;
        self.writer.flush()?;
        Ok(())
    }
}
