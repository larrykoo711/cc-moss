//! Hook latency budget. Run on demand:
//! `cargo test --release -p cc-moss --test hook_perf -- --ignored`

mod common;

use common::{hook, start_json};
use std::time::Instant;

#[test]
#[ignore]
fn hook_p95_under_30ms() {
    let dir = tempfile::tempdir().unwrap();
    let me = Some(std::process::id());
    let mut samples = Vec::with_capacity(200);
    for i in 0..200 {
        let payload = if i == 0 {
            start_json("P", "startup", Some("perf"))
        } else {
            serde_json::json!({ "session_id": "P", "session_title": "perf" }).to_string()
        };
        let event = if i == 0 {
            "session-start"
        } else {
            "user-prompt-submit"
        };
        let t = Instant::now();
        let out = hook(dir.path(), event, &payload, me);
        samples.push(t.elapsed());
        assert!(out.status.success());
    }
    samples.sort();
    let p95 = samples[samples.len() * 95 / 100];
    eprintln!(
        "hook p50={:?} p95={:?} max={:?}",
        samples[100], p95, samples[199]
    );
    assert!(p95.as_millis() < 30, "p95 {p95:?} over 30ms budget");
}

fn p95(mut samples: Vec<std::time::Duration>) -> std::time::Duration {
    samples.sort();
    samples[samples.len() * 95 / 100]
}

/// Time one hook call.
fn timed(dir: &std::path::Path, event: &str, payload: &str) -> std::time::Duration {
    let t = Instant::now();
    let out = hook(dir, event, payload, Some(std::process::id()));
    let took = t.elapsed();
    assert!(out.status.success());
    took
}

fn permission_prompt() -> String {
    serde_json::json!({ "session_id": "P", "hook_event_name": "Notification",
        "message": "Claude needs your permission", "notification_type": "permission_prompt" })
    .to_string()
}

/// PostToolUse runs after every tool call. Usually there is no mark to
/// clear (read only); after a permission prompt there is (one write).
#[test]
#[ignore]
fn post_tool_use() {
    let dir = tempfile::tempdir().unwrap();
    timed(
        dir.path(),
        "session-start",
        &start_json("P", "startup", Some("perf")),
    );
    let payload = serde_json::json!({ "session_id": "P", "hook_event_name": "PostToolUse",
        "tool_name": "Bash", "tool_input": { "command": "ls" },
        "tool_response": { "stdout": "x".repeat(4000) } })
    .to_string();
    let idle: Vec<_> = (0..200)
        .map(|_| timed(dir.path(), "post-tool-use", &payload))
        .collect();
    let clearing: Vec<_> = (0..100)
        .map(|_| {
            timed(dir.path(), "notification", &permission_prompt());
            timed(dir.path(), "post-tool-use", &payload)
        })
        .collect();
    let (idle, clearing) = (p95(idle), p95(clearing));
    eprintln!("post-tool-use p95: nothing to clear={idle:?} clearing={clearing:?}");
    assert!(idle.as_millis() < 30, "p95 {idle:?} over 30ms budget");
    assert!(
        clearing.as_millis() < 30,
        "p95 {clearing:?} over 30ms budget"
    );
}

#[test]
#[ignore]
fn notification() {
    let dir = tempfile::tempdir().unwrap();
    timed(
        dir.path(),
        "session-start",
        &start_json("P", "startup", Some("perf")),
    );
    let samples: Vec<_> = (0..200)
        .map(|_| timed(dir.path(), "notification", &permission_prompt()))
        .collect();
    let p = p95(samples);
    eprintln!("notification p95={p:?}");
    assert!(p.as_millis() < 30, "p95 {p:?} over 30ms budget");
}

#[test]
#[ignore]
fn hook_p95_in_tmux_under_30ms() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let sock = format!("cc-moss-perf-{}", std::process::id());
    let tmux = |args: &[&str]| {
        let out = Command::new("tmux")
            .args(["-L", &sock])
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tmux(&["new-session", "-d", "-s", "host_perf", "sleep", "120"]);
    let socket_path = tmux(&["display-message", "-p", "#{socket_path}"]);
    let pane = tmux(&["display-message", "-p", "-t", "host_perf", "#{pane_id}"]);

    let dir = tempfile::tempdir().unwrap();
    let run = |event: &str, payload: String| {
        let t = Instant::now();
        let mut child = common::bin()
            .env("CC_MOSS_STATE_DIR", dir.path())
            .env("CC_MOSS_HOST_PREFIX", "host")
            .env("CLAUDE_PID", std::process::id().to_string())
            .env("TMUX", format!("{socket_path},1,0"))
            .env("TMUX_PANE", &pane)
            .args(["hook", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        t.elapsed()
    };
    run("session-start", start_json("P", "startup", Some("perf")));
    let mut samples: Vec<_> = (0..200)
        .map(|_| {
            run(
                "user-prompt-submit",
                serde_json::json!({ "session_id": "P", "session_title": "perf" }).to_string(),
            )
        })
        .collect();
    tmux(&["kill-server"]);
    let _ = std::fs::remove_file(&socket_path);
    samples.sort();
    let p95 = samples[samples.len() * 95 / 100];
    eprintln!("in-tmux hook p50={:?} p95={:?}", samples[100], p95);
    assert!(p95.as_millis() < 30, "p95 {p95:?} over 30ms budget");
}
