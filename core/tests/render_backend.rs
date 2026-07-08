//! Phase 3 render backend: kernel correctness on the CPU backend, backend
//! replaceability (via `dyn RenderBackend`), and capability reporting.

use ferrox_core::render::{default_backend, Capabilities, CpuBackend, RenderBackend};
use ferrox_core::{AscCdl, BlendMode, ColorGrade, Frame, Keyer, Mask, PixelFormat};

fn rgba(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> Frame {
    Frame::new(w, h, PixelFormat::Rgba8, [r, g, b, a].repeat((w * h) as usize))
}

#[test]
fn cpu_backend_reports_capabilities() {
    let b = CpuBackend;
    assert_eq!(b.name(), "cpu");
    assert_eq!(b.capabilities(), Capabilities { gpu_accelerated: false });
}

#[test]
fn color_grade_kernel_matches_baseline() {
    let b = CpuBackend;
    let out = b
        .color_grade(rgba(1, 1, 64, 64, 64, 255), &ColorGrade::from_cdl(AscCdl { slope: [2.0, 2.0, 2.0], ..Default::default() }))
        .unwrap();
    assert_eq!(out.data[0], 128);
}

#[test]
fn resize_kernel_changes_dimensions() {
    let b = CpuBackend;
    let out = b.resize(rgba(4, 4, 10, 20, 30, 255), 2, 2).unwrap();
    assert_eq!((out.width, out.height), (2, 2));
}

#[test]
fn chroma_key_kernel_removes_key_colour() {
    let b = CpuBackend;
    let keyer = Keyer { key: [0, 255, 0], tolerance: 0.2, softness: 0.1, despill: false };
    let out = b.chroma_key(rgba(1, 1, 0, 255, 0, 255), &keyer).unwrap();
    assert_eq!(out.data[3], 0, "green keyed to transparent");
}

#[test]
fn mask_kernel_multiplies_alpha() {
    let b = CpuBackend;
    // Right-half rectangle mask → the single left-edge pixel is outside it.
    let mask = Mask::Rectangle { x: 0.5, y: 0.0, w: 0.5, h: 1.0, feather: 0.0, invert: false };
    let out = b.apply_mask(rgba(4, 1, 255, 255, 255, 255), &mask).unwrap();
    assert_eq!(out.data[3], 0);
}

#[test]
fn composite_kernel_blends_over() {
    let b = CpuBackend;
    let mut base = rgba(1, 1, 0, 0, 0, 255);
    let top = rgba(1, 1, 255, 255, 255, 255);
    b.composite(&mut base, &top, 0, 0, 0.5, BlendMode::Normal).unwrap();
    assert!((base.data[0] as i32 - 128).abs() <= 1, "50% white over black ≈ 128");
}

#[test]
fn backend_is_replaceable_behind_a_trait_object() {
    // The whole point of the port: swap backends without touching callers.
    let backend: Box<dyn RenderBackend> = default_backend();
    let out = backend
        .color_grade(rgba(1, 1, 32, 32, 32, 255), &ColorGrade::from_cdl(AscCdl { slope: [2.0, 2.0, 2.0], ..Default::default() }))
        .unwrap();
    assert_eq!(out.data[0], 64);
    // default_backend picks CPU on headless CI (no adapter) or GPU where present.
    assert!(matches!(backend.name(), "cpu" | "gpu"));
}
