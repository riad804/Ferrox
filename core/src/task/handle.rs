//! [`TaskHandle`] — the submitter-side handle to poll and control a task.

use std::sync::Arc;

use super::control::Shared;
use super::{TaskId, TaskState};

/// A cloneable handle to a submitted task. Poll [`state`](TaskHandle::state) /
/// [`progress`](TaskHandle::progress) and drive lifecycle with
/// [`cancel`](TaskHandle::cancel) / [`pause`](TaskHandle::pause) /
/// [`resume`](TaskHandle::resume). The result is delivered to the task's
/// `on_complete` callback.
#[derive(Clone)]
pub struct TaskHandle {
    id: TaskId,
    shared: Arc<Shared>,
}

impl TaskHandle {
    pub(crate) fn new(id: TaskId, shared: Arc<Shared>) -> Self {
        Self { id, shared }
    }

    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Current lifecycle state.
    pub fn state(&self) -> TaskState {
        self.shared.state()
    }

    /// Reported progress in `[0, 1]`.
    pub fn progress(&self) -> f32 {
        self.shared.progress()
    }

    /// Whether the task reached a terminal state.
    pub fn is_finished(&self) -> bool {
        self.shared.state().is_finished()
    }

    /// Request cancellation (cooperative — honoured at the task's next checkpoint).
    pub fn cancel(&self) {
        self.shared.cancel();
    }

    /// Request a pause (task blocks at its next checkpoint).
    pub fn pause(&self) {
        self.shared.pause();
    }

    /// Resume a paused task.
    pub fn resume(&self) {
        self.shared.resume();
    }
}
