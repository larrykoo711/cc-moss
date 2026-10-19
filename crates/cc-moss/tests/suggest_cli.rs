//! `cc-moss suggest`: no suggestion is still a success; only a name that
//! matches nothing fails.

mod common;

use common::{bin, dead_pid, hook, start_json};

#[test]
fn suggest_says_null_for_a_session_with_nothing_to_suggest() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let out = hook(
        &state,
        "session-start",
        &start_json("sug-0000-1111", "startup", Some("api")),
        Some(dead_pid()),
    );
    assert!(out.status.success(), "{out:?}");
    let run = |args: &[&str]| {
        bin()
            .env("CC_MOSS_STATE_DIR", &state)
            .args(args)
            .output()
            .unwrap()
    };
    // Not running (its process is gone): nothing, and no error.
    let out = run(&["suggest", "api", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v, serde_json::json!({ "schema": 1, "suggestion": null }));
    let out = run(&["suggest", "api"]);
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    // A name that matches nothing.
    let out = run(&["suggest", "nobody", "--json"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
}
