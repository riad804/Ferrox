/// Phase 7 integration tests: MP3 encoding (mp3-encode feature) and
/// Opus encoding (opus-encode feature).
///
/// These tests are compiled only when the relevant feature is enabled.
/// Run with:
///   cargo test -p ferrox-core --features "mp3-encode,opus-encode"

use ferrox_core::{AudioFrame, AudioGraph};
use ferrox_core::codecs::WavEncoder;
use ferrox_core::traits::AudioEncoder;
use tempfile::NamedTempFile;

// ── helpers ───────────────────────────────────────────────────────────────────

fn sine_wave(sample_rate: u32, channels: u16, duration_secs: f32) -> AudioFrame {
    let n_frames = (sample_rate as f32 * duration_secs) as usize;
    let mut samples = Vec::with_capacity(n_frames * channels as usize);
    for i in 0..n_frames {
        let t = i as f32 / sample_rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.4;
        for _ in 0..channels { samples.push(s); }
    }
    AudioFrame::new(sample_rate, channels, samples)
}

fn encode_wav(frame: &AudioFrame) -> Vec<u8> {
    let mut buf = Vec::new();
    WavEncoder.encode_audio(frame, &mut buf).expect("wav encode");
    buf
}

// ── MP3 encoder tests ─────────────────────────────────────────────────────────

#[cfg(feature = "mp3-encode")]
mod mp3_encode {
    use super::*;
    use ferrox_core::codecs::{Mp3Encoder, Mp3Options, Mp3Quality};
    use ferrox_core::traits::AudioEncoder;

    #[test]
    fn mp3_encode_mono_produces_non_empty_output() {
        let frame = sine_wave(44100, 1, 0.5);
        let mut out = Vec::new();
        Mp3Encoder::new().encode_audio(&frame, &mut out).expect("mp3 mono encode");
        // Minimal MP3 file has sync bytes 0xFF 0xFB or 0xFF 0xFA.
        assert!(out.len() > 128, "expected MP3 output, got {} bytes", out.len());
        // Find first MPEG sync word.
        let has_sync = out.windows(2).any(|w| w[0] == 0xFF && (w[1] & 0xE0) == 0xE0);
        assert!(has_sync, "no MPEG sync word found in MP3 output");
    }

    #[test]
    fn mp3_encode_stereo_produces_non_empty_output() {
        let frame = sine_wave(44100, 2, 0.5);
        let mut out = Vec::new();
        Mp3Encoder::new().encode_audio(&frame, &mut out).expect("mp3 stereo encode");
        assert!(out.len() > 128);
    }

    #[test]
    fn mp3_encode_cbr_128_is_accepted() {
        let frame = sine_wave(44100, 2, 0.3);
        let enc = Mp3Encoder::cbr(128);
        let mut out = Vec::new();
        enc.encode_audio(&frame, &mut out).expect("mp3 cbr 128");
        assert!(!out.is_empty());
    }

    #[test]
    fn mp3_encode_cbr_320_is_accepted() {
        let frame = sine_wave(44100, 2, 0.3);
        let enc = Mp3Encoder::cbr(320);
        let mut out = Vec::new();
        enc.encode_audio(&frame, &mut out).expect("mp3 cbr 320");
        assert!(!out.is_empty());
    }

    #[test]
    fn mp3_encode_quality_best() {
        let frame = sine_wave(44100, 1, 0.2);
        let enc = Mp3Encoder::with_opts(Mp3Options {
            bitrate_kbps: Some(192),
            quality: Mp3Quality::Best,
        });
        let mut out = Vec::new();
        enc.encode_audio(&frame, &mut out).expect("mp3 quality best");
        assert!(!out.is_empty());
    }

    #[test]
    fn mp3_encode_empty_frame_errors() {
        let frame = AudioFrame::new(44100, 2, vec![]);
        let mut out = Vec::new();
        assert!(Mp3Encoder::new().encode_audio(&frame, &mut out).is_err());
    }

    #[test]
    fn audiograph_wav_to_mp3_roundtrip() {
        let frame = sine_wave(44100, 2, 0.3);
        // Write WAV to temp file.
        let wav_data = encode_wav(&frame);
        let mut wav_tmp = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut wav_tmp, &wav_data).unwrap();
        let wav_path = wav_tmp.path().with_extension("wav");
        std::fs::rename(wav_tmp.path(), &wav_path).ok();

        let mp3_tmp = tempfile::Builder::new().suffix(".mp3").tempfile().unwrap();
        let mp3_path = mp3_tmp.path().to_path_buf();

        AudioGraph::new().run(&wav_path, &mp3_path).expect("audio-graph wav→mp3");

        let meta = std::fs::metadata(&mp3_path).unwrap();
        assert!(meta.len() > 0, "mp3 output file is empty");
    }
}

// ── Opus encoder tests ────────────────────────────────────────────────────────

#[cfg(feature = "opus-encode")]
mod opus_encode {
    use super::*;
    use ferrox_core::codecs::{OpusEncoder, OpusOptions, OpusApplication};
    use ferrox_core::traits::AudioEncoder;

    fn has_ogg_opus_magic(data: &[u8]) -> bool {
        // Ogg page: "OggS" at offset 0; OpusHead in first audio page.
        data.starts_with(b"OggS")
    }

    #[test]
    fn opus_encode_mono_produces_ogg_container() {
        let frame = sine_wave(48000, 1, 0.5);
        let mut out = Vec::new();
        OpusEncoder::new().encode_audio(&frame, &mut out).expect("opus mono encode");
        assert!(out.len() > 64);
        assert!(has_ogg_opus_magic(&out), "output is not an Ogg stream");
        // OpusHead magic
        assert!(out.windows(8).any(|w| w == b"OpusHead"), "no OpusHead found");
    }

    #[test]
    fn opus_encode_stereo_produces_ogg_container() {
        let frame = sine_wave(48000, 2, 0.5);
        let mut out = Vec::new();
        OpusEncoder::new().encode_audio(&frame, &mut out).expect("opus stereo encode");
        assert!(has_ogg_opus_magic(&out));
        assert!(out.windows(8).any(|w| w == b"OpusTags"), "no OpusTags found");
    }

    #[test]
    fn opus_encode_resamples_44100_to_48000() {
        // Input at 44100 Hz — encoder must resample to 48 kHz internally.
        let frame = sine_wave(44100, 2, 0.3);
        let mut out = Vec::new();
        OpusEncoder::new().encode_audio(&frame, &mut out).expect("opus 44100 encode");
        assert!(has_ogg_opus_magic(&out));
    }

    #[test]
    fn opus_encode_high_bitrate() {
        let frame = sine_wave(48000, 2, 0.2);
        let enc = OpusEncoder::with_bitrate(320_000);
        let mut out = Vec::new();
        enc.encode_audio(&frame, &mut out).expect("opus 320kbps");
        assert!(!out.is_empty());
    }

    #[test]
    fn opus_encode_voip_application() {
        let frame = sine_wave(48000, 1, 0.2);
        let enc = OpusEncoder::with_opts(OpusOptions {
            bitrate_bps: 32_000,
            application: OpusApplication::Voip,
        });
        let mut out = Vec::new();
        enc.encode_audio(&frame, &mut out).expect("opus voip");
        assert!(!out.is_empty());
    }

    #[test]
    fn opus_encode_empty_frame_errors() {
        let frame = AudioFrame::new(48000, 2, vec![]);
        let mut out = Vec::new();
        assert!(OpusEncoder::new().encode_audio(&frame, &mut out).is_err());
    }

    #[test]
    fn audiograph_wav_to_opus_roundtrip() {
        let frame = sine_wave(48000, 2, 0.3);
        let wav_data = encode_wav(&frame);
        let mut wav_tmp = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut wav_tmp, &wav_data).unwrap();
        let wav_path = wav_tmp.path().with_extension("wav");
        std::fs::rename(wav_tmp.path(), &wav_path).ok();

        let opus_tmp = tempfile::Builder::new().suffix(".opus").tempfile().unwrap();
        let opus_path = opus_tmp.path().to_path_buf();

        AudioGraph::new().run(&wav_path, &opus_path).expect("audio-graph wav→opus");

        let meta = std::fs::metadata(&opus_path).unwrap();
        assert!(meta.len() > 0, "opus output file is empty");
    }
}
