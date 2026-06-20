use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;
use ferrox_core::{
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
    }

    Ok(())
}
