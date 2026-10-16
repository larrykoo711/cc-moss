//! `cc-moss events` and `ls --json`'s `attention`, against a throwaway state dir.

mod common;

use std::path::Path;

use common::{bin, end_json, hook, ls_json, start_json, wait_for, FakeProcess, World};
use serde_json::{json, Value};

fn events(state: &Path, args: &[&str]) -> Value {
    let out = bin()
        .env("CC_MOSS_STATE_DIR", state)
        .args(["events", "--json"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "events failed: {out:?}");
    serde_json::from_slice(&out.stdout).expect("events --json is valid JSON")
}

fn kinds(v: &Value) -> Vec<String> {
    v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect()
}

fn seqs(v: &Value) -> Vec<i64> {
    v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["seq"].as_i64().unwrap())
        .collect()
}

fn ok(state: &Path, event: &str, payload: &str, pid: Option<u32>) {
    let out = hook(state, event, payload, pid);
    assert!(out.status.success(), "{event}: {out:?}");
    // SessionStart outside tmux says so in a systemMessage; nothing else prints.
    if event != "session-start" {
        assert!(out.stdout.is_empty(), "{event} printed {out:?}");
    }
}

fn prompt_json(id: &str) -> String {
    json!({ "session_id": id, "hook_event_name": "UserPromptSubmit", "session_title": "job-a",
            "prompt": "a secret plan" })
    .to_string()
}

fn stop_json(id: &str) -> String {
    json!({ "session_id": id, "hook_event_name": "Stop" }).to_string()
}

fn notification_json(id: &str, kind: &str) -> String {
    json!({ "session_id": id, "cwd": "/work", "hook_event_name": "Notification",
            "message": "Claude needs your permission", "notification_type": kind })
    .to_string()
}

fn post_tool_use_json(id: &str) -> String {
    json!({ "session_id": id, "hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_input": { "command": "ls" }, "tool_response": { "stdout": "" } })
    .to_string()
}

fn attention(state: &Path) -> Value {
    ls_json(state, true)["sessions"][0]["attention"].clone()
}

#[test]
fn json_contract() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    assert_eq!(
        events(&state, &[]),
        json!({ "schema": 1, "events": [], "last_seq": 0 }),
        "no registry yet"
    );

    let claude = FakeProcess::spawn();
    ok(
        &state,
        "session-start",
        &start_json("U1", "startup", Some("job-a")),
        claude.pid(),
    );
    ok(
        &state,
        "user-prompt-submit",
        &prompt_json("U1"),
        claude.pid(),
    );
    ok(
        &state,
        "notification",
        &notification_json("U1", "permission_prompt"),
        claude.pid(),
    );
    assert_eq!(attention(&state), "permission");
    ok(
        &state,
        "notification",
        &notification_json("U1", "idle_prompt"),
        claude.pid(),
    );
    ok(
        &state,
        "post-tool-use",
        &post_tool_use_json("U1"),
        claude.pid(),
    );
    assert_eq!(attention(&state), Value::Null);
    ok(&state, "stop", &stop_json("U1"), claude.pid());

    let v = events(&state, &[]);
    assert_eq!(v["schema"], 1);
    assert_eq!(kinds(&v), ["started", "working", "needs_you", "finished"]);
    let all = v["events"].as_array().unwrap();
    let first = all[0].as_object().unwrap();
    let mut keys: Vec<&str> = first.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["at", "detail", "kind", "name", "seq", "slot_id"]);
    assert_eq!(first["name"], "job-a");
    assert_eq!(first["slot_id"], ls_json(&state, true)["sessions"][0]["id"]);
    assert!(first["at"].as_str().unwrap().ends_with('Z'), "{first:?}");
    assert_eq!(first["detail"], json!({ "source": "startup" }));
    assert_eq!(
        all[2]["detail"],
        json!({ "reason": "permission", "message": "Claude needs your permission" })
    );
    assert_eq!(v["last_seq"], all[3]["seq"]);
    let text = serde_json::to_string(&v).unwrap();
    assert!(!text.contains("secret plan"), "no prompt text: {text}");
}

#[test]
fn unknown_notification_is_logged_not_guessed() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let claude = FakeProcess::spawn();
    ok(
        &state,
        "session-start",
        &start_json("U1", "startup", Some("job-a")),
        claude.pid(),
    );
    ok(
        &state,
        "notification",
        &notification_json("U1", "brand_new_type"),
        claude.pid(),
    );
    ok(
        &state,
        "notification",
        &notification_json("U1", "idle_prompt"),
        claude.pid(),
    );
    assert_eq!(attention(&state), Value::Null);
    assert_eq!(kinds(&events(&state, &[])), ["started"]);
    let log = std::fs::read_to_string(state.join("hook.log")).unwrap();
    assert!(log.contains("brand_new_type"), "{log}");
    assert!(!log.contains("idle_prompt"), "known and ignored: {log}");
}

#[test]
fn since_and_limit() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let claude = FakeProcess::spawn();
    ok(
        &state,
        "session-start",
        &start_json("U1", "startup", Some("job-a")),
        claude.pid(),
    );
    for _ in 0..3 {
        ok(
            &state,
            "user-prompt-submit",
            &prompt_json("U1"),
            claude.pid(),
        );
        ok(&state, "stop", &stop_json("U1"), claude.pid());
    }
    let all = seqs(&events(&state, &[]));
    assert_eq!(all.len(), 7);

    let since = all[2].to_string();
    let v = events(&state, &["--since", &since]);
    assert_eq!(seqs(&v), all[3..]);
    assert_eq!(v["last_seq"], all[6]);

    let v = events(&state, &["--since", &since, "--limit", "2"]);
    assert_eq!(seqs(&v), all[3..5]);
    assert_eq!(v["last_seq"], all[4], "continue from the last one returned");

    let v = events(&state, &["--since", &all[6].to_string()]);
    assert!(seqs(&v).is_empty());
    assert_eq!(v["last_seq"], all[6]);
}

#[test]
fn crash_is_reported_once() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let claude = FakeProcess::spawn();
    ok(
        &state,
        "session-start",
        &start_json("U1", "startup", Some("job-a")),
        claude.pid(),
    );
    ok(
        &state,
        "user-prompt-submit",
        &prompt_json("U1"),
        claude.pid(),
    );
    ok(
        &state,
        "notification",
        &notification_json("U1", "permission_prompt"),
        claude.pid(),
    );
    // SIGKILL: no SessionEnd ever arrives.
    drop(claude);

    for _ in 0..3 {
        let ls = ls_json(&state, true);
        assert_eq!(ls["sessions"][0]["status"], "interrupted");
        assert_eq!(
            ls["sessions"][0]["attention"],
            Value::Null,
            "dead: not waiting"
        );
    }
    let v = events(&state, &[]);
    assert_eq!(
        kinds(&v),
        ["started", "working", "needs_you", "interrupted"]
    );
    assert_eq!(
        v["events"][3]["detail"],
        json!({ "reason": "process-gone" })
    );

    // A late SessionEnd(other) for the same interruption adds nothing.
    ok(&state, "session-end", &end_json("U1", "other"), None);
    ls_json(&state, true);
    assert_eq!(kinds(&events(&state, &[])).len(), 4);
}

#[test]
fn restore_and_close_emit_events() {
    let w = World::new("events");
    w.new_session("job-a");
    let id = w.session_id("job-a");
    w.send(&id, "prompt job-a");
    w.tmux(&["kill-server"]);
    wait_for(|| {
        (ls_json(&w.state(), false)["sessions"][0]["status"] == "interrupted").then_some(())
    });
    let out = w.cc_moss().args(["restore", "--yes"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let v = events(&w.state(), &[]);
    let k = kinds(&v);
    assert_eq!(
        k.iter().filter(|k| *k == "interrupted").count(),
        1,
        "one crash, one report: {k:?}"
    );
    assert!(k.contains(&"restored".to_string()), "{k:?}");

    // Interrupt it again and close it for good.
    wait_for(|| (ls_json(&w.state(), false)["sessions"][0]["status"] == "running").then_some(()));
    w.tmux(&["kill-server"]);
    wait_for(|| {
        (ls_json(&w.state(), false)["sessions"][0]["status"] == "interrupted").then_some(())
    });
    let out = w.cc_moss().args(["close", "job-a"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let k = kinds(&events(&w.state(), &[]));
    assert_eq!(k.last().map(String::as_str), Some("closed"), "{k:?}");
    assert_eq!(k.iter().filter(|k| *k == "interrupted").count(), 2, "{k:?}");
}
