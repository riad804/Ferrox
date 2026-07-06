//! Benchmark: plugin registry lookup throughput — the baseline for the plugin
//! resolution hot path.

use criterion::{criterion_group, criterion_main, Criterion};
use ferrox_core::plugin::{register_builtins, CapabilitySet, PluginKind, PluginManager, PLUGIN_API_VERSION};

fn bench_lookup(c: &mut Criterion) {
    let mgr = PluginManager::new(PLUGIN_API_VERSION, CapabilitySet::new());
    register_builtins(&mgr).unwrap();

    c.bench_function("plugin_get_by_id", |b| {
        b.iter(|| mgr.get("ferrox.builtin.color_grade").unwrap())
    });
    c.bench_function("plugin_ids_by_kind", |b| {
        b.iter(|| mgr.ids_by_kind(PluginKind::VideoEffect))
    });
}

criterion_group!(benches, bench_lookup);
criterion_main!(benches);
