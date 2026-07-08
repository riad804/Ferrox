//! Phase 2 asset manager: content-addressed dedup, reference counting with
//! dependency cascade, lazy cached decoding (image/audio/LUT), metadata,
//! thumbnail + waveform generation, and background generation on the task pool.

use std::io::Cursor;
use std::sync::Arc;

use ferrox_core::asset::{generate, AssetKind, AssetManager, AssetSource};
use ferrox_core::Error;

// ── fixtures ────────────────────────────────────────────────────────────────

fn png_bytes() -> Vec<u8> {
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
    let data = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255]; // 2×2 RGBA
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf).write_image(&data, 2, 2, ExtendedColorType::Rgba8).unwrap();
    buf
}

fn wav_bytes() -> Vec<u8> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut cursor = Cursor::new(Vec::new());
    let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
    for i in 0..100 {
        w.write_sample(((i % 50) as i16) * 100).unwrap();
    }
    w.finalize().unwrap();
    cursor.into_inner()
}

fn cube_bytes() -> Vec<u8> {
    b"LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n".to_vec()
}

fn bytes_source(b: Vec<u8>) -> AssetSource {
    AssetSource::Bytes(Arc::new(b))
}

// ── import / dedup / metadata ───────────────────────────────────────────────

#[test]
fn import_deduplicates_identical_sources() {
    let m = AssetManager::new();
    let a = m.import(bytes_source(png_bytes()), AssetKind::Image);
    let b = m.import(bytes_source(png_bytes()), AssetKind::Image);
    assert_eq!(a, b, "identical bytes → identical id");
    assert_eq!(m.count(), 1);
    assert_eq!(m.refcount(a), Some(2), "second import bumps refcount");
}

#[test]
fn different_sources_get_distinct_ids() {
    let m = AssetManager::new();
    let a = m.import(bytes_source(png_bytes()), AssetKind::Image);
    let b = m.import(bytes_source(cube_bytes()), AssetKind::Lut);
    assert_ne!(a, b);
    assert_eq!(m.count(), 2);
}

#[test]
fn metadata_reflects_kind_and_size() {
    let m = AssetManager::new();
    let bytes = png_bytes();
    let n = bytes.len();
    let id = m.import(bytes_source(bytes), AssetKind::Image);
    let meta = m.metadata(id).unwrap();
    assert_eq!(meta.kind, AssetKind::Image);
    assert_eq!(meta.byte_size, n);
}

#[test]
fn kind_inferred_from_extension() {
    assert_eq!(AssetKind::from_extension("PNG"), Some(AssetKind::Image));
    assert_eq!(AssetKind::from_extension("wav"), Some(AssetKind::Audio));
    assert_eq!(AssetKind::from_extension("cube"), Some(AssetKind::Lut));
    assert_eq!(AssetKind::from_extension("srt"), Some(AssetKind::Subtitle));
    assert_eq!(AssetKind::from_extension("xyz"), None);
}

// ── reference counting ──────────────────────────────────────────────────────

#[test]
fn retain_release_removes_at_zero() {
    let m = AssetManager::new();
    let id = m.import(bytes_source(png_bytes()), AssetKind::Image);
    m.retain(id).unwrap(); // rc 2
    assert!(!m.release(id), "still referenced");
    assert!(m.contains(id));
    assert!(m.release(id), "last reference dropped");
    assert!(!m.contains(id));
}

#[test]
fn retain_unknown_errors() {
    let m = AssetManager::new();
    assert!(matches!(m.retain(ferrox_core::AssetId(999)), Err(Error::NotFound(_))));
}

// ── lazy, cached loading ────────────────────────────────────────────────────

#[test]
fn import_is_lazy_load_caches() {
    let m = AssetManager::new();
    let id = m.import(bytes_source(png_bytes()), AssetKind::Image);
    assert_eq!(m.cache().images.len(), 0, "nothing decoded on import");

    let a = m.load_image(id).unwrap();
    assert_eq!((a.width, a.height), (2, 2));
    let b = m.load_image(id).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "second load is a cache hit");
    assert_eq!(m.cache().images.len(), 1);
}

#[test]
fn loads_audio_and_lut() {
    let m = AssetManager::new();
    let audio_id = m.import(bytes_source(wav_bytes()), AssetKind::Audio);
    let a = m.load_audio(audio_id).unwrap();
    assert_eq!(a.sample_rate, 8000);
    assert_eq!(a.frame_count(), 100);

    let lut_id = m.import(bytes_source(cube_bytes()), AssetKind::Lut);
    let lut = m.load_lut(lut_id).unwrap();
    assert_eq!(lut.size(), 2);
}

#[test]
fn loading_unimported_asset_errors() {
    let m = AssetManager::new();
    assert!(matches!(m.load_image(ferrox_core::AssetId(1)), Err(Error::NotFound(_))));
}

// ── dependency graph ────────────────────────────────────────────────────────

#[test]
fn dependency_release_cascades() {
    let m = AssetManager::new();
    let parent = m.import(bytes_source(png_bytes()), AssetKind::Image);
    let child = m.import(bytes_source(cube_bytes()), AssetKind::Lut);
    m.add_dependency(parent, child).unwrap(); // child rc 2, edge recorded
    assert_eq!(m.dependencies(parent), vec![child]);
    assert_eq!(m.dependents(child), vec![parent]);

    m.release(child); // child now only referenced through the parent (rc 1)
    assert!(m.contains(child));
    assert!(m.release(parent), "parent removed");
    assert!(!m.contains(child), "cascade released the child");
}

#[test]
fn transitive_dependencies_traverse() {
    let m = AssetManager::new();
    let a = m.import(bytes_source(png_bytes()), AssetKind::Image);
    let b = m.import(bytes_source(cube_bytes()), AssetKind::Lut);
    let c = m.import(bytes_source(wav_bytes()), AssetKind::Audio);
    m.add_dependency(a, b).unwrap();
    m.add_dependency(b, c).unwrap();
    let mut t = m.transitive_dependencies(a);
    t.sort();
    let mut expect = vec![b, c];
    expect.sort();
    assert_eq!(t, expect);
}

// ── generation ──────────────────────────────────────────────────────────────

#[test]
fn thumbnail_fits_within_bounds() {
    let m = AssetManager::new();
    let id = m.import(bytes_source(png_bytes()), AssetKind::Image);
    let thumb = generate::thumbnail(&m, id, 1, 1).unwrap();
    assert!(thumb.width <= 1 && thumb.height <= 1);
}

#[test]
fn waveform_has_requested_buckets() {
    let m = AssetManager::new();
    let id = m.import(bytes_source(wav_bytes()), AssetKind::Audio);
    let wf = generate::waveform(&m, id, 8).unwrap();
    assert_eq!(wf.len(), 8);
}

#[test]
fn background_thumbnail_runs_on_task_pool() {
    use ferrox_core::task::{TaskManager, TaskOutcome};
    use std::sync::mpsc;

    let m = Arc::new(AssetManager::new());
    let id = m.import(bytes_source(png_bytes()), AssetKind::Image);
    let tasks = TaskManager::new(2);
    let (tx, rx) = mpsc::channel();
    generate::background::thumbnail(Arc::clone(&m), &tasks, id, 1, 1, move |o| tx.send(o).unwrap());
    match rx.recv().unwrap() {
        TaskOutcome::Completed(frame) => assert!(frame.width <= 1 && frame.height <= 1),
        other => panic!("expected thumbnail, got {other:?}"),
    }
}
