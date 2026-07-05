use std::{fs::File, io::BufWriter, path::Path};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use tracing::{debug, info, instrument, warn};
use crate::{
    codecs::video::{IvfDemuxer, Mp4Demuxer, Vp8Decoder, WebmDemuxer},
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::{ContainerDemuxer, VideoDecoder},
    video::{CodecId, VideoFrame},
    AudioFrame, AudioGraph,
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

/// Demux audio from a video container and write it to `output` (WAV only for now).
#[instrument(skip_all, fields(input = %input.display(), output = %output.display()))]
pub fn extract_audio(input: &Path, output: &Path) -> Result<()> {
    // We extract raw compressed audio packets and then re-encode only for
    // codecs we can already decode (PCM / Vorbis / Opus inside WebM).
    // For MP4/AAC we return an informative error.
    let out_ext = output.extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| Error::UnsupportedFormat("output has no extension".into()))?;

    if out_ext != "wav" {
        return Err(Error::UnsupportedFormat(
            format!("audio extraction only supports WAV output; got '.{out_ext}'")
        ));
    }

    let kind = ContainerKind::from_path(input).ok_or_else(|| {
        Error::UnsupportedFormat(format!(
            "unrecognised container extension: '{}'",
            input.display()
        ))
    })?;

    match kind {
        ContainerKind::Mp4 => {
            Err(Error::Video(
                "audio extraction from MP4 is not yet supported \
                 (AAC decoder not implemented). \
                 Use a WebM/MKV source with PCM audio, \
                 or extract audio externally first.".into()
            ))
        }
        ContainerKind::Mkv => extract_mkv_audio(input, output),
        ContainerKind::Ivf => {
            Err(Error::Video("IVF containers carry video only — no audio stream.".into()))
        }
    }
}

fn extract_mkv_audio(input: &Path, output: &Path) -> Result<()> {
    use std::io::BufReader;
    let file = File::open(input)?;
    let demuxer = WebmDemuxer::open(BufReader::new(file))?;

    let streams = demuxer.streams().to_vec();
    let audio_stream = streams.iter().find(|s| s.is_audio()).ok_or_else(|| {
        Error::Video("no audio stream in container".into())
    })?;

    let audio_idx = audio_stream.index;
    let codec = &audio_stream.codec;

    // We can handle Vorbis (our existing VorbisDecoder) and PCM directly.
    // Opus support would require an Opus decoder — not yet implemented.
    match codec {
        CodecId::Vorbis | CodecId::Pcm => {}
        other => {
            return Err(Error::Video(format!(
                "audio codec {other} extraction from MKV not yet supported"
            )));
        }
    }

    // Collect all audio packets into a single blob then decode via Vorbis.
    // (Vorbis inside Ogg is handled by lewton; raw WebM Vorbis packets
    //  need the Ogg framing removed, which lewton's inside_ogg does for us
    //  when we use the OGG path. For raw WebM Vorbis packets we need a
    //  different approach — collect and re-wrap into Ogg, or use a
    //  packet-level interface. lewton only exposes an Ogg-stream API,
    //  so we take the pragmatic route: re-wrap raw Vorbis packets into
    //  a temporary Ogg stream in memory.)
    //
    // For PCM tracks (rare but valid in MKV), we parse the raw bytes
    // directly according to the track's sample format.

    if *codec == CodecId::Pcm {
        return extract_pcm_from_mkv(demuxer, audio_idx, audio_stream, output);
    }

    // Vorbis: collect raw packets; build temporary Ogg wrapper in memory.
    // The first three packets are the Vorbis header packets (identification,
    // comment, setup). For the common case they are stored in codec_private.
    let _vorbis_priv = audio_stream.codec_private.clone();
    // For now, fall back with a clear message — Vorbis-in-WebM raw packet
    // re-wrapping into Ogg is a non-trivial implementation.  The proper
    // future fix is to add a lewton raw-packet API or use a different crate.
    Err(Error::Video(
        "Vorbis audio extraction from WebM requires raw-packet decoding \
         (Ogg re-wrapping not yet implemented). \
         Convert the WebM to an Ogg Vorbis file first with an external tool.".into()
    ))
}

fn extract_pcm_from_mkv<D: ContainerDemuxer>(
    mut demuxer: D,
    audio_idx: usize,
    stream: &crate::video::StreamInfo,
    output: &Path,
) -> Result<()> {
    // Accumulate raw PCM bytes (assumed i16 LE stereo/mono).
    let mut raw: Vec<u8> = Vec::new();
    while let Some((idx, pkt)) = demuxer.next_packet()? {
        if idx == audio_idx {
            raw.extend_from_slice(&pkt.data);
        }
    }

    let channels = stream.channels.max(1);
    let sample_rate = stream.sample_rate.max(8000);
    // Assume i16 little-endian (the most common MKV/PCM subformat)
    let samples: Vec<f32> = raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
        .collect();

    let frame = AudioFrame::new(sample_rate, channels, samples);

    // Use the existing WAV encoder path.
    AudioGraph::new().run_frame(&frame, output)
}

// ── helpers ───────────────────────────────────────────────────────────────────

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

/// Convert any supported YUV or RGB frame to RGB8.
///
/// Accepts: `Rgb8`, `Yuv420p`, `Yuv420p10`, `Yuv420p12`, `Yuv422p`, `Yuv444p`.
pub fn any_yuv_to_rgb8(frame: &Frame) -> Result<Frame> {
    match frame.format {
        PixelFormat::Rgb8    => Ok(frame.clone()),
        PixelFormat::Yuv420p => yuv420p_to_rgb8(frame),
        PixelFormat::Yuv420p10 | PixelFormat::Yuv420p12 => yuv420p_hdr_to_rgb8(frame),
        PixelFormat::Yuv422p => yuv422p_to_rgb8(frame),
        PixelFormat::Yuv444p => yuv444p_to_rgb8(frame),
        fmt => Err(Error::Filter(format!("unsupported pixel format for RGB conversion: {fmt:?}"))),
    }
}

/// BT.601 full-range YUV420p (8-bit) → packed RGB8.
pub fn yuv420p_to_rgb8(frame: &Frame) -> Result<Frame> {
    if frame.format != PixelFormat::Yuv420p {
        return Err(Error::Filter("yuv420p_to_rgb8 expects Yuv420p input".into()));
    }
    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = (w + 1) / 2;
    let uv_h = (h + 1) / 2;
    let y_plane = &frame.data[..w * h];
    let u_plane = &frame.data[w * h..w * h + uv_w * uv_h];
    let v_plane = &frame.data[w * h + uv_w * uv_h..];

    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col] as f32;
            let u = u_plane[(row / 2) * uv_w + col / 2] as f32 - 128.0;
            let v = v_plane[(row / 2) * uv_w + col / 2] as f32 - 128.0;
            yuv_to_rgb_pixel(&mut rgb, row * w + col, y, u, v);
        }
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// Convert an `Rgb8`/`Rgba8` frame to packed planar `Yuv420p` (Y then U then V),
/// using full-range BT.601 (JPEG) coefficients — the exact inverse of
/// [`yuv420p_to_rgb8`]. Chroma is 4:2:0 subsampled by averaging each 2×2 block.
///
/// This is the encode-side companion to the decoders' YUV→RGB path: the
/// compositor renders RGBA, and the exporter feeds `rav1e` YUV420p.
pub fn rgb8_to_yuv420p(frame: &Frame) -> Result<Frame> {
    let bpp = match frame.format {
        PixelFormat::Rgb8 => 3usize,
        PixelFormat::Rgba8 => 4usize,
        other => {
            return Err(Error::Filter(format!("rgb8_to_yuv420p expects Rgb8/Rgba8, got {other:?}")))
        }
    };
    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = w.div_ceil(2);
    let uv_h = h.div_ceil(2);

    let mut y_plane = vec![0u8; w * h];
    let mut u_plane = vec![0u8; uv_w * uv_h];
    let mut v_plane = vec![0u8; uv_w * uv_h];

    // Luma for every pixel.
    for row in 0..h {
        for col in 0..w {
            let i = (row * w + col) * bpp;
            let r = frame.data[i] as f32;
            let g = frame.data[i + 1] as f32;
            let b = frame.data[i + 2] as f32;
            y_plane[row * w + col] = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0) as u8;
        }
    }

    // Chroma: average each 2×2 block (clamped at edges).
    for by in 0..uv_h {
        for bx in 0..uv_w {
            let (mut cb, mut cr, mut n) = (0.0f32, 0.0f32, 0.0f32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let row = (by * 2 + dy).min(h - 1);
                    let col = (bx * 2 + dx).min(w - 1);
                    let i = (row * w + col) * bpp;
                    let r = frame.data[i] as f32;
                    let g = frame.data[i + 1] as f32;
                    let b = frame.data[i + 2] as f32;
                    cb += -0.168736 * r - 0.331264 * g + 0.5 * b + 128.0;
                    cr += 0.5 * r - 0.418688 * g - 0.081312 * b + 128.0;
                    n += 1.0;
                }
            }
            u_plane[by * uv_w + bx] = (cb / n).round().clamp(0.0, 255.0) as u8;
            v_plane[by * uv_w + bx] = (cr / n).round().clamp(0.0, 255.0) as u8;
        }
    }

    let mut data = y_plane;
    data.extend_from_slice(&u_plane);
    data.extend_from_slice(&v_plane);
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Yuv420p, data))
}

/// BT.2020 10/12-bit YUV420p → 8-bit RGB8 (tone-mapped by bit-shift).
pub fn yuv420p_hdr_to_rgb8(frame: &Frame) -> Result<Frame> {
    let depth = match frame.format {
        PixelFormat::Yuv420p10 => 10u32,
        PixelFormat::Yuv420p12 => 12u32,
        _ => return Err(Error::Filter("yuv420p_hdr_to_rgb8 expects Yuv420p10/12".into())),
    };
    let shift = depth - 8; // shift down to 8-bit range
    let bias  = (1u16 << (depth - 1)) as f32; // 512 for 10-bit, 2048 for 12-bit

    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = (w + 1) / 2;
    let uv_h = (h + 1) / 2;

    // Samples are little-endian u16.
    let read_u16 = |data: &[u8], idx: usize| -> f32 {
        let b0 = data[idx * 2] as u16;
        let b1 = data[idx * 2 + 1] as u16;
        ((b0 | (b1 << 8)) >> shift) as f32
    };

    let y_end  = w * h * 2;
    let u_end  = y_end + uv_w * uv_h * 2;
    let y_plane = &frame.data[..y_end];
    let u_plane = &frame.data[y_end..u_end];
    let v_plane = &frame.data[u_end..];

    let bias8 = (bias as u32 >> shift) as f32;

    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let y = read_u16(y_plane, row * w + col);
            let u = read_u16(u_plane, (row / 2) * uv_w + col / 2) - bias8;
            let v = read_u16(v_plane, (row / 2) * uv_w + col / 2) - bias8;
            yuv_to_rgb_pixel(&mut rgb, row * w + col, y, u, v);
        }
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// BT.601 YUV422p (8-bit) → RGB8.
pub fn yuv422p_to_rgb8(frame: &Frame) -> Result<Frame> {
    if frame.format != PixelFormat::Yuv422p {
        return Err(Error::Filter("yuv422p_to_rgb8 expects Yuv422p input".into()));
    }
    let w = frame.width as usize;
    let h = frame.height as usize;
    let uv_w = (w + 1) / 2;
    let y_plane = &frame.data[..w * h];
    let u_plane = &frame.data[w * h..w * h + uv_w * h];
    let v_plane = &frame.data[w * h + uv_w * h..];

    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col] as f32;
            let u = u_plane[row * uv_w + col / 2] as f32 - 128.0;
            let v = v_plane[row * uv_w + col / 2] as f32 - 128.0;
            yuv_to_rgb_pixel(&mut rgb, row * w + col, y, u, v);
        }
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// BT.601 YUV444p (8-bit) → RGB8.
pub fn yuv444p_to_rgb8(frame: &Frame) -> Result<Frame> {
    if frame.format != PixelFormat::Yuv444p {
        return Err(Error::Filter("yuv444p_to_rgb8 expects Yuv444p input".into()));
    }
    let w = frame.width as usize;
    let h = frame.height as usize;
    let n = w * h;
    let y_plane = &frame.data[..n];
    let u_plane = &frame.data[n..2 * n];
    let v_plane = &frame.data[2 * n..];

    let mut rgb = vec![0u8; n * 3];
    for i in 0..n {
        let y = y_plane[i] as f32;
        let u = u_plane[i] as f32 - 128.0;
        let v = v_plane[i] as f32 - 128.0;
        yuv_to_rgb_pixel(&mut rgb, i, y, u, v);
    }
    Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgb8, rgb))
}

/// BT.601 YUV → RGB conversion for one pixel, written into `rgb` at `pixel_idx`.
#[inline(always)]
fn yuv_to_rgb_pixel(rgb: &mut [u8], pixel_idx: usize, y: f32, u: f32, v: f32) {
    let r = (y + 1.402 * v).clamp(0.0, 255.0) as u8;
    let g = (y - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8;
    let b = (y + 1.772 * u).clamp(0.0, 255.0) as u8;
    let off = pixel_idx * 3;
    rgb[off]     = r;
    rgb[off + 1] = g;
    rgb[off + 2] = b;
}
