//! [`SharedCache`] — a cloneable, thread-safe handle around an [`LruCache`].
//!
//! Uses `std::sync::Mutex` (poison-recovered) so it compiles unchanged to
//! `wasm32`. A `Mutex` (not `RwLock`) because LRU `get` mutates recency.

use std::hash::Hash;
use std::sync::{Arc, Mutex};

use super::lru::{LruCache, Weight};

/// A thread-safe, cloneable LRU cache. Clones share the same underlying cache.
pub struct SharedCache<K, V> {
    inner: Arc<Mutex<LruCache<K, V>>>,
}

impl<K, V> Clone for SharedCache<K, V> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl<K: Eq + Hash + Clone, V: Weight> SharedCache<K, V> {
    /// A cache bounded by `max_entries` and/or `max_bytes`.
    pub fn new(max_entries: Option<usize>, max_bytes: Option<usize>) -> Self {
        Self { inner: Arc::new(Mutex::new(LruCache::new(max_entries, max_bytes))) }
    }

    /// A cache bounded only by a byte budget.
    pub fn with_max_bytes(max_bytes: usize) -> Self {
        Self::new(None, Some(max_bytes))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LruCache<K, V>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        self.lock().get(key)
    }

    pub fn put(&self, key: K, value: V) -> Arc<V> {
        self.lock().put(key, value)
    }

    pub fn get_or_insert_with(&self, key: K, f: impl FnOnce() -> V) -> Arc<V> {
        self.lock().get_or_insert_with(key, f)
    }

    pub fn remove(&self, key: &K) -> Option<Arc<V>> {
        self.lock().remove(key)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.lock().contains(key)
    }

    pub fn clear(&self) {
        self.lock().clear()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Total bytes currently held.
    pub fn bytes(&self) -> usize {
        self.lock().bytes()
    }
}
