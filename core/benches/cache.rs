//! Benchmark: LRU cache get/put throughput (with eviction under a byte budget).

use criterion::{criterion_group, criterion_main, Criterion};
use ferrox_core::cache::LruCache;

fn bench_cache(c: &mut Criterion) {
    c.bench_function("lru_get_hit", |b| {
        let mut cache: LruCache<u32, Vec<u8>> = LruCache::new(None, None);
        cache.put(1, vec![0u8; 64]);
        b.iter(|| cache.get(&1).unwrap());
    });
    c.bench_function("lru_put_with_eviction", |b| {
        let mut cache: LruCache<u32, Vec<u8>> = LruCache::with_max_bytes(64 * 1024);
        let mut k = 0u32;
        b.iter(|| {
            k = k.wrapping_add(1);
            cache.put(k, vec![0u8; 256]);
        });
    });
}

criterion_group!(benches, bench_cache);
criterion_main!(benches);
