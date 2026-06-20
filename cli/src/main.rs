use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;
use ferrox_core::{
    demux_graph,
    filters::{ResampleFilter, ResizeFilter, VolumeFilter},
    AudioGraph, Graph,
};

#[derive(Parser)]
#[command(name = "ferrox", version, about = "Graph-based media processing pipeline")]
struct Cli {
    /// Logging verbosity: off, error, warn, info, debug, trace.
    #[arg(long, global = true, default_value = "info", env = "FERROX_LOG")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert an image from one format to another (PNG ↔ JPEG).
    Convert {
        /// Input file path.
        input: PathBuf,
        /// Output file path.
        output: PathBuf,
    },

    /// Resize an image and write to output (format driven by output extension).
    Resize {
        /// Input file path.
        input: PathBuf,
        /// Output file path (use .jpg to transcode while resizing).
        output: PathBuf,
        /// Target width in pixels.
        #[arg(short = 'W', long)]
        width: u32,
        /// Target height in pixels.
        #[arg(short = 'H', long)]
        height: u32,
    },

    /// Convert audio between formats (WAV, MP3, FLAC, OGG → WAV).
    AudioConvert {
        /// Input audio file (wav/mp3/flac/ogg).
        input: PathBuf,
        /// Output audio file (wav).
        output: PathBuf,
    },

    /// Adjust audio volume (gain multiplier).
    AudioVolume {
        /// Input audio file.
        input: PathBuf,
        /// Output audio file.
        output: PathBuf,
        /// Gain multiplier (e.g. 0.5 for half volume, 2.0 for double).
        #[arg(short, long)]
        gain: f32,
    },

    /// Resample audio to a target sample rate.
    AudioResample {
        /// Input audio file.
        input: PathBuf,
        /// Output audio file.
        output: PathBuf,
        /// Target sample rate in Hz (e.g. 44100, 48000).
        #[arg(short = 'r', long)]
        rate: u32,
    },

    /// Extract video frames from a container as PNG images.
    ///
    /// Example: ferrox video-extract-frames input.webm frame_%03d.png --count 10
    VideoExtractFrames {
        /// Input video file (WebM/MKV with VP8, or MP4).
        input: PathBuf,
        /// Output path pattern with a printf-style %d placeholder, e.g. frame_%03d.png.
        output_pattern: String,
        /// Maximum number of frames to extract.
        #[arg(short, long, default_value = "10")]
        count: usize,
    },

    /// Extract audio from a video container to WAV.
    ///
    /// Example: ferrox video-extract-audio input.webm audio.wav
    VideoExtractAudio {
        /// Input video file (WebM/MKV with PCM audio).
        input: PathBuf,
        /// Output WAV file.
        output: PathBuf,
    },

    /// Print stream metadata for a video/audio container file.
    VideoInfo {
        /// Input container file (WebM, MKV, or MP4).
        input: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cli.log_level.parse().unwrap_or_default()),
        )
        .with_target(false)
        .compact()
        .init();

    match cli.command {
        Command::Convert { input, output } => {
            let graph = Graph::new();
            graph
                .run(&input, &output)
                .with_context(|| format!("converting '{}' → '{}'", input.display(), output.display()))?;
            info!("converted {} → {}", input.display(), output.display());
        }

        Command::Resize { input, output, width, height } => {
            let graph = Graph::new().with_filter(ResizeFilter::new(width, height));
            graph
                .run(&input, &output)
                .with_context(|| {
                    format!(
                        "resizing '{}' → '{}' ({}×{})",
                        input.display(), output.display(), width, height,
                    )
                })?;
            info!("resized to {}×{}: {} → {}", width, height, input.display(), output.display());
        }

        Command::AudioConvert { input, output } => {
            let graph = AudioGraph::new();
            graph
                .run(&input, &output)
                .with_context(|| {
                    format!("audio-converting '{}' → '{}'", input.display(), output.display())
                })?;
            info!("audio-converted {} → {}", input.display(), output.display());
        }

        Command::AudioVolume { input, output, gain } => {
            let graph = AudioGraph::new().with_filter(VolumeFilter::new(gain));
            graph
                .run(&input, &output)
                .with_context(|| {
                    format!(
                        "adjusting volume (gain={gain}) '{}' → '{}'",
                        input.display(), output.display()
                    )
                })?;
            info!("volume adjusted (gain={gain}): {} → {}", input.display(), output.display());
        }

        Command::AudioResample { input, output, rate } => {
            let graph = AudioGraph::new().with_filter(ResampleFilter::new(rate));
            graph
                .run(&input, &output)
                .with_context(|| {
                    format!(
                        "resampling to {rate}Hz '{}' → '{}'",
                        input.display(), output.display()
                    )
                })?;
            info!("resampled to {rate}Hz: {} → {}", input.display(), output.display());
        }

        Command::VideoExtractFrames { input, output_pattern, count } => {
            let result = demux_graph::extract_frames(&input, &output_pattern, count)
                .with_context(|| {
                    format!(
                        "extracting {count} frame(s) from '{}' → '{output_pattern}'",
                        input.display()
                    )
                })?;
            info!(
                "extracted {} frame(s) ({} skipped) from {}",
                result.frame_paths.len(),
                result.skipped,
                input.display()
            );
            for p in &result.frame_paths {
                println!("{}", p.display());
            }
        }

        Command::VideoExtractAudio { input, output } => {
            demux_graph::extract_audio(&input, &output)
                .with_context(|| {
                    format!(
                        "extracting audio from '{}' → '{}'",
                        input.display(), output.display()
                    )
                })?;
            info!("extracted audio: {} → {}", input.display(), output.display());
        }

        Command::VideoInfo { input } => {
            use ferrox_core::{codecs::{Mp4Demuxer, WebmDemuxer}, demux_graph::ContainerKind, traits::ContainerDemuxer};
            use std::fs::File;
            use ferrox_core::IvfDemuxer;
            let kind = ContainerKind::from_path(&input)
                .ok_or_else(|| anyhow::anyhow!("unrecognised container extension"))?;
            let streams: Vec<_> = match kind {
                ContainerKind::Mp4 => {
                    let f = File::open(&input)?;
                    let size = f.metadata()?.len();
                    Mp4Demuxer::open(f, size)?.streams().to_vec()
                }
                ContainerKind::Mkv => {
                    let f = File::open(&input)?;
                    WebmDemuxer::open(f)?.streams().to_vec()
                }
                ContainerKind::Ivf => {
                    let f = File::open(&input)?;
                    IvfDemuxer::open(f)?.streams().to_vec()
                }
            };
            println!("Streams in '{}':", input.display());
            for s in &streams {
                println!(
                    "  [{}] {:?} codec={} {}",
                    s.index, s.kind, s.codec,
                    if s.is_video() {
                        format!("{}×{} {:.2}fps", s.width, s.height, s.frame_rate)
                    } else if s.is_audio() {
                        format!("{}Hz {}ch", s.sample_rate, s.channels)
                    } else {
                        String::new()
                    }
                );
            }
        }
    }

    Ok(())
}
