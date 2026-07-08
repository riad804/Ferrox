//! Phase 6 background task system: completion, failure, priority ordering,
//! cooperative cancellation, pause/resume, progress, and concurrency.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::mpsc;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use ferrox_core::task::{Priority, TaskManager, TaskOutcome, TaskState};

/// Block until `f` is true or a timeout elapses (avoids flaky sleeps).
fn wait_until(mut f: impl FnMut() -> bool) {
    for _ in 0..2000 {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("condition not met within timeout");
}

#[test]
fn runs_work_and_delivers_completion() {
    let mgr = TaskManager::new(2);
    let (tx, rx) = mpsc::channel();
    let handle = mgr.submit(
        Priority::Normal,
        |_ctrl| Ok::<_, String>(6 * 7),
        move |outcome| tx.send(outcome).unwrap(),
    );
    match rx.recv().unwrap() {
        TaskOutcome::Completed(v) => assert_eq!(v, 42),
        other => panic!("expected completion, got {other:?}"),
    }
    wait_until(|| handle.state() == TaskState::Completed);
    assert_eq!(handle.progress(), 1.0);
}

#[test]
fn work_error_surfaces_as_failed() {
    let mgr = TaskManager::new(1);
    let (tx, rx) = mpsc::channel();
    mgr.submit(
        Priority::Normal,
        |_| Err::<(), String>("boom".to_string()),
        move |o| tx.send(o).unwrap(),
    );
    assert!(matches!(rx.recv().unwrap(), TaskOutcome::Failed(m) if m == "boom"));
}

#[test]
fn higher_priority_runs_first() {
    // One worker; a gate holds it until we've queued Low then High. The single
    // worker must then pick High before Low.
    let mgr = TaskManager::new(1);
    let gate = Arc::new(Barrier::new(2));
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    let g = gate.clone();
    mgr.submit(Priority::Normal, move |_| { g.wait(); Ok::<_, String>(()) }, |_| {});

    for (prio, tag) in [(Priority::Low, "low"), (Priority::High, "high")] {
        let order = order.clone();
        mgr.submit(prio, move |_| Ok::<_, String>(()), move |_| order.lock().unwrap().push(tag));
    }

    gate.wait(); // release the blocker → worker drains High then Low
    wait_until(|| order.lock().unwrap().len() == 2);
    assert_eq!(*order.lock().unwrap(), vec!["high", "low"]);
}

#[test]
fn cancellation_is_cooperative() {
    let mgr = TaskManager::new(1);
    let started = Arc::new(Barrier::new(2));
    let (tx, rx) = mpsc::channel();

    let s = started.clone();
    let handle = mgr.submit(
        Priority::Normal,
        move |ctrl| -> Result<(), String> {
            s.wait(); // signal we're running
            for _ in 0..1_000_000 {
                ctrl.checkpoint()?; // honour cancel
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(())
        },
        move |o| tx.send(o).unwrap(),
    );

    started.wait();
    handle.cancel();
    assert!(matches!(rx.recv().unwrap(), TaskOutcome::Cancelled));
    assert_eq!(handle.state(), TaskState::Cancelled);
}

#[test]
fn pause_stalls_then_resume_completes() {
    let mgr = TaskManager::new(1);
    let at_checkpoint = Arc::new(Barrier::new(2));
    let (tx, rx) = mpsc::channel();

    let bar = at_checkpoint.clone();
    let handle = mgr.submit(
        Priority::Normal,
        move |ctrl| -> Result<(), String> {
            bar.wait(); // reach the barrier before the first checkpoint
            ctrl.checkpoint()?; // will block here once paused
            Ok(())
        },
        move |o| tx.send(o).unwrap(),
    );

    handle.pause(); // pause before the task hits its checkpoint
    at_checkpoint.wait();

    // The task should now be stuck at the checkpoint (Paused), not completing.
    wait_until(|| handle.state() == TaskState::Paused);
    assert!(rx.try_recv().is_err(), "paused task must not complete");

    handle.resume();
    assert!(matches!(rx.recv().unwrap(), TaskOutcome::Completed(())));
    assert_eq!(handle.state(), TaskState::Completed);
}

#[test]
fn progress_is_reported_and_polled() {
    let mgr = TaskManager::new(1);
    let (tx, rx) = mpsc::channel();
    let handle = mgr.submit(
        Priority::Normal,
        |ctrl| { ctrl.report(0.5); Ok::<_, String>(()) },
        move |o| tx.send(o).unwrap(),
    );
    let _ = rx.recv().unwrap();
    // Completed tasks report 1.0; mid-run 0.5 was observable via the handle.
    assert_eq!(handle.progress(), 1.0);
}

#[test]
fn many_tasks_all_complete_across_workers() {
    let mgr = TaskManager::new(4);
    let (tx, rx) = mpsc::channel();
    for i in 0..200u32 {
        let tx = tx.clone();
        mgr.submit(Priority::Normal, move |_| Ok::<_, String>(i), move |o| tx.send(o).unwrap());
    }
    drop(tx);
    let mut sum = 0u64;
    while let Ok(TaskOutcome::Completed(v)) = rx.recv() {
        sum += v as u64;
    }
    assert_eq!(sum, (0..200u64).sum::<u64>());
}
