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
    assert_eq!(v["slots"][0]["status"], "interrupted");
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
    assert_eq!(v["slots"][0]["status"], "running");
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
    let slots = v["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 1, "closed slots hidden without --all");
    let s = &slots[0];
    for key in [
        "slot_id",
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
    assert_eq!(all["slots"].as_array().unwrap().len(), 2);
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
    let s = &ls_json(dir.path(), false)["slots"][0];
    assert_eq!(s["busy"], true);
    assert_eq!(s["last_prompt"], "deploy the gateway");
}
