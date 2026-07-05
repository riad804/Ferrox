//! Golden-frame tests for the timeline model + compositor.
//!
//! These exercise the seam that makes ferrox an *editing* engine:
//! `compose_frame(project, t)` must render a timeline position deterministically
//! — correct z-order, per-clip position, temporal activation, opacity blending —
//! and projects must round-trip through JSON (save/load).

use ferrox_core::{compose_frame, Clip, ClipSource, PixelFormat, Project, Track, Transform};

/// Read the RGBA of pixel (x, y) from an Rgba8 frame.
fn px(frame: &ferrox_core::Frame, x: u32, y: u32) -> [u8; 4] {
    assert_eq!(frame.format, PixelFormat::Rgba8);
    let i = ((y * frame.width + x) * 4) as usize;
    [frame.data[i], frame.data[i + 1], frame.data[i + 2], frame.data[i + 3]]
}

fn solid(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> ClipSource {
    ClipSource::Solid { width, height, r, g, b, a }
}

#[test]
fn composites_position_zorder_and_temporal_activation() {
    // 4×4 canvas on a distinct background; one 2×2 green clip placed at (1,1),
    // visible for [0, 1) seconds.
    let project = Project::new(4, 4, 30.0)
        .with_background(10, 20, 30)
        .with_track(Track::new().with_clip(Clip::new(
            solid(2, 2, 0, 255, 0, 255),
            0.0,
            1.0,
            Transform::at(1, 1),
        )));

    // While the clip is active, only its 2×2 footprint is green.
    let f = compose_frame(&project, 0.5).unwrap();
    assert_eq!(f.width, 4);
    assert_eq!(f.height, 4);
    assert_eq!(px(&f, 0, 0), [10, 20, 30, 255], "background outside clip");
    assert_eq!(px(&f, 1, 1), [0, 255, 0, 255], "clip top-left");
    assert_eq!(px(&f, 2, 2), [0, 255, 0, 255], "clip bottom-right");
    assert_eq!(px(&f, 3, 3), [10, 20, 30, 255], "background past clip");

    // Past the clip's out point, the canvas is pure background.
    let empty = compose_frame(&project, 2.0).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(px(&empty, x, y), [10, 20, 30, 255], "inactive clip must not draw");
        }
    }
}

#[test]
fn higher_track_composites_on_top() {
    // Bottom track: full red. Top track: full blue. Blue must win everywhere.
    let project = Project::new(2, 2, 30.0)
        .with_track(Track::new().with_clip(Clip::new(
            solid(2, 2, 255, 0, 0, 255),
            0.0,
            5.0,
            Transform::default(),
        )))
        .with_track(Track::new().with_clip(Clip::new(
            solid(2, 2, 0, 0, 255, 255),
            0.0,
            5.0,
            Transform::default(),
        )));

    let f = compose_frame(&project, 1.0).unwrap();
    assert_eq!(px(&f, 0, 0), [0, 0, 255, 255], "top track wins z-order");
}

#[test]
fn opacity_alpha_blends_over_background() {
    // Full-canvas red clip at 50% opacity over background (10, 20, 30).
    let project = Project::new(1, 1, 30.0)
        .with_background(10, 20, 30)
        .with_track(Track::new().with_clip(Clip::new(
            solid(1, 1, 255, 0, 0, 255),
            0.0,
            1.0,
            Transform { x: 0, y: 0, scale: 1.0, opacity: 0.5 },
        )));

    let f = compose_frame(&project, 0.0).unwrap();
    let [r, g, b, a] = px(&f, 0, 0);
    // over-operator: out = src*0.5 + dst*0.5
    assert!((r as i32 - 133).abs() <= 1, "R blended, got {r}");
    assert!((g as i32 - 10).abs() <= 1, "G blended, got {g}");
    assert!((b as i32 - 15).abs() <= 1, "B blended, got {b}");
    assert_eq!(a, 255, "canvas stays opaque");
}

#[test]
fn nearest_scale_enlarges_clip() {
    // A 1×1 green clip scaled 4× fills a 4×4 region from its origin.
    let project = Project::new(4, 4, 30.0)
        .with_track(Track::new().with_clip(Clip::new(
            solid(1, 1, 0, 255, 0, 255),
            0.0,
            1.0,
            Transform { x: 0, y: 0, scale: 4.0, opacity: 1.0 },
        )));

    let f = compose_frame(&project, 0.0).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(px(&f, x, y), [0, 255, 0, 255], "scaled clip fills canvas");
        }
    }
}

#[test]
fn project_round_trips_through_json() {
    let project = Project::new(1920, 1080, 24.0)
        .with_background(5, 6, 7)
        .with_track(Track::new().with_clip(Clip::new(
            solid(100, 100, 1, 2, 3, 200),
            1.5,
            3.0,
            Transform::at(10, 20),
        )));

    let json = project.to_json().unwrap();
    let loaded = Project::from_json(&json).unwrap();
    assert_eq!(project, loaded, "save/load must be lossless");
    assert!((project.duration() - 4.5).abs() < 1e-9, "duration = last clip end");
}
