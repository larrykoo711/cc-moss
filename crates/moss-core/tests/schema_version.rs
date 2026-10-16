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
    assert!(reg.backfill_names().is_err());
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
