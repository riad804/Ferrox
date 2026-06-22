//! HLS (HTTP Live Streaming) segmenter.
//!
//! Produces one segment file per time window + an M3U8 playlist.
//! Three segment formats are supported via [`HlsSegmentFormat`]:
//!
//! | Format | Extension | HLS version | Compatibility |
//! |--------|-----------|-------------|---------------|
//! | `WebM` | `.webm`   | v3          | Modern browsers only |
//! | `FMp4` | `.mp4`    | v6 + `#EXT-X-MAP` | All HLS clients incl. iOS ≥ 10 |
//! | `MpegTs` | `.ts`   | v3          | All HLS clients incl. iOS < 10 |
//!
//! The default format is `FMp4` (broadest device support).
//!
//! # Example
//!
//! ```no_run
//! use ferrox_core::hls::{HlsOptions, HlsSegmentFormat, segment};
//!
//! let opts = HlsOptions {
//!     segment_duration_secs: 6.0,
//!     format: HlsSegmentFormat::FMp4,
//!     ..HlsOptions::default()
//! };
//! // segment(std::path::Path::new("input.webm"), &opts).unwrap();
//! ```

use std::{
    fmt::Write as FmtWrite,
    fs::{self, File},
    io::BufWriter,
    path::{Path, PathBuf},
};

use crate::{
    codecs::video::{
        Av1Encoder, MpegTsMuxer, WebmMuxer,
        build_fmp4_init, build_fmp4_segment,
    },
    demux_graph::ContainerKind,
    error::{Error, Result},
    traits::{ContainerDemuxer, ContainerMuxer, VideoDecoder, VideoEncoder},
    video::{CodecId, EncodedPacket, StreamInfo, StreamKind},
};

// ── Segment format ────────────────────────────────────────────────────────────

/// Output format for HLS media segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HlsSegmentFormat {
    /// WebM segments — HLS v3 compatible; works in modern browsers only.
    WebM,
    /// Fragmented MP4 segments (ISO 14496-12) — HLS v6 + `#EXT-X-MAP`;
    /// supported by iOS ≥ 10, all Android, all modern browsers.
    FMp4,
    /// MPEG-TS segments — HLS v3; supported by all HLS players including
    /// iOS < 10 and older Android devices.
    MpegTs,
}

impl HlsSegmentFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::WebM   => "webm",
            Self::FMp4   => "mp4",
            Self::MpegTs => "ts",
        }
    }

    pub fn hls_version(&self) -> u32 {
        match self {
            Self::FMp4 => 6,
            _          => 3,
        }
    }
}

// ── Options ───────────────────────────────────────────────────────────────────

/// Options controlling HLS output.
#[derive(Debug, Clone)]
pub struct HlsOptions {
    /// Target segment duration in seconds. Actual duration may be slightly
    /// longer because cuts only happen on keyframes.
    pub segment_duration_secs: f64,
    /// Directory where segments and the playlist will be written.
    pub output_dir: PathBuf,
    /// Playlist filename (relative to `output_dir`).
    pub playlist_name: String,
    /// Prefix for segment filenames, e.g. `"seg"` → `seg000.mp4`.
    pub segment_prefix: String,
    /// Output segment container format.
    pub format: HlsSegmentFormat,
    /// AV1 encode speed preset (0 = slowest, 10 = fastest).
    pub speed: u8,
    /// AV1 quantizer (lower = better quality).
    pub quantizer: usize,
}

impl Default for HlsOptions {
    fn default() -> Self {
        Self {
            segment_duration_secs: 6.0,
            output_dir: PathBuf::from("hls_out"),
            playlist_name: "index.m3u8".into(),
            segment_prefix: "seg".into(),
            format: HlsSegmentFormat::FMp4,
            speed: 6,
            quantizer: 100,
        }
    }
}

// ── Public types ──────────────────────────────────────────────────────────────

/// Information about one produced HLS media segment.
#[derive(Debug)]
pub struct SegmentInfo {
    /// Absolute path to the segment file.
    pub path: PathBuf,
    /// Actual duration of the segment in seconds.
    pub duration_secs: f64,
    /// Number of frames in this segment.
    pub frames: usize,
}

/// Result of an HLS segmentation run.
#[derive(Debug)]
pub struct HlsResult {
    /// Path to the generated M3U8 playlist.
    pub playlist_path: PathBuf,
    /// One entry per produced segment.
    pub segments: Vec<SegmentInfo>,
    /// Total frames processed.
    pub total_frames: usize,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Segment a video file into HLS segments + M3U8 playlist.
///
/// Source codec must be VP8 (the only pure-Rust-decodable codec in default build).
/// Re-encodes to AV1 using `rav1e`.  Creates `output_dir` if it does not exist.
pub fn segment(input: &Path, opts: &HlsOptions) -> Result<HlsResult> {
    fs::create_dir_all(&opts.output_dir).map_err(Error::Io)?;

    let kind = ContainerKind::from_path(input)
        .ok_or_else(|| Error::UnsupportedFormat(format!(
            "unrecognised input container: '{}'", input.display()
        )))?;

    match kind {
        ContainerKind::Mkv => {
            let f = File::open(input)?;
            let demuxer = crate::codecs::video::WebmDemuxer::open(f)?;
            segment_with_demuxer(demuxer, opts)
        }
        ContainerKind::Ivf => {
            let f = File::open(input)?;
            let demuxer = crate::codecs::video::IvfDemuxer::open(f)?;
            segment_with_demuxer(demuxer, opts)
        }
        ContainerKind::Mp4 => {
            let f = File::open(input)?;
            let size = f.metadata()?.len();
            let demuxer = crate::codecs::video::Mp4Demuxer::open(f, size)?;
            segment_with_demuxer(demuxer, opts)
        }
    }
}

// ── Core segmentation loop ────────────────────────────────────────────────────

fn segment_with_demuxer<D: ContainerDemuxer>(
    mut demuxer: D,
    opts: &HlsOptions,
) -> Result<HlsResult> {
    let streams = demuxer.streams().to_vec();
    let video_stream = streams.iter().find(|s| s.is_video())
        .ok_or_else(|| Error::Video("no video stream found".into()))?;

    let src_w   = video_stream.width;
    let src_h   = video_stream.height;
    let fps     = video_stream.frame_rate;
    let codec   = video_stream.codec.clone();
    let vid_idx = video_stream.index;

    if codec != CodecId::Vp8 {
        return Err(Error::Video(format!(
            "HLS segmenter: source codec {codec} is not decodable; only VP8 is supported in the default build"
        )));
    }

    let (fps_num, fps_den) = if fps > 0.0 {
        ((fps * 1000.0).round() as u64, 1000u64)
    } else {
        (30_000, 1000)
    };

    let frames_per_seg =
        ((opts.segment_duration_secs * fps_num as f64 / fps_den as f64).ceil() as usize).max(1);

    let out_stream = StreamInfo {
        index: 0,
        kind: StreamKind::Video,
        codec: CodecId::Av1,
        width: src_w,
        height: src_h,
        frame_rate: fps_num as f64 / fps_den as f64,
        sample_rate: 0,
        channels: 0,
        codec_private: Vec::new(),
    };

    // For fMP4: write the init segment once.
    let init_path: Option<PathBuf> = if opts.format == HlsSegmentFormat::FMp4 {
        let init_bytes = build_fmp4_init(&[out_stream.clone()]);
        let p = opts.output_dir.join(format!("{}init.mp4", opts.segment_prefix));
        fs::write(&p, &init_bytes).map_err(Error::Io)?;
        Some(p)
    } else {
        None
    };

    let mut decoder = crate::codecs::video::Vp8Decoder;
    let mut segments: Vec<SegmentInfo> = Vec::new();
    let mut seg_idx  = 0usize;
    let mut total_frames = 0usize;

    let mut seg_pkts:  Vec<EncodedPacket> = Vec::new();
    let mut seg_frames = 0usize;

    // fMP4 segment state (sequence number + running base_decode_time in 90 kHz)
    let mut fmp4_seq: u32 = 1;
    let mut fmp4_bdt: u64 = 0;

    let mut encoder = Av1Encoder::new(src_w, src_h, opts.speed, opts.quantizer, fps_num, fps_den)?;

    loop {
        let Some((stream_idx, packet)) = demuxer.next_packet()? else { break };
        if stream_idx != vid_idx { continue; }

        let vf = match decoder.decode_packet(&packet) {
            Ok(f) => f,
            Err(_) => continue,
        };
        total_frames += 1;

        let is_boundary = vf.is_keyframe && seg_frames >= frames_per_seg;
        if is_boundary && !seg_pkts.is_empty() {
            let (bdt_after, path) = flush_segment(
                &seg_pkts, seg_idx, seg_frames,
                &out_stream, opts, fps_num, fps_den,
                fmp4_seq, fmp4_bdt,
                &mut segments,
            )?;
            fmp4_seq += 1;
            fmp4_bdt  = bdt_after;
            let _ = path;

            seg_pkts.clear();
            seg_frames = 0;
            seg_idx += 1;
            encoder = Av1Encoder::new(src_w, src_h, opts.speed, opts.quantizer, fps_num, fps_den)?;
        }

        let pkts = encoder.encode(&vf)?;
        seg_pkts.extend(pkts);
        seg_frames += 1;
    }

    // Flush last segment.
    let flushed = encoder.flush()?;
    seg_pkts.extend(flushed);
    if !seg_pkts.is_empty() {
        flush_segment(
            &seg_pkts, seg_idx, seg_frames,
            &out_stream, opts, fps_num, fps_den,
            fmp4_seq, fmp4_bdt,
            &mut segments,
        )?;
    }

    // Write M3U8.
    let playlist_path = opts.output_dir.join(&opts.playlist_name);
    write_m3u8(&playlist_path, &segments, opts, init_path.as_deref())?;

    Ok(HlsResult { playlist_path, segments, total_frames })
}

/// Flush one segment; returns `(new_fmp4_base_decode_time, path)`.
fn flush_segment(
    pkts: &[EncodedPacket],
    seg_idx: usize,
    nframes: usize,
    out_stream: &StreamInfo,
    opts: &HlsOptions,
    fps_num: u64,
    fps_den: u64,
    fmp4_seq: u32,
    fmp4_bdt: u64,
    segments: &mut Vec<SegmentInfo>,
) -> Result<(u64, PathBuf)> {
    if pkts.is_empty() {
        let p = opts.output_dir.join("empty");
        return Ok((fmp4_bdt, p));
    }

    let ext = opts.format.extension();
    let filename = format!("{}{:03}.{ext}", opts.segment_prefix, seg_idx);
    let path = opts.output_dir.join(&filename);

    let fps_f = fps_num as f64 / fps_den as f64;
    let duration_secs = if fps_f > 0.0 { nframes as f64 / fps_f } else { 0.0 };

    let new_bdt = match &opts.format {
        HlsSegmentFormat::WebM => {
            let f = BufWriter::new(File::create(&path)?);
            let mut muxer = WebmMuxer::new(f, &[out_stream.clone()], fps_num, fps_den)?;
            muxer.write_header()?;
            for pkt in pkts { muxer.write_packet(pkt)?; }
            muxer.write_trailer()?;
            fmp4_bdt
        }
        HlsSegmentFormat::FMp4 => {
            // Each HLS segment is a self-contained moof+mdat (no ftyp/moov).
            let timescale = 90_000u32;
            let seg_bytes = build_fmp4_segment(
                fmp4_seq, 1, timescale, fmp4_bdt, pkts, fps_num, fps_den,
            );
            fs::write(&path, &seg_bytes).map_err(Error::Io)?;

            // Advance base_decode_time by the total frame duration (in timescale units).
            let total_dur: u64 = pkts.iter().map(|p| p.duration.max(1)).sum();
            let scaled = if fps_num > 0 {
                total_dur * fps_den * timescale as u64 / fps_num
            } else {
                total_dur
            };
            fmp4_bdt + scaled
        }
        HlsSegmentFormat::MpegTs => {
            let mut buf: Vec<u8> = Vec::new();
            let mut muxer = MpegTsMuxer::new(&mut buf, &[out_stream.clone()], fps_num, fps_den)?;
            muxer.write_header()?;
            for pkt in pkts { muxer.write_packet(pkt)?; }
            muxer.write_trailer()?;
            fs::write(&path, &buf).map_err(Error::Io)?;
            fmp4_bdt
        }
    };

    segments.push(SegmentInfo { path: path.clone(), duration_secs, frames: nframes });
    Ok((new_bdt, path))
}

// ── M3U8 writer ───────────────────────────────────────────────────────────────

fn write_m3u8(
    path: &Path,
    segments: &[SegmentInfo],
    opts: &HlsOptions,
    init_path: Option<&Path>,
) -> Result<()> {
    let max_dur = segments.iter().map(|s| s.duration_secs).fold(0.0_f64, f64::max);
    let version = opts.format.hls_version();

    let mut m3u8 = String::new();
    writeln!(m3u8, "#EXTM3U").unwrap();
    writeln!(m3u8, "#EXT-X-VERSION:{version}").unwrap();
    writeln!(m3u8, "#EXT-X-TARGETDURATION:{}", max_dur.ceil() as u64).unwrap();
    writeln!(m3u8, "#EXT-X-MEDIA-SEQUENCE:0").unwrap();

    // fMP4 requires an EXT-X-MAP pointing to the init segment.
    if let Some(ip) = init_path {
        let name = ip.file_name().and_then(|n| n.to_str()).unwrap_or("init.mp4");
        writeln!(m3u8, "#EXT-X-MAP:URI=\"{name}\"").unwrap();
    }

    for seg in segments {
        let filename = seg.path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("segment");
        writeln!(m3u8, "#EXTINF:{:.6},", seg.duration_secs).unwrap();
        writeln!(m3u8, "{filename}").unwrap();
    }
    writeln!(m3u8, "#EXT-X-ENDLIST").unwrap();

    fs::write(path, m3u8).map_err(Error::Io)?;
    Ok(())
}

// ── M3U8 parser (for fuzzing + CLI round-trip) ────────────────────────────────

/// A parsed HLS M3U8 playlist (subset sufficient for media segment lists).
#[derive(Debug, Default)]
pub struct M3u8Playlist {
    pub version: u32,
    pub target_duration: u64,
    pub media_sequence: u64,
    pub segments: Vec<M3u8Segment>,
    pub is_ended: bool,
    /// URI from `#EXT-X-MAP`, if present.
    pub init_segment_uri: Option<String>,
}

/// One media segment entry in an M3U8 playlist.
#[derive(Debug)]
pub struct M3u8Segment {
    pub duration_secs: f64,
    pub uri: String,
}

/// Parse an M3U8 playlist from bytes.
///
/// Returns an error on malformed input. Unrecognised tags are silently skipped.
pub fn parse_m3u8(data: &[u8]) -> Result<M3u8Playlist> {
    let text = std::str::from_utf8(data)
        .map_err(|_| Error::UnsupportedFormat("M3U8 is not valid UTF-8".into()))?;

    let mut playlist = M3u8Playlist::default();
    let mut pending_extinf: Option<f64> = None;

    let mut lines = text.lines().peekable();
    let first = lines.next().unwrap_or("").trim();
    if first != "#EXTM3U" {
        return Err(Error::UnsupportedFormat("not an M3U8 file (missing #EXTM3U)".into()));
    }

    for line in lines {
        let line = line.trim();
        if line.is_empty() { continue; }

        if let Some(rest) = line.strip_prefix("#EXT-X-VERSION:") {
            playlist.version = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            playlist.target_duration = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            playlist.media_sequence = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            // Parse URI="…"
            if let Some(uri_start) = rest.find("URI=\"") {
                let after = &rest[uri_start + 5..];
                if let Some(uri_end) = after.find('"') {
                    playlist.init_segment_uri = Some(after[..uri_end].to_string());
                }
            }
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let dur_str = rest.split(',').next().unwrap_or("0").trim();
            pending_extinf = Some(dur_str.parse().unwrap_or(0.0));
        } else if line == "#EXT-X-ENDLIST" {
            playlist.is_ended = true;
        } else if !line.starts_with('#') {
            let duration_secs = pending_extinf.take().unwrap_or(0.0);
            playlist.segments.push(M3u8Segment {
                duration_secs,
                uri: line.to_string(),
            });
        }
    }

    Ok(playlist)
}
