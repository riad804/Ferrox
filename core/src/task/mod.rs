//! # Background task system (Phase 6)
//!
//! A priority-scheduled thread pool for long-running work (export, import, proxy
//! /thumbnail/waveform generation, AI inference) with **cooperative**
//! cancellation, pause/resume, and progress reporting.
//!
//! - [`TaskManager`] — the thread pool + priority queue.
//! - [`TaskControl`] — passed to the work closure; call `checkpoint()` to honour
//!   pause/cancel and `report()` to publish progress.
//! - [`TaskHandle`] — returned to the submitter to poll state/progress and
//!   `cancel`/`pause`/`resume`. Results arrive via the `on_complete` callback
//!   (FFI-friendly — no `Future` crosses the boundary).
//!
//! Native only: real threads don't exist on `wasm32`, where long operations run
//! cooperatively on the host event loop instead.

pub mod control;
pub mod handle;
pub mod manager;

pub use control::{Cancelled, TaskControl};
pub use handle::TaskHandle;
pub use manager::TaskManager;

/// A unique task identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

/// Scheduling priority. Higher runs first; FIFO within a priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
}

/// The lifecycle state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Running,
    Paused,
    Completed,
    Cancelled,
    Failed,
}

impl TaskState {
    /// Whether the task has reached a terminal state.
    pub fn is_finished(self) -> bool {
        matches!(self, TaskState::Completed | TaskState::Cancelled | TaskState::Failed)
    }
}

/// The result delivered to a task's `on_complete` callback.
#[derive(Debug)]
pub enum TaskOutcome<T> {
    Completed(T),
    Cancelled,
    Failed(String),
}
