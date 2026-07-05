//! Tests for the color pipeline: ASC-CDL (lift/gamma/gain + saturation), 3D LUT
//! `.cube` parsing + trilinear interpolation, and the compositor output-LUT hook.

use ferrox_core::{
    compose_frame_graded, AscCdl, Clip, ClipSource, ColorGrade, Frame, Lut3D, PixelFormat, Project,
    Track, Transform,
};

fn rgba(w: u32, h: u32, r: u8, g: u8, b: u8) -> Frame {
    Frame::new(w, h, PixelFormat::Rgba8, [r, g, b, 255].repeat((w * h) as usize))
}

fn near(a: u8, b: u8) {
    assert!((a as i32 - b as i32).abs() <= 1, "got {a}, expected {b}");
}

#[test]
fn cdl_identity_is_a_no_op() {
    let cdl = AscCdl::default();
    assert!(cdl.is_identity());
    let mut f = rgba(2, 2, 40, 130, 200);
    cdl.apply_frame(&mut f).unwrap();
    assert_eq!(&f.data[0..4], &[40, 130, 200, 255]);
}

#[test]
fn cdl_slope_offset_power_apply_per_channel() {
    // slope = gain: 0.25 * 2 = 0.5 → 128.
    let mut gain = rgba(1, 1, 64, 64, 64);
    AscCdl { slope: [2.0, 2.0, 2.0], ..Default::default() }.apply_frame(&mut gain).unwrap();
    near(gain.data[0], 128);

    // offset = lift: 0 + 0.5 = 0.5 → 128.
    let mut lift = rgba(1, 1, 0, 0, 0);
    AscCdl { offset: [0.5, 0.5, 0.5], ..Default::default() }.apply_frame(&mut lift).unwrap();
    near(lift.data[0], 128);

    // power = gamma: 0.5 ^ 2 = 0.25 → 64.
    let mut gamma = rgba(1, 1, 128, 128, 128);
    AscCdl { power: [2.0, 2.0, 2.0], ..Default::default() }.apply_frame(&mut gamma).unwrap();
    near(gamma.data[0], 64);
}

#[test]
fn cdl_zero_saturation_desaturates_to_luma() {
    // Pure red → luma 0.2126 → ~54 on all channels.
    let mut f = rgba(1, 1, 255, 0, 0);
    AscCdl { saturation: 0.0, ..Default::default() }.apply_frame(&mut f).unwrap();
    near(f.data[0], 54);
    near(f.data[1], 54);
    near(f.data[2], 54);
}

#[test]
fn cube_identity_parses_and_is_neutral() {
    let cube = "\
# identity 2^3
LUT_3D_SIZE 2
0 0 0
1 0 0
0 1 0
1 1 0
0 0 1
1 0 1
0 1 1
1 1 1
";
    let lut = Lut3D::parse_cube(cube).unwrap();
    assert_eq!(lut.size(), 2);
    // Trilinear on the identity cube reproduces the input.
    let out = lut.apply_rgb([0.5, 0.25, 0.75]);
    near((out[0] * 255.0).round() as u8, 128);
    near((out[1] * 255.0).round() as u8, 64);
    near((out[2] * 255.0).round() as u8, 191);
}

#[test]
fn programmatic_identity_lut_is_neutral_at_higher_resolution() {
    let lut = Lut3D::identity(17).unwrap();
    let mut f = rgba(1, 1, 30, 150, 220);
    lut.apply_frame(&mut f).unwrap();
    near(f.data[0], 30);
    near(f.data[1], 150);
    near(f.data[2], 220);
}

#[test]
fn cube_file_round_trips_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id.cube");
    std::fs::write(&path, "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
    let lut = Lut3D::from_cube_file(&path).unwrap();
    assert_eq!(lut.size(), 2);
}

#[test]
fn malformed_cube_is_rejected() {
    assert!(Lut3D::parse_cube("no size here\n0 0 0\n").is_err());
    assert!(Lut3D::new(2, vec![[0.0; 3]; 3]).is_err(), "wrong sample count");
}

#[test]
fn per_clip_color_grade_applies_before_compositing() {
    // A grey clip graded with gain 2 becomes brighter on the canvas.
    let clip = Clip::new(ClipSource::Solid { width: 1, height: 1, r: 64, g: 64, b: 64, a: 255 }, 0.0, 1.0, Transform::default())
        .with_color(ColorGrade::from_cdl(AscCdl { slope: [2.0, 2.0, 2.0], ..Default::default() }));
    let project = Project::new(1, 1, 30.0).with_track(Track::new().with_clip(clip));
    let f = compose_frame_graded(&project, 0.0, None).unwrap();
    near(f.data[0], 128);
}

#[test]
fn output_lut_applies_after_blending() {
    // An identity output LUT leaves the composite unchanged.
    let clip = Clip::new(ClipSource::Solid { width: 1, height: 1, r: 90, g: 120, b: 200, a: 255 }, 0.0, 1.0, Transform::default());
    let project = Project::new(1, 1, 30.0).with_track(Track::new().with_clip(clip));
    let lut = Lut3D::identity(9).unwrap();
    let f = compose_frame_graded(&project, 0.0, Some(&lut)).unwrap();
    near(f.data[0], 90);
    near(f.data[1], 120);
    near(f.data[2], 200);
}

#[test]
fn color_grade_omitted_from_json_when_identity() {
    let project = Project::new(2, 2, 30.0)
        .with_track(Track::new().with_clip(Clip::new(ClipSource::Solid { width: 1, height: 1, r: 1, g: 1, b: 1, a: 255 }, 0.0, 1.0, Transform::default())));
    assert!(!project.to_json().unwrap().contains("color"), "identity grade omitted");
}
