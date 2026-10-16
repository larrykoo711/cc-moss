mod common;

use common::{bin, dead_pid, end_json, hook, ls_json, start_json, FakeProcess};

#[test]
fn dead_pid_shows_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    // Simulates SIGKILL: SessionStart was recorded, no SessionEnd ever arrived.
    hook(
        dir.path(),
        "session-start",
        &start_json("U1", "startup", Some("gone")),
        Some(dead_pid()),
    );
    let v = ls_json(dir.path(), false);
    assert_eq!(v["sessions"][0]["status"], "interrupted");
}

#[test]
fn live_pid_stays_running() {
    let dir = tempfile::tempdir().unwrap();
    hook(
        dir.path(),
        "session-start",
        &start_json("U1", "startup", Some("alive")),
        Some(std::process::id()),
    );
    let v = ls_json(dir.path(), false);
    assert_eq!(v["sessions"][0]["status"], "running");
}

#[test]
fn json_schema_v1_and_all_flag() {
    let dir = tempfile::tempdir().unwrap();
    // Each Claude Code session is its own process.
    let (pa, pb) = (FakeProcess::spawn(), FakeProcess::spawn());
    hook(
        dir.path(),
        "session-start",
        &start_json("A", "startup", Some("a")),
        pa.pid(),
    );
    hook(
        dir.path(),
        "session-start",
        &start_json("B", "startup", Some("b")),
        pb.pid(),
    );
    hook(
        dir.path(),
        "session-end",
        &end_json("B", "prompt_input_exit"),
        pb.pid(),
    );

    let v = ls_json(dir.path(), false);
    assert_eq!(v["schema"], 1);
    let slots = v["sessions"].as_array().unwrap();
    assert_eq!(slots.len(), 1, "closed slots hidden without --all");
    let s = &slots[0];
    assert!(v.get("slots").is_none(), "the internal word stays internal");
    for key in [
        "id",
        "agent",
        "name",
        "status",
        "session_id",
        "session_ids",
        "cwd",
        "tmux",
        "pid",
        "end_reason",
        "notes",
        "created_at",
        "last_active_at",
        "ended_at",
    ] {
        assert!(s.get(key).is_some(), "missing key {key} in {s}");
    }
    assert_eq!(s["agent"], "claude");
    assert_eq!(s["tmux"], serde_json::Value::Null);
    assert_eq!(s["session_ids"], serde_json::json!(["A"]));
    assert!(s["created_at"].as_str().unwrap().ends_with('Z'));
    for key in ["goal", "state", "note"] {
        assert!(s["notes"].get(key).is_some(), "notes.{key}");
    }

    let all = ls_json(dir.path(), true);
    assert_eq!(all["sessions"].as_array().unwrap().len(), 2);
}

#[test]
fn text_sort_order() {
    let dir = tempfile::tempdir().unwrap();
    let (pr, pi) = (FakeProcess::spawn(), FakeProcess::spawn());
    hook(
        dir.path(),
        "session-start",
        &start_json("R", "startup", Some("running-one")),
        pr.pid(),
    );
    hook(
        dir.path(),
        "session-start",
        &start_json("I", "startup", Some("interrupted-one")),
        pi.pid(),
    );
    hook(dir.path(), "session-end", &end_json("I", "other"), pi.pid());

    let out = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .arg("ls")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let i = text.find("interrupted-one").expect(&text);
    let r = text.find("running-one").expect(&text);
    assert!(i < r, "interrupted sessions come first:\n{text}");
}

#[test]
fn empty_registry_prints_hint() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .arg("ls")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .contains("No sessions"));
}

#[test]
fn json_has_busy_and_preview() {
    let dir = tempfile::tempdir().unwrap();
    let p = FakeProcess::spawn();
    hook(
        dir.path(),
        "session-start",
        &start_json("A", "startup", Some("a")),
        p.pid(),
    );
    let prompt =
        serde_json::json!({"session_id": "A", "prompt": "deploy the gateway\nplease"}).to_string();
    hook(dir.path(), "user-prompt-submit", &prompt, p.pid());
    let s = &ls_json(dir.path(), false)["sessions"][0];
    assert_eq!(s["busy"], true);
    assert_eq!(s["last_prompt"], "deploy the gateway");
}

#[test]
fn running_sessions_are_adopted_automatically() {
    // A Claude Code session started before the hooks existed: only `claude agents` knows it.
    let dir = tempfile::tempdir().unwrap();
    let p = FakeProcess::spawn();
    let stub = dir.path().join("claude");
    let agents = serde_json::json!([
        {"pid": p.pid().unwrap(), "cwd": "/work/acme", "kind": "interactive",
         "sessionId": "OLD-1", "name": "job-old", "status": "busy"}
    ]);
    std::fs::write(&stub, format!("#!/bin/sh\necho '{agents}'\n")).unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&stub)
        .status()
        .unwrap();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .env("CC_MOSS_CLAUDE_BIN", &stub)
        .args(["ls", "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let s = &v["sessions"][0];
    assert_eq!(s["session_id"], "OLD-1");
    assert_eq!(s["name"], "job-old");
    assert_eq!(s["status"], "running");
    assert_eq!(s["busy"], true);
    assert_eq!(s["cwd"], "/work/acme");
}

#[test]
fn adopting_leaves_sessions_the_hooks_track_alone() {
    let dir = tempfile::tempdir().unwrap();
    let p = FakeProcess::spawn();
    hook(
        dir.path(),
        "session-start",
        &start_json("A", "startup", Some("a")),
        p.pid(),
    );
    let before = ls_json(dir.path(), false)["sessions"][0]["last_active_at"].clone();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let stub = dir.path().join("claude");
    let agents = serde_json::json!([
        {"pid": p.pid().unwrap(), "cwd": "/work", "kind": "interactive",
         "sessionId": "A", "name": "a", "status": "idle"}
    ]);
    std::fs::write(&stub, format!("#!/bin/sh\necho '{agents}'\n")).unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&stub)
        .status()
        .unwrap();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .env("CC_MOSS_CLAUDE_BIN", &stub)
        .args(["ls", "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(v["sessions"][0]["last_active_at"], before);
}

#[test]
fn adopted_sessions_keep_their_real_last_activity() {
    let dir = tempfile::tempdir().unwrap();
    let p = FakeProcess::spawn();
    let config = dir.path().join("claude-config");
    let project = config.join("projects").join("-work-acme");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join("OLD-2.jsonl");
    std::fs::write(&transcript, "{}\n").unwrap();
    // Last written an hour ago.
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&transcript)
        .unwrap()
        .set_modified(hour_ago)
        .unwrap();
    let stub = dir.path().join("claude");
    let agents = serde_json::json!([
        {"pid": p.pid().unwrap(), "cwd": "/work/acme", "kind": "interactive",
         "sessionId": "OLD-2", "status": "idle"}
    ]);
    std::fs::write(&stub, format!("#!/bin/sh\necho '{agents}'\n")).unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&stub)
        .status()
        .unwrap();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .env("CC_MOSS_CLAUDE_BIN", &stub)
        .env("CLAUDE_CONFIG_DIR", &config)
        .args(["ls"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("1h"), "{text}");
}

/// A hook-tracked session marked busy by its last prompt; `claude agents`
/// reports `status`, and its transcript ends with `last`.
fn busy_after_reconcile(status: &str, last: &str) -> serde_json::Value {
    let dir = tempfile::tempdir().unwrap();
    let p = FakeProcess::spawn();
    let transcript = dir.path().join("B.jsonl");
    std::fs::write(&transcript, "").unwrap();
    let start = serde_json::json!({
        "session_id": "B", "cwd": "/work", "source": "startup",
        "session_title": "b", "transcript_path": transcript,
    })
    .to_string();
    hook(dir.path(), "session-start", &start, p.pid());
    let prompt = serde_json::json!({"session_id": "B", "prompt": "go"}).to_string();
    hook(dir.path(), "user-prompt-submit", &prompt, p.pid());
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&transcript, format!("{last}\n")).unwrap();
    let stub = dir.path().join("claude");
    let agents = serde_json::json!([
        {"pid": p.pid().unwrap(), "cwd": "/work", "kind": "interactive",
         "sessionId": "B", "name": "b", "status": status}
    ]);
    std::fs::write(&stub, format!("#!/bin/sh\necho '{agents}'\n")).unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&stub)
        .status()
        .unwrap();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .env("CC_MOSS_CLAUDE_BIN", &stub)
        .args(["ls", "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v["sessions"][0]["busy"].clone()
}

const TURN_DONE: &str = r#"{"type":"system","subtype":"turn_duration"}"#;
const TOOL_CALL: &str =
    r#"{"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"tool_use"}]}}"#;

#[test]
fn an_idle_session_is_not_shown_working() {
    // e.g. interrupted with Esc: Claude Code sends no Stop hook.
    assert_eq!(busy_after_reconcile("idle", TOOL_CALL), false);
}

#[test]
fn background_tasks_alone_do_not_mean_working() {
    // `claude agents` says busy while background shells run after the turn ended.
    assert_eq!(busy_after_reconcile("busy", TURN_DONE), false);
}

#[test]
fn a_turn_in_progress_stays_working() {
    assert_eq!(busy_after_reconcile("busy", TOOL_CALL), true);
}
