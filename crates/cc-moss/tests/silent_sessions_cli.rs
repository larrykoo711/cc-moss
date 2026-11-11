//! A session nobody spoke to is closed once its process is gone (F62), read
//! from the real transcript the way an adopted session's words are only
//! there: through the binary's own look at the registry.

mod common;

use common::{dead_pid, hook, ls_json};

fn session_start(state: &std::path::Path, sid: &str, transcript: &std::path::Path, pid: u32) {
    let start = serde_json::json!({
        "session_id": sid,
        "transcript_path": transcript,
        "cwd": "/work",
        "hook_event_name": "SessionStart",
        "source": "startup",
    });
    let out = hook(state, "session-start", &start.to_string(), Some(pid));
    assert!(out.status.success(), "{out:?}");
}

fn status(v: &serde_json::Value, sid: &str) -> String {
    v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == sid)
        .unwrap_or_else(|| panic!("{sid} not in {v}"))["status"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn silent_cli_reads_what_you_typed_in_the_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    // Words you typed, and the turn they started finished: no Prompt hook
    // ever saw them.
    let said = dir.path().join("said.jsonl");
    std::fs::write(
        &said,
        concat!(
            r#"{"type":"user","timestamp":"2026-11-09T10:00:00Z","message":{"role":"user","content":"fix the login page"}}"#,
            "\n",
            r#"{"type":"system","subtype":"turn_duration"}"#,
            "\n",
        ),
    )
    .unwrap();
    // Only Claude Code's own records.
    let quiet = dir.path().join("quiet.jsonl");
    std::fs::write(
        &quiet,
        "{\"type\":\"system\",\"subtype\":\"turn_duration\"}\n",
    )
    .unwrap();
    session_start(&state, "said-0001", &said, dead_pid());
    session_start(&state, "quiet-0002", &quiet, dead_pid());
    let v = ls_json(&state, true);
    assert_eq!(status(&v, "said-0001"), "interrupted", "{v}");
    assert_eq!(status(&v, "quiet-0002"), "closed", "{v}");
}
