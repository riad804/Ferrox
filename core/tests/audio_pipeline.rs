use std::io::Cursor;
use ferrox_core::{
    AudioFrame,
    codecs::{Mp3Decoder, WavDecoder, WavEncoder},
    filters::{ResampleFilter, VolumeFilter},
    traits::{AudioDecoder, AudioEncoder, AudioFilter},
    AudioGraph,
};
use tempfile::NamedTempFile;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Generate a 440 Hz sine wave at `sample_rate`, `channels`, for `duration_secs`.
fn sine_wave(sample_rate: u32, channels: u16, duration_secs: f32) -> AudioFrame {
    let n_frames = (sample_rate as f32 * duration_secs) as usize;
    let mut samples = Vec::with_capacity(n_frames * channels as usize);
    for i in 0..n_frames {
        let t = i as f32 / sample_rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        for _ in 0..channels {
            samples.push(s);
        }
    }
    AudioFrame::new(sample_rate, channels, samples)
}

fn encode_wav(frame: &AudioFrame) -> Vec<u8> {
    let mut buf = Vec::new();
    WavEncoder.encode_audio(frame, &mut buf).expect("wav encode");
    buf
}

fn decode_wav(data: &[u8]) -> AudioFrame {
    WavDecoder.decode_audio(Cursor::new(data)).expect("wav decode")
}

/// Build a minimal valid MPEG-1 Layer III bitstream (silent frames).
///
/// Header bytes 0xFF 0xFB 0x90 0xC0 → MPEG1 / Layer3 / 128 kbps / 44100 Hz / mono.
/// Each frame is 417 bytes (4-byte header + 17-byte side-info + 396 bytes main data).
/// minimp3 decodes each frame into 1152 PCM samples.
fn make_synthetic_mp3(n_frames: usize) -> Vec<u8> {
    let header   = [0xFF_u8, 0xFB, 0x90, 0xC0];
    let side_info = [0u8; 17];
    let main_data = vec![0u8; 417 - 4 - 17];
    let frame: Vec<u8> = header.iter()
        .chain(side_info.iter())
        .chain(main_data.iter())
        .copied()
        .collect();
    frame.iter().cycle().take(417 * n_frames).copied().collect()
}

// ── WAV codec ─────────────────────────────────────────────────────────────────

#[test]
fn wav_roundtrip() {
    let original = sine_wave(44100, 2, 0.1);
    let encoded = encode_wav(&original);
    let decoded = decode_wav(&encoded);

    assert_eq!(decoded.sample_rate, original.sample_rate);
    assert_eq!(decoded.channels, original.channels);
    assert_eq!(decoded.samples.len(), original.samples.len());

    for (a, b) in original.samples.iter().zip(&decoded.samples) {
        assert!((a - b).abs() < 1e-5, "sample mismatch: {a} vs {b}");
    }
}

// ── MP3 decoder ───────────────────────────────────────────────────────────────

#[test]
fn mp3_decode_sample_rate_and_channels() {
    let mp3 = make_synthetic_mp3(10);
    let frame = Mp3Decoder.decode_audio(Cursor::new(&mp3)).expect("mp3 decode");
    assert_eq!(frame.sample_rate, 44100, "expected 44100 Hz");
    assert_eq!(frame.channels, 1, "expected mono");
    assert!(!frame.samples.is_empty(), "expected at least one sample");
}

#[test]
fn mp3_decode_returns_error_on_empty_input() {
    let result = Mp3Decoder.decode_audio(Cursor::new(b""));
    assert!(result.is_err(), "empty input should fail");
}

#[test]
fn mp3_decode_returns_error_on_garbage() {
    let result = Mp3Decoder.decode_audio(Cursor::new(b"not an mp3 at all!!!"));
    assert!(result.is_err(), "garbage input should fail");
}

// ── MP3 → resample → WAV integration ─────────────────────────────────────────

#[test]
fn mp3_resample_to_wav_via_codec_pipeline() {
    // Decode a synthetic 44100 Hz MP3 directly, resample to 22050 Hz, encode WAV.
    let mp3 = make_synthetic_mp3(10);
    let decoded = Mp3Decoder.decode_audio(Cursor::new(&mp3)).expect("mp3 decode");
    assert_eq!(decoded.sample_rate, 44100);

    let resampled = ResampleFilter::new(22050)
        .process_audio(decoded)
        .expect("resample");
    assert_eq!(resampled.sample_rate, 22050, "sample rate after resample");
    assert!(!resampled.samples.is_empty());

    // Encode to WAV and read back — verify the header carries the new rate.
    let wav_bytes = encode_wav(&resampled);
    let roundtripped = decode_wav(&wav_bytes);
    assert_eq!(roundtripped.sample_rate, 22050, "WAV header must reflect resampled rate");
    assert_eq!(roundtripped.channels, 1);
}

#[test]
fn mp3_resample_to_wav_via_graph() {
    // Full AudioGraph path: .mp3 file → ResampleFilter(22050) → .wav file.
    let mp3 = make_synthetic_mp3(10);

    let input = NamedTempFile::with_suffix(".mp3").expect("tmpfile");
    std::fs::write(input.path(), &mp3).expect("write mp3");

    let output = NamedTempFile::with_suffix(".wav").expect("tmpfile");

    AudioGraph::new()
        .with_filter(ResampleFilter::new(22050))
        .run(input.path(), output.path())
        .expect("graph: mp3 → resample → wav");

    let wav_bytes = std::fs::read(output.path()).expect("read output wav");
    let frame = decode_wav(&wav_bytes);

    assert_eq!(frame.sample_rate, 22050, "output WAV must be 22050 Hz");
    assert_eq!(frame.channels, 1, "mono MP3 → mono WAV");
    assert!(!frame.samples.is_empty(), "output must have samples");
}

#[test]
fn mp3_resample_upsample_to_48k() {
    let mp3 = make_synthetic_mp3(10);
    let decoded = Mp3Decoder.decode_audio(Cursor::new(&mp3)).expect("mp3 decode");

    let resampled = ResampleFilter::new(48000)
        .process_audio(decoded)
        .expect("upsample 44100 → 48000");

    assert_eq!(resampled.sample_rate, 48000);
    // ~10% more samples than source
    let wav_bytes = encode_wav(&resampled);
    let rt = decode_wav(&wav_bytes);
    assert_eq!(rt.sample_rate, 48000);
}

// ── VolumeFilter ──────────────────────────────────────────────────────────────

#[test]
fn volume_filter_attenuates() {
    let frame = sine_wave(44100, 1, 0.05);
    let out = VolumeFilter::new(0.5).process_audio(frame.clone()).expect("volume filter");

    assert_eq!(out.samples.len(), frame.samples.len());
    for (orig, scaled) in frame.samples.iter().zip(&out.samples) {
        assert!((scaled - orig * 0.5).abs() < 1e-6);
    }
}

#[test]
fn volume_filter_clamps() {
    let frame = AudioFrame::new(44100, 1, vec![0.8, -0.8, 0.5]);
    let out = VolumeFilter::new(2.0).process_audio(frame).expect("volume clamp");
    // 0.8 * 2.0 = 1.6 → clamped to 1.0; 0.5 * 2.0 = 1.0 (edge, exact)
    assert!((out.samples[0] - 1.0).abs() < 1e-6);
    assert!((out.samples[1] - (-1.0)).abs() < 1e-6);
    assert!((out.samples[2] - 1.0).abs() < 1e-6);
}

// ── ResampleFilter ────────────────────────────────────────────────────────────

#[test]
fn resample_filter_changes_rate() {
    let frame = sine_wave(44100, 1, 0.5);
    let out = ResampleFilter::new(22050).process_audio(frame).expect("resample");

    assert_eq!(out.sample_rate, 22050);
    let expected = 22050 / 2usize; // 0.5 sec at 22050 Hz
    let tolerance = expected / 10;
    assert!(
        (out.frame_count() as i64 - expected as i64).abs() < tolerance as i64,
        "expected ~{expected} frames, got {}",
        out.frame_count()
    );
}

#[test]
fn resample_filter_noop_same_rate() {
    let frame = sine_wave(44100, 2, 0.1);
    let n = frame.samples.len();
    let out = ResampleFilter::new(44100).process_audio(frame).expect("noop resample");
    assert_eq!(out.samples.len(), n);
}

// ── AudioGraph end-to-end ─────────────────────────────────────────────────────

#[test]
fn audio_graph_wav_passthrough() {
    let src = sine_wave(48000, 2, 0.2);
    let input = NamedTempFile::with_suffix(".wav").expect("tmpfile");
    std::fs::write(input.path(), encode_wav(&src)).expect("write input");
    let output = NamedTempFile::with_suffix(".wav").expect("tmpfile");

    AudioGraph::new().run(input.path(), output.path()).expect("graph run");

    let decoded = decode_wav(&std::fs::read(output.path()).expect("read output"));
    assert_eq!(decoded.sample_rate, 48000);
    assert_eq!(decoded.channels, 2);
}

#[test]
fn audio_graph_volume_pipeline() {
    let src = sine_wave(44100, 1, 0.1);
    let input = NamedTempFile::with_suffix(".wav").expect("tmpfile");
    std::fs::write(input.path(), encode_wav(&src)).expect("write input");
    let output = NamedTempFile::with_suffix(".wav").expect("tmpfile");

    AudioGraph::new()
        .with_filter(VolumeFilter::new(0.25))
        .run(input.path(), output.path())
        .expect("graph run");

    let decoded = decode_wav(&std::fs::read(output.path()).expect("read output"));
    let peak = decoded.samples.iter().cloned().fold(0.0f32, f32::max);
    assert!(peak < 0.2, "peak {peak} should be attenuated below 0.2");
}

#[test]
fn audio_graph_unsupported_output_format_errors() {
    let src = sine_wave(44100, 1, 0.05);
    let input = NamedTempFile::with_suffix(".wav").expect("tmpfile");
    std::fs::write(input.path(), encode_wav(&src)).expect("write input");
    let output = NamedTempFile::with_suffix(".flac").expect("tmpfile");

    let result = AudioGraph::new().run(input.path(), output.path());
    assert!(result.is_err(), "encoding to .flac should error (no encoder registered)");
}
