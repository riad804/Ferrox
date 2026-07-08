//! Phase 8 playback engine: transport controls, speed/reverse/loop, end
//! handling, frame stepping, frame-skip-via-advance, scrubbing, and rendering
//! the current frame.

use ferrox_core::playback::{PlaybackController, PlayState, Transport};
use ferrox_core::{Clip, ClipSource, PixelFormat, Project, RenderProfile, Track, Transform};

const EPS: f64 = 1e-9;

fn transport() -> Transport {
    Transport::new(10.0, 30.0) // 10s @ 30fps
}

// ── state machine ───────────────────────────────────────────────────────────

#[test]
fn starts_stopped_and_transitions() {
    let mut t = transport();
    assert_eq!(t.state(), PlayState::Stopped);
    t.play();
    assert!(t.is_playing());
    t.pause();
    assert_eq!(t.state(), PlayState::Paused);
    t.play();
    t.stop();
    assert_eq!(t.state(), PlayState::Stopped);
    assert_eq!(t.position(), 0.0);
}

#[test]
fn advance_moves_only_while_playing() {
    let mut t = transport();
    t.advance(1.0);
    assert_eq!(t.position(), 0.0, "paused/stopped does not move");
    t.play();
    t.advance(2.0);
    assert!((t.position() - 2.0).abs() < EPS);
}

#[test]
fn speed_scales_advance() {
    let mut t = transport();
    t.play();
    t.set_speed(2.0);
    t.advance(1.0);
    assert!((t.position() - 2.0).abs() < EPS, "2x speed → double distance");
}

#[test]
fn reverse_plays_backwards() {
    let mut t = transport();
    t.play();
    t.seek(5.0);
    t.set_reversed(true);
    t.advance(2.0);
    assert!((t.position() - 3.0).abs() < EPS);
}

#[test]
fn non_looping_clamps_and_pauses_at_end() {
    let mut t = transport();
    t.play();
    t.seek(9.5);
    t.advance(2.0); // would overshoot 10s
    assert!((t.position() - 10.0).abs() < EPS);
    assert_eq!(t.state(), PlayState::Paused, "pauses at the end");
}

#[test]
fn reverse_pauses_at_start() {
    let mut t = transport();
    t.play();
    t.seek(1.0);
    t.set_reversed(true);
    t.advance(2.0);
    assert_eq!(t.position(), 0.0);
    assert_eq!(t.state(), PlayState::Paused);
}

#[test]
fn looping_wraps_around() {
    let mut t = transport();
    t.play();
    t.set_looping(true);
    t.seek(9.0);
    t.advance(2.0); // 9 + 2 = 11 → wraps to 1
    assert!((t.position() - 1.0).abs() < EPS);
    assert!(t.is_playing(), "looping keeps playing");
}

#[test]
fn frame_step_and_index() {
    let mut t = transport();
    t.step(3); // 3 frames @ 30fps = 0.1s
    assert!((t.position() - 0.1).abs() < EPS);
    assert_eq!(t.current_frame(), 3);
    t.step(-3);
    assert!(t.position().abs() < EPS);
}

#[test]
fn seek_clamps_and_supports_scrubbing() {
    let mut t = transport();
    t.seek(-5.0);
    assert_eq!(t.position(), 0.0);
    t.seek(999.0);
    assert_eq!(t.position(), 10.0);
    // Scrubbing = repeated seeks while paused.
    for p in [2.0, 4.0, 1.5] {
        t.seek(p);
        assert!((t.position() - p).abs() < EPS);
    }
}

#[test]
fn large_elapsed_skips_frames_via_advance() {
    // The scheduler property: a slow host passing a big elapsed jumps the
    // playhead forward (skipping intermediate frames) rather than lagging.
    let mut t = transport();
    t.play();
    t.advance(0.5); // 0.5s at 30fps = 15 frames advanced in one tick
    assert!((t.position() - 0.5).abs() < EPS);
    assert_eq!(t.current_frame(), 15);
}

// ── controller rendering ────────────────────────────────────────────────────

#[test]
fn controller_renders_current_frame() {
    let clip = Clip::new(ClipSource::Solid { width: 32, height: 32, r: 10, g: 200, b: 30, a: 255 }, 0.0, 5.0, Transform::default());
    let project = Project::new(32, 32, 30.0).with_track(Track::new().with_clip(clip));

    let mut pc = PlaybackController::for_project(&project);
    pc.transport().play();
    pc.advance(1.0);
    let frame = pc.render(&project, &RenderProfile::preview(0.5), None).unwrap();
    assert_eq!((frame.width, frame.height), (16, 16), "adaptive preview resolution");
    assert_eq!(frame.format, PixelFormat::Rgba8);
    assert_eq!(&frame.data[..4], &[10, 200, 30, 255], "clip visible at the playhead");
}

#[test]
fn controller_duration_covers_video_and_audio() {
    let clip = Clip::new(ClipSource::Solid { width: 4, height: 4, r: 0, g: 0, b: 0, a: 255 }, 0.0, 3.0, Transform::default());
    let project = Project::new(4, 4, 30.0).with_track(Track::new().with_clip(clip));
    let mut pc = PlaybackController::for_project(&project);
    assert!((pc.transport().duration() - 3.0).abs() < EPS);
}
