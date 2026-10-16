//! Liveness checks spawn `ps`; they must not run while the registry's write
//! lock is held, or concurrent hooks time out and drop their events.

use std::cell::Cell;
use std::path::Path;
use std::time::Duration;

use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;

fn ctx(pid: u32) -> Context {
    Context {
        pid: Some(pid),
        pid_start: Some(format!("start-{pid}")),
        tmux: None,
        launch_args: None,
        now_ms: 1_000,
    }
}

fn start(source: StartSource, id: &str) -> Event {
    Event::Start {
        source,
        session_id: id.into(),
        cwd: "/work".into(),
        title: None,
        transcript_path: None,
    }
}

/// True when another writer could take the lock right now.
fn lock_is_free(path: &Path) -> bool {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.busy_timeout(Duration::ZERO).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").is_ok()
}

fn check(event: Event, from_pid: u32) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    let mut reg = Registry::open(&path).unwrap();
    reg.apply(&start(StartSource::Startup, "U1"), &ctx(100))
        .unwrap();

    let calls = Cell::new(0);
    let locked_during_check = Cell::new(false);
    let is_alive = |_pid: u32, _start: Option<&str>| {
        calls.set(calls.get() + 1);
        if !lock_is_free(&path) {
            locked_during_check.set(true);
        }
        true
    };
    reg.apply_with(&event, &ctx(from_pid), &is_alive).unwrap();
    assert!(calls.get() > 0, "the owner's liveness was consulted");
    assert!(!locked_during_check.get(), "checked outside the write lock");
}

#[test]
fn resume_by_another_process_checks_liveness_outside_the_lock() {
    check(start(StartSource::Resume, "U1"), 200);
}

#[test]
fn prompt_from_another_process_checks_liveness_outside_the_lock() {
    check(
        Event::Prompt {
            session_id: "U1".into(),
            title: None,
            prompt: None,
        },
        200,
    );
}

#[test]
fn liveness_still_decides_the_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    let mut reg = Registry::open(&path).unwrap();
    reg.apply(&start(StartSource::Startup, "U1"), &ctx(100))
        .unwrap();
    // The owner is alive: a second process on the conversation gets its own slot.
    reg.apply_with(&start(StartSource::Resume, "U1"), &ctx(200), &|_, _| true)
        .unwrap();
    assert_eq!(reg.slots().unwrap().len(), 2);
    // The owner is dead: the resume takes the slot over.
    let dir = tempfile::tempdir().unwrap();
    let mut reg = Registry::open(&dir.path().join("registry.db")).unwrap();
    reg.apply(&start(StartSource::Startup, "U1"), &ctx(100))
        .unwrap();
    reg.apply_with(&start(StartSource::Resume, "U1"), &ctx(200), &|_, _| false)
        .unwrap();
    assert_eq!(reg.slots().unwrap().len(), 1);
}
