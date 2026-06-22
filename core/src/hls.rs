//! HLS (HTTP Live Streaming) segmenter.
//!
//! Produces:
//! - One WebM segment file per time window (e.g. 10 s each).
//! - A master M3U8 playlist referencing the segments.
//!
//! WebM segments are valid standalone WebM files, compatible with HLS v6+
//! (`#EXT-X-MAP` init segment). Each segment begins on a keyframe.
//!
//! # Example
//!
//! ```no_run
//! use ferrox_core::hls::{HlsOptions, segment};
//!
//! let opts = HlsOptions {
//!     segment_duration_secs: 10.0,
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
    codecs::video::{Av1Encoder, WebmMuxer},
    demux_graph::ContainerKind,
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::{ContainerDemuxer, ContainerMuxer, VideoDecoder, VideoEncoder},
    video::{CodecId, EncodedPacket, StreamInfo, StreamKind, VideoFrame},
};

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
    /// Prefix for segment filenames, e.g. `"seg"` → `seg000.webm`.
    pub segment_prefix: String,
    /// AV1 encode speed preset (0 = slowest, 10 = fastest).
    pub speed: u8,
    /// AV1 quantizer (lower = better quality).
    pub quantizer: usize,
}

impl Default for HlsOptions {
    fn default() -> Self {
        Self {
            segment_duration_secs: 10.0,
            output_dir: PathBuf::from("hls_out"),
            playlist_name: "index.m3u8".into(),
            segment_prefix: "seg".into(),
            speed: 6,
            quantizer: 100,
        }
    }
}

/// Information about a produced segment.
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

/// Segment a video file into HLS WebM segments + M3U8 playlist.
///
/// Only VP8 source video is currently decodable (AV1 re-encode target).
/// The function creates `output_dir` if it does not exist.
pub fn segment(input: &Path, opts: &HlsOptions) -> Result<HlsResult> {
    fs::create_dir_all(&opts.output_dir)
        .map_err(|e| Error::Io(e))?;

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

fn segment_with_demuxer<D: ContainerDemuxer>(
    mut demuxer: D,
    opts: &HlsOptions,
) -> Result<HlsResult> {
    let streams = demuxer.streams().to_vec();
    let video_stream = streams.iter().find(|s| s.is_video())
        .ok_or_else(|| Error::Video("no video stream found".into()))?;

    let src_w    = video_stream.width;
    let src_h    = video_stream.height;
    let fps      = video_stream.frame_rate;
    let codec    = video_stream.codec.clone();
    let vid_idx  = video_stream.index;

    if codec != CodecId::Vp8 {
        return Err(Error::Video(format!(
            "HLS segmenter: source codec {codec} cannot be decoded; only VP8 is supported"
        )));
    }

    let (fps_num, fps_den) = if fps > 0.0 {
        ((fps * 1000.0).round() as u64, 1000u64)
    } else {
        (30, 1)
    };

    let frames_per_seg = ((opts.segment_duration_secs * fps_num as f64 / fps_den as f64).ceil() as usize).max(1);

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

    let mut decoder  = crate::codecs::video::Vp8Decoder;
    let mut segments: Vec<SegmentInfo> = Vec::new();
    let mut seg_idx  = 0usize;
    let mut total_frames = 0usize;

    // Buffers for current segment's encoded packets.
    let mut seg_pkts: Vec<EncodedPacket> = Vec::new();
    let mut seg_frames = 0usize;

    // We need a fresh encoder per segment (rav1e doesn't support flush-and-reset).
    let mut encoder = Av1Encoder::new(src_w, src_h, opts.speed, opts.quantizer, fps_num, fps_den)?;

    let flush_segment = |
        pkts: &[EncodedPacket],
        seg_idx: usize,
        nframes: usize,
        out_stream: &StreamInfo,
        opts: &HlsOptions,
        fps_num: u64,
        fps_den: u64,
        segments: &mut Vec<SegmentInfo>,
    | -> Result<()> {
        if pkts.is_empty() { return Ok(()); }
        let filename = format!("{}{:03}.webm", opts.segment_prefix, seg_idx);
        let path = opts.output_dir.join(&filename);
        let f = BufWriter::new(File::create(&path)?);
        let mut muxer = WebmMuxer::new(f, &[out_stream.clone()], fps_num, fps_den)?;
        muxer.write_header()?;
        for pkt in pkts { muxer.write_packet(pkt)?; }
        muxer.write_trailer()?;

        let fps_f = fps_num as f64 / fps_den as f64;
        let duration_secs = if fps_f > 0.0 { nframes as f64 / fps_f } else { 0.0 };
        segments.push(SegmentInfo { path, duration_secs, frames: nframes });
        Ok(())
    };

    loop {
        let Some((stream_idx, packet)) = demuxer.next_packet()? else { break };
        if stream_idx != vid_idx { continue; }

        let vf = match decoder.decode_packet(&packet) {
            Ok(f) => f,
            Err(_) => continue,
        };
        total_frames += 1;

        // Start a new segment at every keyframe boundary after enough frames.
        let is_boundary = vf.is_keyframe && seg_frames >= frames_per_seg;
        if is_boundary && !seg_pkts.is_empty() {
            flush_segment(&seg_pkts, seg_idx, seg_frames, &out_stream, opts, fps_num, fps_den, &mut segments)?;
            seg_pkts.clear();
            seg_frames = 0;
            seg_idx += 1;
            // New encoder for the new segment (fresh keyframe).
            encoder = Av1Encoder::new(src_w, src_h, opts.speed, opts.quantizer, fps_num, fps_den)?;
        }

        let pkts = encoder.encode(&vf)?;
        seg_pkts.extend(pkts);
        seg_frames += 1;
    }

    // Flush the last segment.
    let flushed = encoder.flush()?;
    seg_pkts.extend(flushed);
    flush_segment(&seg_pkts, seg_idx, seg_frames, &out_stream, opts, fps_num, fps_den, &mut segments)?;

    // Write M3U8 playlist.
    let playlist_path = opts.output_dir.join(&opts.playlist_name);
    write_m3u8(&playlist_path, &segments, opts)?;

    Ok(HlsResult { playlist_path, segments, total_frames })
}

// ── M3U8 writer ───────────────────────────────────────────────────────────────

fn write_m3u8(path: &Path, segments: &[SegmentInfo], _opts: &HlsOptions) -> Result<()> {
    let max_dur = segments.iter().map(|s| s.duration_secs).fold(0.0_f64, f64::max);

    let mut m3u8 = String::new();
    writeln!(m3u8, "#EXTM3U").unwrap();
    writeln!(m3u8, "#EXT-X-VERSION:3").unwrap();
    writeln!(m3u8, "#EXT-X-TARGETDURATION:{}", max_dur.ceil() as u64).unwrap();
    writeln!(m3u8, "#EXT-X-MEDIA-SEQUENCE:0").unwrap();

    for seg in segments {
        let filename = seg.path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("segment.webm");
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
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            // #EXTINF:<duration>[,<title>]
            let dur_str = rest.split(',').next().unwrap_or("0").trim();
            pending_extinf = Some(dur_str.parse().unwrap_or(0.0));
        } else if line == "#EXT-X-ENDLIST" {
            playlist.is_ended = true;
        } else if !line.starts_with('#') {
            // URI line following an #EXTINF tag.
            let duration_secs = pending_extinf.take().unwrap_or(0.0);
            playlist.segments.push(M3u8Segment {
                duration_secs,
                uri: line.to_string(),
            });
        }
        // Other tags (#EXT-X-MAP, #EXT-X-KEY, …) are silently ignored.
    }

    Ok(playlist)
}
