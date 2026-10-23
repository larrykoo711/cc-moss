//! Duplicate-restore and wrong-target cases from the T1.4 independent review
//! (reports/T1.4-review-1.md). Each test is the reviewer's reproduction with
//! the assertion turned around.

mod common;

use std::io::Write;
use std::process::{Child, Command, Stdio};

use common::{fixture, hook, ls_json, wait_for, FakeProcess, World};

struct SilentProcess(Child);

impl SilentProcess {
    fn spawn(w: &World, sid: &str) -> Self {
        let mut cmd = Command::new(&w.claude);
        for (key, value) in w.cc_moss().get_envs() {
            match value {
                Some(value) => {
                    cmd.env(key, value);
                }
                None => {
                    cmd.env_remove(key);
                }
            }
        }
        let child = cmd
            .args(["--session-id", sid])
            .current_dir(w.path("work"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // 等到已声明的脚本入口可读，不能用 spawn 后仍处于 exec 前的 argv。
        wait_for(|| {
            moss_core::launch_args::exact_argv(child.id())
                .filter(|args| args.iter().any(|arg| arg == sid))
        });
        Self(child)
    }

    fn pid(&self) -> Option<u32> {
        Some(self.0.id())
    }
}

impl Drop for SilentProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(w: &World, sid: &str, source: &str, title: Option<&str>, pid: Option<u32>) {
    let t = w.path("fifos").join(format!("{sid}.jsonl"));
    std::fs::write(&t, "{}\n").unwrap();
    let j = serde_json::json!({
        "session_id": sid, "cwd": w.path("work"), "source": source,
        "session_title": title, "transcript_path": t,
    })
    .to_string();
    let mut cmd = w.cc_moss();
    cmd.args(["hook", "session-start"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(pid) = pid {
        cmd.env("CLAUDE_PID", pid.to_string());
    }
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(j.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
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
    let p1 = SilentProcess::spawn(&w, "aaaaaaaa-1111");
    let p2 = SilentProcess::spawn(&w, "aaaaaaaa-1111");
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
    let p = SilentProcess::spawn(&w, "cccccccc-3333");
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
    let p = SilentProcess::spawn(&w, "dddddddd-4444");
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

/// AMEND-5: the hidden exact route pins slot and current UUID before any
/// name lookup; a slot whose name is the target UUID is never claimed.
#[test]
fn exact_slot_restore_ignores_a_name_equal_to_the_target_uuid() {
    const TARGET: &str = "aaaaaaaa-5555-4555-8555-555555555555";
    const SHADOW: &str = "bbbbbbbb-5555-4555-8555-555555555555";
    let mut w = World::new("exact");
    w.claude = fixture("silent-claude");
    let p1 = SilentProcess::spawn(&w, TARGET);
    let p2 = SilentProcess::spawn(&w, SHADOW);
    start(&w, TARGET, "startup", Some("original"), p1.pid());
    start(&w, SHADOW, "startup", Some(TARGET), p2.pid());
    drop((p1, p2));
    let listed = ls_json(&w.state(), true);
    let slot_of = |sid: &str| {
        listed["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["session_id"] == sid)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (target_slot, shadow_slot) = (slot_of(TARGET), slot_of(SHADOW));
    // 普通文本 restore 的名称优先级不变：完整 UUID 先按显示名命中遮蔽会话。
    let t = dry(&w, &[TARGET]);
    assert!(t.contains("would restore 1"), "{t}");
    assert!(t.contains(TARGET) && !t.contains("original"), "{t}");

    // 仅测试库的观测触发器：记录任何恢复 claim，不能事后清空伪造零副作用。
    let probe = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    probe
        .execute_batch(
            "CREATE TABLE exact_claim_probe(slot_id TEXT NOT NULL);
            CREATE TRIGGER exact_claim_probe AFTER UPDATE OF restore_started_at ON slots
            WHEN NEW.restore_started_at IS NOT NULL AND OLD.restore_started_at IS NULL
            BEGIN INSERT INTO exact_claim_probe VALUES(NEW.slot_id); END;",
        )
        .unwrap();
    let claims = || -> Vec<String> {
        probe
            .prepare("SELECT slot_id FROM exact_claim_probe")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let exact = |slot: &str, sid: &str| {
        w.cc_moss()
            .args([
                "restore",
                "--exact-slot-id",
                slot,
                "--yes",
                "--json",
                "--",
                sid,
            ])
            .output()
            .unwrap()
    };

    // 错配：遮蔽 slot + 目标 UUID，不能回退到显示名或另一 slot。
    let out = exact(&shadow_slot, TARGET);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["results"][0]["ok"], false, "{v}");
    assert_eq!(v["results"][0]["reason"], "not_found", "{v}");
    // 不完整 UUID 直接拒绝。
    let out = exact(&target_slot, "aaaaaaaa");
    assert!(!out.status.success(), "{out:?}");
    assert!(claims().is_empty(), "{:?}", claims());
    assert_eq!(resumes(&w, TARGET) + resumes(&w, SHADOW), 0);

    // 匹配：只恢复选中 slot 的原 UUID，遮蔽会话零 claim/launch。
    let out = exact(&target_slot, TARGET);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["results"][0]["id"], target_slot.as_str(), "{v}");
    assert_eq!(v["results"][0]["ok"], true, "{v}");
    assert!(
        matches!(
            v["results"][0]["result"].as_str(),
            Some("restored" | "starting")
        ),
        "{v}"
    );
    assert_eq!(claims(), std::slice::from_ref(&target_slot));
    assert_eq!(resumes(&w, TARGET), 1);
    assert_eq!(resumes(&w, SHADOW), 0);
}

/// AMEND-6: the app learns of the exact route only from a positive capability.
#[test]
fn ls_advertises_exact_restore_capability() {
    let w = World::new("exactcap");
    w.new_session("cap");
    let v = ls_json(&w.state(), true);
    assert_eq!(v["capabilities"]["exact_restore"], 1, "{v}");
}
