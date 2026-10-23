//! The archive and restart claims on the registry: a registry written by
//! 0.3.0 opens and takes them, and archiving leaves a session alone that
//! came back while it was being looked at.

use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;

/// The `slots`, `slot_sessions` and `events` tables exactly as 0.3.0 left
/// them (schema 1, before the archive and provider columns), with one
/// dropped session in them.
fn registry_030(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE slots (
           slot_id TEXT PRIMARY KEY,
           agent TEXT NOT NULL,
           session_id TEXT NOT NULL,
           name TEXT,
           cwd TEXT NOT NULL,
           pid INTEGER,
           pid_start TEXT,
           tmux_socket TEXT,
           tmux_session_id TEXT,
           tmux_session_name TEXT,
           tmux_pane TEXT,
           launch_args TEXT NOT NULL DEFAULT '[]',
           note_goal TEXT,
           note_state TEXT,
           note_text TEXT,
           status TEXT NOT NULL CHECK (status IN ('running','closed','interrupted')),
           restore_started_at INTEGER,
           busy INTEGER NOT NULL DEFAULT 0,
           last_prompt TEXT,
           name_scanned_to INTEGER NOT NULL DEFAULT 0,
           end_reason TEXT,
           created_at INTEGER NOT NULL,
           last_active_at INTEGER NOT NULL,
           ended_at INTEGER,
           attention TEXT,
           lost_reported_at INTEGER,
           role TEXT
         );
         CREATE INDEX slots_pid ON slots(pid, status);
         CREATE TABLE slot_sessions (
           slot_id TEXT NOT NULL REFERENCES slots(slot_id),
           session_id TEXT NOT NULL,
           seq INTEGER NOT NULL,
           transcript_path TEXT,
           PRIMARY KEY (slot_id, session_id)
         );
         CREATE INDEX slot_sessions_session ON slot_sessions(session_id);
         CREATE TABLE events (
           seq INTEGER PRIMARY KEY AUTOINCREMENT,
           slot_id TEXT NOT NULL,
           kind TEXT NOT NULL,
           detail TEXT NOT NULL DEFAULT '{}',
           at INTEGER NOT NULL
         );
         CREATE INDEX events_slot ON events(slot_id, seq);
         CREATE INDEX events_at ON events(at);
         INSERT INTO slots (slot_id, agent, session_id, name, cwd, status, end_reason,
                            created_at, last_active_at, ended_at)
           VALUES ('OLD', 'claude', 'sess-old', 'old', '/work', 'interrupted', 'other',
                   1000, 1000, 1000);
         INSERT INTO slot_sessions VALUES ('OLD', 'sess-old', 1, '/nonexistent/sess-old.jsonl');
         PRAGMA user_version = 1;",
    )
    .unwrap();
}

#[test]
fn a_030_registry_takes_the_archive_and_restart_claims() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    registry_030(&path);
    let r = Registry::open(&path).unwrap();
    let old = r.slots().unwrap().remove(0);
    assert_eq!(old.archived_at_ms, None);
    assert_eq!(old.provider_started, None);
    // Archive, bring back, and claim it for a restart.
    assert_eq!(
        r.archive(&["OLD".into()], "you", &|_| false, 5_000)
            .unwrap(),
        ["OLD"]
    );
    assert!(r.slots().unwrap()[0].is_archived());
    assert_eq!(r.unarchive(&["OLD".into()], "you", 6_000).unwrap(), ["OLD"]);
    let old = r.slots().unwrap().remove(0);
    assert!(!old.is_archived());
    assert!(r.claim_restore(&old, 7_000).unwrap());
    assert!(!r.claim_restore(&old, 7_001).unwrap(), "claimed once");
    // Durable process binding invariants require schema 2; old writers must refuse.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
}

#[test]
fn archive_leaves_a_session_that_came_back_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    let mut r = Registry::open(&path).unwrap();
    let ctx = |now_ms| Context {
        pid: None,
        pid_start: None,
        tmux: None,
        launch_args: None,
        now_ms,
    };
    r.apply(
        &Event::Start {
            source: StartSource::Startup,
            session_id: "s".into(),
            cwd: "/w".into(),
            title: Some("api".into()),
            transcript_path: None,
        },
        &ctx(1_000),
    )
    .unwrap();
    let id = r.slots().unwrap()[0].slot_id.clone();
    // While it is being looked at, a prompt comes in from another process.
    let came_back = || {
        let mut other = Registry::open(&path).unwrap();
        other
            .apply(
                &Event::Prompt {
                    session_id: "s".into(),
                    title: None,
                    prompt: Some("hi".into()),
                },
                &ctx(2_000),
            )
            .unwrap();
        false
    };
    let done = r
        .archive(std::slice::from_ref(&id), "auto", &|_| came_back(), 3_000)
        .unwrap();
    assert!(done.is_empty(), "it moved on meanwhile");
    assert!(!r.slots().unwrap()[0].is_archived());
}
