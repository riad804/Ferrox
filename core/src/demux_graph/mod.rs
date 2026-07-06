use std::{fs::File, io::BufWriter, path::Path};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use tracing::{debug, info, instrument, warn};
use crate::{
    codecs::video::{IvfDemuxer, Mp4Demuxer, Vp8Decoder, WebmDemuxer},
    error::{Error, Result},
    traits::{ContainerDemuxer, VideoDecoder},
    video::{CodecId, VideoFrame},
};

/// Instantiate the correct decoder for the given codec, or return an error
/// with a clear message about which feature flag to enable.
fn make_decoder(codec: &CodecId) -> Result<Box<dyn VideoDecoder>> {
    match codec {
        CodecId::Vp8 => Ok(Box::new(Vp8Decoder)),
        #[cfg(feature = "vp9")]
        CodecId::Vp9 => {
            use crate::codecs::video::Vp9Decoder;
            Ok(Box::new(Vp9Decoder::new()?))
        }
        #[cfg(feature = "h264")]
        CodecId::H264 => {
            use crate::codecs::video::H264Decoder;
            Ok(Box::new(H264Decoder::new()?))
        }
        other => {
            let hint = match other {
                CodecId::Vp9  => " — enable the `vp9` feature flag to decode VP9 with libdav1d",
                CodecId::H264 => " — enable the `h264` feature flag to decode H.264 with OpenH264",
                _              => "",
            };
            Err(Error::Video(format!(
                "no pixel decoder available for {other}{hint}"
            )))
        }
    }
}

/// Detects the container format from the file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Mp4,
    /// Covers both .mkv and .webm — parsed with `matroska-demuxer`.
    Mkv,
    /// Raw IVF container (DKIF magic) — commonly used for VP8 test streams.
    Ivf,
}

impl ContainerKind {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "mp4" | "m4v" => Some(Self::Mp4),
            "mkv" | "webm" => Some(Self::Mkv),
            "ivf" => Some(Self::Ivf),
            _ => None,
        }
    }
}

/// Result of a frame-extraction run.
#[derive(Debug, Default)]
pub struct ExtractResult {
    /// Paths of PNG files written.
    pub frame_paths: Vec<std::path::PathBuf>,
    /// Number of video packets skipped (inter-frames, unsupported codecs, etc.).
    pub skipped: usize,
    /// Stream metadata snapshot.
    pub stream_count: usize,
}

/// Demux a video file and write the first `count` decodable video frames
/// as PNG images to `output_pattern` (a printf-style path like
/// `frame_%03d.png`).
///
/// Returns an [`ExtractResult`] describing what was written.
///
/// # Supported combinations
///
/// | Container | Video codec | Notes |
/// |-----------|-------------|-------|
/// | WebM/MKV  | VP8         | Full keyframe decode via `oxideav-vp8`. |
/// | MP4       | H.264       | **Container-only**: raw NAL bytes are extracted but *not decoded* to pixels. Returns [`Error::Video`] for unsupported codec decode. |
/// | MP4       | VP9         | Same as H.264 — container parse only. |
///
/// H.264 / VP9 pixel decoding requires a future `H264Decoder` /
/// `Vp9Decoder` implementation (see [`crate::video`] module docs).
#[instrument(skip_all, fields(input = %input.display(), count))]
pub fn extract_frames(
    input: &Path,
    output_pattern: &str,
    count: usize,
) -> Result<ExtractResult> {
    let kind = ContainerKind::from_path(input).ok_or_else(|| {
        Error::UnsupportedFormat(format!(
            "unrecognised container extension: '{}'",
            input.display()
        ))
    })?;

    match kind {
        ContainerKind::Mp4 => {
            let file = File::open(input)?;
            let size = file.metadata()?.len();
            let demuxer = Mp4Demuxer::open(file, size)?;
            extract_with_demuxer(demuxer, output_pattern, count)
        }
        ContainerKind::Mkv => {
            let file = File::open(input)?;
            let demuxer = WebmDemuxer::open(file)?;
            extract_with_demuxer(demuxer, output_pattern, count)
        }
        ContainerKind::Ivf => {
            let file = File::open(input)?;
            let demuxer = IvfDemuxer::open(file)?;
            extract_with_demuxer(demuxer, output_pattern, count)
        }
    }
}

fn extract_with_demuxer<D: ContainerDemuxer>(
    mut demuxer: D,
    output_pattern: &str,
    count: usize,
) -> Result<ExtractResult> {
    let streams = demuxer.streams().to_vec();
    let stream_count = streams.len();

    // Find first video stream and choose a decoder.
    let video_stream = streams.iter().find(|s| s.is_video());
    let Some(vs) = video_stream else {
        return Err(Error::Video("no video stream found in container".into()));
    };

    info!(
        codec = %vs.codec,
        width = vs.width,
        height = vs.height,
        "video stream"
    );

    let video_stream_idx = vs.index;
    let codec = vs.codec.clone();

    let mut decoder = make_decoder(&codec)?;
    let mut result = ExtractResult { stream_count, ..Default::default() };
    let mut written = 0usize;

    while written < count {
        let Some((stream_idx, packet)) = demuxer.next_packet()? else { break };
        if stream_idx != video_stream_idx { continue; }

        debug!(pts = packet.pts, keyframe = packet.is_keyframe, "video packet");

        match decoder.decode_packet(&packet) {
            Ok(vf) => {
                let path = format_output_path(output_pattern, written);
                write_video_frame_as_png(&vf, &path)?;
                info!(frame = written, path = %path.display(), "wrote frame");
                result.frame_paths.push(path);
                written += 1;
            }
            Err(e) => {
                warn!(pts = packet.pts, err = %e, "frame skipped");
                result.skipped += 1;
            }
        }
    }

    Ok(result)
}

/// Like [`extract_frames`] but skips the first `start` decodable frames before
/// writing, then writes up to `count` frames.
#[instrument(skip_all, fields(input = %input.display(), start, count))]
pub fn extract_frames_range(
    input: &Path,
    output_pattern: &str,
    start: usize,
    count: usize,
) -> Result<ExtractResult> {
    let kind = ContainerKind::from_path(input).ok_or_else(|| {
        Error::UnsupportedFormat(format!(
            "unrecognised container extension: '{}'",
            input.display()
        ))
    })?;

    match kind {
        ContainerKind::Mp4 => {
            let file = File::open(input)?;
            let size = file.metadata()?.len();
            let demuxer = Mp4Demuxer::open(file, size)?;
            extract_range_with_demuxer(demuxer, output_pattern, start, count)
        }
        ContainerKind::Mkv => {
            let file = File::open(input)?;
            let demuxer = WebmDemuxer::open(file)?;
            extract_range_with_demuxer(demuxer, output_pattern, start, count)
        }
        ContainerKind::Ivf => {
            let file = File::open(input)?;
            let demuxer = IvfDemuxer::open(file)?;
            extract_range_with_demuxer(demuxer, output_pattern, start, count)
        }
    }
}

fn extract_range_with_demuxer<D: ContainerDemuxer>(
    mut demuxer: D,
    output_pattern: &str,
    start: usize,
    count: usize,
) -> Result<ExtractResult> {
    let streams = demuxer.streams().to_vec();
    let stream_count = streams.len();

    let video_stream = streams.iter().find(|s| s.is_video());
    let Some(vs) = video_stream else {
        return Err(Error::Video("no video stream found in container".into()));
    };

    let video_stream_idx = vs.index;
    let codec = vs.codec.clone();

    let mut decoder = make_decoder(&codec)?;
    let mut result = ExtractResult { stream_count, ..Default::default() };
    let mut decoded = 0usize; // total successfully decoded frames
    let mut written = 0usize; // frames written to disk

    let total_needed = start + count;

    loop {
        if written >= count { break; }
        if count > 0 && decoded >= total_needed { break; }

        let Some((stream_idx, packet)) = demuxer.next_packet()? else { break };
        if stream_idx != video_stream_idx { continue; }

        match decoder.decode_packet(&packet) {
            Ok(vf) => {
                if decoded >= start {
                    let path = format_output_path(output_pattern, decoded - start);
                    write_video_frame_as_png(&vf, &path)?;
                    result.frame_paths.push(path);
                    written += 1;
                }
                decoded += 1;
            }
            Err(e) => {
                warn!(pts = packet.pts, err = %e, "frame skipped");
                result.skipped += 1;
            }
        }
    }

    Ok(result)
}


mod audio_extract;
mod convert;
pub use audio_extract::extract_audio;
pub use convert::*;

fn format_output_path(pattern: &str, index: usize) -> std::path::PathBuf {
    // Replace the first printf-style %…d (or %…03d) with the formatted index.
    // We do a simple scan: find '%', skip flags/width, match 'd'.
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    let mut substituted = false;
    while let Some(c) = chars.next() {
        if c == '%' && !substituted {
            // Collect optional flags/width digits
            let mut fmt = String::from('%');
            let mut width_str = String::new();
            let mut fill_zero = false;
            for ch in chars.by_ref() {
                if ch == '0' && width_str.is_empty() {
                    fill_zero = true;
                    fmt.push(ch);
                } else if ch.is_ascii_digit() {
                    width_str.push(ch);
                    fmt.push(ch);
                } else if ch == 'd' {
                    let width: usize = width_str.parse().unwrap_or(0);
                    if fill_zero && width > 0 {
                        out.push_str(&format!("{index:0>width$}"));
                    } else if width > 0 {
                        out.push_str(&format!("{index:>width$}"));
                    } else {
                        out.push_str(&index.to_string());
                    }
                    substituted = true;
                    break;
                } else {
                    // Not a %d pattern — emit literally
                    out.push_str(&fmt);
                    out.push(ch);
                    break;
                }
            }
            if !substituted {
                out.push_str(&fmt);
            }
        } else {
            out.push(c);
        }
    }
    std::path::PathBuf::from(out)
}

/// Write a decoded [`VideoFrame`] as a PNG. Handles all supported pixel formats.
fn write_video_frame_as_png(vf: &VideoFrame, path: &Path) -> Result<()> {
    let rgb = any_yuv_to_rgb8(&vf.frame)?;
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    PngEncoder::new(&mut writer)
        .write_image(&rgb.data, rgb.width, rgb.height, ColorType::Rgb8.into())
        .map_err(|e| Error::Image(e))?;
    Ok(())
}
