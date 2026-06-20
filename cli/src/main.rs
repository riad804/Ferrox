use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use tracing::info;
use ferrox_core::{
    demux_graph,
    filter_graph::FilterGraph,
    filters::{ResampleFilter, ResizeFilter, VolumeFilter},
    transcode_graph::{TranscodeOptions, VideoCodecChoice},
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

    /// Apply a filtergraph expression to an image (similar to -filter_complex).
    ///
    /// Example: ferrox filter-apply input.png output.png "blur=2.0,grayscale"
    ///
    /// Supported tokens: blur=<sigma>, grayscale, negate, scale=<w>:<h>,
    /// brightness=<delta>, contrast=<factor>, saturation=<factor>.
    FilterApply {
        /// Input image file (PNG/JPEG).
        input: PathBuf,
        /// Output image file.
        output: PathBuf,
        /// Filtergraph expression (comma-separated filter tokens).
        #[arg(short = 'f', long = "filter-complex")]
        filter_complex: String,
    },

    /// Decode an animated GIF into individual PNG frames.
    ///
    /// Example: ferrox gif-decode animation.gif frames/frame_%03d.png
    GifDecode {
        /// Input GIF file.
        input: PathBuf,
        /// Output path pattern with printf-style %d, e.g. frame_%03d.png.
        output_pattern: String,
        /// Maximum frames to extract (0 = all).
        #[arg(long, default_value = "0")]
        max_frames: usize,
    },

    /// Encode PNG frames into an animated GIF.
    ///
    /// Frames are read in the order provided.
    ///
    /// Example: ferrox gif-encode frame_001.png frame_002.png -o out.gif --delay 10
    GifEncode {
        /// Input PNG frames (one or more).
        inputs: Vec<PathBuf>,
        /// Output GIF file.
        #[arg(short, long)]
        output: PathBuf,
        /// Frame delay in centiseconds (default 10 = 100 ms per frame).
        #[arg(long, default_value = "10")]
        delay: u16,
        /// Palette size (2–256; larger = better quality but bigger file).
        #[arg(long, default_value = "256")]
        palette: usize,
    },

    /// Transcode a video file (decode → filter → encode → mux).
    ///
    /// Example: ferrox transcode input.ivf output.webm --codec av1 --resize 320x240
    Transcode {
        /// Input video file (IVF/WebM/MKV/MP4).
        input: PathBuf,
        /// Output file (.webm).
        output: PathBuf,
        /// Video codec: av1 (default), copy.
        #[arg(short = 'c', long = "codec", default_value = "av1")]
        codec: String,
        /// Resize output: WIDTHxHEIGHT, e.g. 320x240.
        #[arg(long)]
        resize: Option<String>,
        /// Output frame rate: NUM/DEN or NUM (e.g. 30 or 24000/1001).
        #[arg(long)]
        fps: Option<String>,
        /// rav1e speed preset 0 (slowest) to 10 (fastest).
        #[arg(long, default_value = "6")]
        speed: u8,
        /// rav1e quantizer 0 (lossless) to 255 (worst).
        #[arg(long, default_value = "100")]
        quantizer: usize,
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

        Command::Transcode { input, output, codec, resize, fps, speed, quantizer } => {
            let video_codec = match codec.to_ascii_lowercase().as_str() {
                "av1" => VideoCodecChoice::Av1,
                "copy" => VideoCodecChoice::Av1, // copy handled via flag below
                other => anyhow::bail!("unsupported video codec '{}'; supported: av1, copy", other),
            };
            let copy_video = codec.to_ascii_lowercase() == "copy";

            let resize_dim = resize.as_deref().map(|s| {
                let (w, h) = s.split_once('x')
                    .ok_or_else(|| anyhow::anyhow!("--resize must be WIDTHxHEIGHT, e.g. 320x240"))?;
                Ok::<_, anyhow::Error>((w.parse::<u32>()?, h.parse::<u32>()?))
            }).transpose()?;

            let fps_ratio = fps.as_deref().map(|s| {
                if let Some((n, d)) = s.split_once('/') {
                    Ok::<_, anyhow::Error>((n.parse::<u64>()?, d.parse::<u64>()?))
                } else {
                    Ok((s.parse::<u64>()?, 1u64))
                }
            }).transpose()?;

            let opts = TranscodeOptions {
                video_codec,
                resize: resize_dim,
                fps: fps_ratio,
                speed,
                quantizer,
                copy_video,
            };

            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {msg}")
                    .unwrap()
            );
            pb.set_message("transcoding…");
            let pb_clone = pb.clone();

            let result = ferrox_core::transcode_graph::transcode(
                &input,
                &output,
                &opts,
                Some(Box::new(move |frames, _total| {
                    pb_clone.set_message(format!("encoded {frames} frames"));
                    pb_clone.tick();
                })),
            ).with_context(|| format!(
                "transcoding '{}' → '{}'",
                input.display(), output.display()
            ))?;

            pb.finish_and_clear();
            info!(
                frames_encoded = result.frames_encoded,
                frames_copied = result.frames_copied,
                "transcode complete: {} → {}",
                input.display(), output.display()
            );
            println!(
                "Transcoded {} → {} ({} frames encoded, {} copied)",
                input.display(), output.display(),
                result.frames_encoded, result.frames_copied
            );
        }

        Command::FilterApply { input, output, filter_complex } => {
            use ferrox_core::codecs::{JpegDecoder, JpegEncoder, PngDecoder, PngEncoder};
            use ferrox_core::traits::{DynDecoder, DynEncoder};
            use std::fs::File;
            use std::io::BufWriter;

            let in_ext = input.extension().and_then(|e| e.to_str())
                .ok_or_else(|| anyhow::anyhow!("no extension on input file"))?
                .to_ascii_lowercase();
            let out_ext = output.extension().and_then(|e| e.to_str())
                .ok_or_else(|| anyhow::anyhow!("no extension on output file"))?
                .to_ascii_lowercase();

            let mut in_file = std::io::BufReader::new(File::open(&input)?);
            let frame = match in_ext.as_str() {
                "png"  => PngDecoder.decode_dyn(&mut in_file)?,
                "jpg" | "jpeg" => JpegDecoder.decode_dyn(&mut in_file)?,
                other  => anyhow::bail!("unsupported input format '{other}'"),
            };

            let out_frame = FilterGraph::parse_and_run(frame, &filter_complex)
                .with_context(|| format!("applying filtergraph '{filter_complex}'"))?;

            let out_file = BufWriter::new(File::create(&output)?);
            match out_ext.as_str() {
                "png"  => PngEncoder.encode_dyn(&out_frame, &mut { out_file })?,
                "jpg" | "jpeg" => JpegEncoder::default().encode_dyn(&out_frame, &mut { out_file })?,
                other  => anyhow::bail!("unsupported output format '{other}'"),
            }
            info!("filter-apply complete: {} → {}", input.display(), output.display());
            println!("Applied '{}': {} → {}", filter_complex, input.display(), output.display());
        }

        Command::GifDecode { input, output_pattern, max_frames } => {
            use ferrox_core::decode_gif;
            use ferrox_core::codecs::PngEncoder;
            use ferrox_core::traits::DynEncoder;
            use std::fs::File;
            use std::io::BufWriter;

            let data = std::fs::read(&input)
                .with_context(|| format!("reading '{}'", input.display()))?;
            let frames = decode_gif(std::io::Cursor::new(data))
                .with_context(|| format!("decoding GIF '{}'", input.display()))?;

            let limit = if max_frames == 0 { frames.len() } else { max_frames.min(frames.len()) };
            let mut paths: Vec<String> = Vec::new();

            for (i, gif_frame) in frames[..limit].iter().enumerate() {
                let path = output_pattern.replace("%03d", &format!("{i:03}"))
                    .replace("%d", &i.to_string());
                let f = BufWriter::new(File::create(&path)
                    .with_context(|| format!("creating '{path}'"))?);
                PngEncoder.encode_dyn(&gif_frame.frame, &mut { f })?;
                paths.push(path);
            }

            info!("gif-decode: extracted {} frames from {}", limit, input.display());
            for p in &paths { println!("{p}"); }
        }

        Command::GifEncode { inputs, output, delay, palette } => {
            use ferrox_core::{encode_gif, GifEncodeOptions, GifFrame};
            use ferrox_core::codecs::PngDecoder;
            use ferrox_core::traits::DynDecoder;
            use std::fs::File;
            use std::io::BufWriter;

            let mut gif_frames: Vec<GifFrame> = Vec::new();
            for path in &inputs {
                let ext = path.extension().and_then(|e| e.to_str())
                    .unwrap_or("").to_ascii_lowercase();
                let mut f = std::io::BufReader::new(File::open(path)
                    .with_context(|| format!("opening '{}'", path.display()))?);
                let frame = match ext.as_str() {
                    "png" => PngDecoder.decode_dyn(&mut f)?,
                    other => anyhow::bail!("gif-encode: unsupported input format '{other}'"),
                };
                gif_frames.push(GifFrame { frame, delay_cs: delay });
            }

            let opts = GifEncodeOptions {
                palette_size: palette,
                default_delay_cs: delay,
                ..Default::default()
            };
            let out_file = BufWriter::new(File::create(&output)
                .with_context(|| format!("creating '{}'", output.display()))?);
            encode_gif(out_file, &gif_frames, &opts)
                .with_context(|| format!("encoding GIF '{}'", output.display()))?;

            info!("gif-encode: wrote {} frames → {}", gif_frames.len(), output.display());
            println!("Encoded {} frames → {}", gif_frames.len(), output.display());
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
