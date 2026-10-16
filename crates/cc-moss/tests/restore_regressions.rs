//! Duplicate-restore and wrong-target cases from the T1.4 independent review
//! (reports/T1.4-review-1.md). Each test is the reviewer's reproduction with
//! the assertion turned around.

mod common;

use std::process::Command;

use common::{fixture, hook, ls_json, wait_for, FakeProcess, World};

fn start(w: &World, sid: &str, source: &str, title: Option<&str>, pid: Option<u32>) {
    let t = w.path("fifos").join(format!("{sid}.jsonl"));
    std::fs::write(&t, "{}\n").unwrap();
    let j = serde_json::json!({
        "session_id": sid, "cwd": w.path("work"), "source": source,
        "session_title": title, "transcript_path": t,
    })
    .to_string();
    assert!(hook(&w.state(), "session-start", &j, pid).status.success());
}

fn dry(w: &World, extra: &[&str]) -> String {
    let out = w
        .cc_moss()
        .args(["restore", "--dry-run"])
        .args(extra)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn resumes(w: &World, sid: &str) -> usize {
    w.tmux(&["list-panes", "-a", "-F", "#{pane_start_command}"])
        .matches(&format!("--resume {sid}"))
        .count()
}

/// C1: two slots share a conversation id.
#[test]
fn shared_id_is_restored_at_most_once() {
    let mut w = World::new("shared");
    w.claude = fixture("silent-claude");
    let p1 = FakeProcess::spawn();
    let p2 = FakeProcess::spawn();
    start(&w, "aaaaaaaa-1111", "startup", Some("api"), p1.pid());
    start(&w, "aaaaaaaa-1111", "resume", Some("api"), p2.pid());
    drop(p2);
    // The conversation is still open in p1: nothing to restore.
    let t = dry(&w, &[]);
    assert!(!t.contains("would restore"), "{t}");
    drop(p1);
    assert!(w
        .cc_moss()
        .args(["restore", "--yes"])
        .status()
        .unwrap()
        .success());
    assert_eq!(resumes(&w, "aaaaaaaa-1111"), 1);
}

/// I1: SessionEnd(other) from a process that kept running.
#[test]
fn live_process_is_never_offered() {
    let w = World::new("endother");
    let p = FakeProcess::spawn();
    start(&w, "bbbbbbbb-2222", "startup", Some("live"), p.pid());
    let end = r#"{"session_id":"bbbbbbbb-2222","reason":"other"}"#;
    assert!(hook(&w.state(), "session-end", end, p.pid())
        .status
        .success());
    let t = dry(&w, &[]);
    assert!(!t.contains("would restore"), "{t}");
}

/// C2: the restored process is tracked by pid, not by a timer.
#[test]
fn restored_process_outlives_the_pending_window() {
    let mut w = World::new("expire");
    w.claude = fixture("silent-claude");
    let p = FakeProcess::spawn();
    start(&w, "cccccccc-3333", "startup", Some("exp"), p.pid());
    drop(p);
    assert!(w
        .cc_moss()
        .args(["restore", "--yes"])
        .status()
        .unwrap()
        .success());
    // 61 s later the restored claude is still alive and never reported in.
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET restore_started_at = restore_started_at - 61000",
        [],
    )
    .unwrap();
    assert!(w
        .cc_moss()
        .args(["restore", "--yes"])
        .status()
        .unwrap()
        .success());
    assert_eq!(resumes(&w, "cccccccc-3333"), 1);
}

/// C2: a slow `tmux new-session` must not be reported as a failure.
#[test]
fn slow_tmux_is_not_a_failure() {
    let mut w = World::new("slowtmux");
    w.claude = fixture("silent-claude");
    let p = FakeProcess::spawn();
    start(&w, "dddddddd-4444", "startup", Some("slow"), p.pid());
    drop(p);
    let shim = w.path("shim");
    std::fs::create_dir_all(&shim).unwrap();
    let real = String::from_utf8(Command::new("which").arg("tmux").output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_string();
    std::fs::write(
        shim.join("tmux"),
        format!("#!/bin/sh\n\"{real}\" \"$@\"; rc=$?\ncase \" $* \" in *\" new-session \"*) sleep 1;; esac\nexit $rc\n"),
    )
    .unwrap();
    Command::new("chmod")
        .arg("+x")
        .arg(shim.join("tmux"))
        .status()
        .unwrap();
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    let first = w
        .cc_moss()
        .env("PATH", &path)
        .args(["restore", "--yes"])
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    assert!(w
        .cc_moss()
        .args(["restore", "--yes"])
        .status()
        .unwrap()
        .success());
    assert_eq!(resumes(&w, "dddddddd-4444"), 1);
}

/// I4: selecting by id prefix restores exactly that slot, named or not.
#[test]
fn name_arguments_select_exact_slots() {
    let w = World::new("filter");
    let p1 = FakeProcess::spawn();
    let p2 = FakeProcess::spawn();
    let p3 = FakeProcess::spawn();
    start(&w, "eeeeeeee-0001", "startup", Some("api"), p1.pid());
    start(&w, "ffffffff-0002", "startup", Some("api"), p2.pid());
    start(&w, "99999999-0003", "startup", None, p3.pid());
    drop((p1, p2, p3));
    let t = dry(&w, &["ffffffff"]);
    assert!(t.contains("would restore 1"), "{t}");
    let t = dry(&w, &["99999999"]);
    assert!(t.contains("would restore 1"), "{t}");
}

/// I6: attach never follows a stale tmux id to another session.
#[test]
fn attach_refuses_a_session_that_is_not_running() {
    let w = World::new("attach");
    w.new_session("aa");
    w.new_session("bb");
    for n in ["aa", "bb"] {
        let id = w.session_id(n);
        w.send(&id, &format!("prompt {n}"));
    }
    wait_for(|| {
        let v = ls_json(&w.state(), true);
        let ok = v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| !s["tmux"]["session_id"].is_null());
        ok.then_some(())
    });
    w.tmux(&["kill-server"]);
    wait_for(|| {
        let v = ls_json(&w.state(), true);
        let n = v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["status"] == "interrupted")
            .count();
        (n == 2).then_some(())
    });
    assert!(w
        .cc_moss()
        .args(["restore", "--yes", "bb"])
        .status()
        .unwrap()
        .success());
    let out = w.cc_moss().args(["attach", "aa"]).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not running"),
        "{out:?}"
    );
}

/// I5 / M1: directories with `#` and names ending in `;` reach claude intact.
#[test]
fn tmux_special_characters_are_escaped() {
    let w = World::new("escape");
    let dir = w.path("C#");
    std::fs::create_dir_all(&dir).unwrap();
    let out = w
        .cc_moss()
        .args(["new", "wip;", "--detach", "-C"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let id = w.session_id("wip;");
    let slot = &ls_json(&w.state(), false)["sessions"][0];
    assert!(slot["cwd"].as_str().unwrap().ends_with("/C#"), "{slot}");
    let _ = id;
}
