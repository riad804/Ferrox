//! Tests for spatial transitions built as ClipAnimation curves, including the
//! continuity mandate (no teleporting when fade + slide are combined).

use ferrox_core::{
    Clip, ClipAnimation, ClipSource, Curve, Direction, Easing, Keyframe, Transform, Transition,
};

fn clip_with(anim: ClipAnimation) -> Clip {
    Clip::new(ClipSource::Solid { width: 2, height: 2, r: 1, g: 2, b: 3, a: 255 }, 0.0, 4.0, Transform::default())
        .with_animation(anim)
}

#[test]
fn slide_in_from_left_animates_x_to_rest() {
    let clip = clip_with(Transition::slide_in(Direction::Left, 100, 1.0, Easing::Linear));
    assert_eq!(clip.effective_transform(0.0).x, -100, "starts off-screen left");
    assert_eq!(clip.effective_transform(0.5).x, -50, "half-way");
    assert_eq!(clip.effective_transform(1.0).x, 0, "rests at 0");
    // Opacity fades in alongside.
    assert!(clip.effective_transform(0.0).opacity < 0.01);
    assert!((clip.effective_transform(1.0).opacity - 1.0).abs() < 0.01);
}

#[test]
fn slide_directions_pick_the_right_axis() {
    let up = clip_with(Transition::slide_in(Direction::Up, 80, 1.0, Easing::Linear));
    assert_eq!(up.effective_transform(0.0).y, -80);
    assert_eq!(up.effective_transform(0.0).x, 0, "x untouched for vertical slide");

    let right = clip_with(Transition::slide_in(Direction::Right, 80, 1.0, Easing::Linear));
    assert_eq!(right.effective_transform(0.0).x, 80, "positive offset from the right");
}

#[test]
fn zoom_in_grows_scale_from_zero() {
    let clip = clip_with(Transition::zoom_in(1.0, Easing::Linear));
    assert!(clip.effective_transform(0.0).scale.abs() < 1e-6, "starts collapsed");
    assert!((clip.effective_transform(0.5).scale - 0.5).abs() < 0.01);
    assert!((clip.effective_transform(1.0).scale - 1.0).abs() < 1e-6);
}

#[test]
fn fade_out_plus_slide_transforms_smoothly_without_teleport() {
    // Combine a slide (x: 60→0 over 1s) with a fade-out (opacity 1→0 over last 1s
    // of the 4s clip). Sample transform.x densely and assert no jumps.
    let anim = ClipAnimation {
        x: Some(Curve::keyed(vec![Keyframe::new(0.0, 60.0), Keyframe::new(1.0, 0.0)])),
        opacity: Some(Curve::keyed(vec![Keyframe::new(3.0, 1.0), Keyframe::new(4.0, 0.0)])),
        ..ClipAnimation::default()
    };
    let clip = clip_with(anim);

    let mut prev = clip.effective_transform(0.0).x;
    let steps = 4000;
    for i in 1..=steps {
        let t = i as f64 / steps as f64 * 4.0;
        let x = clip.effective_transform(t).x;
        assert!((x - prev).abs() <= 1, "no teleport: dx={} at t={t}", x - prev);
        assert!(x <= prev, "x is monotonically decreasing toward rest");
        prev = x;
    }
    // Opacity is continuous across the fade boundary too.
    let mut po = clip.effective_transform(0.0).opacity;
    for i in 1..=steps {
        let t = i as f64 / steps as f64 * 4.0;
        let o = clip.effective_transform(t).opacity;
        assert!((o - po).abs() < 0.01, "opacity continuous at t={t}");
        po = o;
    }
}

#[test]
fn transitions_are_plain_clip_animations() {
    // A transition is data (ClipAnimation), so it serialises with the clip.
    use ferrox_core::{Project, Track};
    let clip = clip_with(Transition::slide_in(Direction::Down, 50, 0.5, Easing::EaseOut));
    let project = Project::new(16, 16, 30.0).with_track(Track::new().with_clip(clip));
    assert_eq!(Project::from_json(&project.to_json().unwrap()).unwrap(), project);
}
