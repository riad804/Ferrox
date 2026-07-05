//! Golden-sample tests for the audio-editing engine: timeline model, multi-track
//! mixer, per-clip gain/pan/fades/crossfade, the DSP effects, waveform, and WAV
//! export. Deterministic and fixture-free (inline `Samples`/`Silence` sources).

use ferrox_core::{
    apply_effects, generate_waveform, mix, mix_full, render_audio, AudioClip, AudioClipSource,
    AudioEffect, AudioFrame, AudioTrack, EqBand, EqKind, Project,
};

/// A mono `Samples` source of `n` frames all equal to `v` at `rate`.
fn mono(rate: u32, v: f32, n: usize) -> AudioClipSource {
    AudioClipSource::Samples { sample_rate: rate, channels: 1, samples: vec![v; n] }
}

fn project_mono(rate: u32) -> Project {
    Project::new(16, 16, 30.0).with_audio_format(rate, 1)
}

fn peak(frame: &AudioFrame) -> f32 {
    frame.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn sums_tracks_sample_accurately() {
    let rate = 48_000;
    // Two mono tracks, constant 0.3 and 0.2, each 1s, both from t=0.
    let p = project_mono(rate)
        .with_audio_track(AudioTrack::new().with_clip(AudioClip::new(mono(rate, 0.3, rate as usize), 0.0, 1.0)))
        .with_audio_track(AudioTrack::new().with_clip(AudioClip::new(mono(rate, 0.2, rate as usize), 0.0, 1.0)));

    let out = mix_full(&p).unwrap();
    assert_eq!(out.sample_rate, rate);
    assert_eq!(out.channels, 1);
    assert_eq!(out.frame_count(), rate as usize);
    // Every sample is the exact sum.
    for s in &out.samples {
        assert!((s - 0.5).abs() < 1e-6, "sum should be 0.5, got {s}");
    }
}

#[test]
fn muted_track_and_out_of_window_contribute_nothing() {
    let rate = 8_000;
    let p = project_mono(rate)
        .with_audio_track(AudioTrack::new().muted().with_clip(AudioClip::new(mono(rate, 1.0, rate as usize), 0.0, 1.0)))
        .with_audio_track(AudioTrack::new().with_clip(AudioClip::new(mono(rate, 0.5, rate as usize), 5.0, 1.0)));

    // Window [0,1): muted track silent, other clip starts at 5s → all zero.
    let out = mix(&p, 0.0, 1.0).unwrap();
    assert!(out.samples.iter().all(|s| *s == 0.0), "no audible contribution expected");
}

#[test]
fn placement_and_trim_land_on_the_right_samples() {
    let rate = 1_000;
    // Source: ramp 0..999 so we can detect which slice was taken.
    let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
    let src = AudioClipSource::Samples { sample_rate: rate, channels: 1, samples: ramp };
    // Clip starts at 1.0s on the timeline, trims 0.5s into the source, lasts 0.25s.
    let clip = AudioClip::new(src, 1.0, 0.25).with_in_offset(0.5);
    let p = project_mono(rate).with_audio_track(AudioTrack::new().with_clip(clip));

    let out = mix(&p, 0.0, 2.0).unwrap(); // 2000 frames
    // Before 1.0s (frame 1000) it's silent; sample at frame 1000 == source[500] == 0.5.
    assert_eq!(out.samples[999], 0.0);
    assert!((out.samples[1000] - 0.5).abs() < 1e-6, "trim start = source[500]");
    // Clip is 0.25s = 250 frames → last audible at frame 1249 (source[749]).
    assert!((out.samples[1249] - 0.749).abs() < 1e-3);
    assert_eq!(out.samples[1250], 0.0, "clip ended");
}

#[test]
fn clip_gain_and_pan_apply() {
    let rate = 8_000;
    // -6 dB ≈ ×0.501.
    let p = project_mono(rate).with_audio_track(
        AudioTrack::new().with_clip(AudioClip::new(mono(rate, 1.0, rate as usize), 0.0, 1.0).with_gain_db(-6.0)),
    );
    let out = mix_full(&p).unwrap();
    assert!((peak(&out) - 0.501).abs() < 2e-3, "-6dB gain, peak {}", peak(&out));

    // Hard-left pan on a stereo project zeroes the right channel.
    let stereo_src = AudioClipSource::Samples { sample_rate: rate, channels: 2, samples: vec![0.8; rate as usize * 2] };
    let ps = Project::new(16, 16, 30.0).with_audio_format(rate, 2).with_audio_track(
        AudioTrack::new().with_clip(AudioClip::new(stereo_src, 0.0, 1.0).with_pan(-1.0)),
    );
    let so = mix_full(&ps).unwrap();
    // Right channel (odd indices) ~0; left (even) non-zero.
    assert!(so.samples.iter().skip(1).step_by(2).all(|r| r.abs() < 1e-6), "right muted by hard-left pan");
    assert!(so.samples.iter().step_by(2).any(|l| l.abs() > 0.1), "left retains signal");
}

#[test]
fn fade_in_endpoints_are_exact() {
    let rate = 1_000;
    // 1s clip, 0.1s (100-frame) linear fade-in.
    let p = project_mono(rate).with_audio_track(
        AudioTrack::new().with_clip(AudioClip::new(mono(rate, 1.0, rate as usize), 0.0, 1.0).with_fade(0.1, 0.0)),
    );
    let out = mix_full(&p).unwrap();
    assert_eq!(out.samples[0], 0.0, "fade-in starts at silence");
    assert!((out.samples[50] - 0.5).abs() < 1e-3, "halfway through fade-in ≈ 0.5");
    assert!((out.samples[100] - 1.0).abs() < 1e-6, "fade-in complete at full gain");
}

#[test]
fn overlapping_fades_form_an_equal_power_crossfade() {
    let rate = 1_000;
    // Clip A: [0,1) fading out over its last 0.5s. Clip B: [0.5,1.5) fading in over 0.5s.
    // They overlap on [0.5,1.0); summed linear fades cross at ~equal level.
    let a = AudioClip::new(mono(rate, 1.0, rate as usize), 0.0, 1.0).with_fade(0.0, 0.5);
    let b = AudioClip::new(mono(rate, 1.0, rate as usize), 0.5, 1.0).with_fade(0.5, 0.0);
    let p = project_mono(rate)
        .with_audio_track(AudioTrack::new().with_clip(a))
        .with_audio_track(AudioTrack::new().with_clip(b));

    let out = mix(&p, 0.0, 1.5).unwrap();
    // In the overlap region the sum of the two linear ramps stays ≈ 1.0.
    for f in 520..980 {
        assert!((out.samples[f] - 1.0).abs() < 0.05, "crossfade sum ≈ 1.0 at {f}, got {}", out.samples[f]);
    }
}

#[test]
fn eq_flat_peaking_band_is_identity() {
    let rate = 48_000;
    let sig: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let frame = AudioFrame::new(rate, 1, sig.clone());
    let out = apply_effects(
        frame,
        &[AudioEffect::Eq { bands: vec![EqBand { kind: EqKind::Peaking, freq_hz: 1000.0, gain_db: 0.0, q: 1.0 }] }],
    )
    .unwrap();
    for (a, b) in sig.iter().zip(out.samples.iter()) {
        assert!((a - b).abs() < 1e-4, "flat EQ must be identity");
    }
}

#[test]
fn normalize_hits_target_peak() {
    let rate = 8_000;
    let frame = AudioFrame::new(rate, 1, vec![0.1; 1000]);
    let out = apply_effects(frame, &[AudioEffect::Normalize { target_db: -6.0, rms: false }]).unwrap();
    // -6 dBFS ≈ 0.501 peak.
    assert!((peak(&out) - 0.501).abs() < 2e-3, "normalized peak {}", peak(&out));
}

#[test]
fn compressor_reduces_hot_signal() {
    let rate = 48_000;
    let frame = AudioFrame::new(rate, 1, vec![0.9; rate as usize]); // 1s at ~-0.9 dBFS
    let out = apply_effects(
        frame,
        &[AudioEffect::Compressor { threshold_db: -20.0, ratio: 4.0, attack_ms: 5.0, release_ms: 50.0, makeup_db: 0.0 }],
    )
    .unwrap();
    // After the attack settles, the tail must be well below the input level.
    let tail = out.samples[rate as usize - 1].abs();
    assert!(tail < 0.5, "compressor should pull 0.9 down, tail {tail}");
}

#[test]
fn delay_produces_an_attenuated_echo() {
    let rate = 1_000;
    let mut sig = vec![0.0f32; 500];
    sig[0] = 1.0; // impulse
    let frame = AudioFrame::new(rate, 1, sig);
    // 100 ms = 100-frame delay, half-level echo, no feedback.
    let out = apply_effects(frame, &[AudioEffect::Delay { time_ms: 100.0, feedback: 0.0, mix: 0.5 }]).unwrap();
    assert!((out.samples[0] - 1.0).abs() < 1e-6, "dry impulse preserved");
    assert!(out.samples[1..100].iter().all(|s| s.abs() < 1e-6), "silence before echo");
    assert!((out.samples[100] - 0.5).abs() < 1e-6, "echo at 100 frames, half level");
}

#[test]
fn noise_gate_silences_low_level_signal() {
    let rate = 8_000;
    // Very quiet constant (~-60 dBFS) well below a -40 dB threshold.
    let frame = AudioFrame::new(rate, 1, vec![0.001; rate as usize]);
    let out = apply_effects(
        frame,
        &[AudioEffect::NoiseGate { threshold_db: -40.0, attack_ms: 1.0, release_ms: 20.0 }],
    )
    .unwrap();
    let tail = out.samples[rate as usize - 1].abs();
    assert!(tail < 1e-4, "gate should close on sub-threshold noise, tail {tail}");
}

#[test]
fn reverb_bypass_and_tail() {
    let rate = 48_000;
    let mut sig = vec![0.0f32; 4000];
    sig[0] = 1.0;
    // dry=1, wet=0 → identity bypass.
    let bypass = apply_effects(
        AudioFrame::new(rate, 1, sig.clone()),
        &[AudioEffect::Reverb { room_size: 0.5, damping: 0.5, wet: 0.0, dry: 1.0 }],
    )
    .unwrap();
    assert!((bypass.samples[0] - 1.0).abs() < 1e-6 && bypass.samples[1..].iter().all(|s| s.abs() < 1e-6));

    // wet reverb adds decaying energy after the impulse; all finite & in-range.
    let wet = apply_effects(
        AudioFrame::new(rate, 1, sig),
        &[AudioEffect::Reverb { room_size: 0.8, damping: 0.3, wet: 0.6, dry: 0.4 }],
    )
    .unwrap();
    let tail_energy: f32 = wet.samples[2000..].iter().map(|s| s * s).sum();
    assert!(tail_energy > 0.0, "reverb should produce a tail");
    assert!(wet.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
}

#[test]
fn waveform_buckets_bound_the_signal() {
    let rate = 8_000;
    let sig: Vec<f32> = (0..1000).map(|i| if i % 2 == 0 { 0.7 } else { -0.7 }).collect();
    let wf = generate_waveform(&AudioFrame::new(rate, 1, sig), 10);
    assert_eq!(wf.len(), 10);
    for b in &wf {
        assert!(b.max <= 0.7001 && b.min >= -0.7001, "bucket bounds within signal");
        assert!((b.rms - 0.7).abs() < 1e-3, "rms of ±0.7 square ≈ 0.7");
    }
    // Silence → zero buckets.
    let silent = generate_waveform(&AudioFrame::new(rate, 1, vec![0.0; 100]), 4);
    assert!(silent.iter().all(|b| b.min == 0.0 && b.max == 0.0 && b.rms == 0.0));
}

#[test]
fn render_audio_round_trips_through_wav() {
    let rate = 8_000;
    let p = project_mono(rate).with_audio_track(
        AudioTrack::new().with_clip(AudioClip::new(mono(rate, 0.25, rate as usize), 0.0, 1.0)),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.wav");
    render_audio(&p, &path).unwrap();
    assert!(path.exists() && std::fs::metadata(&path).unwrap().len() > 44, "wav has data past header");

    // Re-decode and check format + a sample value.
    use ferrox_core::codecs::WavDecoder;
    use ferrox_core::traits::AudioDecoder;
    let decoded = WavDecoder.decode_audio(std::io::BufReader::new(std::fs::File::open(&path).unwrap())).unwrap();
    assert_eq!(decoded.sample_rate, rate);
    assert_eq!(decoded.channels, 1);
    assert_eq!(decoded.frame_count(), rate as usize);
    assert!((decoded.samples[100] - 0.25).abs() < 1e-3, "round-tripped sample ≈ 0.25");
}

#[test]
fn legacy_video_only_project_json_still_loads() {
    // A project JSON written before audio fields existed must still parse.
    let json = r#"{
        "width": 1920, "height": 1080, "fps": 30.0,
        "background": [0,0,0],
        "tracks": []
    }"#;
    let p = Project::from_json(json).unwrap();
    assert_eq!(p.width, 1920);
    assert_eq!(p.sample_rate, 48_000, "audio sample_rate defaulted");
    assert_eq!(p.channels, 2, "audio channels defaulted");
    assert!(p.audio_tracks.is_empty());
}
