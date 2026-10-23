use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;

fn context(pid: u32, at: i64) -> Context {
    Context {
        pid: Some(pid),
        pid_start: Some(format!("start-{pid}")),
        tmux: None,
        launch_args: None,
        now_ms: at,
    }
}
fn startup(id: &str) -> Event {
    Event::Start {
        source: StartSource::Startup,
        session_id: id.into(),
        cwd: "/work".into(),
        title: None,
        transcript_path: None,
    }
}

#[test]
fn process_binding_legacy_migration_is_empty_and_preserves_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let mut reg = Registry::open(&path).unwrap();
    reg.apply(&startup("legacy"), &context(11, 100)).unwrap();
    drop(reg);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER IF EXISTS process_binding_identity_changed; DROP TRIGGER IF EXISTS process_binding_owner_deleted; DROP TABLE IF EXISTS process_bindings; PRAGMA user_version=1;").unwrap();
    drop(db);
    let reg = Registry::open(&path).unwrap();
    let slots = reg.slots().unwrap();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].session_ids, ["legacy"]);
    let db = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2, "old writers must refuse the new invariants");
    let count: i64 = db
        .query_row("SELECT count(*) FROM process_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "migration must never authorize old records");
}

fn resume(id: &str) -> Event {
    Event::Start {
        source: StartSource::Resume,
        session_id: id.into(),
        cwd: "/work".into(),
        title: None,
        transcript_path: None,
    }
}
fn end_resume(id: &str) -> Event {
    Event::End {
        session_id: id.into(),
        reason: moss_core::event::EndReason::parse("resume"),
    }
}
fn verified(reg: &mut Registry, event: Event, pid: u32, at: i64, origin: &str) {
    reg.apply_verified(&event, &context(pid, at), &|_, _| false, origin)
        .unwrap();
}

#[test]
fn resume_identity_process_binding_single_hop_survives_event_pruning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut reg = Registry::open(&path).unwrap();
    verified(&mut reg, startup("A"), 11, 100, "A");
    let a = reg.slots().unwrap().remove(0);
    let root = reg.process_binding(&a).unwrap().unwrap();
    verified(&mut reg, end_resume("A"), 11, 101, "A");
    verified(&mut reg, resume("B"), 11, 102, "A");
    let b = reg
        .slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "B")
        .unwrap();
    let binding = reg.process_binding(&b).unwrap().unwrap();
    assert_eq!(binding.generation, root.generation);
    assert_eq!(binding.origin_session_id, "A");
    assert_eq!(binding.hops, 1);
    assert_eq!(b.session_ids, ["B"]);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM events", [])
        .unwrap();
    drop(reg);
    let reg = Registry::open(&path).unwrap();
    assert_eq!(reg.process_binding(&b).unwrap(), Some(binding));
}

#[test]
fn resume_identity_process_binding_repair_never_reuses_old_generation() {
    let mut reg = Registry::open_in_memory().unwrap();
    verified(&mut reg, startup("A"), 11, 100, "A");
    reg.apply_with(
        &Event::Prompt {
            session_id: "A".into(),
            title: None,
            prompt: None,
        },
        &context(22, 200),
        &|_, _| false,
    )
    .unwrap();
    verified(&mut reg, end_resume("A"), 22, 201, "A");
    verified(&mut reg, resume("B"), 22, 202, "A");
    let b = reg
        .slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "B")
        .unwrap();
    assert!(reg.process_binding(&b).unwrap().is_none());
}

#[test]
fn resume_identity_process_binding_missing_end_and_second_hop_are_invalid() {
    for missing_end in [true, false] {
        let mut reg = Registry::open_in_memory().unwrap();
        verified(&mut reg, startup("A"), 11, 100, "A");
        if !missing_end {
            verified(&mut reg, end_resume("A"), 11, 101, "A");
        }
        verified(&mut reg, resume("B"), 11, 102, "A");
        if !missing_end {
            verified(&mut reg, end_resume("B"), 11, 103, "A");
            verified(&mut reg, resume("C"), 11, 104, "A");
        }
        let current = reg
            .slots()
            .unwrap()
            .into_iter()
            .find(|s| s.status == moss_core::slot::Status::Running)
            .unwrap();
        assert!(reg.process_binding(&current).unwrap().is_none());
        verified(&mut reg, startup("A"), 11, 105, "A");
        let a = reg
            .slots()
            .unwrap()
            .into_iter()
            .find(|s| s.session_id == "A")
            .unwrap();
        assert!(
            reg.process_binding(&a).unwrap().is_none(),
            "repeated Start cannot reset consumed generation"
        );
    }
}

#[test]
fn process_binding_legacy_start_and_orphan_resume_never_backfill() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&startup("A"), &context(11, 100)).unwrap();
    verified(&mut reg, startup("A"), 11, 101, "A");
    let a = reg.slots().unwrap().remove(0);
    assert!(reg.process_binding(&a).unwrap().is_none());
    verified(&mut reg, resume("B"), 22, 102, "B");
    let b = reg
        .slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "B")
        .unwrap();
    assert!(reg.process_binding(&b).unwrap().is_none());
}

#[test]
fn resume_identity_process_binding_rejects_identity_pane_and_owner_changes() {
    use moss_core::event::TmuxLoc;
    for change in [
        "pid",
        "start",
        "missing-start",
        "pane",
        "socket",
        "origin",
        "no-proof",
    ] {
        let mut reg = Registry::open_in_memory().unwrap();
        let mut first = context(11, 100);
        first.tmux = Some(TmuxLoc {
            socket: "/socket".into(),
            session_id: Some("$1".into()),
            session_name: None,
            pane_id: "%1".into(),
        });
        reg.apply_verified(&startup("A"), &first, &|_, _| false, "A")
            .unwrap();
        reg.apply_verified(&end_resume("A"), &first, &|_, _| false, "A")
            .unwrap();
        let mut next = first.clone();
        next.now_ms = 101;
        match change {
            "pid" => next.pid = Some(22),
            "start" => next.pid_start = Some("other".into()),
            "missing-start" => next.pid_start = None,
            "pane" => next.tmux.as_mut().unwrap().pane_id = "%2".into(),
            "socket" => next.tmux.as_mut().unwrap().socket = "/other".into(),
            _ => {}
        }
        if change == "no-proof" {
            reg.apply_with(&resume("B"), &next, &|_, _| false).unwrap();
        } else {
            reg.apply_verified(
                &resume("B"),
                &next,
                &|_, _| false,
                if change == "origin" { "wrong" } else { "A" },
            )
            .unwrap();
        }
        let b = reg
            .slots()
            .unwrap()
            .into_iter()
            .find(|s| s.session_id == "B")
            .unwrap();
        assert!(reg.process_binding(&b).unwrap().is_none(), "{change}");
    }
}

#[test]
fn process_binding_raw_identity_writes_and_multiple_live_owners_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut reg = Registry::open(&path).unwrap();
    verified(&mut reg, startup("A"), 11, 100, "A");
    verified(&mut reg, end_resume("A"), 11, 101, "A");
    verified(&mut reg, resume("B"), 11, 102, "A");
    let b = reg
        .slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "B")
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE slots SET status='running' WHERE session_id='A'", [])
        .unwrap();
    assert!(reg.process_binding(&b).unwrap().is_none());
    db.execute("UPDATE slots SET status='closed' WHERE session_id='A'", [])
        .unwrap();
    db.execute(
        "UPDATE slots SET pid_start='replacement' WHERE slot_id=?1",
        [&b.slot_id],
    )
    .unwrap();
    db.execute(
        "UPDATE slots SET pid_start='start-11' WHERE slot_id=?1",
        [&b.slot_id],
    )
    .unwrap();
    assert!(
        reg.process_binding(&b).unwrap().is_none(),
        "identity ABA cannot restore trust"
    );
}

#[test]
fn process_binding_clear_or_intervening_activity_invalidates_transfer() {
    for clear in [false, true] {
        let mut reg = Registry::open_in_memory().unwrap();
        verified(&mut reg, startup("A"), 11, 100, "A");
        verified(&mut reg, end_resume("A"), 11, 101, "A");
        if clear {
            let event = Event::Start {
                source: StartSource::Clear,
                session_id: "C".into(),
                cwd: "/work".into(),
                title: None,
                transcript_path: None,
            };
            reg.apply_verified(&event, &context(11, 102), &|_, _| false, "A")
                .unwrap();
        } else {
            reg.apply(
                &Event::Prompt {
                    session_id: "A".into(),
                    title: None,
                    prompt: None,
                },
                &context(11, 102),
            )
            .unwrap();
        }
        verified(&mut reg, resume("B"), 11, 103, "A");
        let b = reg
            .slots()
            .unwrap()
            .into_iter()
            .find(|s| s.session_id == "B")
            .unwrap();
        assert!(reg.process_binding(&b).unwrap().is_none());
    }
}

#[test]
fn process_binding_replayed_end_keeps_pending_generation() {
    let mut reg = Registry::open_in_memory().unwrap();
    verified(&mut reg, startup("A"), 11, 100, "A");
    let a = reg.slots().unwrap().remove(0);
    let generation = reg.process_binding(&a).unwrap().unwrap().generation;
    verified(&mut reg, end_resume("A"), 11, 101, "A");
    verified(&mut reg, end_resume("A"), 11, 102, "A");
    verified(&mut reg, resume("B"), 11, 103, "A");
    let b = reg
        .slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "B")
        .unwrap();
    assert_eq!(
        reg.process_binding(&b).unwrap().unwrap().generation,
        generation
    );
}
