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
#[command(
    name = "ferrox",
    version,
    about = "Graph-based media processing pipeline",
    long_about = concat!(
        "ferrox — a pure-Rust media processing toolkit.\n\n",
        "EXAMPLES:\n",
        "  ferrox probe input.mp4\n",
        "  ferrox convert input.png output.jpg\n",
        "  ferrox resize -W 640 -H 480 input.jpg output.jpg\n",
        "  ferrox filter-apply input.png output.png --filter-complex \"blur=2.0,grayscale\"\n",
        "  ferrox gif-decode animation.gif frames/frame_%03d.png\n",
        "  ferrox gif-encode frames/*.png -o out.gif --delay 10\n",
        "  ferrox transcode input.webm output.webm --codec av1 --resize 1280x720\n",
        "  ferrox video-extract-frames input.webm frame_%03d.png --count 30 --start-frame 10\n",
    ),
)]
struct Cli {
    /// Logging verbosity: off, error, warn, info, debug, trace.
    #[arg(long, global = true, default_value = "warn", env = "FERROX_LOG")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print stream metadata for any supported media file as JSON.
    ///
    /// Example:
    ///   ferrox probe input.mp4
    ///   ferrox probe input.webm
    Probe {
        /// Input container file (WebM, MKV, MP4, IVF, PNG, JPEG, WAV, MP3, FLAC, OGG).
        input: PathBuf,
        /// Output compact JSON instead of pretty-printed.
        #[arg(long)]
        compact: bool,
    },

    /// Convert an image from one format to another (PNG ↔ JPEG).
    ///
    /// Example:
    ///   ferrox convert input.png output.jpg
    Convert {
        /// Input file path.
        input: PathBuf,
        /// Output file path.
        output: PathBuf,
    },

    /// Resize an image and write to output (format driven by output extension).
    ///
    /// Example:
    ///   ferrox resize input.jpg output.jpg -W 640 -H 480
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
    ///
    /// Example:
    ///   ferrox audio-convert input.mp3 output.wav
    AudioConvert {
        /// Input audio file (wav/mp3/flac/ogg).
        input: PathBuf,
        /// Output audio file (wav).
        output: PathBuf,
    },

    /// Adjust audio volume (gain multiplier).
    ///
    /// Example:
    ///   ferrox audio-volume input.wav output.wav --gain 0.5
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
    ///
    /// Example:
    ///   ferrox audio-resample input.wav output.wav -r 44100
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
    /// Example:
    ///   ferrox video-extract-frames input.webm frame_%03d.png --count 10
    ///   ferrox video-extract-frames input.webm frame_%03d.png --start-frame 5 --count 20
    VideoExtractFrames {
        /// Input video file (WebM/MKV with VP8, or MP4).
        input: PathBuf,
        /// Output path pattern with a printf-style %d placeholder, e.g. frame_%03d.png.
        output_pattern: String,
        /// Maximum number of frames to extract (0 = all).
        #[arg(short, long, default_value = "10")]
        count: usize,
        /// First frame index to extract (0-based, skips earlier frames).
        #[arg(long, default_value = "0")]
        start_frame: usize,
    },

    /// Extract audio from a video container to WAV.
    ///
    /// Example:
    ///   ferrox video-extract-audio input.webm audio.wav
    VideoExtractAudio {
        /// Input video file (WebM/MKV with PCM audio).
        input: PathBuf,
        /// Output WAV file.
        output: PathBuf,
    },

    /// Apply a filtergraph expression to an image (similar to -filter_complex).
    ///
    /// Supported tokens: blur=<sigma>, grayscale, negate, scale=<w>:<h>,
    /// brightness=<delta>, contrast=<factor>, saturation=<factor>.
    ///
    /// Example:
    ///   ferrox filter-apply input.png output.png --filter-complex "scale=640:480,blur=2.0,grayscale"
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
    /// Example:
    ///   ferrox gif-decode animation.gif frames/frame_%03d.png
    ///   ferrox gif-decode animation.gif frame_%d.png --max-frames 5
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
    /// Example:
    ///   ferrox gif-encode frame_001.png frame_002.png -o out.gif --delay 10
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
    /// Example:
    ///   ferrox transcode input.webm output.webm --codec av1 --resize 1280x720
    ///   ferrox transcode input.mp4 output.webm --speed 4 --quantizer 80
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
        Command::Probe { input, compact } => {
            run_probe(&input, compact)?;
        }

        Command::Convert { input, output } => {
            let pb = spinner("converting…");
            let graph = Graph::new();
            graph
                .run(&input, &output)
                .with_context(|| format!("converting '{}' → '{}'", input.display(), output.display()))?;
            pb.finish_and_clear();
            println!("Converted {} → {}", input.display(), output.display());
        }

        Command::Resize { input, output, width, height } => {
            let pb = spinner("resizing…");
            let graph = Graph::new().with_filter(ResizeFilter::new(width, height));
            graph
                .run(&input, &output)
                .with_context(|| format!(
                    "resizing '{}' → '{}' ({}×{})",
                    input.display(), output.display(), width, height,
                ))?;
            pb.finish_and_clear();
            println!("Resized to {}×{}: {} → {}", width, height, input.display(), output.display());
        }

        Command::AudioConvert { input, output } => {
            let pb = spinner("converting audio…");
            let graph = AudioGraph::new();
            graph
                .run(&input, &output)
                .with_context(|| format!("audio-converting '{}' → '{}'", input.display(), output.display()))?;
            pb.finish_and_clear();
            println!("Audio converted {} → {}", input.display(), output.display());
        }

        Command::AudioVolume { input, output, gain } => {
            let pb = spinner("adjusting volume…");
            let graph = AudioGraph::new().with_filter(VolumeFilter::new(gain));
            graph
                .run(&input, &output)
                .with_context(|| format!(
                    "adjusting volume (gain={gain}) '{}' → '{}'",
                    input.display(), output.display()
                ))?;
            pb.finish_and_clear();
            println!("Volume adjusted (gain={gain}): {} → {}", input.display(), output.display());
        }

        Command::AudioResample { input, output, rate } => {
            let pb = spinner("resampling…");
            let graph = AudioGraph::new().with_filter(ResampleFilter::new(rate));
            graph
                .run(&input, &output)
                .with_context(|| format!(
                    "resampling to {rate}Hz '{}' → '{}'",
                    input.display(), output.display()
                ))?;
            pb.finish_and_clear();
            println!("Resampled to {rate}Hz: {} → {}", input.display(), output.display());
        }

        Command::VideoExtractFrames { input, output_pattern, count, start_frame } => {
            let pb = spinner("extracting frames…");
            let result = demux_graph::extract_frames_range(&input, &output_pattern, start_frame, count)
                .with_context(|| format!(
                    "extracting frame(s) from '{}' → '{output_pattern}'",
                    input.display()
                ))?;
            pb.finish_and_clear();
            info!(
                "extracted {} frame(s) ({} skipped) from {}",
                result.frame_paths.len(), result.skipped, input.display()
            );
            for p in &result.frame_paths {
                println!("{}", p.display());
            }
        }

        Command::VideoExtractAudio { input, output } => {
            let pb = spinner("extracting audio…");
            demux_graph::extract_audio(&input, &output)
                .with_context(|| format!(
                    "extracting audio from '{}' → '{}'",
                    input.display(), output.display()
                ))?;
            pb.finish_and_clear();
            println!("Extracted audio: {} → {}", input.display(), output.display());
        }

        Command::FilterApply { input, output, filter_complex } => {
            use ferrox_core::codecs::{JpegDecoder, JpegEncoder, PngDecoder, PngEncoder};
            use ferrox_core::traits::{DynDecoder, DynEncoder};
            use std::fs::File;
            use std::io::BufWriter;

            let pb = spinner("applying filters…");

            let in_ext = input.extension().and_then(|e| e.to_str())
                .ok_or_else(|| anyhow::anyhow!("no extension on input file"))?
                .to_ascii_lowercase();
            let out_ext = output.extension().and_then(|e| e.to_str())
                .ok_or_else(|| anyhow::anyhow!("no extension on output file"))?
                .to_ascii_lowercase();

            let mut in_file = std::io::BufReader::new(File::open(&input)?);
            let frame = match in_ext.as_str() {
                "png"        => PngDecoder.decode_dyn(&mut in_file)?,
                "jpg"|"jpeg" => JpegDecoder.decode_dyn(&mut in_file)?,
                other        => anyhow::bail!("unsupported input format '{other}'"),
            };

            let out_frame = FilterGraph::parse_and_run(frame, &filter_complex)
                .with_context(|| format!("applying filtergraph '{filter_complex}'"))?;

            let out_file = BufWriter::new(File::create(&output)?);
            match out_ext.as_str() {
                "png"        => PngEncoder.encode_dyn(&out_frame, &mut { out_file })?,
                "jpg"|"jpeg" => JpegEncoder::default().encode_dyn(&out_frame, &mut { out_file })?,
                other        => anyhow::bail!("unsupported output format '{other}'"),
            }

            pb.finish_and_clear();
            println!("Applied '{}': {} → {}", filter_complex, input.display(), output.display());
        }

        Command::GifDecode { input, output_pattern, max_frames } => {
            use ferrox_core::decode_gif;
            use ferrox_core::codecs::PngEncoder;
            use ferrox_core::traits::DynEncoder;
            use std::fs::File;
            use std::io::BufWriter;

            let pb = spinner("decoding GIF…");
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

            pb.finish_and_clear();
            for p in &paths { println!("{p}"); }
        }

        Command::GifEncode { inputs, output, delay, palette } => {
            use ferrox_core::{encode_gif, GifEncodeOptions, GifFrame};
            use ferrox_core::codecs::PngDecoder;
            use ferrox_core::traits::DynDecoder;
            use std::fs::File;
            use std::io::BufWriter;

            let pb = ProgressBar::new(inputs.len() as u64);
            pb.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} frames  [{elapsed_precise}]"
                ).unwrap().progress_chars("=> "),
            );

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
                pb.inc(1);
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

            pb.finish_and_clear();
            println!("Encoded {} frames → {}", gif_frames.len(), output.display());
        }

        Command::Transcode { input, output, codec, resize, fps, speed, quantizer } => {
            let video_codec = match codec.to_ascii_lowercase().as_str() {
                "av1" | "copy" => VideoCodecChoice::Av1,
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
                ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {msg}").unwrap(),
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
            ).with_context(|| format!("transcoding '{}' → '{}'", input.display(), output.display()))?;

            pb.finish_and_clear();
            println!(
                "Transcoded {} → {} ({} frames encoded, {} copied)",
                input.display(), output.display(),
                result.frames_encoded, result.frames_copied
            );
        }
    }

    Ok(())
}

// ── probe ─────────────────────────────────────────────────────────────────────

fn run_probe(input: &PathBuf, compact: bool) -> Result<()> {
    use ferrox_core::{
        codecs::{Mp4Demuxer, WebmDemuxer},
        demux_graph::ContainerKind,
        traits::ContainerDemuxer,
        IvfDemuxer,
    };
    use std::fs::File;

    let ext = input.extension().and_then(|e| e.to_str())
        .unwrap_or("").to_ascii_lowercase();

    // Image probe — decode just enough to get dimensions.
    match ext.as_str() {
        "png" | "jpg" | "jpeg" => {
            use ferrox_core::{codecs::{PngDecoder, JpegDecoder}, traits::DynDecoder};
            let f = File::open(input).with_context(|| format!("opening '{}'", input.display()))?;
            let mut r = std::io::BufReader::new(f);
            let frame = match ext.as_str() {
                "png" => PngDecoder.decode_dyn(&mut r)?,
                _     => JpegDecoder.decode_dyn(&mut r)?,
            };
            let info = serde_json::json!({
                "filename": input.display().to_string(),
                "format": ext,
                "streams": [{
                    "index": 0,
                    "kind": "video",
                    "codec": ext.to_uppercase(),
                    "width":  frame.width,
                    "height": frame.height,
                    "pixel_format": format!("{:?}", frame.format),
                }]
            });
            return Ok(print_json(&info, compact));
        }
        "wav" | "mp3" | "flac" | "ogg" => {
            let info = serde_json::json!({
                "filename": input.display().to_string(),
                "format": ext,
                "streams": [{
                    "index": 0,
                    "kind":  "audio",
                    "codec": ext.to_uppercase(),
                }]
            });
            return Ok(print_json(&info, compact));
        }
        _ => {}
    }

    // Video container probe.
    let kind = ContainerKind::from_path(input)
        .ok_or_else(|| anyhow::anyhow!("unrecognised container extension '{ext}'"))?;

    let streams: Vec<_> = match kind {
        ContainerKind::Mp4 => {
            let f = File::open(input)?;
            let size = f.metadata()?.len();
            Mp4Demuxer::open(f, size)?.streams().to_vec()
        }
        ContainerKind::Mkv => {
            let f = File::open(input)?;
            WebmDemuxer::open(f)?.streams().to_vec()
        }
        ContainerKind::Ivf => {
            let f = File::open(input)?;
            IvfDemuxer::open(f)?.streams().to_vec()
        }
    };

    let stream_json: Vec<serde_json::Value> = streams.iter().map(|s| {
        let kind_str = if s.is_video() { "video" } else if s.is_audio() { "audio" } else { "data" };
        let mut obj = serde_json::json!({
            "index": s.index,
            "kind":  kind_str,
            "codec": format!("{:?}", s.codec),
        });
        if s.is_video() {
            obj["width"]      = serde_json::json!(s.width);
            obj["height"]     = serde_json::json!(s.height);
            obj["frame_rate"] = serde_json::json!(s.frame_rate);
        }
        if s.is_audio() {
            obj["sample_rate"] = serde_json::json!(s.sample_rate);
            obj["channels"]    = serde_json::json!(s.channels);
        }
        obj
    }).collect();

    let info = serde_json::json!({
        "filename": input.display().to_string(),
        "format":   format!("{kind:?}").to_lowercase(),
        "streams":  stream_json,
    });
    print_json(&info, compact);
    Ok(())
}

fn print_json(v: &serde_json::Value, compact: bool) {
    if compact {
        println!("{v}");
    } else {
        println!("{}", serde_json::to_string_pretty(v).unwrap());
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn spinner(msg: &'static str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}  [{elapsed_precise}]").unwrap(),
    );
    pb.set_message(msg);
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}
