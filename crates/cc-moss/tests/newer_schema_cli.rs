//! After a downgrade: a registry from a newer cc-moss is read, never written.

mod common;

use std::path::Path;

use common::{bin, hook, ls_json, start_json};

/// A state dir with one session, then stamped as a future schema version.
fn future_state() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let out = hook(
        dir.path(),
        "session-start",
        &start_json("U1", "startup", Some("job-a")),
        Some(std::process::id()),
    );
    assert!(out.status.success());
    rusqlite::Connection::open(dir.path().join("registry.db"))
        .unwrap()
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    dir
}

fn user_version(state: &Path) -> i64 {
    rusqlite::Connection::open(state.join("registry.db"))
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

#[test]
fn hooks_skip_quietly_and_log_once() {
    let dir = future_state();
    let _ = std::fs::remove_file(dir.path().join("hook.log"));
    for id in ["U2", "U3"] {
        let out = hook(
            dir.path(),
            "session-start",
            &start_json(id, "startup", Some(id)),
            Some(std::process::id()),
        );
        assert_eq!(out.status.code(), Some(0));
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    }
    let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
    assert_eq!(log.matches("newer cc-moss").count(), 1, "{log}");
    assert_eq!(user_version(dir.path()), 99);
    let v = ls_json(dir.path(), true);
    assert_eq!(
        v["sessions"].as_array().unwrap().len(),
        1,
        "nothing written"
    );
}

#[test]
fn ls_reads_and_writes_say_upgrade() {
    let dir = future_state();
    let v = ls_json(dir.path(), true);
    assert_eq!(v["sessions"][0]["name"], "job-a");

    let note = |lang: &str| {
        bin()
            .env("CC_MOSS_STATE_DIR", dir.path())
            .env("CC_MOSS_LANG", lang)
            .args(["note", "job-a", "--goal", "ship"])
            .output()
            .unwrap()
    };
    let out = note("en");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("newer cc-moss") && err.contains("upgrade cc-moss"),
        "{err}"
    );
    let out = note("zh");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("更新版本的 cc-moss"), "{err}");
    assert_eq!(user_version(dir.path()), 99);
}
