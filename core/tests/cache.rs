//! Phase 5 resource cache: LRU eviction by count and by bytes, recency updates,
//! get-or-insert, thread-safety, and the ResourceCache facade with real engine
//! resources.

use std::sync::Arc;
use std::thread;

use ferrox_core::cache::{LruCache, ResourceCache, SharedCache, Weight};
use ferrox_core::{AudioFrame, Frame, Lut3D, PixelFormat};

// ── generic LRU ─────────────────────────────────────────────────────────────

#[test]
fn evicts_least_recently_used_by_count() {
    let mut c: LruCache<u32, Vec<u8>> = LruCache::new(Some(2), None);
    c.put(1, vec![0; 10]);
    c.put(2, vec![0; 10]);
    let _ = c.get(&1); // 1 is now most-recently-used
    c.put(3, vec![0; 10]); // over count → evict LRU (2)
    assert!(c.contains(&1));
    assert!(!c.contains(&2), "least-recently-used evicted");
    assert!(c.contains(&3));
    assert_eq!(c.len(), 2);
}

#[test]
fn evicts_to_fit_byte_budget() {
    let mut c: LruCache<u32, Vec<u8>> = LruCache::with_max_bytes(100);
    c.put(1, vec![0; 60]);
    c.put(2, vec![0; 60]); // 120 > 100 → evict 1
    assert!(!c.contains(&1));
    assert!(c.contains(&2));
    assert_eq!(c.bytes(), 60);
}

#[test]
fn keeps_single_oversized_value() {
    let mut c: LruCache<u32, Vec<u8>> = LruCache::with_max_bytes(50);
    c.put(1, vec![0; 200]); // bigger than the whole budget
    assert!(c.contains(&1), "never evict the only entry");
    assert_eq!(c.bytes(), 200);
}

#[test]
fn get_updates_recency() {
    let mut c: LruCache<u32, Vec<u8>> = LruCache::new(Some(2), None);
    c.put(1, vec![0; 1]);
    c.put(2, vec![0; 1]);
    let _ = c.get(&1);
    let _ = c.get(&1);
    c.put(3, vec![0; 1]); // 2 is LRU → evicted, 1 survives
    assert!(c.contains(&1));
    assert!(!c.contains(&2));
}

#[test]
fn get_or_insert_computes_once() {
    let mut c: LruCache<u32, Vec<u8>> = LruCache::new(None, None);
    let mut calls = 0;
    let a = c.get_or_insert_with(1, || {
        calls += 1;
        vec![7; 4]
    });
    let b = c.get_or_insert_with(1, || {
        calls += 1;
        vec![9; 4]
    });
    assert_eq!(calls, 1, "second call is a hit");
    assert_eq!(&*a, &[7, 7, 7, 7]);
    assert_eq!(a, b);
}

#[test]
fn remove_and_clear_track_bytes() {
    let mut c: LruCache<u32, Vec<u8>> = LruCache::new(None, None);
    c.put(1, vec![0; 10]);
    c.put(2, vec![0; 20]);
    assert_eq!(c.bytes(), 30);
    c.remove(&1);
    assert_eq!(c.bytes(), 20);
    c.clear();
    assert_eq!(c.bytes(), 0);
    assert!(c.is_empty());
}

#[test]
fn replacing_a_key_adjusts_bytes() {
    let mut c: LruCache<u32, Vec<u8>> = LruCache::new(None, None);
    c.put(1, vec![0; 10]);
    c.put(1, vec![0; 3]); // replace
    assert_eq!(c.len(), 1);
    assert_eq!(c.bytes(), 3);
}

// ── SharedCache (thread-safe) ───────────────────────────────────────────────

#[test]
fn shared_cache_is_concurrent() {
    let cache: SharedCache<u32, Vec<u8>> = SharedCache::new(None, Some(1_000_000));
    let mut handles = Vec::new();
    for t in 0..8u32 {
        let cache = cache.clone();
        handles.push(thread::spawn(move || {
            for i in 0..100u32 {
                let key = t * 1000 + i;
                let v = cache.get_or_insert_with(key, || vec![0u8; 8]);
                assert_eq!(v.len(), 8);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert!(!cache.is_empty());
    assert!(cache.bytes() <= 1_000_000, "byte budget respected under contention");
}

#[test]
fn shared_cache_clones_share_state() {
    let a: SharedCache<u32, Vec<u8>> = SharedCache::new(None, None);
    let b = a.clone();
    a.put(1, vec![0; 4]);
    assert!(b.contains(&1), "clone sees the same cache");
}

// ── ResourceCache facade ────────────────────────────────────────────────────

fn frame(bytes: usize) -> Frame {
    Frame::new(1, (bytes / 4) as u32, PixelFormat::Rgba8, vec![0u8; bytes])
}

#[test]
fn weight_impls_match_resource_bytes() {
    assert_eq!(frame(40).weight(), 40);
    assert_eq!(AudioFrame::new(48_000, 2, vec![0.0; 100]).weight(), 400);
    // identity LUT of size 8 → 8³ × 3 × 4 bytes.
    assert_eq!(Lut3D::identity(8).unwrap().weight(), 8 * 8 * 8 * 3 * 4);
}

#[test]
fn resource_cache_facade_aggregates_and_clears() {
    let rc = ResourceCache::default();
    rc.frames.put("f1".into(), frame(1000));
    rc.images.put("img".into(), frame(500));
    rc.luts.put("lut".into(), Lut3D::identity(2).unwrap());
    rc.audio.put("a".into(), AudioFrame::new(48_000, 1, vec![0.0; 50]));

    assert_eq!(rc.total_entries(), 4);
    assert_eq!(rc.total_bytes(), 1000 + 500 + (2 * 2 * 2 * 3 * 4) + 200);

    // A cached frame is retrievable and Arc-shared.
    let got = rc.frames.get(&"f1".to_string()).unwrap();
    assert_eq!(got.data.len(), 1000);

    rc.clear_all();
    assert_eq!(rc.total_entries(), 0);
    assert_eq!(rc.total_bytes(), 0);
}

#[test]
fn resource_cache_frame_budget_evicts() {
    use ferrox_core::cache::CacheBudgets;
    let rc = ResourceCache::with_budgets(CacheBudgets { frames: 1000, images: 1000, luts: 1000, audio: 1000 });
    rc.frames.put("a".into(), frame(600));
    rc.frames.put("b".into(), frame(600)); // 1200 > 1000 → evict "a"
    assert!(!rc.frames.contains(&"a".to_string()));
    assert!(rc.frames.contains(&"b".to_string()));
}
