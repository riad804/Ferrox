//! [`TaskManager`] — a fixed-size thread pool that runs submitted work in
//! priority order (FIFO within a priority), with graceful shutdown.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use super::control::{Shared, TaskControl};
use super::{Priority, TaskHandle, TaskId, TaskOutcome, TaskState};

/// A queued unit of work with its scheduling key and shared control state.
struct QueuedTask {
    priority: Priority,
    seq: u64,
    shared: Arc<Shared>,
    run: Box<dyn FnOnce(&TaskControl) + Send>,
}

// Ordering for the max-heap: higher priority first, then lower `seq` (FIFO).
impl PartialEq for QueuedTask {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.seq == other.seq
    }
}
impl Eq for QueuedTask {}
impl Ord for QueuedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority).then_with(|| other.seq.cmp(&self.seq))
    }
}
impl PartialOrd for QueuedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Pool {
    queue: Mutex<BinaryHeap<QueuedTask>>,
    available: Condvar,
    shutdown: AtomicBool,
    next_id: AtomicU64,
    next_seq: AtomicU64,
}

/// A priority thread pool for background tasks.
pub struct TaskManager {
    pool: Arc<Pool>,
    workers: Vec<JoinHandle<()>>,
}

impl TaskManager {
    /// A manager with `workers` worker threads (at least 1).
    pub fn new(workers: usize) -> Self {
        let pool = Arc::new(Pool {
            queue: Mutex::new(BinaryHeap::new()),
            available: Condvar::new(),
            shutdown: AtomicBool::new(false),
            next_id: AtomicU64::new(0),
            next_seq: AtomicU64::new(0),
        });
        let workers = (0..workers.max(1))
            .map(|_| {
                let pool = Arc::clone(&pool);
                thread::spawn(move || worker_loop(pool))
            })
            .collect();
        Self { pool, workers }
    }

    /// A manager sized to the machine's parallelism.
    pub fn with_default_workers() -> Self {
        let n = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Self::new(n)
    }

    /// Submit `work` at `priority`. `work` receives a [`TaskControl`] and returns
    /// `Ok(value)` or `Err(message)`; the outcome is delivered to `on_complete`.
    /// Returns a [`TaskHandle`] to poll/cancel/pause/resume the task.
    pub fn submit<T, W, C>(&self, priority: Priority, work: W, on_complete: C) -> TaskHandle
    where
        T: Send + 'static,
        W: FnOnce(&TaskControl) -> Result<T, String> + Send + 'static,
        C: FnOnce(TaskOutcome<T>) + Send + 'static,
    {
        let id = TaskId(self.pool.next_id.fetch_add(1, AtomicOrdering::Relaxed));
        let seq = self.pool.next_seq.fetch_add(1, AtomicOrdering::Relaxed);
        let shared = Shared::new();
        let handle = TaskHandle::new(id, Arc::clone(&shared));

        let run_shared = Arc::clone(&shared);
        let run = Box::new(move |ctrl: &TaskControl| {
            run_shared.set_state(TaskState::Running);
            let outcome = if ctrl.is_cancelled() {
                TaskOutcome::Cancelled
            } else {
                let result = work(ctrl);
                // A task cancelled during `work` (e.g. via `checkpoint()?`) is
                // Cancelled, whatever error it returned on the way out.
                if ctrl.is_cancelled() {
                    TaskOutcome::Cancelled
                } else {
                    match result {
                        Ok(v) => TaskOutcome::Completed(v),
                        Err(e) => TaskOutcome::Failed(e),
                    }
                }
            };
            let final_state = match &outcome {
                TaskOutcome::Completed(_) => TaskState::Completed,
                TaskOutcome::Cancelled => TaskState::Cancelled,
                TaskOutcome::Failed(_) => TaskState::Failed,
            };
            if matches!(final_state, TaskState::Completed) {
                ctrl.report(1.0);
            }
            run_shared.set_state(final_state);
            on_complete(outcome);
        });

        self.pool.queue.lock().unwrap_or_else(|e| e.into_inner()).push(QueuedTask { priority, seq, shared, run });
        self.pool.available.notify_one();
        handle
    }

    /// Number of tasks currently queued (not yet picked up).
    pub fn pending(&self) -> usize {
        self.pool.queue.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.pool.shutdown.store(true, AtomicOrdering::Release);
        self.pool.available.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker_loop(pool: Arc<Pool>) {
    loop {
        let task = {
            let mut q = pool.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if pool.shutdown.load(AtomicOrdering::Acquire) {
                    return;
                }
                if let Some(t) = q.pop() {
                    break t;
                }
                q = pool.available.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let ctrl = TaskControl::new(Arc::clone(&task.shared));
        (task.run)(&ctrl);
    }
}
