//! The multi-track audio **mixer** — the audio analog of the video compositor's
//! `compose_frame`. [`mix`] renders a time window of a [`Project`]'s audio tracks
//! into one [`AudioFrame`]; [`render_audio`] mixes the whole timeline and encodes
//! it to a file.
//!
//! Per clip the mixer: obtains the source frame, resamples to the project rate,
//! channel-maps to the project channel count, slices the trimmed region, runs the
//! clip effect stack, applies gain/pan and a linear fade envelope, then sums into
//! the output at the right sample offset (scaled by track gain). Overlapping
//! fade-out/fade-in clips therefore sum into a natural crossfade.

use std::io::BufWriter;
use std::path::Path;

use crate::audio::effects::{apply_effects, db_to_linear, PanFilter};
use crate::audio::AudioFrame;
use crate::error::{Error, Result};
use crate::timeline::Project;
use crate::traits::AudioFilter;
use crate::ResampleFilter;

/// Mix the entire audio timeline of `project` into one [`AudioFrame`].
pub fn mix_full(project: &Project) -> Result<AudioFrame> {
    mix(project, 0.0, project.audio_duration())
}

/// Mix the audio tracks of `project` over `[start, end)` seconds into a single
/// [`AudioFrame`] at the project's `sample_rate` and `channels`.
pub fn mix(project: &Project, start: f64, end: f64) -> Result<AudioFrame> {
    let rate = project.sample_rate.max(1);
    let ch = project.channels.max(1) as usize;
    let n_out = (((end - start).max(0.0)) * rate as f64).round() as usize;
    let mut out = vec![0.0f32; n_out * ch];

    for track in &project.audio_tracks {
        if track.muted {
            continue;
        }
        let track_gain = db_to_linear(track.gain_db);
        for clip in &track.clips {
            // Skip clips that don't overlap the requested window.
            if clip.end() <= start || clip.start >= end {
                continue;
            }

            let clip_buf = render_clip(clip, rate, ch)?;
            let len = clip_buf.frame_count();
            let offset = ((clip.start - start) * rate as f64).round() as i64;

            for f in 0..len {
                let out_f = offset + f as i64;
                if out_f < 0 || out_f as usize >= n_out {
                    continue;
                }
                let of = out_f as usize;
                for c in 0..ch {
                    out[of * ch + c] += clip_buf.samples[f * ch + c] * track_gain;
                }
            }
        }
    }

    // Master safety clamp (linear — keeps in-range sums exact).
    for s in &mut out {
        *s = s.clamp(-1.0, 1.0);
    }
    Ok(AudioFrame::new(rate, ch as u16, out))
}

/// Render one clip to a project-rate, project-channel [`AudioFrame`] of exactly
/// `duration` seconds, with effects, gain, pan and fades applied.
fn render_clip(clip: &crate::timeline::AudioClip, rate: u32, ch: usize) -> Result<AudioFrame> {
    // Decode / synthesize the source, then match project rate + channels.
    let mut src = clip.source.render()?;
    if src.sample_rate != rate {
        src = ResampleFilter::new(rate).process_audio(src)?;
    }
    src = map_channels(src, ch);

    // Slice the trimmed region [in_offset, in_offset + duration).
    let len = (clip.duration * rate as f64).round() as usize;
    let in_start = (clip.in_offset * rate as f64).round() as usize;
    let src_frames = src.frame_count();
    let mut samples = vec![0.0f32; len * ch];
    for f in 0..len {
        let sf = in_start + f;
        if sf < src_frames {
            for c in 0..ch {
                samples[f * ch + c] = src.samples[sf * ch + c];
            }
        }
    }
    let frame = AudioFrame::new(rate, ch as u16, samples);

    // Clip effect stack, then gain, pan, fades.
    let mut frame = apply_effects(frame, &clip.effects)?;
    let g = db_to_linear(clip.gain_db);
    if g != 1.0 {
        for s in &mut frame.samples {
            *s *= g;
        }
    }
    if clip.pan != 0.0 {
        frame = (PanFilter { pan: clip.pan }).process_audio(frame)?;
    }
    apply_fades(&mut frame, clip.fade.in_secs, clip.fade.out_secs, rate, ch, len);
    Ok(frame)
}

/// Apply linear fade-in/out envelopes in place.
fn apply_fades(frame: &mut AudioFrame, in_secs: f64, out_secs: f64, rate: u32, ch: usize, len: usize) {
    let fade_in = (in_secs * rate as f64).round() as usize;
    let fade_out = (out_secs * rate as f64).round() as usize;
    if fade_in == 0 && fade_out == 0 {
        return;
    }
    for f in 0..len {
        let mut env = 1.0f32;
        if fade_in > 0 && f < fade_in {
            env = env.min(f as f32 / fade_in as f32);
        }
        if fade_out > 0 {
            let from_end = len - f; // len at f=0, 1 at last frame
            env = env.min(from_end as f32 / fade_out as f32);
        }
        if env < 1.0 {
            for c in 0..ch {
                frame.samples[f * ch + c] *= env;
            }
        }
    }
}

/// Up-/down-mix interleaved audio to `target` channels.
fn map_channels(src: AudioFrame, target: usize) -> AudioFrame {
    let sc = src.channels.max(1) as usize;
    if sc == target {
        return src;
    }
    let n = src.frame_count();
    let mut out = vec![0.0f32; n * target];
    for f in 0..n {
        if sc == 1 {
            let v = src.samples[f];
            for c in 0..target {
                out[f * target + c] = v;
            }
        } else if target == 1 {
            let mut acc = 0.0f32;
            for c in 0..sc {
                acc += src.samples[f * sc + c];
            }
            out[f] = acc / sc as f32;
        } else {
            for c in 0..target {
                out[f * target + c] = src.samples[f * sc + c.min(sc - 1)];
            }
        }
    }
    AudioFrame::new(src.sample_rate, target as u16, out)
}

/// Mix the whole timeline and encode it to `out_path`, choosing the encoder by
/// file extension (`.wav` always; `.mp3`/`.opus`/`.ogg` when those features are
/// enabled).
pub fn render_audio(project: &Project, out_path: &Path) -> Result<()> {
    use crate::registry::AudioEncoderRegistry;
    let mixed = mix_full(project)?;
    let ext = out_path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| Error::UnsupportedFormat(format!("no extension on '{}'", out_path.display())))?;
    let registry = AudioEncoderRegistry::default();
    let encoder = registry
        .get(ext)
        .ok_or_else(|| Error::UnsupportedFormat(format!("no audio encoder for '{ext}'")))?;
    let file = std::fs::File::create(out_path)?;
    let mut writer = BufWriter::new(file);
    encoder.encode_audio_dyn(&mixed, &mut writer)?;
    Ok(())
}
