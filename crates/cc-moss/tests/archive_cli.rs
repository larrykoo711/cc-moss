//! The archive (T2.5): `archive`, `unarchive`, what `ls` shows, the
//! daemon's daily tidy-up and a resume taking a session back out.

mod common;

use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{bin, dead_pid, end_json, hook, start_json, FakeProcess};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn session_of(w: &common::World, name: &str) -> serde_json::Value {
    common::ls_json(&w.state(), true)["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no session {name}"))
}

fn archive_end(w: &common::World, name: &str) -> (serde_json::Value, bool) {
    let out = w
        .cc_moss()
        .args(["archive", name, "--end", "--json"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    (v, out.status.success())
}

fn alive(pid: u64) -> bool {
    // SAFETY: signal 0 only checks.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[test]
fn restore_all_skips_a_session_ended_on_purpose() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path();
    let p = FakeProcess::spawn();
    hook(
        state,
        "session-start",
        &start_json("done1", "startup", Some("done1")),
        p.pid(),
    );
    hook(
        state,
        "session-end",
        &end_json("done1", "prompt_input_exit"),
        p.pid(),
    );
    drop(p);
    assert!(cc_moss(state, &["archive", "done1"]).status.success());
    assert!(cc_moss(state, &["unarchive", "done1"]).status.success());
    // Back in the list, ready to pick up...
    assert!(ls_text(state, &[]).contains("done1"));
    // ...but restoring everything leaves out what the user ended.
    let all = cc_moss(state, &["restore", "--dry-run"]);
    let all = String::from_utf8_lossy(&all.stdout);
    assert!(!all.contains("done1"), "{all}");
    let named = cc_moss(state, &["restore", "--dry-run", "done1"]);
    let named = String::from_utf8_lossy(&named.stdout);
    assert!(named.contains("done1"), "{named}");
}

#[test]
fn archive_end_ends_an_idle_session() {
    let w = common::World::new("aee");
    w.new_session("api");
    w.session_id("api");
    let before = session_of(&w, "api");
    let pid = before["pid"].as_u64().unwrap();
    let tmux_name = before["tmux"]["session_name"].as_str().unwrap().to_string();
    // Without --end, a running session is refused as before.
    let out = w
        .cc_moss()
        .args(["archive", "api", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"][0]["reason"], "running");

    let (v, ok) = archive_end(&w, "api");
    assert!(ok, "{v}");
    assert_eq!(v["results"][0]["result"], "archived", "{v}");
    let until = Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(Instant::now() < until, "its process is still there");
        std::thread::sleep(Duration::from_millis(100));
    }
    let after = session_of(&w, "api");
    assert_eq!(after["status"], "closed", "{after}");
    assert!(after["archived_at"].as_str().is_some(), "{after}");
    // claude was the pane's program: its tmux session went with it.
    let until = Instant::now() + Duration::from_secs(10);
    while w.session_names().contains(&tmux_name) {
        assert!(Instant::now() < until, "its tmux session stayed");
        std::thread::sleep(Duration::from_millis(100));
    }
    let events = w.cc_moss().args(["events", "--json"]).output().unwrap();
    let events: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    let kinds: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(!kinds.contains(&"interrupted"), "{kinds:?}");
    assert!(kinds.contains(&"archived"), "{kinds:?}");
}

#[test]
fn archive_end_refuses_a_busy_session() {
    let w = common::World::new("aeb");
    w.new_session("api");
    let sid = w.session_id("api");
    w.first_prompt(&sid, "api");
    let until = Instant::now() + Duration::from_secs(10);
    while session_of(&w, "api")["busy"] != true {
        assert!(Instant::now() < until, "never busy");
        std::thread::sleep(Duration::from_millis(100));
    }
    let pid = session_of(&w, "api")["pid"].as_u64().unwrap();
    let (v, ok) = archive_end(&w, "api");
    assert!(!ok, "{v}");
    assert_eq!(v["results"][0]["result"], "refused", "{v}");
    assert_eq!(v["results"][0]["reason"], "busy", "{v}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(alive(pid), "a busy session keeps running");
    let after = session_of(&w, "api");
    assert_eq!(after["status"], "running");
    assert!(after["archived_at"].is_null());
}

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
fn an_ended_session_brought_back_shows_in_the_list() {
    // End and archive, then bring back: it must be in the list again,
    // ready to pick up, not hidden as ended (user 2026-10-20).
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path();
    let gone = Some(dead_pid());
    hook(
        state,
        "session-start",
        &start_json("api", "startup", Some("api")),
        gone,
    );
    hook(
        state,
        "session-end",
        &end_json("api", "prompt_input_exit"),
        gone,
    );
    assert!(cc_moss(state, &["archive", "api"]).status.success());
    assert!(cc_moss(state, &["unarchive", "api"]).status.success());
    let v = ls(state, &["--json"]);
    assert_eq!(names(&v), ["api"], "{v}");
    assert_eq!(v["sessions"][0]["status"], "interrupted");
    // No "dropped" news for it.
    let events = cc_moss(state, &["events", "--json"]);
    let events: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    assert!(!events.to_string().contains("\"interrupted\""), "{events}");
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
