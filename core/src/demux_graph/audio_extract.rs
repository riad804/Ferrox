//! Audio extraction from containers (MP4/MKV → PCM/encoded).

use std::fs::File;
use std::path::Path;
use tracing::instrument;

use crate::{
    codecs::video::WebmDemuxer,
    error::{Error, Result},
    traits::ContainerDemuxer,
    video::CodecId,
    AudioFrame, AudioGraph,
};

use super::ContainerKind;

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

