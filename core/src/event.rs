//! Domain **events** and the [`EventSink`] port — the Observer backbone of the
//! SDK. The application layer publishes events describing what changed; adapters
//! (UI, telemetry, collaboration) subscribe. The port keeps the domain unaware
//! of any concrete delivery mechanism (Dependency Inversion).
//!
//! Delivery is WASM- and single-thread-safe: [`NoopSink`] for the default no-op,
//! and [`crate::bus::InProcessBus`] for real fan-out.

/// A change that happened to a project / session. Extended in Phase 7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The project changed in some unspecified way (coarse-grained catch-all).
    ProjectChanged,
    ClipAdded { track: usize, index: usize },
    ClipRemoved { track: usize, index: usize },
    TrackAdded { index: usize },
    TrackRemoved { index: usize },
    Undo,
    Redo,
}

/// A **port** for publishing [`Event`]s. Injected into the application layer so
/// the engine never depends on a concrete bus.
pub trait EventSink: Send + Sync {
    /// Publish an event to any subscribers.
    fn publish(&self, event: Event);
}

/// A subscriber that reacts to events (used by [`crate::bus::InProcessBus`]).
pub trait EventListener: Send + Sync {
    fn on_event(&self, event: &Event);
}

/// An [`EventSink`] that drops every event — the default when no bus is wired.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, _event: Event) {}
}
