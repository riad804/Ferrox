//! Integration tests for the Editor state machine: command apply/undo/redo,
//! full-sequence revert, serialize→reload→render hash parity, thread-safety, and
//! backward-compatible project loading.

use std::sync::Arc;
use std::thread;

use ferrox_sdk::commands::*;
use ferrox_sdk::{
    AnimField, AscCdl, BlendMode, Clip, ClipSource, ColorGrade, Easing, Editor, Keyer, Project,
    Transform,
};

// ── helpers ─────────────────────────────────────────────────────────────────

fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> ClipSource {
    ClipSource::Solid { width: w, height: h, r, g, b, a: 255 }
}

fn clip(w: u32, h: u32, r: u8, g: u8, b: u8, start: f64, dur: f64) -> Clip {
    Clip::new(solid(w, h, r, g, b), start, dur, Transform::default())
}

/// A 64×64 editor with one video track already added.
fn editor_with_track() -> (Editor, usize) {
    let e = Editor::new(64, 64, 30.0);
    let t = e.add_track().unwrap();
    (e, t)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn px(rgba: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

// ── construction / basic ────────────────────────────────────────────────────

#[test]
fn new_editor_is_empty() {
    let e = Editor::new(64, 64, 30.0);
    assert_eq!(e.undo_depth(), 0);
    assert_eq!(e.redo_depth(), 0);
    assert!(e.with_project(|p| p.tracks.is_empty()).unwrap());
}

#[test]
fn add_track_returns_index() {
    let e = Editor::new(64, 64, 30.0);
    assert_eq!(e.add_track().unwrap(), 0);
    assert_eq!(e.add_track().unwrap(), 1);
    assert_eq!(e.with_project(|p| p.tracks.len()).unwrap(), 2);
}

#[test]
fn add_clip_increases_count() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 1);
}

#[test]
fn add_clip_invalid_track_errors() {
    let e = Editor::new(64, 64, 30.0);
    assert!(e.add_clip(5, clip(64, 64, 1, 2, 3, 0.0, 5.0)).is_err());
}

// ── rendering ───────────────────────────────────────────────────────────────

#[test]
fn render_frame_size_matches_project() {
    let e = Editor::new(64, 48, 30.0);
    assert_eq!(e.render_frame(0.0, 0, 0).unwrap().len(), 64 * 48 * 4);
}

#[test]
fn render_frame_resizes_to_requested_dims() {
    let e = Editor::new(64, 64, 30.0);
    assert_eq!(e.render_frame(0.0, 32, 16).unwrap().len(), 32 * 16 * 4);
}

#[test]
fn render_frame_background_then_clip() {
    let (e, t) = editor_with_track();
    let bg = e.render_frame(0.0, 0, 0).unwrap();
    assert_eq!(px(&bg, 64, 0, 0), [0, 0, 0, 255], "empty → black");
    e.add_clip(t, clip(64, 64, 10, 200, 30, 0.0, 5.0)).unwrap();
    let with = e.render_frame(1.0, 0, 0).unwrap();
    assert_eq!(px(&with, 64, 0, 0), [10, 200, 30, 255], "clip visible");
}

// ── AddClip / undo / redo ───────────────────────────────────────────────────

#[test]
fn add_clip_undo_removes_it() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    assert!(e.undo().unwrap());
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 0);
}

#[test]
fn add_clip_undo_then_redo_restores() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 9, 9, 9, 0.0, 5.0)).unwrap();
    e.undo().unwrap();
    assert!(e.redo().unwrap());
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 1);
}

// ── RemoveClip ──────────────────────────────────────────────────────────────

#[test]
fn remove_clip_applies_and_undoes() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    e.execute(Box::new(RemoveClipCommand::new(t, 0))).unwrap();
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 0);
    e.undo().unwrap();
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 1);
}

#[test]
fn remove_clip_invalid_index_errors() {
    let (e, t) = editor_with_track();
    assert!(e.execute(Box::new(RemoveClipCommand::new(t, 0))).is_err());
}

// ── MoveClip / TrimClip ─────────────────────────────────────────────────────

#[test]
fn move_clip_changes_and_reverts_start() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    e.execute(Box::new(MoveClipCommand::new(t, 0, 3.5))).unwrap();
    assert_eq!(e.with_project(|p| p.tracks[t].clips[0].start).unwrap(), 3.5);
    e.undo().unwrap();
    assert_eq!(e.with_project(|p| p.tracks[t].clips[0].start).unwrap(), 0.0);
}

#[test]
fn trim_clip_changes_and_reverts() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    e.execute(Box::new(TrimClipCommand::new(t, 0, 1.0, 2.0))).unwrap();
    assert_eq!(e.with_project(|p| (p.tracks[t].clips[0].start, p.tracks[t].clips[0].duration)).unwrap(), (1.0, 2.0));
    e.undo().unwrap();
    assert_eq!(e.with_project(|p| (p.tracks[t].clips[0].start, p.tracks[t].clips[0].duration)).unwrap(), (0.0, 5.0));
}

// ── effect commands ─────────────────────────────────────────────────────────

#[test]
fn set_color_grade_applies_and_reverts() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 64, 64, 64, 0.0, 5.0)).unwrap();
    let grade = ColorGrade::from_cdl(AscCdl { slope: [2.0, 2.0, 2.0], ..Default::default() });
    e.execute(Box::new(SetColorGradeCommand::new(t, 0, grade))).unwrap();
    // 64 * 2 = 128 on the rendered pixel.
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 64, 0, 0)[0], 128);
    e.undo().unwrap();
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 64, 0, 0)[0], 64);
}

#[test]
fn set_keyer_applies_and_reverts() {
    let (e, t) = editor_with_track();
    // Green clip; keying it out reveals the black background.
    e.add_clip(t, clip(64, 64, 0, 255, 0, 0.0, 5.0)).unwrap();
    e.execute(Box::new(SetKeyerCommand::new(t, 0, Some(Keyer::green())))).unwrap();
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 64, 0, 0), [0, 0, 0, 255], "green keyed → bg");
    e.undo().unwrap();
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 64, 0, 0)[1], 255, "green restored");
}

#[test]
fn set_blend_mode_applies_and_reverts() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    e.execute(Box::new(SetBlendModeCommand::new(t, 0, BlendMode::Screen))).unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].blend == BlendMode::Screen).unwrap());
    e.undo().unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].blend == BlendMode::Normal).unwrap());
}

// ── keyframe commands (per field) ───────────────────────────────────────────

fn setup_clip() -> (Editor, usize) {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(2, 2, 0, 255, 0, 0.0, 5.0)).unwrap();
    (e, t)
}

#[test]
fn add_keyframe_x() {
    let (e, t) = setup_clip();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 0.0, 10.0, Easing::Linear))).unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].animation.x.is_some()).unwrap());
}

#[test]
fn add_keyframe_y() {
    let (e, t) = setup_clip();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::Y, 0.0, 5.0, Easing::Linear))).unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].animation.y.is_some()).unwrap());
}

#[test]
fn add_keyframe_scale() {
    let (e, t) = setup_clip();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::Scale, 1.0, 2.0, Easing::Linear))).unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].animation.scale.is_some()).unwrap());
}

#[test]
fn add_keyframe_opacity_animates_render() {
    let (e, t) = setup_clip();
    // Opacity 0 at t=0 → 1 at t=1; pixel goes from bg to green.
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::Opacity, 0.0, 0.0, Easing::Linear))).unwrap();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::Opacity, 1.0, 1.0, Easing::Linear))).unwrap();
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 2, 0, 0), [0, 0, 0, 255], "opacity 0");
    assert_eq!(px(&e.render_frame(1.0, 0, 0).unwrap(), 2, 0, 0), [0, 255, 0, 255], "opacity 1");
}

#[test]
fn add_keyframe_undo_restores_none() {
    let (e, t) = setup_clip();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 0.0, 10.0, Easing::Linear))).unwrap();
    e.undo().unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].animation.x.is_none()).unwrap());
}

#[test]
fn add_keyframe_overwrites_same_time() {
    let (e, t) = setup_clip();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 1.0, 10.0, Easing::Linear))).unwrap();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 1.0, 20.0, Easing::Linear))).unwrap();
    // Only one keyframe at t=1.0, with the new value.
    let n = e.with_project(|p| match &p.tracks[t].clips[0].animation.x {
        Some(ferrox_sdk::Curve::Keyed(k)) => k.len(),
        _ => 0,
    }).unwrap();
    assert_eq!(n, 1, "same-time keyframe overwritten");
}

#[test]
fn remove_keyframe_applies_and_reverts() {
    let (e, t) = setup_clip();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 0.0, 0.0, Easing::Linear))).unwrap();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 1.0, 10.0, Easing::Linear))).unwrap();
    e.execute(Box::new(RemoveKeyframeCommand::new(t, 0, AnimField::X, 1.0))).unwrap();
    let n = e.with_project(|p| match &p.tracks[t].clips[0].animation.x {
        Some(ferrox_sdk::Curve::Keyed(k)) => k.len(),
        _ => 0,
    }).unwrap();
    assert_eq!(n, 1, "one keyframe removed");
    e.undo().unwrap();
    let n2 = e.with_project(|p| match &p.tracks[t].clips[0].animation.x {
        Some(ferrox_sdk::Curve::Keyed(k)) => k.len(),
        _ => 0,
    }).unwrap();
    assert_eq!(n2, 2, "removal undone");
}

// ── stacks ──────────────────────────────────────────────────────────────────

#[test]
fn undo_empty_returns_false() {
    let e = Editor::new(64, 64, 30.0);
    assert!(!e.undo().unwrap());
}

#[test]
fn redo_empty_returns_false() {
    let e = Editor::new(64, 64, 30.0);
    assert!(!e.redo().unwrap());
}

#[test]
fn new_execute_clears_redo() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 1, 1, 0.0, 5.0)).unwrap();
    e.undo().unwrap();
    assert_eq!(e.redo_depth(), 1);
    e.add_clip(t, clip(64, 64, 2, 2, 2, 0.0, 5.0)).unwrap();
    assert_eq!(e.redo_depth(), 0, "new command clears redo");
}

#[test]
fn undo_redo_depths_track() {
    let (e, _t) = editor_with_track();
    assert_eq!(e.undo_depth(), 1); // AddTrack
    e.undo().unwrap();
    assert_eq!(e.undo_depth(), 0);
    assert_eq!(e.redo_depth(), 1);
}

// ── full sequence & parity ──────────────────────────────────────────────────

#[test]
fn full_sequence_undo_all_reverts_to_empty() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 10, 20, 30, 0.0, 5.0)).unwrap();
    e.add_clip(t, clip(64, 64, 40, 50, 60, 5.0, 5.0)).unwrap();
    e.execute(Box::new(SetColorGradeCommand::new(t, 0, ColorGrade::from_cdl(AscCdl { saturation: 0.0, ..Default::default() })))).unwrap();
    e.execute(Box::new(SetKeyerCommand::new(t, 1, Some(Keyer::green())))).unwrap();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::Opacity, 0.0, 0.0, Easing::Linear))).unwrap();

    while e.undo().unwrap() {}
    assert_eq!(e.project_snapshot().unwrap(), Project::new(64, 64, 30.0), "fully reverted to a fresh project");
}

#[test]
fn serialize_reload_render_hash_parity() {
    // Build a non-trivial project.
    let (e, t) = editor_with_track();
    e.add_track().unwrap();
    e.add_clip(t, clip(64, 64, 200, 40, 40, 0.0, 5.0)).unwrap();
    e.add_clip(1, clip(64, 64, 40, 40, 200, 0.0, 5.0)).unwrap();
    e.execute(Box::new(SetBlendModeCommand::new(1, 0, BlendMode::Screen))).unwrap();
    e.execute(Box::new(SetColorGradeCommand::new(t, 0, ColorGrade::from_cdl(AscCdl { slope: [1.5, 1.0, 1.0], ..Default::default() })))).unwrap();
    e.execute(Box::new(AddKeyframeCommand::new(1, 0, AnimField::X, 0.0, 5.0, Easing::EaseInOut))).unwrap();

    let hash_before = fnv1a(&e.render_frame(0.5, 0, 0).unwrap());

    // Round-trip through JSON into a fresh editor.
    let json = e.save_json().unwrap();
    let e2 = Editor::new(1, 1, 1.0);
    e2.load_json(&json).unwrap();
    let hash_after = fnv1a(&e2.render_frame(0.5, 0, 0).unwrap());

    assert_eq!(hash_before, hash_after, "render is identical after save+reload");
    assert_eq!(e.project_snapshot().unwrap(), e2.project_snapshot().unwrap());
}

#[test]
fn load_json_clears_history() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    let json = e.save_json().unwrap();
    e.load_json(&json).unwrap();
    assert_eq!(e.undo_depth(), 0);
    assert_eq!(e.redo_depth(), 0);
    assert!(!e.undo().unwrap());
}

#[test]
fn backward_compatible_old_json_loads() {
    // JSON written before blend/color/keyer/mask/animation/audio fields existed.
    let json = r#"{
        "width": 64, "height": 64, "fps": 30.0,
        "tracks": [ { "clips": [
            { "source": { "kind": "solid", "width": 64, "height": 64, "r": 10, "g": 20, "b": 30, "a": 255 },
              "start": 0.0, "duration": 5.0 }
        ] } ]
    }"#;
    let e = Editor::new(1, 1, 1.0);
    e.load_json(json).unwrap();
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 64, 0, 0), [10, 20, 30, 255]);
}

#[test]
fn add_clip_json_parses_and_renders() {
    let (e, t) = editor_with_track();
    let clip_json = r#"{"source":{"kind":"solid","width":64,"height":64,"r":5,"g":6,"b":7,"a":255},"start":0.0,"duration":5.0}"#;
    e.add_clip_json(t, clip_json).unwrap();
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 64, 0, 0), [5, 6, 7, 255]);
}

#[test]
fn add_clip_json_invalid_errors() {
    let (e, t) = editor_with_track();
    assert!(e.add_clip_json(t, "{ not valid json").is_err());
}

#[test]
fn save_json_roundtrips_project_equal() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    let p1 = e.project_snapshot().unwrap();
    let json = e.save_json().unwrap();
    let e2 = Editor::new(1, 1, 1.0);
    e2.load_json(&json).unwrap();
    assert_eq!(p1, e2.project_snapshot().unwrap());
}

#[test]
fn render_after_undo_matches_prior_hash() {
    let (e, t) = editor_with_track();
    let before = fnv1a(&e.render_frame(0.0, 0, 0).unwrap());
    e.add_clip(t, clip(64, 64, 100, 100, 100, 0.0, 5.0)).unwrap();
    e.undo().unwrap();
    let after = fnv1a(&e.render_frame(0.0, 0, 0).unwrap());
    assert_eq!(before, after, "undo restores exact pixels");
}

#[test]
fn multiple_undo_then_redo_roundtrip() {
    let (e, t) = editor_with_track();
    for i in 0..5 {
        e.add_clip(t, clip(64, 64, i * 10, 0, 0, i as f64, 1.0)).unwrap();
    }
    let full = e.project_snapshot().unwrap();
    for _ in 0..5 {
        e.undo().unwrap();
    }
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 0);
    for _ in 0..5 {
        e.redo().unwrap();
    }
    assert_eq!(e.project_snapshot().unwrap(), full, "redo reconstructs the exact project");
}

#[test]
fn command_names_are_reported() {
    assert_eq!(AddTrackCommand::new().name(), "AddTrack");
    assert_eq!(AddClipCommand::new(0, clip(1, 1, 0, 0, 0, 0.0, 1.0)).name(), "AddClip");
    assert_eq!(MoveClipCommand::new(0, 0, 1.0).name(), "MoveClip");
    assert_eq!(SetBlendModeCommand::new(0, 0, BlendMode::Multiply).name(), "SetBlendMode");
    assert_eq!(AddKeyframeCommand::new(0, 0, AnimField::X, 0.0, 0.0, Easing::Linear).name(), "AddKeyframe");
}

// ── thread-safety ───────────────────────────────────────────────────────────

#[test]
fn concurrent_undo_redo_is_safe_and_preserves_invariant() {
    let (e, t) = editor_with_track();
    // Execute a batch of commands so both stacks have material to move around.
    for i in 0..20 {
        e.add_clip(t, clip(64, 64, i as u8, 0, 0, i as f64, 1.0)).unwrap();
    }
    let total = e.undo_depth(); // every command lives in exactly one stack
    assert_eq!(e.undo_depth() + e.redo_depth(), total);

    let e = Arc::new(e);
    let mut handles = Vec::new();
    for _ in 0..100 {
        let e = Arc::clone(&e);
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                let _ = e.undo();
                let _ = e.render_frame(0.0, 0, 0);
                let _ = e.redo();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // Invariant: no command is lost or duplicated — the two stacks still hold
    // exactly `total` commands between them, and the state machine is usable.
    assert_eq!(e.undo_depth() + e.redo_depth(), total, "no command lost/duplicated under contention");
    let _ = e.render_frame(0.0, 0, 0).unwrap();
}

// ── additional coverage ─────────────────────────────────────────────────────

#[test]
fn move_clip_invalid_index_errors() {
    let (e, t) = editor_with_track();
    assert!(e.execute(Box::new(MoveClipCommand::new(t, 9, 1.0))).is_err());
}

#[test]
fn trim_clip_invalid_track_errors() {
    let e = Editor::new(64, 64, 30.0);
    assert!(e.execute(Box::new(TrimClipCommand::new(3, 0, 0.0, 1.0))).is_err());
}

#[test]
fn set_color_grade_invalid_index_errors() {
    let (e, t) = editor_with_track();
    assert!(e.execute(Box::new(SetColorGradeCommand::new(t, 0, ColorGrade::default()))).is_err());
}

#[test]
fn add_keyframe_invalid_clip_errors() {
    let (e, t) = editor_with_track();
    assert!(e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 0.0, 1.0, Easing::Linear))).is_err());
}

#[test]
fn keyframe_on_const_curve_preserves_constant() {
    let (e, t) = editor_with_track();
    let c = clip(2, 2, 0, 255, 0, 0.0, 5.0)
        .with_animation(ferrox_sdk::ClipAnimation { x: Some(ferrox_sdk::Curve::Const(3.0)), ..Default::default() });
    e.add_clip(t, c).unwrap();
    e.execute(Box::new(AddKeyframeCommand::new(t, 0, AnimField::X, 1.0, 10.0, Easing::Linear))).unwrap();
    let n = e.with_project(|p| match &p.tracks[t].clips[0].animation.x {
        Some(ferrox_sdk::Curve::Keyed(k)) => k.len(),
        _ => 0,
    }).unwrap();
    assert_eq!(n, 2, "constant becomes a keyframe at t=0 plus the new one");
}

#[test]
fn editor_clone_shares_state() {
    let (e, t) = editor_with_track();
    let e2 = e.clone();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    assert_eq!(e2.with_project(|p| p.tracks[t].clips.len()).unwrap(), 1, "clone observes mutation");
    e2.undo().unwrap();
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 0, "undo via clone affects original");
}

#[test]
fn resized_render_preserves_clip_color() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 12, 34, 56, 0.0, 5.0)).unwrap();
    let small = e.render_frame(0.0, 8, 8).unwrap();
    assert_eq!(small.len(), 8 * 8 * 4);
    assert_eq!(px(&small, 8, 4, 4), [12, 34, 56, 255], "downscaled pixel keeps color");
}

#[test]
fn project_file_round_trip() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 9, 8, 7, 0.0, 5.0)).unwrap();
    let p = e.project_snapshot().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proj.json");
    ferrox_sdk::project_io::save_project(&p, &path).unwrap();
    let loaded = ferrox_sdk::project_io::load_project(&path).unwrap();
    assert_eq!(p, loaded);
}

#[test]
fn blend_mode_variants_apply() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    for mode in [BlendMode::Multiply, BlendMode::Overlay, BlendMode::Difference, BlendMode::SoftLight] {
        e.execute(Box::new(SetBlendModeCommand::new(t, 0, mode))).unwrap();
        assert!(e.with_project(|p| p.tracks[t].clips[0].blend == mode).unwrap());
    }
}

#[test]
fn set_keyer_clear_then_undo() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 0, 255, 0, 0.0, 5.0)).unwrap();
    e.execute(Box::new(SetKeyerCommand::new(t, 0, Some(Keyer::green())))).unwrap();
    e.execute(Box::new(SetKeyerCommand::new(t, 0, None))).unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].keyer.is_none()).unwrap());
    e.undo().unwrap();
    assert!(e.with_project(|p| p.tracks[t].clips[0].keyer.is_some()).unwrap(), "clear undone → keyer back");
}

#[test]
fn undo_all_then_redo_all_hash_parity() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 200, 10, 10, 0.0, 5.0)).unwrap();
    e.execute(Box::new(SetColorGradeCommand::new(t, 0, ColorGrade::from_cdl(AscCdl { power: [2.0, 2.0, 2.0], ..Default::default() })))).unwrap();
    let full = fnv1a(&e.render_frame(0.0, 0, 0).unwrap());
    while e.undo().unwrap() {}
    while e.redo().unwrap() {}
    assert_eq!(fnv1a(&e.render_frame(0.0, 0, 0).unwrap()), full, "undo-all then redo-all is a no-op");
}

#[test]
fn second_track_composites_on_top() {
    let (e, t0) = editor_with_track();
    let t1 = e.add_track().unwrap();
    e.add_clip(t0, clip(64, 64, 255, 0, 0, 0.0, 5.0)).unwrap();
    e.add_clip(t1, clip(64, 64, 0, 0, 255, 0.0, 5.0)).unwrap();
    assert_eq!(px(&e.render_frame(0.0, 0, 0).unwrap(), 64, 0, 0), [0, 0, 255, 255], "top track wins");
}

#[test]
fn add_clip_on_missing_second_track_errors() {
    let (e, _t) = editor_with_track();
    assert!(e.add_clip(1, clip(64, 64, 1, 2, 3, 0.0, 5.0)).is_err());
}

#[test]
fn bad_json_load_errors_and_leaves_state() {
    let (e, t) = editor_with_track();
    e.add_clip(t, clip(64, 64, 1, 2, 3, 0.0, 5.0)).unwrap();
    assert!(e.load_json("{ not json").is_err());
    // State is unchanged after a failed load (parse happens before swap).
    assert_eq!(e.with_project(|p| p.tracks[t].clips.len()).unwrap(), 1);
}

#[test]
fn zero_dims_render_uses_project_size() {
    let e = Editor::new(20, 10, 30.0);
    assert_eq!(e.render_frame(0.0, 0, 0).unwrap().len(), 20 * 10 * 4);
}
