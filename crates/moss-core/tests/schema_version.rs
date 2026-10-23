//! A registry written by a newer cc-moss is read, never written.

use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::{NewerSchema, Registry};

fn ctx() -> Context {
    Context {
        pid: Some(4242),
        pid_start: Some("start-4242".into()),
        tmux: None,
        launch_args: None,
        now_ms: 1_000,
    }
}

fn start(id: &str) -> Event {
    Event::Start {
        source: StartSource::Startup,
        session_id: id.into(),
        cwd: "/work".into(),
        title: Some("job-a".into()),
        transcript_path: None,
    }
}

fn user_version(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn columns(path: &std::path::Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('slots')")
        .unwrap();
    let names = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap();
    names
}

/// A registry with one slot, then stamped as a future schema version that
/// also dropped a column this version would add back.
fn future_registry(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("registry.db");
    let mut reg = Registry::open(&path).unwrap();
    reg.apply(&start("U1"), &ctx()).unwrap();
    drop(reg);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE slots DROP COLUMN name_scanned_to; PRAGMA user_version = 99;")
        .unwrap();
    path
}

#[test]
fn newer_schema_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = future_registry(dir.path());
    let before = columns(&path);

    let mut reg = Registry::open(&path).unwrap();
    assert_eq!(reg.newer_schema(), Some(99));
    assert_eq!(columns(&path), before, "ensure_columns left it alone");

    let err = reg.apply(&start("U2"), &ctx()).unwrap_err();
    let newer = err.downcast_ref::<NewerSchema>().expect("typed error");
    assert_eq!(newer.found, 99);
    let slots = reg.slots().expect("reads still work");
    assert_eq!(slots.len(), 1);
    let id = &slots[0].slot_id;
    assert!(reg
        .set_name(id, "x")
        .unwrap_err()
        .downcast_ref::<NewerSchema>()
        .is_some());
    assert!(reg.close_by_user(id, 2_000).is_err());
    assert!(reg.backfill_names(&mut Default::default()).is_err());
    assert_eq!(reg.slots().unwrap()[0].name.as_deref(), Some("job-a"));
    assert_eq!(user_version(&path), 99, "never downgraded");
}

#[test]
fn current_schema_is_writable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    let reg = Registry::open(&path).unwrap();
    assert_eq!(reg.newer_schema(), None);
}

fn has_table(path: &std::path::Path, name: &str) -> bool {
    rusqlite::Connection::open(path)
        .unwrap()
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")
        .unwrap()
        .exists([name])
        .unwrap()
}

#[test]
fn process_binding_migrates_legacy_events_and_blocks_v1_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    // A registry as 0.1.x left it: schema 1, no event table, no attention.
    let mut reg = Registry::open(&path).unwrap();
    reg.apply(&start("U1"), &ctx()).unwrap();
    drop(reg);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP TABLE events; ALTER TABLE slots DROP COLUMN attention;
         DROP TRIGGER process_binding_identity_changed; DROP TRIGGER process_binding_owner_deleted;
         DROP TABLE process_bindings; PRAGMA user_version=1;",
        )
        .unwrap();
    assert!(!has_table(&path, "events"));
    assert_eq!(user_version(&path), 1);

    // Identity invariants require a version bump, with no binding backfill.
    let mut reg = Registry::open(&path).unwrap();
    assert!(has_table(&path, "events"));
    assert!(columns(&path).contains(&"attention".to_string()));
    let ended = Event::End {
        session_id: "U1".into(),
        reason: moss_core::event::EndReason::parse("other"),
    };
    reg.apply(&ended, &ctx()).unwrap();
    assert_eq!(reg.events(Some(0), 10).unwrap()[0].kind, "interrupted");
    drop(reg);
    assert_eq!(user_version(&path), 2);

    // Version-1 Registry::init enters query_only for any version above 1.
    // Mirror that existing old-writer gate, including SQLite enforcement.
    let old = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = old
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert!(version > 1);
    old.pragma_update(None, "query_only", true).unwrap();
    assert!(old
        .execute("UPDATE slots SET status='running'", [])
        .is_err());
    let count: i64 = old
        .query_row("SELECT count(*) FROM process_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(Registry::open(&path).unwrap().slots().unwrap().len(), 1);
}
