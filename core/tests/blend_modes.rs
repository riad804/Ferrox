//! Golden pixel tests for Porter-Duff / Photoshop blend modes. A full-cover clip
//! (colour s) is composited over a solid canvas (colour d); with opacity 1 the
//! output equals `blend(d, s)`, so each expected value is computed by hand.

use ferrox_core::{BlendMode, Clip, ClipSource, PixelFormat, Project, Track, Transform};

/// Compose a 1x1 project: background grey `d`, one full-cover clip of grey `s`.
fn blend_px(d: u8, s: u8, mode: BlendMode) -> [u8; 4] {
    let clip = Clip::new(ClipSource::Solid { width: 1, height: 1, r: s, g: s, b: s, a: 255 }, 0.0, 1.0, Transform::default())
        .with_blend(mode);
    let project = Project::new(1, 1, 30.0)
        .with_background(d, d, d)
        .with_track(Track::new().with_clip(clip));
    let f = ferrox_core::compose_frame(&project, 0.0).unwrap();
    assert_eq!(f.format, PixelFormat::Rgba8);
    [f.data[0], f.data[1], f.data[2], f.data[3]]
}

fn near(actual: u8, expected: u8) {
    assert!((actual as i32 - expected as i32).abs() <= 1, "got {actual}, expected {expected}");
}

#[test]
fn separable_blend_formulas_match_hand_computed_values() {
    // d = 0.4 (102), s = 0.6 (153).
    let (d, s) = (102u8, 153u8);
    near(blend_px(d, s, BlendMode::Normal)[0], 153);
    near(blend_px(d, s, BlendMode::Multiply)[0], 61); // 0.24
    near(blend_px(d, s, BlendMode::Screen)[0], 194); // 0.76
    near(blend_px(d, s, BlendMode::Overlay)[0], 122); // 0.48
    near(blend_px(d, s, BlendMode::HardLight)[0], 133); // 0.52
    near(blend_px(d, s, BlendMode::Darken)[0], 102);
    near(blend_px(d, s, BlendMode::Lighten)[0], 153);
    near(blend_px(d, s, BlendMode::Difference)[0], 51); // 0.2
    near(blend_px(d, s, BlendMode::Exclusion)[0], 133); // 0.52
    near(blend_px(d, s, BlendMode::Add)[0], 255); // clamped
    near(blend_px(d, s, BlendMode::SoftLight)[0], 114); // ~0.4465
}

#[test]
fn multiply_by_white_and_black_are_identity_and_black() {
    // Multiply with white (s=255) leaves d unchanged; with black (0) → black.
    near(blend_px(200, 255, BlendMode::Multiply)[0], 200);
    near(blend_px(200, 0, BlendMode::Multiply)[0], 0);
    // Screen with black leaves d; with white → white.
    near(blend_px(200, 0, BlendMode::Screen)[0], 200);
    near(blend_px(50, 255, BlendMode::Screen)[0], 255);
}

#[test]
fn opacity_scales_blend_contribution() {
    // Normal over black, s=255, opacity 0.5 → 128.
    let clip = Clip::new(ClipSource::Solid { width: 1, height: 1, r: 255, g: 255, b: 255, a: 255 }, 0.0, 1.0, Transform { x: 0, y: 0, scale: 1.0, opacity: 0.5 });
    let project = Project::new(1, 1, 30.0).with_background(0, 0, 0).with_track(Track::new().with_clip(clip));
    let f = ferrox_core::compose_frame(&project, 0.0).unwrap();
    near(f.data[0], 128);
}

#[test]
fn blend_mode_omitted_from_json_when_normal() {
    let project = Project::new(2, 2, 30.0)
        .with_track(Track::new().with_clip(Clip::new(ClipSource::Solid { width: 1, height: 1, r: 1, g: 1, b: 1, a: 255 }, 0.0, 1.0, Transform::default())));
    let json = project.to_json().unwrap();
    assert!(!json.contains("blend"), "Normal blend omitted");
    // A non-normal blend survives round-trip.
    let p2 = Project::new(2, 2, 30.0).with_track(Track::new().with_clip(
        Clip::new(ClipSource::Solid { width: 1, height: 1, r: 1, g: 1, b: 1, a: 255 }, 0.0, 1.0, Transform::default()).with_blend(BlendMode::Screen),
    ));
    assert_eq!(Project::from_json(&p2.to_json().unwrap()).unwrap(), p2);
}
