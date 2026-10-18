//! p95 of each observing hook, to compare two builds on the same machine.
//! Run on demand:
//! `cargo test -p cc-moss --release --test hook_latency -- --ignored --nocapture`
//!
//! With `CC_MOSS_BASELINE_BIN=<an older cc-moss>` the two binaries run
//! alternately, sample by sample, each against its own state dir, so a busy
//! machine slows both alike; the test then fails when this build's p95 is
//! 5 ms or more above the baseline's for any event.

mod common;

use common::{end_json, start_json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

const SAMPLES: usize = 200;

/// An event, its payload, and a hook to run untimed before each sample.
type Case<'a> = (&'a str, &'a str, Option<(&'a str, &'a str)>);

fn timed(exe: &Path, state: &Path, event: &str, payload: &str) -> Duration {
    // The same isolated environment `common::bin` sets, on either binary.
    let template = common::bin();
    let mut child = std::process::Command::new(exe);
    for (k, v) in template.get_envs() {
        match v {
            Some(v) => child.env(k, v),
            None => child.env_remove(k),
        };
    }
    let t = Instant::now();
    let mut proc = child
        .env("CC_MOSS_STATE_DIR", state)
        .env("CLAUDE_PID", std::process::id().to_string())
        .args(["hook", event])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _ = proc.stdin.take().unwrap().write_all(payload.as_bytes());
    assert!(proc.wait().unwrap().success());
    t.elapsed()
}

fn p(samples: &mut [Duration], pct: usize) -> Duration {
    samples.sort();
    samples[samples.len() * pct / 100]
}

struct Build {
    label: &'static str,
    exe: PathBuf,
    state: tempfile::TempDir,
}

#[test]
#[ignore]
fn observing_hooks_p95() {
    let mut builds = vec![Build {
        label: "this",
        exe: PathBuf::from(env!("CARGO_BIN_EXE_cc-moss")),
        state: tempfile::tempdir().unwrap(),
    }];
    if let Some(base) = std::env::var_os("CC_MOSS_BASELINE_BIN").filter(|b| !b.is_empty()) {
        builds.push(Build {
            label: "baseline",
            exe: PathBuf::from(base),
            state: tempfile::tempdir().unwrap(),
        });
    }
    let start = start_json("P", "resume", Some("perf"));
    let prompt = serde_json::json!({ "session_id": "P", "session_title": "perf" }).to_string();
    let stop = serde_json::json!({ "session_id": "P", "hook_event_name": "Stop" }).to_string();
    let note = serde_json::json!({ "session_id": "P", "hook_event_name": "Notification",
        "message": "Claude needs your permission", "notification_type": "permission_prompt" })
    .to_string();
    let tool = serde_json::json!({ "session_id": "P", "hook_event_name": "PostToolUse",
        "tool_name": "Bash", "tool_input": { "command": "ls" },
        "tool_response": { "stdout": "x".repeat(4000) } })
    .to_string();
    let end = end_json("P", "other");
    for b in &builds {
        timed(&b.exe, b.state.path(), "session-start", &start);
    }

    // (event, payload, untimed setup before each sample)
    let cases: [Case; 6] = [
        ("session-start", &start, None),
        ("user-prompt-submit", &prompt, None),
        ("stop", &stop, None),
        ("notification", &note, None),
        ("post-tool-use", &tool, None),
        ("session-end", &end, Some(("session-start", &start))),
    ];
    let mut worst_gap = Duration::ZERO;
    for (event, payload, setup) in cases {
        let mut samples: Vec<Vec<Duration>> = vec![Vec::new(); builds.len()];
        for _ in 0..SAMPLES {
            for (i, b) in builds.iter().enumerate() {
                if let Some((e, p)) = setup {
                    timed(&b.exe, b.state.path(), e, p);
                }
                samples[i].push(timed(&b.exe, b.state.path(), event, payload));
            }
        }
        let p95: Vec<Duration> = samples.iter_mut().map(|s| p(s, 95)).collect();
        let line: Vec<String> = builds
            .iter()
            .zip(samples.iter_mut())
            .zip(&p95)
            .map(|((b, s), p95)| format!("{} p50={:?} p95={p95:?}", b.label, p(s, 50)))
            .collect();
        eprintln!("latency {event:<20} {}", line.join("  "));
        if let [this, base] = p95[..] {
            worst_gap = worst_gap.max(this.saturating_sub(base));
        }
    }
    assert!(
        worst_gap < Duration::from_millis(5),
        "p95 is {worst_gap:?} above the baseline"
    );
}
