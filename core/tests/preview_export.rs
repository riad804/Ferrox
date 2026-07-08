//! Phase 9 preview/export split: export determinism + full resolution, preview
//! resolution scaling (content-preserving), adaptive quality, and frame skipping.

use ferrox_core::render::{frames_to_skip, render_profiled, AdaptiveQuality, RenderProfile};
use ferrox_core::{Clip, ClipSource, PixelFormat, Project, Track, Transform};

fn full_clip_project(w: u32, h: u32, r: u8, g: u8, b: u8) -> Project {
    let clip = Clip::new(ClipSource::Solid { width: w, height: h, r, g, b, a: 255 }, 0.0, 5.0, Transform::default());
    Project::new(w, h, 30.0).with_track(Track::new().with_clip(clip))
}

fn px(f: &ferrox_core::Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * f.width + x) * 4) as usize;
    [f.data[i], f.data[i + 1], f.data[i + 2], f.data[i + 3]]
}

#[test]
fn export_is_full_resolution_and_deterministic() {
    let project = full_clip_project(64, 48, 30, 120, 200);
    let profile = RenderProfile::export();
    let a = render_profiled(&project, 0.5, &profile, None).unwrap();
    let b = render_profiled(&project, 0.5, &profile, None).unwrap();
    assert_eq!((a.width, a.height), (64, 48), "export renders full resolution");
    assert_eq!(a.data, b.data, "export is bit-identical on re-render");
    assert_eq!(a.format, PixelFormat::Rgba8);
}

#[test]
fn preview_renders_at_reduced_resolution() {
    let project = full_clip_project(64, 48, 30, 120, 200);
    let out = render_profiled(&project, 0.0, &RenderProfile::preview(0.5), None).unwrap();
    assert_eq!((out.width, out.height), (32, 24), "half-resolution preview");
    // The full-canvas clip still fills the (smaller) preview with its colour.
    assert_eq!(px(&out, 16, 12), [30, 120, 200, 255]);
}

#[test]
fn preview_scale_is_clamped() {
    let project = full_clip_project(64, 64, 1, 2, 3);
    // Absurd scales clamp into (0, 1].
    let tiny = render_profiled(&project, 0.0, &RenderProfile::preview(0.0), None).unwrap();
    assert!(tiny.width >= 1 && tiny.width <= 64);
    let big = render_profiled(&project, 0.0, &RenderProfile::preview(5.0), None).unwrap();
    assert_eq!((big.width, big.height), (64, 64), "scale > 1 clamps to full");
}

#[test]
fn positioned_clip_scales_with_the_preview() {
    // A 32×32 red clip at (32,0) on a 64×64 canvas → in a 0.5 preview it lands at
    // (16,0) as a 16×16 block.
    let clip = Clip::new(ClipSource::Solid { width: 32, height: 32, r: 255, g: 0, b: 0, a: 255 }, 0.0, 5.0, Transform::at(32, 0));
    let project = Project::new(64, 64, 30.0).with_track(Track::new().with_clip(clip));
    let out = render_profiled(&project, 0.0, &RenderProfile::preview(0.5), None).unwrap();
    assert_eq!((out.width, out.height), (32, 32));
    assert_eq!(px(&out, 24, 4), [255, 0, 0, 255], "scaled clip present on the right");
    assert_eq!(px(&out, 4, 4), [0, 0, 0, 255], "background on the left");
}

#[test]
fn adaptive_quality_drops_when_slow_rises_when_fast() {
    let mut q = AdaptiveQuality::new(16.0); // 60fps budget
    assert_eq!(q.scale(), 1.0);
    // Consistently slow frames → scale drops toward the floor.
    for _ in 0..10 {
        q.update(50.0);
    }
    assert!(q.scale() < 1.0, "slow frames lower the resolution");
    let low = q.scale();
    // Then fast frames → scale recovers.
    for _ in 0..10 {
        q.update(2.0);
    }
    assert!(q.scale() > low, "headroom raises the resolution back");
    assert!(q.scale() <= 1.0);
    // The controller yields a valid preview profile.
    assert!(!q.profile().deterministic);
}

#[test]
fn frame_skip_math() {
    assert_eq!(frames_to_skip(0.0, 30.0), 0);
    assert_eq!(frames_to_skip(0.1, 30.0), 3); // 0.1s at 30fps ≈ 3 frames behind
    assert_eq!(frames_to_skip(-1.0, 30.0), 0);
    assert_eq!(frames_to_skip(1.0, 0.0), 0);
}
