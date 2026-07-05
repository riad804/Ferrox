//! Headless video export: sample the timeline across `[0, duration)` at the
//! target frame rate, compose each frame (the full color/keyer/mask/blend/LUT
//! pipeline runs here), convert RGBA → YUV420p, encode with `rav1e` (AV1) and
//! mux into a fragmented MP4 (`av01`).
//!
//! This is the end-to-end smoke test for the whole engine: if timeline
//! sampling, keyframes or transitions were wrong, the exported file shows it.
//! The loop is serial (the encoder is serial); parallel frame *prefetch* is a
//! future optimization behind this same API.

use std::fs::File;
use std::path::Path;

use ferrox_core::traits::VideoEncoder;
use ferrox_core::video::{CodecId, StreamInfo, StreamKind, VideoFrame};
use ferrox_core::{rgb8_to_yuv420p, Av1Encoder, Frame, Mp4Muxer, PixelFormat};

use crate::editor::Editor;
use crate::error::{Result, SdkError};

/// Output specification for an export.
#[derive(Debug, Clone, Copy)]
pub struct ExportSettings {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    /// rav1e speed preset (0 = best quality … 10 = fastest).
    pub speed: u8,
    /// Base quantizer (0 = lossless … 255 = worst).
    pub quantizer: usize,
}

impl ExportSettings {
    /// A reasonable default at the given size and integer fps.
    pub fn new(width: u32, height: u32, fps: u32) -> Self {
        Self { width, height, fps_num: fps, fps_den: 1, speed: 8, quantizer: 100 }
    }

    fn fps(&self) -> f64 {
        self.fps_num as f64 / self.fps_den.max(1) as f64
    }
}

/// Export `editor`'s project to a fragmented-MP4 (AV1) file at `path`.
///
/// `progress(done, total)` is called after each encoded frame so callers can
/// drive a progress bar.
pub fn export_mp4<P: AsRef<Path>>(
    editor: &Editor,
    path: P,
    settings: &ExportSettings,
    mut progress: impl FnMut(u32, u32),
) -> Result<()> {
    if settings.width == 0 || settings.height == 0 {
        return Err(SdkError::InvalidHandle("export size must be non-zero".into()));
    }
    let fps = settings.fps();
    let duration = editor.with_project(|p| p.duration())?;
    let total = (((duration * fps).ceil()) as u32).max(1);

    let mut encoder = Av1Encoder::new(
        settings.width,
        settings.height,
        settings.speed,
        settings.quantizer,
        settings.fps_num as u64,
        settings.fps_den as u64,
    )?;

    let stream = StreamInfo {
        index: 0,
        kind: StreamKind::Video,
        codec: CodecId::Av1,
        width: settings.width,
        height: settings.height,
        frame_rate: fps,
        sample_rate: 0,
        channels: 0,
        codec_private: Vec::new(),
    };

    // Progressive (non-fragmented) MP4 for broad player/demuxer compatibility.
    let file = File::create(path.as_ref()).map_err(|e| SdkError::Core(e.into()))?;
    let mut muxer = Mp4Muxer::new(file, stream, settings.fps_num as u64, settings.fps_den as u64)?;

    for i in 0..total {
        let t = i as f64 / fps;
        let rgba = editor.render_frame(t, settings.width, settings.height)?;
        let rgba_frame = Frame::new(settings.width, settings.height, PixelFormat::Rgba8, rgba);
        let yuv = rgb8_to_yuv420p(&rgba_frame)?;
        let vframe = VideoFrame::new(yuv, i as u64, 1, false);
        for pkt in encoder.encode(&vframe)? {
            muxer.write_packet(&pkt)?;
        }
        progress(i + 1, total);
    }

    // Drain the encoder's pipeline, then finalize the container.
    for pkt in encoder.flush()? {
        muxer.write_packet(&pkt)?;
    }
    muxer.finish()?;
    Ok(())
}
