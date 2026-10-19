//! The archive (T2.5): `archive`, `unarchive`, what `ls` shows, the
//! daemon's daily tidy-up and a resume taking a session back out.

mod common;

use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{bin, dead_pid, end_json, hook, start_json, FakeProcess};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn ls(state: &Path, args: &[&str]) -> serde_json::Value {
    let out = bin()
        .env("CC_MOSS_STATE_DIR", state)
        .arg("ls")
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

fn names(v: &serde_json::Value) -> Vec<String> {
    let mut n: Vec<String> = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap_or("").to_string())
        .collect();
    n.sort();
    n
}

fn ls_text(state: &Path, args: &[&str]) -> String {
    let out = bin()
        .env("CC_MOSS_STATE_DIR", state)
        .arg("ls")
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn cc_moss(state: &Path, args: &[&str]) -> std::process::Output {
    bin()
        .env("CC_MOSS_STATE_DIR", state)
        .args(args)
        .output()
        .unwrap()
}

/// A session that ended (its process is gone).
fn ended(state: &Path, sid: &str) {
    hook(
        state,
        "session-start",
        &start_json(sid, "startup", Some(sid)),
        Some(dead_pid()),
    );
}

/// Make a session look last used `days` ago.
fn age(state: &Path, sid: &str, days: i64) {
    let db = rusqlite::Connection::open(state.join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET last_active_at = ?2, created_at = ?2 WHERE session_id = ?1",
        rusqlite::params![sid, now_ms() - days * DAY_MS],
    )
    .unwrap();
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn archive_after_idle_days() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    // (A running session is never archived: `archive_skips_running_sessions`.
    // One can't be aged here: a pid last seen before the machine started
    // reads as dropped.)
    ended(&state, "old");
    ended(&state, "recent");
    age(&state, "old", 31);
    age(&state, "recent", 29);
    // The daemon's tidy-up runs as it starts, then once a day.
    let child = bin()
        .env("CC_MOSS_STATE_DIR", &state)
        .env("CC_MOSS_CONFIG", dir.path().join("config.toml"))
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _daemon = Daemon(child);
    let until = Instant::now() + Duration::from_secs(30);
    while names(&ls(&state, &["--json", "--archived"])) != ["old"] {
        assert!(
            Instant::now() < until,
            "old was not archived: {}\n{:?}",
            ls(&state, &["--json", "--all"]),
            std::fs::read_to_string(state.join("daemon/daemon.log"))
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = ls_text(&state, &[]);
    assert!(!text.contains("old"), "{text}");
    assert!(text.contains("recent"), "{text}");
    assert!(ls_text(&state, &["--archived"]).contains("old"));
    let all = ls(&state, &["--json"]);
    let old = all["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "old")
        .expect("ls --json lists archived sessions too");
    assert!(old["archived_at"].as_str().unwrap().ends_with('Z'));
    let events = cc_moss(&state, &["events", "--json"]);
    let events: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    let archived = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "archived")
        .expect("an archived event");
    assert_eq!(archived["slot_id"], "");
    assert_eq!(archived["detail"]["count"], 1);
    assert_eq!(archived["detail"]["by"], "auto");
}

#[test]
fn archive_by_hand_and_bring_back() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path();
    let alive = FakeProcess::spawn();
    ended(state, "api");
    hook(
        state,
        "session-start",
        &start_json("web", "startup", Some("web")),
        alive.pid(),
    );
    // Running: refused, with a reason the app can read.
    let out = cc_moss(state, &["archive", "web", "--json"]);
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["results"][0]["result"], "refused");
    assert_eq!(v["results"][0]["reason"], "running");
    let out = cc_moss(state, &["archive", "api", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"][0]["result"], "archived");
    assert_eq!(v["results"][0]["session_id"], "api");
    assert_eq!(names(&ls(state, &["--json", "--archived"])), ["api"]);
    assert!(!ls_text(state, &[]).contains("api"));
    let out = cc_moss(state, &["unarchive", "api", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"][0]["result"], "unarchived");
    assert!(ls_text(state, &[]).contains("api"));
    let all = ls(state, &["--json"]);
    assert!(all["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["archived_at"].is_null()));
}

#[test]
fn ls_json_lists_archived_closed_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path();
    let p = FakeProcess::spawn();
    for sid in ["kept", "gone"] {
        hook(
            state,
            "session-start",
            &start_json(sid, "startup", Some(sid)),
            p.pid(),
        );
        hook(
            state,
            "session-end",
            &end_json(sid, "prompt_input_exit"),
            p.pid(),
        );
    }
    assert!(cc_moss(state, &["archive", "gone"]).status.success());
    // Closed and archived: in `ls --json` for the app; closed and in the
    // list: only with --all.
    assert_eq!(names(&ls(state, &["--json"])), ["gone"]);
    assert_eq!(names(&ls(state, &["--json", "--all"])), ["gone", "kept"]);
}

#[test]
fn resumed_session_leaves_archive() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path();
    ended(state, "api");
    assert!(cc_moss(state, &["archive", "api"]).status.success());
    assert_eq!(names(&ls(state, &["--json", "--archived"])), ["api"]);
    // `claude --resume api` somewhere.
    let p = FakeProcess::spawn();
    hook(
        state,
        "session-start",
        &start_json("api", "resume", None),
        p.pid(),
    );
    assert!(names(&ls(state, &["--json", "--archived"])).is_empty());
    let text = ls_text(state, &[]);
    assert!(text.contains("api"), "{text}");
    let events = cc_moss(state, &["events", "--json"]);
    let events: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    let back = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "unarchived")
        .expect("an unarchived event");
    assert_eq!(back["detail"]["by"], "resume");
}

#[test]
fn archive_after_days_is_a_setting() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let run = |args: &[&str]| {
        bin()
            .env("CC_MOSS_CONFIG", &config)
            .args(args)
            .output()
            .unwrap()
    };
    let out = run(&["config", "get", "sessions.archive_after_days"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "30");
    assert!(run(&["config", "set", "sessions.archive_after_days", "0"])
        .status
        .success());
    let out = run(&["config", "get", "sessions.archive_after_days"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0");
    assert!(
        !run(&["config", "set", "sessions.archive_after_days", "soon"])
            .status
            .success()
    );
}
