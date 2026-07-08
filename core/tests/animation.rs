//! Phase 10 animation system: expressive easings (spring/elastic/bounce/back),
//! their use in curves, serde round-trips, and nestable animation groups.

use ferrox_core::{AnimationGroup, Curve, Easing, Keyframe};

fn near(a: f32, b: f32, tol: f32) {
    assert!((a - b).abs() <= tol, "expected ~{b}, got {a}");
}

// ── expressive easings ──────────────────────────────────────────────────────

#[test]
fn all_easings_hit_their_endpoints() {
    let easings = [
        Easing::Spring { stiffness: 12.0, damping: 5.0 },
        Easing::Elastic,
        Easing::Bounce,
        Easing::Back { overshoot: 1.70158 },
    ];
    for e in easings {
        near(e.ease(0.0), 0.0, 1e-4);
        near(e.ease(1.0), 1.0, 1e-4);
    }
}

#[test]
fn back_and_elastic_overshoot() {
    // Back ease-out overshoots past 1 near the end.
    let back = Easing::Back { overshoot: 2.0 };
    assert!((0..100).map(|i| back.ease(i as f32 / 100.0)).any(|v| v > 1.0), "back overshoots");
    // Elastic oscillates below/above on its way to 1.
    let el = Easing::Elastic;
    let vals: Vec<f32> = (0..100).map(|i| el.ease(i as f32 / 100.0)).collect();
    assert!(vals.iter().any(|&v| v > 1.0) || vals.iter().any(|&v| v < 0.0), "elastic oscillates");
}

#[test]
fn bounce_stays_within_unit_and_settles() {
    let b = Easing::Bounce;
    for i in 0..=100 {
        let v = b.ease(i as f32 / 100.0);
        assert!((0.0..=1.0001).contains(&v), "bounce in range, got {v}");
    }
    near(b.ease(1.0), 1.0, 1e-4);
}

#[test]
fn spring_easing_drives_a_curve() {
    let c = Curve::keyed(vec![
        Keyframe::new(0.0, 0.0).with_ease(Easing::Spring { stiffness: 10.0, damping: 4.0 }),
        Keyframe::new(1.0, 100.0),
    ]);
    near(c.sample(0.0), 0.0, 0.5);
    near(c.sample(1.0), 100.0, 0.5);
    // Mid-curve the spring produces a value between (not linear).
    let mid = c.sample(0.5);
    assert!(mid.is_finite());
}

#[test]
fn new_easings_round_trip_through_json() {
    for e in [
        Easing::Spring { stiffness: 8.5, damping: 3.0 },
        Easing::Elastic,
        Easing::Bounce,
        Easing::Back { overshoot: 1.5 },
    ] {
        let json = serde_json::to_string(&e).unwrap();
        let back: Easing = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }
    // Backward compatibility: old JSON still parses.
    assert_eq!(serde_json::from_str::<Easing>("\"linear\"").unwrap(), Easing::Linear);
    assert_eq!(serde_json::from_str::<Easing>("\"ease_in_out\"").unwrap(), Easing::EaseInOut);
}

// ── animation groups ────────────────────────────────────────────────────────

#[test]
fn group_samples_named_tracks() {
    let g = AnimationGroup::new()
        .track("opacity", Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 1.0)]))
        .track("scale", Curve::Const(2.0));
    near(g.sample("opacity", 0.5).unwrap(), 0.5, 1e-4);
    assert_eq!(g.sample("scale", 99.0), Some(2.0));
    assert_eq!(g.sample("missing", 0.0), None);
}

#[test]
fn group_offset_shifts_time() {
    // The curve rises 0→1 over [0,1]; with a +2s group offset it rises over [2,3].
    let g = AnimationGroup::with_offset(2.0)
        .track("x", Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 10.0)]));
    near(g.sample("x", 2.0).unwrap(), 0.0, 1e-4);
    near(g.sample("x", 2.5).unwrap(), 5.0, 1e-4);
    near(g.sample("x", 3.0).unwrap(), 10.0, 1e-4);
}

#[test]
fn nested_group_offsets_compose() {
    // Parent offset 1 + child offset 2 = curve effectively starts at t=3.
    let child = AnimationGroup::with_offset(2.0)
        .track("y", Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 8.0)]));
    let parent = AnimationGroup::with_offset(1.0).child(child);
    near(parent.sample("y", 3.0).unwrap(), 0.0, 1e-4);
    near(parent.sample("y", 4.0).unwrap(), 8.0, 1e-4);
}

#[test]
fn group_converts_to_clip_animation() {
    let g = AnimationGroup::with_offset(1.0)
        .track("scale", Curve::keyed(vec![Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 1.0)]))
        .track("opacity", Curve::Const(1.0));
    let anim = g.to_clip_animation();
    assert!(anim.scale.is_some());
    assert!(anim.opacity.is_some());
    assert!(anim.x.is_none(), "unmapped fields stay None");
    // Offset baked in: the scale curve now spans [1,2] in clip-local time.
    if let Some(Curve::Keyed(keys)) = &anim.scale {
        assert!((keys[0].t - 1.0).abs() < 1e-9 && (keys[1].t - 2.0).abs() < 1e-9);
    } else {
        panic!("expected keyed scale curve");
    }
}
