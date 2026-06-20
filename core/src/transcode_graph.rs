use std::path::Path;
use tracing::{debug, info, instrument, warn};
use crate::{
    codecs::video::{Av1Encoder, WebmMuxer},
    demux_graph::ContainerKind,
    error::{Error, Result},
    frame::{Frame, PixelFormat},
    traits::{ContainerDemuxer, ContainerMuxer, VideoDecoder, VideoEncoder},
    video::{CodecId, StreamInfo, VideoFrame},
};

/// Options for transcoding video.
#[derive(Debug, Clone)]
pub struct TranscodeOptions {
    /// Video codec to encode with. Currently only `"av1"` is supported.
    pub video_codec: VideoCodecChoice,
    /// Optional resize: `Some((width, height))` to scale output.
    pub resize: Option<(u32, u32)>,
    /// Target frame rate (fps_num, fps_den). `None` = keep source rate.
    pub fps: Option<(u64, u64)>,
    /// rav1e speed preset 0–10 (lower = smaller / slower).
    pub speed: u8,
    /// rav1e base quantizer 0–255 (lower = better quality).
    pub quantizer: usize,
    /// If `true`, the video stream is passed through without decoding.
    /// Only valid when the source codec matches the target codec.
    pub copy_video: bool,
}

impl Default for TranscodeOptions {
    fn default() -> Self {
        Self {
            video_codec: VideoCodecChoice::Av1,
            resize: None,
            fps: None,
            speed: 6,
            quantizer: 100,
            copy_video: false,
        }
    }
}

/// Which video codec to encode with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodecChoice {
    /// AV1 via `rav1e` (pure Rust).
    Av1,
}

/// Progress callback type. Called with `(frames_encoded, frames_total)`.
/// `frames_total` is `None` when the stream length is unknown.
pub type ProgressCb = Box<dyn Fn(usize, Option<usize>) + Send>;

/// Result of a transcode operation.
#[derive(Debug)]
pub struct TranscodeResult {
    pub frames_encoded: usize,
    pub frames_copied: usize,
    pub output_path: std::path::PathBuf,
}

/// Demux → decode → filter → encode → mux pipeline.
///
/// Reads a video container from `input`, transcodes it according to
/// `opts`, and writes the result to `output` (WebM for AV1).
#[instrument(skip(opts, progress), fields(input = %input.display(), output = %output.display()))]
pub fn transcode(
    input: &Path,
    output: &Path,
    opts: &TranscodeOptions,
    progress: Option<ProgressCb>,
) -> Result<TranscodeResult> {
    use std::fs::File;

    let kind = ContainerKind::from_path(input).ok_or_else(|| {
        Error::UnsupportedFormat(format!(
            "unrecognised input container: '{}'",
            input.display()
        ))
    })?;

    // --- demux ---
    match kind {
        ContainerKind::Mp4 => {
            let f = File::open(input)?;
            let size = f.metadata()?.len();
            let demuxer = crate::codecs::video::Mp4Demuxer::open(f, size)?;
            transcode_with_demuxer(demuxer, output, opts, progress)
        }
        ContainerKind::Mkv => {
            let f = File::open(input)?;
            let demuxer = crate::codecs::video::WebmDemuxer::open(f)?;
            transcode_with_demuxer(demuxer, output, opts, progress)
        }
        ContainerKind::Ivf => {
            let f = File::open(input)?;
            let demuxer = crate::codecs::video::IvfDemuxer::open(f)?;
            transcode_with_demuxer(demuxer, output, opts, progress)
        }
    }
}

fn transcode_with_demuxer<D: ContainerDemuxer>(
    mut demuxer: D,
    output: &Path,
    opts: &TranscodeOptions,
    progress: Option<ProgressCb>,
) -> Result<TranscodeResult> {
    use std::fs::File;

    let streams = demuxer.streams().to_vec();
    let video_stream = streams.iter().find(|s| s.is_video())
        .ok_or_else(|| Error::Video("no video stream found".into()))?;

    let src_w = video_stream.width;
    let src_h = video_stream.height;
    let src_codec = video_stream.codec.clone();
    let video_idx = video_stream.index;

    // Determine output dimensions.
    let (out_w, out_h) = opts.resize.unwrap_or((src_w, src_h));

    // Determine frame rate from stream metadata or default 30 fps.
    let (fps_num, fps_den) = opts.fps.unwrap_or_else(|| {
        let fps = video_stream.frame_rate;
        if fps > 0.0 {
            // Convert to integer ratio (1/1000 timebase granularity).
            let num = (fps * 1000.0).round() as u64;
            (num, 1000)
        } else {
            (30, 1)
        }
    });

    info!(src_w, src_h, out_w, out_h, fps_num, fps_den, codec = %src_codec, "transcode start");

    if opts.copy_video {
        return transcode_copy(demuxer, output, video_idx, &streams, fps_num, fps_den);
    }

    // Only VP8 source decoding is currently implemented.
    match &src_codec {
        CodecId::Vp8 => {}
        other => {
            return Err(Error::Video(format!(
                "source codec {other} cannot be decoded; only VP8 is supported as transcode input"
            )));
        }
    }

    let mut decoder = crate::codecs::video::Vp8Decoder;

    // Build output stream descriptor (AV1 video only).
    let out_stream = StreamInfo {
        index: 0,
        kind: crate::video::StreamKind::Video,
        codec: CodecId::Av1,
        width: out_w,
        height: out_h,
        frame_rate: fps_num as f64 / fps_den as f64,
        sample_rate: 0,
        channels: 0,
        codec_private: Vec::new(),
    };

    let out_file = File::create(output)?;
    let mut muxer = WebmMuxer::new(out_file, &[out_stream], fps_num, fps_den)?;

    let mut encoder = Av1Encoder::new(out_w, out_h, opts.speed, opts.quantizer, fps_num, fps_den)?;

    muxer.write_header()?;

    let mut frames_encoded = 0usize;

    loop {
        let Some((stream_idx, packet)) = demuxer.next_packet()? else { break };
        if stream_idx != video_idx { continue; }

        let vf = match decoder.decode_packet(&packet) {
            Ok(f) => f,
            Err(e) => {
                warn!(pts = packet.pts, err = %e, "frame skipped during transcode");
                continue;
            }
        };

        // Resize if needed.
        let vf = if out_w != src_w || out_h != src_h {
            resize_yuv420p(vf, out_w, out_h)?
        } else {
            vf
        };

        let encoded = encoder.encode(&vf)?;
        for pkt in encoded {
            muxer.write_packet(&pkt)?;
            frames_encoded += 1;
            debug!(frames_encoded, pts = pkt.pts, "muxed packet");
            if let Some(cb) = &progress {
                cb(frames_encoded, None);
            }
        }
    }

    // Flush encoder.
    let flushed = encoder.flush()?;
    for pkt in flushed {
        muxer.write_packet(&pkt)?;
        frames_encoded += 1;
        if let Some(cb) = &progress {
            cb(frames_encoded, None);
        }
    }

    muxer.write_trailer()?;

    info!(frames_encoded, "transcode complete");
    Ok(TranscodeResult {
        frames_encoded,
        frames_copied: 0,
        output_path: output.to_path_buf(),
    })
}

/// Copy-mode: pass video packets directly to the muxer without decoding.
fn transcode_copy<D: ContainerDemuxer>(
    mut demuxer: D,
    output: &Path,
    video_idx: usize,
    streams: &[StreamInfo],
    fps_num: u64,
    fps_den: u64,
) -> Result<TranscodeResult> {
    use std::fs::File;
    use crate::video::EncodedPacket;

    let out_file = File::create(output)?;
    let mut muxer = WebmMuxer::new(out_file, streams, fps_num, fps_den)?;
    muxer.write_header()?;

    let mut frames_copied = 0usize;
    loop {
        let Some((stream_idx, packet)) = demuxer.next_packet()? else { break };
        if stream_idx != video_idx { continue; }
        let enc = EncodedPacket {
            data: packet.data,
            pts: packet.pts,
            duration: packet.duration,
            is_keyframe: packet.is_keyframe,
            stream_index: 0,
        };
        muxer.write_packet(&enc)?;
        frames_copied += 1;
    }
    muxer.write_trailer()?;

    Ok(TranscodeResult {
        frames_encoded: 0,
        frames_copied,
        output_path: output.to_path_buf(),
    })
}

// ── YUV420p resize ────────────────────────────────────────────────────────────

/// Nearest-neighbour YUV420p resize.
///
/// Uses bilinear sampling for luma and nearest-neighbour for chroma.
/// Fast enough for integration testing; a production path would use a
/// proper scaler (e.g., libyuv or a Rust equivalent).
fn resize_yuv420p(vf: VideoFrame, out_w: u32, out_h: u32) -> Result<VideoFrame> {
    let in_w = vf.width() as usize;
    let in_h = vf.height() as usize;
    let ow = out_w as usize;
    let oh = out_h as usize;

    let src_y_len = in_w * in_h;
    let src_uv_w = (in_w + 1) / 2;
    let src_uv_h = (in_h + 1) / 2;
    let src_u_len = src_uv_w * src_uv_h;

    let dst_uv_w = (ow + 1) / 2;
    let dst_uv_h = (oh + 1) / 2;

    let src_y = &vf.frame.data[..src_y_len];
    let src_u = &vf.frame.data[src_y_len..src_y_len + src_u_len];
    let src_v = &vf.frame.data[src_y_len + src_u_len..];

    let mut dst = Vec::with_capacity(ow * oh + 2 * dst_uv_w * dst_uv_h);

    // Luma (nearest neighbour).
    for dy in 0..oh {
        let sy = dy * in_h / oh;
        for dx in 0..ow {
            let sx = dx * in_w / ow;
            dst.push(src_y[sy * in_w + sx]);
        }
    }
    // Cb.
    for dy in 0..dst_uv_h {
        let sy = dy * src_uv_h / dst_uv_h;
        for dx in 0..dst_uv_w {
            let sx = dx * src_uv_w / dst_uv_w;
            dst.push(src_u[sy * src_uv_w + sx]);
        }
    }
    // Cr.
    for dy in 0..dst_uv_h {
        let sy = dy * src_uv_h / dst_uv_h;
        for dx in 0..dst_uv_w {
            let sx = dx * src_uv_w / dst_uv_w;
            dst.push(src_v[sy * src_uv_w + sx]);
        }
    }

    let frame = Frame::new(out_w, out_h, PixelFormat::Yuv420p, dst);
    Ok(VideoFrame::new(frame, vf.pts, vf.duration, vf.is_keyframe))
}
