//! Moss from the command line without tmux: the hook records its role,
//! `close` will not end it, and its guard hook says no when in doubt.

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

use common::{bin, dead_pid, ls_json, start_json};

/// A SessionStart from Moss's Claude Code (`CC_MOSS_ROLE=moss`), or from an
/// ordinary session.
fn start(state: &Path, sid: &str, title: &str, moss: bool) {
    let mut cmd = bin();
    cmd.env("CC_MOSS_STATE_DIR", state)
        .env("CLAUDE_PID", dead_pid().to_string())
        .args(["hook", "session-start"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if moss {
        cmd.env("CC_MOSS_ROLE", "moss");
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(start_json(sid, "startup", Some(title)).as_bytes())
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
}

fn session<'a>(ls: &'a serde_json::Value, sid: &str) -> &'a serde_json::Value {
    ls["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == sid)
        .unwrap_or_else(|| panic!("{sid} not listed in {ls}"))
}

#[test]
fn hook_records_the_moss_role() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    start(&state, "moss-0000-1111", "moss", true);
    start(&state, "work-0000-1111", "work", false);
    let ls = ls_json(&state, true);
    assert_eq!(session(&ls, "moss-0000-1111")["role"], "moss");
    assert!(session(&ls, "work-0000-1111")["role"].is_null());
}

#[test]
fn close_refuses_moss() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    // Both died (dead pids): interrupted, which `close` would normally end.
    start(&state, "moss-0000-1111", "moss", true);
    start(&state, "work-0000-1111", "work", false);
    let out = bin()
        .env("CC_MOSS_STATE_DIR", &state)
        .args(["close", "moss", "work"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Moss is looked after by cc-moss"), "{err}");
    assert!(err.contains("Settings"), "{err}");
    let ls = ls_json(&state, true);
    assert_ne!(session(&ls, "moss-0000-1111")["status"], "closed");
    assert_eq!(
        session(&ls, "work-0000-1111")["status"],
        "closed",
        "the others are still closed"
    );
}

fn guard(home: &Path, payload: &str) -> Output {
    let mut child = bin()
        .env("CLAUDE_PROJECT_DIR", home)
        .args(["hook", "moss-guard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
    child.wait_with_output().unwrap()
}

#[test]
fn guard_hook_blocks_with_exit_2_and_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("moss");
    std::fs::create_dir_all(&home).unwrap();
    let write = |path: &str| {
        serde_json::json!({
            "tool_name": "Write",
            "cwd": home,
            "tool_input": { "file_path": path, "content": "x" }
        })
        .to_string()
    };
    let inside = guard(&home, &write("journal/today.md"));
    assert_eq!(inside.status.code(), Some(0), "{inside:?}");
    assert!(inside.stdout.is_empty() && inside.stderr.is_empty());

    let outside = guard(&home, &write("/etc/hosts"));
    assert_eq!(outside.status.code(), Some(2), "{outside:?}");
    assert!(String::from_utf8_lossy(&outside.stderr).contains("own home"));

    let settings = guard(&home, &write(".claude/settings.json"));
    assert_eq!(settings.status.code(), Some(2));

    // Unreadable input: no.
    for junk in ["", "not json", "{\"tool_input\":{}}"] {
        let out = guard(&home, junk);
        assert_eq!(out.status.code(), Some(2), "{junk:?}: {out:?}");
    }
}

#[test]
fn moss_status_reports_off_after_stop_without_a_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let config = dir.path().join("config.toml");
    let run = |args: &[&str]| {
        bin()
            .env("CC_MOSS_STATE_DIR", &state)
            .env("CC_MOSS_CONFIG", &config)
            .args(args)
            .output()
            .unwrap()
    };
    let st: serde_json::Value =
        serde_json::from_slice(&run(&["moss", "status", "--json"]).stdout).unwrap();
    assert_eq!(
        st,
        serde_json::json!({"schema":1,"enabled":true,"running":false,"restarts":0})
    );
    start(&state, "moss-0000-1111", "moss", true);
    let out = run(&["moss", "stop"]);
    assert!(out.status.success(), "{out:?}");
    let st: serde_json::Value =
        serde_json::from_slice(&run(&["moss", "status", "--json"]).stdout).unwrap();
    assert_eq!(st["enabled"], false);
    assert_eq!(
        session(&ls_json(&state, true), "moss-0000-1111")["status"],
        "closed"
    );
    let on = run(&["moss", "start"]);
    assert!(on.status.success(), "{on:?}");
    assert!(
        String::from_utf8_lossy(&on.stdout).contains("daemon start"),
        "says the daemon is not running: {on:?}"
    );
}
