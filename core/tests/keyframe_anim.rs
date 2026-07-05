//! Tests for the keyframe animation engine and animated clip transforms:
//! curve interpolation/easing, range clamping, an animated `compose_frame` whose
//! pixels change over time, cross-dissolve via overlapping opacity fades, and
//! serde round-trip of an animated project.

use ferrox_core::{
    compose_frame, Clip, ClipAnimation, ClipSource, Curve, Easing, Keyframe, PixelFormat, Project,
    Track, Transform,
};

fn px(frame: &ferrox_core::Frame, x: u32, y: u32) -> [u8; 4] {
    assert_eq!(frame.format, PixelFormat::Rgba8);
    let i = ((y * frame.width + x) * 4) as usize;
    [frame.data[i], frame.data[i + 1], frame.data[i + 2], frame.data[i + 3]]
}

fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> ClipSource {
    ClipSource::Solid { width: w, height: h, r, g, b, a: 255 }
}

#[test]
fn linear_curve_interpolates_and_clamps() {
    let c = Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(2.0, 10.0)]);
    assert_eq!(c.sample(-1.0), 0.0, "clamp before first");
    assert_eq!(c.sample(0.0), 0.0);
    assert!((c.sample(1.0) - 5.0).abs() < 1e-5, "midpoint");
    assert_eq!(c.sample(2.0), 10.0);
    assert_eq!(c.sample(99.0), 10.0, "clamp after last");
}

#[test]
fn const_curve_is_flat() {
    let c = Curve::Const(0.75);
    assert_eq!(c.sample(0.0), 0.75);
    assert_eq!(c.sample(1000.0), 0.75);
}

#[test]
fn hold_easing_steps() {
    let c = Curve::keyed(vec![
        Keyframe::new(0.0, 1.0).with_ease(Easing::Hold),
        Keyframe::new(1.0, 5.0),
    ]);
    assert_eq!(c.sample(0.0), 1.0);
    assert_eq!(c.sample(0.999), 1.0, "holds start value across the segment");
    assert_eq!(c.sample(1.0), 5.0, "jumps at the next keyframe");
}

#[test]
fn bezier_ease_is_monotonic_and_bounded() {
    let c = Curve::keyed(vec![
        Keyframe::new(0.0, 0.0).with_ease(Easing::EaseInOut),
        Keyframe::new(1.0, 1.0),
    ]);
    // Endpoints exact; interior stays within [0,1] and increases.
    assert!(c.sample(0.0).abs() < 1e-4);
    assert!((c.sample(1.0) - 1.0).abs() < 1e-4);
    let mut prev = -1.0;
    for i in 0..=10 {
        let v = c.sample(i as f64 / 10.0);
        assert!((0.0..=1.0).contains(&v), "eased value in range: {v}");
        assert!(v >= prev - 1e-4, "monotonic non-decreasing");
        prev = v;
    }
    // Ease-in-out is symmetric: midpoint ≈ 0.5.
    assert!((c.sample(0.5) - 0.5).abs() < 0.05);
}

#[test]
fn animated_opacity_changes_composited_pixels_over_time() {
    // A green clip fading in over 1s on a black canvas; sample the pixel at
    // t=0 (invisible) and t=1 (fully visible).
    let clip = Clip::new(solid(2, 2, 0, 255, 0), 0.0, 2.0, Transform::default())
        .with_animation(ClipAnimation::fade_in(1.0));
    let project = Project::new(2, 2, 30.0).with_track(Track::new().with_clip(clip));

    let start = compose_frame(&project, 0.0).unwrap();
    assert_eq!(px(&start, 0, 0), [0, 0, 0, 255], "opacity 0 → background only");

    let mid = compose_frame(&project, 0.5).unwrap();
    let g = px(&mid, 0, 0)[1];
    assert!(g > 100 && g < 160, "half-faded green ≈ 128, got {g}");

    let end = compose_frame(&project, 1.0).unwrap();
    assert_eq!(px(&end, 0, 0), [0, 255, 0, 255], "opacity 1 → full green");
}

#[test]
fn animated_position_moves_a_clip() {
    // A 1x1 red clip whose x animates 0→3 over 3s across a 4-wide canvas.
    let anim = ClipAnimation {
        x: Some(Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(3.0, 3.0)])),
        ..ClipAnimation::default()
    };
    let clip = Clip::new(solid(1, 1, 255, 0, 0), 0.0, 4.0, Transform::default()).with_animation(anim);
    let project = Project::new(4, 1, 30.0).with_track(Track::new().with_clip(clip));

    // At t=0 the red pixel is at x=0; at t=3 it's at x=3.
    let f0 = compose_frame(&project, 0.0).unwrap();
    assert_eq!(px(&f0, 0, 0), [255, 0, 0, 255]);
    assert_eq!(px(&f0, 3, 0), [0, 0, 0, 255]);

    let f3 = compose_frame(&project, 3.0).unwrap();
    assert_eq!(px(&f3, 0, 0), [0, 0, 0, 255]);
    assert_eq!(px(&f3, 3, 0), [255, 0, 0, 255]);
}

#[test]
fn cross_dissolve_blends_two_clips() {
    // Bottom clip red (fading out), top clip blue (fading in), overlapping the
    // whole 1s. At the midpoint the pixel is a ~50/50 blend.
    let red = Clip::new(solid(1, 1, 255, 0, 0), 0.0, 1.0, Transform::default())
        .with_animation(ClipAnimation::fade_out(1.0, 1.0));
    let blue = Clip::new(solid(1, 1, 0, 0, 255), 0.0, 1.0, Transform::default())
        .with_animation(ClipAnimation::fade_in(1.0));
    let project = Project::new(1, 1, 30.0)
        .with_track(Track::new().with_clip(red))
        .with_track(Track::new().with_clip(blue));

    let mid = compose_frame(&project, 0.5).unwrap();
    let [r, _g, b, _a] = px(&mid, 0, 0);
    // Red is drawn first at ~50% over black → ~128; blue at ~50% over that.
    assert!(b > 100, "blue present in dissolve, got {b}");
    assert!(r > 40 && r < 160, "red partially remains, got {r}");
}

#[test]
fn animated_project_round_trips_through_json() {
    let clip = Clip::new(solid(4, 4, 1, 2, 3), 0.0, 5.0, Transform::at(1, 1)).with_animation(
        ClipAnimation {
            opacity: Some(Curve::keyed(vec![
                Keyframe::new(0.0, 0.0).with_ease(Easing::EaseIn),
                Keyframe::new(1.0, 1.0),
            ])),
            scale: Some(Curve::Const(2.0)),
            ..ClipAnimation::default()
        },
    );
    let project = Project::new(64, 64, 30.0).with_track(Track::new().with_clip(clip));
    let json = project.to_json().unwrap();
    let loaded = Project::from_json(&json).unwrap();
    assert_eq!(project, loaded, "animation survives save/load");
}

#[test]
fn clip_without_animation_omits_it_from_json() {
    // Back-compat: a plain clip serialises without an `animation` field, and old
    // JSON lacking the field still loads.
    let project = Project::new(8, 8, 30.0)
        .with_track(Track::new().with_clip(Clip::new(solid(2, 2, 9, 9, 9), 0.0, 1.0, Transform::default())));
    let json = project.to_json().unwrap();
    assert!(!json.contains("animation"), "empty animation omitted from JSON");
    assert_eq!(Project::from_json(&json).unwrap(), project);
}
