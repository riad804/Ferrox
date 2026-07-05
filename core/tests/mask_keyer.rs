//! Tests for vector masks (rectangle/ellipse/polygon, feather, invert) and the
//! chroma keyer (matte + despill).

use ferrox_core::{Frame, Keyer, Mask, PixelFormat};

fn opaque(w: u32, h: u32) -> Frame {
    Frame::new(w, h, PixelFormat::Rgba8, vec![255u8; (w * h * 4) as usize])
}

fn alpha(f: &Frame, x: u32, y: u32) -> u8 {
    f.data[((y * f.width + x) * 4 + 3) as usize]
}

#[test]
fn rectangle_mask_keeps_right_half() {
    let mut f = opaque(4, 4);
    Mask::Rectangle { x: 0.5, y: 0.0, w: 0.5, h: 1.0, feather: 0.0, invert: false }
        .apply_frame(&mut f)
        .unwrap();
    assert_eq!(alpha(&f, 0, 0), 0, "left outside");
    assert_eq!(alpha(&f, 1, 0), 0, "left outside");
    assert_eq!(alpha(&f, 2, 0), 255, "right inside");
    assert_eq!(alpha(&f, 3, 0), 255, "right inside");
}

#[test]
fn rectangle_invert_flips_coverage() {
    let mut f = opaque(4, 4);
    Mask::Rectangle { x: 0.5, y: 0.0, w: 0.5, h: 1.0, feather: 0.0, invert: true }
        .apply_frame(&mut f)
        .unwrap();
    assert_eq!(alpha(&f, 0, 0), 255, "left now kept");
    assert_eq!(alpha(&f, 3, 0), 0, "right now removed");
}

#[test]
fn ellipse_mask_keeps_center_drops_corners() {
    let mut f = opaque(5, 5);
    Mask::Ellipse { cx: 0.5, cy: 0.5, rx: 0.5, ry: 0.5, feather: 0.0, invert: false }
        .apply_frame(&mut f)
        .unwrap();
    assert_eq!(alpha(&f, 2, 2), 255, "center inside");
    assert_eq!(alpha(&f, 0, 0), 0, "corner outside");
    assert_eq!(alpha(&f, 4, 4), 0, "corner outside");
}

#[test]
fn polygon_mask_fills_triangle() {
    // Lower-left triangle (0,0)-(1,0)-(0,1).
    let mut f = opaque(10, 10);
    Mask::Polygon { points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], feather: 0.0, invert: false }
        .apply_frame(&mut f)
        .unwrap();
    assert_eq!(alpha(&f, 1, 1), 255, "inside triangle");
    assert_eq!(alpha(&f, 9, 9), 0, "outside triangle");
}

#[test]
fn feather_produces_soft_edge() {
    let mut f = opaque(40, 1);
    Mask::Rectangle { x: 0.25, y: 0.0, w: 0.5, h: 1.0, feather: 0.4, invert: false }
        .apply_frame(&mut f)
        .unwrap();
    // Somewhere along the row the alpha must be partial (soft edge).
    let has_partial = (0..40).any(|x| {
        let a = alpha(&f, x, 0);
        a > 10 && a < 245
    });
    assert!(has_partial, "feathered edge should yield intermediate alpha");
}

#[test]
fn mask_round_trips_through_json() {
    use ferrox_core::{Clip, ClipSource, Project, Track, Transform};
    let clip = Clip::new(ClipSource::Solid { width: 4, height: 4, r: 1, g: 2, b: 3, a: 255 }, 0.0, 1.0, Transform::default())
        .with_mask(Mask::Ellipse { cx: 0.5, cy: 0.5, rx: 0.4, ry: 0.3, feather: 0.05, invert: false });
    let project = Project::new(8, 8, 30.0).with_track(Track::new().with_clip(clip));
    assert_eq!(Project::from_json(&project.to_json().unwrap()).unwrap(), project);
}

#[test]
fn chroma_key_removes_key_color_keeps_foreground() {
    // Frame: pixel0 pure green (key), pixel1 pure red (foreground).
    let mut f = Frame::new(2, 1, PixelFormat::Rgba8, vec![0, 255, 0, 255, 255, 0, 0, 255]);
    Keyer { key: [0, 255, 0], tolerance: 0.2, softness: 0.1, despill: false }.apply_frame(&mut f).unwrap();
    assert_eq!(alpha(&f, 0, 0), 0, "green keyed out");
    assert_eq!(alpha(&f, 1, 0), 255, "red retained");
}

#[test]
fn despill_reduces_green_on_retained_pixels() {
    // A near-grey pixel that's retained but slightly green-dominant.
    let mut f = Frame::new(1, 1, PixelFormat::Rgba8, vec![200, 210, 200, 255]);
    Keyer { key: [0, 255, 0], tolerance: 0.2, softness: 0.1, despill: true }.apply_frame(&mut f).unwrap();
    assert_eq!(f.data[3], 255, "pixel retained");
    assert_eq!(f.data[1], 200, "green clamped down to average of R,B");
}

#[test]
fn keyer_round_trips_through_json() {
    use ferrox_core::{Clip, ClipSource, Project, Track, Transform};
    let clip = Clip::new(ClipSource::Solid { width: 4, height: 4, r: 0, g: 255, b: 0, a: 255 }, 0.0, 1.0, Transform::default())
        .with_keyer(Keyer::green());
    let project = Project::new(8, 8, 30.0).with_track(Track::new().with_clip(clip));
    assert_eq!(Project::from_json(&project.to_json().unwrap()).unwrap(), project);
}
