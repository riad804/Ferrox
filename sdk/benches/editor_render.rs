//! Baseline benchmark for the Editor render path — the number to beat as the
//! Render Graph (Phase 4) and GPU backend (Phase 3) land.

use criterion::{criterion_group, criterion_main, Criterion};
use ferrox_sdk::{Clip, ClipSource, Editor, Transform};

fn bench_render_frame(c: &mut Criterion) {
    // A 320x180 project with two composited layers + a per-clip transform.
    let editor = Editor::new(320, 180, 30.0);
    let t0 = editor.add_track().unwrap();
    let t1 = editor.add_track().unwrap();
    editor
        .add_clip(t0, Clip::new(ClipSource::Solid { width: 320, height: 180, r: 20, g: 40, b: 80, a: 255 }, 0.0, 5.0, Transform::default()))
        .unwrap();
    editor
        .add_clip(t1, Clip::new(ClipSource::Solid { width: 160, height: 90, r: 200, g: 120, b: 40, a: 200 }, 0.0, 5.0, Transform::at(40, 20)))
        .unwrap();

    c.bench_function("render_frame_320x180_2layers", |b| {
        b.iter(|| editor.render_frame(1.0, 0, 0).unwrap())
    });
}

criterion_group!(benches, bench_render_frame);
criterion_main!(benches);
