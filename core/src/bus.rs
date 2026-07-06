//! `InProcessBus` — the default **infrastructure** implementation of the
//! [`EventSink`] port. Fans events out to weakly-held listeners synchronously,
//! which keeps ordering deterministic (good for tests) and works single-threaded
//! (WASM). Heavy/async consumers should bridge to a channel from `on_event`.
//!
//! Uses `std::sync::RwLock` (poison-recovered) rather than `parking_lot` so the
//! bus compiles unchanged to `wasm32`.

use std::sync::{Arc, RwLock, Weak};

use crate::event::{Event, EventListener, EventSink};

/// A synchronous, thread-safe, in-process event bus.
#[derive(Default)]
pub struct InProcessBus {
    listeners: RwLock<Vec<Weak<dyn EventListener>>>,
}

impl InProcessBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe a listener. Held weakly, so dropping the listener unsubscribes
    /// it; dead entries are pruned on subscribe and publish.
    pub fn subscribe(&self, listener: Arc<dyn EventListener>) {
        let mut guard = self.listeners.write().unwrap_or_else(|e| e.into_inner());
        guard.retain(|w| w.strong_count() > 0);
        guard.push(Arc::downgrade(&listener));
    }

    /// Number of live subscribers.
    pub fn listener_count(&self) -> usize {
        self.listeners
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|w| w.strong_count() > 0)
            .count()
    }
}

impl EventSink for InProcessBus {
    fn publish(&self, event: Event) {
        // Snapshot under a short read lock so listeners can subscribe/mutate
        // without deadlocking during dispatch.
        let live: Vec<Arc<dyn EventListener>> = {
            let guard = self.listeners.read().unwrap_or_else(|e| e.into_inner());
            guard.iter().filter_map(|w| w.upgrade()).collect()
        };
        for listener in live {
            listener.on_event(&event);
        }
    }
}
