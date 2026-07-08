//! [`TaskControl`] and its shared state — the cooperative primitive that lets a
//! running work closure honour pause/cancel and publish progress.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use super::TaskState;

/// Returned by [`TaskControl::checkpoint`] when the task has been cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// Lets work closures write `ctrl.checkpoint()?` even though they return
/// `Result<T, String>` — the manager reinterprets a cancelled task as
/// [`super::TaskOutcome::Cancelled`], so the message is only a fallback.
impl From<Cancelled> for String {
    fn from(_: Cancelled) -> Self {
        "task cancelled".to_string()
    }
}

/// State shared between a task's [`TaskControl`] (worker side) and its
/// [`super::TaskHandle`] (submitter side).
pub(crate) struct Shared {
    state: Mutex<TaskState>,
    cancelled: AtomicBool,
    paused: AtomicBool,
    progress_bits: AtomicU32,
    pause_lock: Mutex<()>,
    pause_cv: Condvar,
}

impl Shared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TaskState::Queued),
            cancelled: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            progress_bits: AtomicU32::new(0),
            pause_lock: Mutex::new(()),
            pause_cv: Condvar::new(),
        })
    }

    pub(crate) fn set_state(&self, s: TaskState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }

    pub(crate) fn state(&self) -> TaskState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn progress(&self) -> f32 {
        f32::from_bits(self.progress_bits.load(Ordering::Acquire))
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.pause_cv.notify_all(); // wake any paused checkpoint so it can bail
    }

    pub(crate) fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    pub(crate) fn resume(&self) {
        self.paused.store(false, Ordering::Release);
        self.pause_cv.notify_all();
    }
}

/// The worker-side control passed to a task's work closure.
#[derive(Clone)]
pub struct TaskControl {
    shared: Arc<Shared>,
}

impl TaskControl {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// True if the task has been asked to cancel.
    pub fn is_cancelled(&self) -> bool {
        self.shared.is_cancelled()
    }

    /// A cooperation point: blocks while the task is paused and returns
    /// [`Cancelled`] if it has been cancelled. Call it periodically inside long
    /// loops.
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        if self.shared.is_cancelled() {
            return Err(Cancelled);
        }
        if self.shared.paused.load(Ordering::Acquire) {
            let mut guard = self.shared.pause_lock.lock().unwrap_or_else(|e| e.into_inner());
            while self.shared.paused.load(Ordering::Acquire) && !self.shared.is_cancelled() {
                self.shared.set_state(TaskState::Paused);
                guard = self.shared.pause_cv.wait(guard).unwrap_or_else(|e| e.into_inner());
            }
            drop(guard);
            if self.shared.is_cancelled() {
                return Err(Cancelled);
            }
            self.shared.set_state(TaskState::Running);
        }
        Ok(())
    }

    /// Publish progress in `[0, 1]`.
    pub fn report(&self, fraction: f32) {
        self.shared.progress_bits.store(fraction.clamp(0.0, 1.0).to_bits(), Ordering::Release);
    }

    /// Current progress in `[0, 1]`.
    pub fn progress(&self) -> f32 {
        self.shared.progress()
    }
}
