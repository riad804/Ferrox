//! A generic **LRU cache** with count and byte-size budgets and automatic
//! eviction — the reusable engine behind every resource cache (frames, images,
//! LUTs, audio, and later textures/shaders/fonts).
//!
//! Values are stored behind `Arc` so `get` is a cheap clone. Each value's memory
//! cost comes from the [`Weight`] trait, letting the cache enforce a byte budget
//! (essential on low-memory devices). Not internally synchronised — wrap in
//! [`super::SharedCache`] for concurrent use.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

/// The approximate memory cost (bytes) of a cached value.
pub trait Weight {
    fn weight(&self) -> usize;
}

impl Weight for Vec<u8> {
    fn weight(&self) -> usize {
        self.len()
    }
}
impl Weight for String {
    fn weight(&self) -> usize {
        self.len()
    }
}

struct Entry<V> {
    value: Arc<V>,
    weight: usize,
    last_used: u64,
}

/// A least-recently-used cache bounded by entry count and/or total bytes.
pub struct LruCache<K, V> {
    map: HashMap<K, Entry<V>>,
    clock: u64,
    bytes: usize,
    max_entries: Option<usize>,
    max_bytes: Option<usize>,
}

impl<K: Eq + Hash + Clone, V: Weight> LruCache<K, V> {
    /// A cache bounded by `max_entries` and/or `max_bytes` (`None` = unbounded).
    pub fn new(max_entries: Option<usize>, max_bytes: Option<usize>) -> Self {
        Self { map: HashMap::new(), clock: 0, bytes: 0, max_entries, max_bytes }
    }

    /// A cache bounded only by a byte budget.
    pub fn with_max_bytes(max_bytes: usize) -> Self {
        Self::new(None, Some(max_bytes))
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Fetch a value, marking it most-recently-used.
    pub fn get(&mut self, key: &K) -> Option<Arc<V>> {
        let t = self.tick();
        let entry = self.map.get_mut(key)?;
        entry.last_used = t;
        Some(Arc::clone(&entry.value))
    }

    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Insert a value (replacing any existing one) and evict to fit the budget.
    pub fn put(&mut self, key: K, value: V) -> Arc<V> {
        self.put_arc(key, Arc::new(value))
    }

    /// Insert an already-`Arc`-wrapped value.
    pub fn put_arc(&mut self, key: K, value: Arc<V>) -> Arc<V> {
        let weight = value.weight();
        let t = self.tick();
        if let Some(old) = self.map.insert(key, Entry { value: Arc::clone(&value), weight, last_used: t }) {
            self.bytes -= old.weight;
        }
        self.bytes += weight;
        self.evict_to_fit();
        value
    }

    /// Return the cached value, or compute + insert it on a miss.
    pub fn get_or_insert_with(&mut self, key: K, f: impl FnOnce() -> V) -> Arc<V> {
        if let Some(v) = self.get(&key) {
            return v;
        }
        self.put(key, f())
    }

    /// Remove and return a value.
    pub fn remove(&mut self, key: &K) -> Option<Arc<V>> {
        let entry = self.map.remove(key)?;
        self.bytes -= entry.weight;
        Some(entry.value)
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Total bytes currently held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    fn over_budget(&self) -> bool {
        self.max_entries.is_some_and(|c| self.map.len() > c)
            || self.max_bytes.is_some_and(|b| self.bytes > b)
    }

    /// Evict least-recently-used entries until within budget. A single value
    /// larger than the byte budget is kept (we never evict the last entry).
    fn evict_to_fit(&mut self) {
        while self.over_budget() && self.map.len() > 1 {
            if let Some(key) = self.lru_key() {
                self.remove(&key);
            } else {
                break;
            }
        }
    }

    fn lru_key(&self) -> Option<K> {
        self.map
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| k.clone())
    }
}
