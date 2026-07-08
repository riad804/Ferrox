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
    fs::{self, File},
    path::{Path, PathBuf},
};

use crate::{demux_graph::ContainerKind, error::{Error, Result}};

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


mod segmenter;
use segmenter::segment_with_demuxer;

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
