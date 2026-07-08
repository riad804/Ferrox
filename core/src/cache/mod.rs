//! # Resource cache (Phase 5)
//!
//! LRU caches with count + byte-size budgets and automatic eviction — the
//! memory-management substrate for large-project, low-memory-device editing.
//!
//! - [`LruCache`] — the generic engine (`&mut self`, single-threaded).
//! - [`SharedCache`] — a cloneable, thread-safe, WASM-safe handle.
//! - [`ResourceCache`] — the facade grouping typed caches (frames/images/LUTs/audio).
//!
//! Values are `Arc`-shared (cheap `get`), and each value's cost is its
//! [`Weight`] in bytes.
//!
//! ```
//! use ferrox_core::cache::SharedCache;
//! let cache = SharedCache::<String, Vec<u8>>::with_max_bytes(1024);
//! let v = cache.get_or_insert_with("k".into(), || vec![0u8; 256]);
//! assert_eq!(v.len(), 256);
//! assert_eq!(cache.bytes(), 256);
//! ```

pub mod lru;
pub mod resource;
pub mod shared;

pub use lru::{LruCache, Weight};
pub use resource::{CacheBudgets, ResourceCache};
pub use shared::SharedCache;
