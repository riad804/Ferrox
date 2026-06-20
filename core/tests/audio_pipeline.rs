use std::io::Cursor;
use ferrox_core::{
    AudioFrame,
    codecs::{WavDecoder, WavEncoder},
    filters::{ResampleFilter, VolumeFilter},
    traits::{AudioDecoder, AudioEncoder, AudioFilter},
    AudioGraph,
};
use tempfile::NamedTempFile;

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
    let enc = WavEncoder;
    let mut buf = Vec::new();
    enc.encode_audio(frame, &mut buf).expect("wav encode");
    buf
}

fn decode_wav(data: &[u8]) -> AudioFrame {
    let dec = WavDecoder;
    dec.decode_audio(Cursor::new(data)).expect("wav decode")
}

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

#[test]
fn volume_filter_attenuates() {
    let frame = sine_wave(44100, 1, 0.05);
    let filter = VolumeFilter::new(0.5);
    let out = filter.process_audio(frame.clone()).expect("volume filter");

    assert_eq!(out.samples.len(), frame.samples.len());
    for (orig, scaled) in frame.samples.iter().zip(&out.samples) {
        assert!((scaled - orig * 0.5).abs() < 1e-6);
    }
}

#[test]
fn volume_filter_clamps() {
    let frame = AudioFrame::new(44100, 1, vec![0.8, -0.8, 0.5]);
    let filter = VolumeFilter::new(2.0);
    let out = filter.process_audio(frame).expect("volume clamp");
    // 0.8 * 2.0 = 1.6 → clamped to 1.0
    assert!((out.samples[0] - 1.0).abs() < 1e-6);
    assert!((out.samples[1] - (-1.0)).abs() < 1e-6);
    assert!((out.samples[2] - 1.0).abs() < 1e-6);
}

#[test]
fn resample_filter_changes_rate() {
    let frame = sine_wave(44100, 1, 0.5);
    let filter = ResampleFilter::new(22050);
    let out = filter.process_audio(frame).expect("resample");

    assert_eq!(out.sample_rate, 22050);
    // Output should be roughly half as many samples
    let expected = 22050 / 2; // 0.5 sec at 22050 Hz
    let tolerance = expected / 10; // 10% tolerance
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
    let filter = ResampleFilter::new(44100);
    let out = filter.process_audio(frame).expect("noop resample");
    assert_eq!(out.samples.len(), n);
}

#[test]
fn audio_graph_wav_convert() {
    let src = sine_wave(48000, 2, 0.2);
    let src_bytes = encode_wav(&src);

    let input = NamedTempFile::with_suffix(".wav").expect("tmpfile");
    std::fs::write(input.path(), &src_bytes).expect("write input");

    let output = NamedTempFile::with_suffix(".wav").expect("tmpfile");

    let graph = AudioGraph::new();
    graph.run(input.path(), output.path()).expect("graph run");

    let out_bytes = std::fs::read(output.path()).expect("read output");
    let decoded = decode_wav(&out_bytes);

    assert_eq!(decoded.sample_rate, 48000);
    assert_eq!(decoded.channels, 2);
}

#[test]
fn audio_graph_volume_pipeline() {
    let src = sine_wave(44100, 1, 0.1);
    let src_bytes = encode_wav(&src);

    let input = NamedTempFile::with_suffix(".wav").expect("tmpfile");
    std::fs::write(input.path(), &src_bytes).expect("write input");

    let output = NamedTempFile::with_suffix(".wav").expect("tmpfile");

    let graph = AudioGraph::new().with_filter(VolumeFilter::new(0.25));
    graph.run(input.path(), output.path()).expect("graph run");

    let out_bytes = std::fs::read(output.path()).expect("read output");
    let decoded = decode_wav(&out_bytes);

    // Peak amplitude should be ~0.5 * 0.25 = 0.125
    let peak = decoded.samples.iter().cloned().fold(0.0f32, f32::max);
    assert!(peak < 0.2, "peak {peak} should be attenuated");
}
