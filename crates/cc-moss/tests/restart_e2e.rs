//! `cc-moss restart` (T2.6): a session starts again in its own tmux pane
//! after an API provider switch. Fake Claude Code on a test-only tmux
//! socket; state, config and HOME are temporary.

mod common;

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{ls_json, World};

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn session(w: &World, name: &str) -> serde_json::Value {
    ls_json(&w.state(), true)["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no session {name}"))
}

fn wait_until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn restart(w: &World, args: &[&str]) -> (serde_json::Value, bool) {
    let out = w
        .cc_moss()
        .arg("restart")
        .args(args)
        .arg("--json")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    (v, out.status.success())
}

fn shell_exports(w: &World) -> String {
    format!(
        "unset CC_MOSS_TEST_CLAUDE_ENTRY; export HOME='{}' CLAUDE_CONFIG_DIR='{}' CC_MOSS_CONFIG='{}' CC_MOSS_CLAUDE_BIN='{}' CC_MOSS_STATE_DIR='{}' CC_MOSS_BIN='{}' CC_MOSS_FAKE_DIR='{}' CC_MOSS_LANG=en",
        w.path("home").display(), w.path("claude").display(), w.config_path().display(), w.claude.display(),
        w.state().display(), env!("CARGO_BIN_EXE_cc-moss"), w.path("fifos").display()
    )
}

fn args_of(pid: u64) -> String {
    let out = Command::new("ps")
        .args(["-o", "args=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn alive(pid: u64) -> bool {
    // SAFETY: signal 0 only checks.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// The new process once the session runs again under another pid.
fn restarted(w: &World, name: &str, old_pid: u64) -> serde_json::Value {
    wait_until("the session to run again", || {
        let s = session(w, name);
        let pid = s["pid"].as_u64()?;
        (s["status"] == "running" && pid != old_pid && alive(pid)).then_some(s)
    })
}

fn launch_snapshot(w: &World, sid: &str) -> serde_json::Value {
    wait_until("captured launch source", || {
        let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
        let value: Option<String> = db
            .query_row(
                "SELECT provider_started FROM slots WHERE session_id=?1",
                [sid],
                |r| r.get(0),
            )
            .unwrap();
        value.map(|v| serde_json::from_str(&v).unwrap())
    })
}

#[test]
fn launch_source_restart_preserves_environment_under_tmux_pollution() {
    for (tag, source, provider_default) in [
        ("ls-unset", None, false),
        ("ls-default", Some("home/.claude"), false),
        ("ls-custom", Some("custom"), false),
        ("ls-model", None, true),
    ] {
        let w = World::new(tag);
        std::fs::create_dir_all(w.path("home/.claude")).unwrap();
        std::fs::write(
            w.path("home/.claude/settings.json"),
            r#"{"env":{"ANTHROPIC_MODEL":"route-b"}}"#,
        )
        .unwrap();
        let mut cmd = w.cc_moss();
        cmd.env("HOME", w.path("home"));
        if let Some(source) = source {
            cmd.env("CLAUDE_CONFIG_DIR", w.path(source));
        }
        let out = cmd
            .args(["new", "api", "--detach", "-C"])
            .arg(w.path("work"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let sid = w.session_id("api");
        let snapshot = launch_snapshot(&w, &sid);
        let expected = source
            .map(|s| format!("explicit\n{}", w.path(s).display()))
            .unwrap_or_else(|| "unset".into());
        if source.is_none() {
            assert_eq!(snapshot["launch_source"], "unset");
        }
        let old = session(&w, "api")["pid"].as_u64().unwrap();
        w.tmux(&[
            "set-environment",
            "-g",
            "CLAUDE_CONFIG_DIR",
            w.path("polluted").to_str().unwrap(),
        ]);
        let args = if provider_default {
            vec!["--provider-default", "api"]
        } else {
            vec!["api"]
        };
        let (result, ok) = restart(&w, &args);
        assert!(ok, "{result}");
        restarted(&w, "api", old);
        let actual =
            std::fs::read_to_string(w.path(&format!("fifos/{sid}.config-source"))).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn launch_source_unknown_and_cli_markers_refuse_before_restart() {
    for (tag, replacement) in [
        ("ls-unknown", serde_json::Value::Null),
        ("ls-cli", serde_json::json!(["--mcp-config"])),
    ] {
        let w = World::new(tag);
        w.new_session("api");
        let sid = w.session_id("api");
        let mut snapshot = launch_snapshot(&w, &sid);
        if replacement.is_null() {
            snapshot.as_object_mut().unwrap().remove("launch_source");
        } else {
            snapshot["unreplayable_args"] = replacement;
        }
        let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
        db.execute(
            "UPDATE slots SET provider_started=?1 WHERE session_id=?2",
            [snapshot.to_string(), sid],
        )
        .unwrap();
        let old = session(&w, "api")["pid"].as_u64().unwrap();
        let (result, ok) = restart(&w, &["api"]);
        assert!(!ok, "{result}");
        assert!(alive(old));
        assert_eq!(session(&w, "api")["pid"], old);
        assert!(session(&w, "api")["restore_started_at"].is_null());
    }
}

#[test]
fn launch_source_busy_unknown_and_unsupported_refuse_before_queue() {
    for (tag, unsupported) in [("busy-source-unknown", false), ("busy-source-cli", true)] {
        let w = World::new(tag);
        w.new_session("api");
        let sid = w.session_id("api");
        let mut snapshot = launch_snapshot(&w, &sid);
        w.first_prompt(&sid, "api");
        wait_until("busy session", || {
            (session(&w, "api")["busy"] == true).then_some(())
        });
        let old = session(&w, "api")["pid"].as_u64().unwrap();
        let (queued, ok) = restart(&w, &["api"]);
        assert!(ok, "{queued}");
        assert_eq!(queued["results"][0]["result"], "queued");
        if unsupported {
            snapshot["unreplayable_args"] = serde_json::json!(["--mcp-config"]);
        } else {
            snapshot.as_object_mut().unwrap().remove("launch_source");
        }
        let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
        db.execute(
            "UPDATE slots SET provider_started=?1 WHERE session_id=?2",
            [snapshot.to_string(), sid],
        )
        .unwrap();
        let (cancelled, ok) = restart(&w, &["--cancel", "api"]);
        assert!(ok, "{cancelled}");
        assert_eq!(cancelled["results"][0]["result"], "cancelled");
        assert!(
            cancelled["results"][0]["reason"].is_null(),
            "已有队列确实被撤销"
        );
        let (result, ok) = restart(&w, &["api"]);
        assert!(!ok, "{result}");
        assert_eq!(result["results"][0]["result"], "refused");
        assert!(alive(old));
        assert_eq!(session(&w, "api")["restart_queued"], false);
        let queue = w.state().join("daemon/restart-queue.json");
        if queue.exists() {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(queue).unwrap())
                    .unwrap(),
                serde_json::json!([])
            );
        }
        let (cancelled, ok) = restart(&w, &["--cancel", "api"]);
        assert!(ok, "{cancelled}");
        assert_eq!(cancelled["results"][0]["result"], "cancelled");
    }
}

#[test]
fn launch_source_arity_hook_preserves_option_boundaries() {
    for (tag, args, blocked) in [
        (
            "arity-dash-value",
            vec![
                "--append-system-prompt",
                "--",
                "--mcp-config",
                "/tmp/mcp.json",
            ],
            true,
        ),
        (
            "arity-flag-value",
            vec!["--append-system-prompt", "--mcp-config"],
            false,
        ),
    ] {
        let w = World::new(tag);
        let out = w
            .cc_moss()
            .args(["new", "api", "--detach", "-C"])
            .arg(w.path("work"))
            .arg("--")
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let sid = w.session_id("api");
        let snapshot = launch_snapshot(&w, &sid);
        let old = session(&w, "api")["pid"].as_u64().unwrap();
        let (result, ok) = restart(&w, &["api"]);
        if blocked {
            assert_eq!(
                snapshot["unreplayable_args"],
                serde_json::json!(["--mcp-config"])
            );
            assert!(!ok, "{result}");
            assert!(alive(old));
        } else {
            assert!(snapshot["unreplayable_args"].is_null());
            assert!(ok, "{result}");
            restarted(&w, "api", old);
        }
    }
}

#[test]
fn launch_source_bad_settings_still_captures_source_and_cli_names() {
    let w = World::new("ls-bad");
    std::fs::create_dir_all(w.path("broken")).unwrap();
    std::fs::write(w.path("broken/settings.json"), "not-json").unwrap();
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("broken"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .args([
            "--",
            "--mcp-config={\"token\":\"fixture-secret\"}",
            "--strict-mcp-config",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let sid = w.session_id("api");
    let snapshot = launch_snapshot(&w, &sid);
    assert_eq!(
        snapshot["launch_source"]["explicit"],
        w.path("broken").to_str().unwrap()
    );
    assert_eq!(snapshot["settings_valid"], false);
    assert_eq!(
        snapshot["unreplayable_args"],
        serde_json::json!(["--mcp-config", "--strict-mcp-config"])
    );
    assert!(!snapshot.to_string().contains("fixture-secret"));
}

#[test]
fn restart_keeps_the_pane() {
    let w = World::new("rkp");
    w.new_session("api");
    let sid = w.session_id("api");
    let before = session(&w, "api");
    let pane = before["tmux"]["pane_id"].as_str().unwrap().to_string();
    let tmux_name = before["tmux"]["session_name"].as_str().unwrap().to_string();
    let old_pid = before["pid"].as_u64().unwrap();

    let (v, ok) = restart(&w, &["api"]);
    assert!(ok, "{v}");
    assert_eq!(v["schema"], 1);
    assert_eq!(v["results"][0]["result"], "restarted", "{v}");
    assert_eq!(v["results"][0]["session_id"], sid.as_str());

    let after = restarted(&w, "api", old_pid);
    assert_eq!(after["tmux"]["pane_id"], pane.as_str(), "same pane");
    assert_eq!(after["tmux"]["session_name"], tmux_name.as_str());
    assert_eq!(after["session_id"], sid.as_str(), "same conversation");
    assert_eq!(after["id"], before["id"], "same session record");
    let args = args_of(after["pid"].as_u64().unwrap());
    assert!(args.contains(&format!("--resume {sid}")), "{args}");
    assert!(args.contains("--name=api"), "{args}");
    assert!(args.contains("--dangerously-skip-permissions"), "{args}");
    // tmux kept the session and the pane, and its own option is put back.
    assert_eq!(
        w.tmux(&["display-message", "-p", "-t", &pane, "#{pane_id}"])
            .trim(),
        pane
    );
    assert_eq!(
        w.tmux(&[
            "show-options",
            "-p",
            "-q",
            "-v",
            "-t",
            &pane,
            "remain-on-exit"
        ])
        .trim(),
        ""
    );
    // No "dropped" news for a restart, and one `restarted`.
    let events = w.cc_moss().args(["events", "--json"]).output().unwrap();
    let events: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    let kinds: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(!kinds.contains(&"interrupted"), "{kinds:?}");
    assert!(kinds.contains(&"restarted"), "{kinds:?}");
}

#[test]
fn restart_waits_for_a_busy_session() {
    common::debug_only();
    let w = World::new("rwb");
    w.new_session("api");
    let sid = w.session_id("api");
    w.first_prompt(&sid, "api");
    wait_until("the session to be busy", || {
        (session(&w, "api")["busy"] == true).then_some(())
    });
    let before = session(&w, "api");
    let old_pid = before["pid"].as_u64().unwrap();
    let pane = before["tmux"]["pane_id"].as_str().unwrap().to_string();

    // Busy: queued, once, however often it is asked.
    for _ in 0..2 {
        let (v, ok) = restart(&w, &["api"]);
        assert!(ok, "{v}");
        assert_eq!(v["results"][0]["result"], "queued", "{v}");
        assert_eq!(v["results"][0]["reason"], "busy", "{v}");
    }
    let queue = std::fs::read_to_string(w.state().join("daemon/restart-queue.json")).unwrap();
    let queue: Vec<serde_json::Value> = serde_json::from_str(&queue).unwrap();
    assert_eq!(queue.len(), 1, "{queue:?}");
    assert_eq!(session(&w, "api")["restart_queued"], true);
    // --now without a yes, and nobody to ask: refused.
    let (v, ok) = restart(&w, &["api", "--now"]);
    assert!(!ok);
    assert_eq!(v["results"][0]["result"], "refused", "{v}");
    assert_eq!(v["results"][0]["reason"], "busy", "{v}");
    assert_eq!(session(&w, "api")["pid"].as_u64(), Some(old_pid));

    // The daemon waits for the turn to end.
    let _daemon = Daemon(
        w.cc_moss()
            .env("CC_MOSS_DAEMON_HEARTBEAT_MS", "500")
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        session(&w, "api")["pid"].as_u64(),
        Some(old_pid),
        "not while it works"
    );
    assert!(alive(old_pid));
    w.send(&sid, "stop");
    let after = restarted(&w, "api", old_pid);
    assert_eq!(after["tmux"]["pane_id"], pane.as_str());
    assert!(args_of(after["pid"].as_u64().unwrap()).contains(&format!("--resume {sid}")));
    wait_until("the queue to empty", || {
        (session(&w, "api")["restart_queued"] == false).then_some(())
    });
}

#[test]
fn restart_in_a_shell_pane() {
    let w = World::new("rsp");
    let work = w.path("work");
    w.tmux(&[
        "new-session",
        "-d",
        "-s",
        "shellpane",
        "-c",
        work.to_str().unwrap(),
        "bash",
        "--norc",
        "--noprofile",
    ]);
    let info = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        "shellpane",
        "#{pane_id} #{pane_pid}",
    ]);
    let (pane, shell_pid) = info.trim().split_once(' ').unwrap();
    let shell_pid: u64 = shell_pid.parse().unwrap();
    let sid = "11111111-2222-3333-4444-555555555555";
    let exports = shell_exports(&w);
    w.tmux(&["send-keys", "-t", pane, "-l", &exports]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let start = format!("{} --session-id {sid} --name=shellie", w.claude.display());
    w.tmux(&["send-keys", "-t", pane, "-l", &start]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let before = wait_until("the session under the shell", || {
        let all = ls_json(&w.state(), true);
        all["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["name"] == "shellie" && s["status"] == "running")
            .cloned()
    });
    assert_eq!(before["tmux"]["pane_id"], pane);
    let old_pid = before["pid"].as_u64().unwrap();
    assert_ne!(old_pid, shell_pid, "claude runs under the shell");

    let (v, ok) = restart(&w, &["shellie"]);
    assert!(ok, "{v}");
    assert_eq!(v["results"][0]["result"], "restarted", "{v}");
    let after = restarted(&w, "shellie", old_pid);
    assert_eq!(after["tmux"]["pane_id"], pane);
    assert!(alive(shell_pid), "the shell stays");
    let info = w.tmux(&["display-message", "-p", "-t", pane, "#{pane_pid}"]);
    assert_eq!(info.trim(), shell_pid.to_string(), "same shell in the pane");
    let args = args_of(after["pid"].as_u64().unwrap());
    assert!(args.contains(&format!("--resume {sid}")), "{args}");
    assert!(args.contains("--name=shellie"), "{args}");
}

#[test]
fn launch_source_not_running_refusal_does_not_change_queue() {
    let w = World::new("not-running-keeps-queue");
    w.new_session("api");
    let sid = w.session_id("api");
    launch_snapshot(&w, &sid);
    w.first_prompt(&sid, "api");
    wait_until("busy session", || {
        (session(&w, "api")["busy"] == true).then_some(())
    });
    let (queued, ok) = restart(&w, &["api"]);
    assert!(ok, "{queued}");
    assert_eq!(queued["results"][0]["result"], "queued");
    w.tmux(&["kill-server"]);
    wait_until("interrupted session", || {
        (session(&w, "api")["status"] == "interrupted").then_some(())
    });
    let queue = w.state().join("daemon/restart-queue.json");
    let before = std::fs::read(&queue).unwrap();
    let (refusal, ok) = restart(&w, &["api"]);
    assert!(!ok, "{refusal}");
    assert_eq!(refusal["results"][0]["reason"], "not-running");
    assert_eq!(
        std::fs::read(&queue).unwrap(),
        before,
        "拒绝不能隐式改写已有队列"
    );
    let (cancelled, ok) = restart(&w, &["--cancel", "api"]);
    assert!(ok, "{cancelled}");
    assert_eq!(cancelled["results"][0]["result"], "cancelled");
    assert!(cancelled["results"][0]["reason"].is_null());
}

#[test]
fn restart_refuses_what_it_cannot_restart() {
    let w = World::new("rrf");
    // Outside tmux: adopt is the way.
    let p = common::FakeProcess::spawn();
    common::hook(
        &w.state(),
        "session-start",
        &common::start_json("loose", "startup", Some("loose")),
        p.pid(),
    );
    let (v, ok) = restart(&w, &["loose"]);
    assert!(!ok);
    assert_eq!(v["results"][0]["reason"], "not-in-tmux", "{v}");
    // Dropped: nothing to restart.
    common::hook(
        &w.state(),
        "session-start",
        &common::start_json("gone", "startup", Some("gone")),
        Some(common::dead_pid()),
    );
    let (v, _) = restart(&w, &["gone"]);
    assert_eq!(v["results"][0]["reason"], "not-running", "{v}");
    // --stale with nothing stale: nothing to do.
    let (v, ok) = restart(&w, &["--stale"]);
    assert!(ok, "{v}");
    assert_eq!(v["results"], serde_json::json!([]));
}

/// `cc-moss new <name>` with the fake claude doing `on_term` on SIGTERM
/// (the tmux server, started here, passes it on to the pane).
fn new_with(w: &World, name: &str, on_term: &str) {
    let out = w
        .cc_moss()
        .env("CC_MOSS_FAKE_ON_TERM", on_term)
        .args(["new", name, "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn restart_waits_for_tmux_to_see_the_pane_dead() {
    // The process is gone at once, but something it left holds the
    // terminal a while: tmux doesn't count the pane dead yet.
    let w = World::new("rlinger");
    new_with(&w, "api", "linger");
    let sid = w.session_id("api");
    let before = session(&w, "api");
    let pane = before["tmux"]["pane_id"].as_str().unwrap().to_string();
    let old_pid = before["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["api"]);
    assert!(ok, "{v}");
    assert_eq!(v["results"][0]["result"], "restarted", "{v}");
    let after = restarted(&w, "api", old_pid);
    assert_eq!(after["tmux"]["pane_id"], pane.as_str());
    assert!(args_of(after["pid"].as_u64().unwrap()).contains(&format!("--resume {sid}")));
}

#[test]
fn restart_keeps_the_pane_when_it_wont_quit() {
    let w = World::new("rignore");
    new_with(&w, "api", "ignore");
    w.session_id("api");
    let before = session(&w, "api");
    let pane = before["tmux"]["pane_id"].as_str().unwrap().to_string();
    let pid = before["pid"].as_u64().unwrap();
    let out = w
        .cc_moss()
        .env("CC_MOSS_RESTART_QUIT_MS", "1000")
        .args(["restart", "api", "--json"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"][0]["result"], "refused", "{v}");
    assert_eq!(v["results"][0]["reason"], "quit-timeout", "{v}");
    assert!(alive(pid), "it is still there");
    // The pane stays put for whenever it does quit, and nobody hears it
    // dropped.
    assert_eq!(
        w.tmux(&[
            "show-options",
            "-p",
            "-q",
            "-v",
            "-t",
            &pane,
            "remain-on-exit"
        ])
        .trim(),
        "on"
    );
    let s = session(&w, "api");
    assert_eq!(s["status"], "running");
    let events = w.cc_moss().args(["events", "--json"]).output().unwrap();
    let events: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    assert!(events["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["kind"] != "interrupted"));
}

#[test]
fn restart_queues_when_another_restart_holds_it() {
    let w = World::new("rclaim");
    w.new_session("api");
    w.session_id("api");
    let id = session(&w, "api")["id"].as_str().unwrap().to_string();
    // Another restart claimed it a moment ago.
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET restore_started_at = ?2 WHERE slot_id = ?1",
        rusqlite::params![
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        ],
    )
    .unwrap();
    let (v, ok) = restart(&w, &["api"]);
    assert!(ok, "{v}");
    assert_eq!(v["results"][0]["result"], "queued", "{v}");
    assert_eq!(v["results"][0]["reason"], "in-progress", "{v}");
    // And it really is in the queue.
    assert_eq!(session(&w, "api")["restart_queued"], true);
}

#[test]
fn restart_in_a_shell_goes_to_the_sessions_folder() {
    let w = World::new("rcd");
    let work = w.path("work");
    let elsewhere = w.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    // The shell is elsewhere; the session works in `work`.
    w.tmux(&[
        "new-session",
        "-d",
        "-s",
        "shellpane",
        "-c",
        elsewhere.to_str().unwrap(),
        "bash",
        "--norc",
        "--noprofile",
    ]);
    let pane = w
        .tmux(&["display-message", "-p", "-t", "shellpane", "#{pane_id}"])
        .trim()
        .to_string();
    let exports = shell_exports(&w);
    let sid = "22222222-2222-3333-4444-555555555555";
    for line in [
        exports,
        format!(
            "CC_MOSS_FAKE_CWD='{}' {} --session-id {sid} --name=cdme",
            work.display(),
            w.claude.display()
        ),
    ] {
        w.tmux(&["send-keys", "-t", &pane, "-l", &line]);
        w.tmux(&["send-keys", "-t", &pane, "Enter"]);
    }
    let before = wait_until("the session under the shell", || {
        ls_json(&w.state(), true)["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["name"] == "cdme" && s["status"] == "running")
            .cloned()
    });
    let old_pid = before["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["cdme"]);
    assert!(ok, "{v}");
    let after = restarted(&w, "cdme", old_pid);
    let pid = after["pid"].as_u64().unwrap().to_string();
    // Linux has it under /proc; macOS says through lsof.
    let cwd = match std::fs::read_link(format!("/proc/{pid}/cwd")) {
        Ok(p) => format!("n{}", p.display()),
        Err(_) => {
            let out = Command::new("lsof")
                .args(["-a", "-d", "cwd", "-Fn", "-p", &pid])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        }
    };
    let want = std::fs::canonicalize(&work).unwrap();
    assert!(
        cwd.lines().any(|l| l
            .strip_prefix('n')
            .is_some_and(|p| { std::fs::canonicalize(p).ok().as_deref() == Some(want.as_path()) })),
        "{cwd}"
    );
}

fn channel(w: &World, model: &str) {
    std::fs::create_dir_all(w.path("claude")).unwrap();
    std::fs::write(
        w.config_path(),
        format!(
            "[claude]\nconfig_dir = {:?}\ndefault_args = [\"--model\", \"old-route\"]\n",
            w.path("claude").to_str().unwrap()
        ),
    )
    .unwrap();
    std::fs::write(
        w.path("claude/settings.json"),
        serde_json::json!({"env":{"ANTHROPIC_MODEL":model}}).to_string(),
    )
    .unwrap();
}

#[test]
fn provider_default_restart_overrides_old_model() {
    let w = World::new("rmodel");
    channel(&w, "route-b");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .args(["--", "--model", "old-route"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let before = session(&w, "api");
    let old = before["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(ok, "{v}");
    let after = restarted(&w, "api", old);
    assert_eq!(after["session_id"], before["session_id"]);
    assert_eq!(after["tmux"]["pane_id"], before["tmux"]["pane_id"]);
    let out = w.cc_moss().args(["ls", "--json"]).output().unwrap();
    let listed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(listed["sessions"][0]["provider_model"]["uncertain"], false);
    let args = args_of(after["pid"].as_u64().unwrap());
    assert!(args.contains("--model route-b"), "{args}");
    assert!(!args.contains("old-route"), "{args}");
}

#[test]
fn provider_default_survives_queue() {
    let w = World::new("rqmodel");
    channel(&w, "route-b");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .args(["--", "--model", "old-route"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let sid = w.session_id("api");
    w.first_prompt(&sid, "api");
    wait_until("busy", || {
        (session(&w, "api")["busy"] == true).then_some(())
    });
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(ok && v["results"][0]["result"] == "queued", "{v}");
    channel(&w, "route-c");
    let _daemon = Daemon(
        w.cc_moss()
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    w.send(&sid, "stop");
    let after = restarted(&w, "api", old);
    let args = args_of(after["pid"].as_u64().unwrap());
    assert!(args.contains("--model route-c"), "{args}");
}

#[test]
fn provider_default_queue_can_be_cancelled() {
    let w = World::new("rcmodel");
    channel(&w, "route-b");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .args(["--", "--model", "old-route"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let sid = w.session_id("api");
    w.first_prompt(&sid, "api");
    wait_until("busy", || {
        (session(&w, "api")["busy"] == true).then_some(())
    });
    let old = session(&w, "api")["pid"].clone();
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(ok && v["results"][0]["result"] == "queued", "{v}");
    let (v, ok) = restart(&w, &["--cancel", "api"]);
    assert!(ok && v["results"][0]["result"] == "cancelled", "{v}");
    assert_eq!(session(&w, "api")["restart_queued"], false);
    assert_eq!(session(&w, "api")["pid"], old);
}

#[test]
fn provider_default_bad_settings_refused_before_quit() {
    let w = World::new("rbadmodel");
    channel(&w, "route-b");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let before = session(&w, "api");
    std::fs::write(w.path("claude/settings.json"), "not json").unwrap();
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(!ok && v["results"][0]["reason"] == "settings", "{v}");
    assert!(alive(before["pid"].as_u64().unwrap()));
}

#[test]
fn provider_default_project_override_is_not_ignored() {
    let w = World::new("rpmodel");
    channel(&w, "route-b");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    std::fs::create_dir_all(w.path("work/.claude")).unwrap();
    std::fs::write(
        w.path("work/.claude/settings.local.json"),
        r#"{"model":"project-model"}"#,
    )
    .unwrap();
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(!ok && v["results"][0]["reason"] == "settings", "{v}");
    assert!(alive(old));
    std::fs::write(
        w.path("work/.claude/settings.local.json"),
        r#"{"theme":"dark"}"#,
    )
    .unwrap();
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(ok, "{v}");
    restarted(&w, "api", old);
}

#[test]
fn provider_model_ordinary_restart_does_not_force_observed_or_launch_model() {
    let w = World::new("rmanualmodel");
    channel(&w, "route-b");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .args(["--", "--model", "old-route"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let sid = w.session_id("api");
    std::fs::write(w.path(&format!("fifos/{sid}.jsonl")),
        "{\"type\":\"assistant\",\"message\":{\"model\":\"manually-picked\",\"stop_reason\":\"end_turn\"}}\n").unwrap();
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["api"]);
    assert!(ok, "{v}");
    let after = restarted(&w, "api", old);
    let args = args_of(after["pid"].as_u64().unwrap());
    assert!(!args.contains("--model"), "{args}");
    assert!(
        !args.contains("route-b") && !args.contains("old-route"),
        "{args}"
    );
}

#[test]
fn provider_default_restore_reuses_explicit_model_strategy() {
    let w = World::new("restoremodel");
    channel(&w, "route-b");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .args(["--", "--model", "old-route"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let sid = w.session_id("api");
    w.first_prompt(&sid, "api");
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    w.tmux(&["kill-server"]);
    wait_until("interrupted", || {
        (session(&w, "api")["status"] == "interrupted").then_some(())
    });
    channel(&w, "route-c");
    let out = w
        .cc_moss()
        .args(["restore", "--provider-default", "--yes", "--json", "api"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let after = restarted(&w, "api", old);
    assert_eq!(after["session_id"], sid);
    let args = args_of(after["pid"].as_u64().unwrap());
    assert!(args.contains("--model route-c"), "{args}");
    assert!(!args.contains("old-route"), "{args}");
}

#[test]
fn provider_model_restart_without_session_start_reports_starting() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = World::new("rstartingmodel");
    let script = std::fs::read_to_string(&w.claude).unwrap();
    let script = script.replace(
        "start \"$source\"",
        "[ \"$source\" = resume ] || start \"$source\"",
    );
    w.claude = w.path("quiet-claude");
    std::fs::write(&w.claude, script).unwrap();
    std::fs::set_permissions(&w.claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    w.new_session("api");
    w.session_id("api");
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["api"]);
    assert!(ok, "{v}");
    assert_eq!(
        v["results"][0]["result"], "starting",
        "a process alone is not SessionStart: {v}"
    );
    assert!(!alive(old));
}

#[test]
fn provider_default_replays_recorded_settings_directory() {
    let w = World::new("rsourcemodel");
    channel(&w, "wrong-default-route");
    std::fs::create_dir_all(w.path("own channel")).unwrap();
    std::fs::write(
        w.path("own channel/settings.json"),
        r#"{"env":{"ANTHROPIC_MODEL":"own-route","ANTHROPIC_BASE_URL":"https://own.example"}}"#,
    )
    .unwrap();
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("own channel"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(ok, "{v}");
    let after = restarted(&w, "api", old);
    assert!(args_of(after["pid"].as_u64().unwrap()).contains("--model own-route"));
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    let recorded: String = db
        .query_row(
            "SELECT provider_started FROM slots WHERE session_id=?1",
            [after["session_id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let recorded: serde_json::Value = serde_json::from_str(&recorded).unwrap();
    assert_eq!(
        recorded["source"],
        w.path("own channel/settings.json").to_str().unwrap()
    );
    assert_eq!(recorded["host"], "own.example");
}

#[test]
fn provider_model_restore_without_session_start_reports_starting() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = World::new("restorestartingmodel");
    let script = std::fs::read_to_string(&w.claude).unwrap();
    let script = script.replace(
        "start \"$source\"",
        "[ \"$source\" = resume ] || start \"$source\"",
    );
    w.claude = w.path("quiet-claude");
    std::fs::write(&w.claude, script).unwrap();
    std::fs::set_permissions(&w.claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    w.new_session("api");
    let sid = w.session_id("api");
    w.first_prompt(&sid, "api");
    w.tmux(&["kill-server"]);
    wait_until("interrupted", || {
        (session(&w, "api")["status"] == "interrupted").then_some(())
    });
    let out = w
        .cc_moss()
        .args(["restore", "--yes", "--json", "api"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"][0]["result"], "starting", "{v}");
}

#[test]
fn provider_model_direct_restart_refuses_later_channel_request() {
    let w = World::new("rclaimrace");
    channel(&w, "route-b");
    new_with(&w, "api", "ignore");
    w.session_id("api");
    let mut first = w
        .cc_moss()
        .env("CC_MOSS_RESTART_QUIT_MS", "2500")
        .args(["restart", "api", "--json"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    wait_until("restart claim", || {
        let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
        let claimed: bool = db
            .query_row(
                "SELECT restore_started_at IS NOT NULL FROM slots LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        claimed.then_some(())
    });
    let (v, ok) = restart(&w, &["--provider-default", "api"]);
    first.wait().unwrap();
    assert!(
        !ok && v["results"][0]["reason"] == "in-progress",
        "must not promise a queue that the first restart can discard: {v}"
    );
}

#[test]
fn provider_model_queue_write_failure_returns_without_deadlock() {
    let w = World::new("rqueuewrite");
    w.new_session("api");
    w.session_id("api");
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    std::fs::create_dir_all(w.state().join("daemon/restart-queue.json")).unwrap();
    let mut child = w
        .cc_moss()
        .args(["restart", "api", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= until {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("queue persistence failure deadlocked instead of returning an error");
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(!status.success());
    assert!(
        alive(old),
        "failed claim must not terminate the original session"
    );
}

#[test]
fn nested_restart_in_same_tty_shells() {
    let w = World::new("rnested");
    let work = w.path("work");
    w.tmux(&[
        "new-session",
        "-d",
        "-s",
        "shellpane",
        "-c",
        work.to_str().unwrap(),
        "bash",
        "--norc",
        "--noprofile",
    ]);
    let info = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        "shellpane",
        "#{pane_id} #{pane_pid}",
    ]);
    let (pane, shell_pid) = info.trim().split_once(' ').unwrap();
    let shell_pid: u64 = shell_pid.parse().unwrap();
    let sid = "11111111-2222-3333-4444-555555555555";
    let exports = shell_exports(&w);
    w.tmux(&["send-keys", "-t", pane, "-l", &exports]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    w.tmux(&["send-keys", "-t", pane, "-l", "bash --norc --noprofile"]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let start = format!("{} --session-id {sid} --name=shellie", w.claude.display());
    w.tmux(&["send-keys", "-t", pane, "-l", &start]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let before = wait_until("the session under the shell", || {
        let all = ls_json(&w.state(), true);
        all["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["name"] == "shellie" && s["status"] == "running")
            .cloned()
    });
    assert_eq!(before["tmux"]["pane_id"], pane);
    let old_pid = before["pid"].as_u64().unwrap();
    assert_ne!(old_pid, shell_pid, "claude runs under the shell");

    let (v, ok) = restart(&w, &["shellie"]);
    assert!(ok, "{v}");
    assert_eq!(v["results"][0]["result"], "restarted", "{v}");
    let after = restarted(&w, "shellie", old_pid);
    assert_eq!(after["tmux"]["pane_id"], pane);
    assert!(alive(shell_pid), "the shell stays");
    let info = w.tmux(&["display-message", "-p", "-t", pane, "#{pane_pid}"]);
    assert_eq!(info.trim(), shell_pid.to_string(), "same shell in the pane");
    let args = args_of(after["pid"].as_u64().unwrap());
    assert!(args.contains(&format!("--resume {sid}")), "{args}");
    assert!(args.contains("--name=shellie"), "{args}");
}

fn pty_proxy_world(test: &str) -> World {
    use std::os::unix::fs::PermissionsExt;
    let mut w = World::new(test);
    let fake = w.claude.clone();
    w.identity_entry = Some(fake.clone());
    let script = format!(
        r#"#!/usr/bin/env python3
import os, pty, select, sys
if len(sys.argv) > 1 and sys.argv[1] == "agents":
    print("[]")
    sys.exit(0)
pid, fd = pty.fork()
if pid == 0:
    os.execv({fake:?}, [{fake:?}] + sys.argv[1:])
while True:
    for ready in select.select([0, fd], [], [])[0]:
        try:
            data = os.read(ready, 4096)
        except OSError:
            sys.exit(0)
        if not data:
            sys.exit(0)
        os.write(fd if ready == 0 else 1, data)
"#,
        fake = fake.to_str().unwrap()
    );
    w.claude = w.path("proxy-claude");
    std::fs::write(&w.claude, script).unwrap();
    std::fs::set_permissions(&w.claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    w
}

#[test]
fn rebuild_proxy_requires_identity_and_explicit_confirmation() {
    let w = pty_proxy_world("rproxy");
    w.new_session("api");
    w.session_id("api");
    let before = session(&w, "api");
    let (v, ok) = restart(&w, &["api", "--yes"]);
    assert!(!ok && v["results"][0]["reason"] == "pty-proxy", "{v}");
    let identity = v["results"][0]["rebuild_identity"].clone();
    assert!(identity.is_object(), "{v}");
    assert!(alive(before["pid"].as_u64().unwrap()));
    let missing = w
        .cc_moss()
        .args(["restart", "--rebuild-pane", "--yes", "api"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(alive(before["pid"].as_u64().unwrap()));
    let mut changed = identity.clone();
    changed["claude_start"] = "wrong-start".into();
    let (v, ok) = restart(
        &w,
        &[
            "--rebuild-pane",
            "--yes",
            "--rebuild-identity",
            &changed.to_string(),
            "api",
        ],
    );
    assert!(
        !ok && v["results"][0]["reason"] == "confirmation-stale",
        "{v}"
    );
    assert!(alive(before["pid"].as_u64().unwrap()));
    for field in [
        "slot_id",
        "session_id",
        "tmux_socket",
        "pane_id",
        "pane_start",
    ] {
        let mut stale = identity.clone();
        stale[field] = "changed".into();
        let (v, ok) = restart(
            &w,
            &[
                "--rebuild-pane",
                "--yes",
                "--rebuild-identity",
                &stale.to_string(),
                "api",
            ],
        );
        assert!(
            !ok && v["results"][0]["reason"] == "confirmation-stale",
            "{field}: {v}"
        );
    }
    let without_yes = w
        .cc_moss()
        .args([
            "restart",
            "--rebuild-pane",
            "--rebuild-identity",
            &identity.to_string(),
            "api",
        ])
        .output()
        .unwrap();
    assert!(!without_yes.status.success());
    let sid = before["session_id"].as_str().unwrap();
    w.first_prompt(sid, "api");
    wait_until("busy", || {
        (session(&w, "api")["busy"] == true).then_some(())
    });
    let (busy, ok) = restart(
        &w,
        &[
            "--rebuild-pane",
            "--yes",
            "--rebuild-identity",
            &identity.to_string(),
            "api",
        ],
    );
    assert!(!ok && busy["results"][0]["reason"] == "busy", "{busy}");
    assert_eq!(session(&w, "api")["restart_queued"], false);
    assert!(alive(before["pid"].as_u64().unwrap()));
    w.send(sid, "stop");
    wait_until("idle", || {
        (session(&w, "api")["busy"] == false).then_some(())
    });
    let (v, ok) = restart(
        &w,
        &[
            "--rebuild-pane",
            "--yes",
            "--rebuild-identity",
            &identity.to_string(),
            "api",
        ],
    );
    assert!(
        ok,
        "{v}; pane={}; identity={identity}",
        w.tmux(&[
            "display-message",
            "-p",
            "-t",
            before["tmux"]["pane_id"].as_str().unwrap(),
            "#{pane_pid}|#{pane_dead}"
        ])
    );
    let after = restarted(&w, "api", before["pid"].as_u64().unwrap());
    assert!(
        !alive(before["pid"].as_u64().unwrap()),
        "old Claude must have exited before replacement"
    );
    assert_eq!(after["session_id"], before["session_id"]);
    assert_eq!(after["tmux"]["pane_id"], before["tmux"]["pane_id"]);
}

#[test]
fn nested_restart_foreground_takeover_receives_no_input() {
    let w = World::new("rtakeover");
    let work = w.path("work");
    w.tmux(&[
        "new-session",
        "-d",
        "-s",
        "shellpane",
        "-c",
        work.to_str().unwrap(),
        "bash",
        "--norc",
        "--noprofile",
    ]);
    let info = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        "shellpane",
        "#{pane_id} #{pane_pid}",
    ]);
    let (pane, shell_pid) = info.trim().split_once(' ').unwrap();
    let shell_pid: u64 = shell_pid.parse().unwrap();
    let sid = "11111111-2222-3333-4444-555555555555";
    let exports = shell_exports(&w);
    w.tmux(&["send-keys", "-t", pane, "-l", &exports]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let start = format!(
        "trap 'exec cat > {}' CHLD; {} --session-id {sid} --name=shellie",
        w.path("received-input").display(),
        w.claude.display()
    );
    w.tmux(&["send-keys", "-t", pane, "-l", &start]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let before = wait_until("the session under the shell", || {
        let all = ls_json(&w.state(), true);
        all["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["name"] == "shellie" && s["status"] == "running")
            .cloned()
    });
    assert_eq!(before["tmux"]["pane_id"], pane);
    let old_pid = before["pid"].as_u64().unwrap();
    assert_ne!(old_pid, shell_pid, "claude runs under the shell");

    let (v, ok) = restart(&w, &["shellie"]);
    assert!(!ok && v["results"][0]["result"] == "refused", "{v}");
    assert!(!alive(old_pid));
    assert!(alive(shell_pid), "foreground replacement still lives");
    assert_eq!(
        std::fs::read_to_string(w.path("received-input")).unwrap(),
        "",
        "never type a command into cat"
    );
}

#[test]
fn nested_restart_background_claude_is_not_stopped() {
    let w = World::new("rbackground");
    let work = w.path("work");
    w.tmux(&[
        "new-session",
        "-d",
        "-s",
        "shellpane",
        "-c",
        work.to_str().unwrap(),
        "bash",
        "--norc",
        "--noprofile",
    ]);
    let info = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        "shellpane",
        "#{pane_id} #{pane_pid}",
    ]);
    let (pane, shell_pid) = info.trim().split_once(' ').unwrap();
    let shell_pid: u64 = shell_pid.parse().unwrap();
    let sid = "11111111-2222-3333-4444-555555555555";
    let exports = shell_exports(&w);
    w.tmux(&["send-keys", "-t", pane, "-l", &exports]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let start = format!("{} --session-id {sid} --name=shellie &", w.claude.display());
    w.tmux(&["send-keys", "-t", pane, "-l", &start]);
    w.tmux(&["send-keys", "-t", pane, "Enter"]);
    let before = wait_until("the session under the shell", || {
        let all = ls_json(&w.state(), true);
        all["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["name"] == "shellie" && s["status"] == "running")
            .cloned()
    });
    assert_eq!(before["tmux"]["pane_id"], pane);
    let old_pid = before["pid"].as_u64().unwrap();
    assert_ne!(old_pid, shell_pid, "claude runs under the shell");

    let (v, ok) = restart(&w, &["shellie"]);
    assert!(!ok && v["results"][0]["reason"] == "unsafe-terminal", "{v}");
    assert!(alive(old_pid), "background Claude must not be stopped");
    assert!(alive(shell_pid));
}

#[test]
fn nested_restart_unknown_same_tty_wrapper_is_not_stopped() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = World::new("runknownwrapper");
    let fake = w.claude.clone();
    w.claude = w.path("unknown-claude");
    std::fs::write(&w.claude, format!("#!/usr/bin/env python3\nimport subprocess, sys\nif len(sys.argv)>1 and sys.argv[1]=='agents':\n print('[]'); sys.exit(0)\nsubprocess.call([{:?}] + sys.argv[1:])\n", fake.to_str().unwrap())).unwrap();
    std::fs::set_permissions(&w.claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    w.new_session("api");
    w.session_id("api");
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["api", "--yes"]);
    assert!(!ok && v["results"][0]["reason"] == "unsafe-terminal", "{v}");
    assert!(alive(old));
}

#[test]
fn rebuild_contended_restore_claim_is_refused_without_queueing() {
    let w = pty_proxy_world("rebuildclaim");
    w.new_session("api");
    w.session_id("api");
    let before = session(&w, "api");
    let (v, _) = restart(&w, &["api"]);
    let identity = v["results"][0]["rebuild_identity"].to_string();
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET restore_started_at=?1",
        [moss_core::slot::now_ms()],
    )
    .unwrap();
    let (v, ok) = restart(
        &w,
        &[
            "--rebuild-pane",
            "--yes",
            "--rebuild-identity",
            &identity,
            "api",
        ],
    );
    assert!(!ok && v["results"][0]["reason"] == "in-progress", "{v}");
    assert_eq!(
        session(&w, "api")["restart_queued"],
        false,
        "destructive consent must never enter the ordinary queue"
    );
    assert!(alive(before["pid"].as_u64().unwrap()));
}

#[test]
fn rebuild_turn_started_during_model_validation_is_not_queued() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let w = pty_proxy_world("rebuildbusyrace");
    std::fs::create_dir_all(w.path("own-settings")).unwrap();
    let settings = w.path("own-settings/settings.json");
    std::fs::write(&settings, "{}").unwrap();
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("own-settings"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    w.session_id("api");
    let before = session(&w, "api");
    let (refusal, _) = restart(&w, &["api"]);
    let identity = refusal["results"][0]["rebuild_identity"].to_string();
    std::fs::rename(&settings, w.path("own-settings/original.json")).unwrap();
    assert!(Command::new("mkfifo")
        .arg(&settings)
        .status()
        .unwrap()
        .success());
    let mut child = w
        .cc_moss()
        .args([
            "restart",
            "--rebuild-pane",
            "--yes",
            "--provider-default",
            "--rebuild-identity",
            &identity,
            "--json",
            "api",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let mut writer = loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&settings)
        {
            Ok(writer) => break writer,
            Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let out = child.wait_with_output().unwrap();
                panic!("rebuild never reached settings validation: {error}; {out:?}");
            }
        }
    };
    // The FIFO proves that initial identity/busy checks have passed. A turn
    // starts before model validation returns, without timing-based guesses.
    std::fs::write(
        w.path(&format!(
            "fifos/{}.jsonl",
            before["session_id"].as_str().unwrap()
        )),
        "{\"type\":\"user\",\"message\":{\"content\":\"new turn\"}}\n",
    )
    .unwrap();
    writer
        .write_all(br#"{"env":{"ANTHROPIC_MODEL":"route-b"}}"#)
        .unwrap();
    drop(writer);
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= until {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("rebuild failed to reject the new turn promptly");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        !out.status.success() && v["results"][0]["reason"] == "busy",
        "{v}"
    );
    assert!(alive(before["pid"].as_u64().unwrap()));
    let queued = std::fs::read_to_string(w.state().join("daemon/restart-queue.json"))
        .unwrap_or_else(|_| "[]".into());
    let queued: Vec<serde_json::Value> = serde_json::from_str(&queued).unwrap();
    assert!(
        queued.is_empty(),
        "confirmation must never become an ordinary queued restart"
    );
}

#[test]
fn provider_model_explicit_claim_conflict_keeps_queue_despite_success_evidence() {
    let w = World::new("rexplicitevidence");
    channel(&w, "old-route");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.path("claude"))
        .args(["new", "api", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let sid = w.session_id("api");
    let transcript = w.path(&format!("fifos/{sid}.jsonl"));
    std::fs::write(&transcript, "").unwrap();
    channel(&w, "route-b");
    let listing = || {
        let out = w.cc_moss().args(["ls", "--json"]).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let first = listing();
    assert_eq!(first["sessions"][0]["provider_restart"], "model");
    std::thread::sleep(Duration::from_millis(5));
    std::fs::write(&transcript, format!("{}\n", serde_json::json!({
        "type":"assistant", "sessionId":sid, "timestamp":moss_core::slot::rfc3339(moss_core::slot::now_ms()),
        "message":{"model":"route-b","stop_reason":"end_turn"}
    }))).unwrap();
    assert!(listing()["sessions"][0]["provider_restart"].is_null());
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET restore_started_at=?1",
        [moss_core::slot::now_ms()],
    )
    .unwrap();
    let (result, ok) = restart(&w, &["--provider-default", "api"]);
    assert!(ok && result["results"][0]["result"] == "queued", "{result}");
    let queue = w.state().join("daemon/restart-queue.json");
    let read_queue = || {
        serde_json::from_str::<Vec<serde_json::Value>>(&std::fs::read_to_string(&queue).unwrap())
            .unwrap()
    };
    assert_eq!(
        read_queue()[0]["recheck"],
        false,
        "an explicit request is not an automatic stale request"
    );
    let _daemon = Daemon(
        w.cc_moss()
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        read_queue().len(),
        1,
        "successful model evidence must not cancel an explicit request"
    );
}

#[test]
fn rebuild_does_not_launch_while_old_claude_lives() {
    use std::os::unix::fs::PermissionsExt;
    struct IsolatedPids(Vec<u32>);
    impl Drop for IsolatedPids {
        fn drop(&mut self) {
            for pid in &self.0 {
                // Only PIDs returned by this temporary fake-Claude world.
                unsafe {
                    libc::kill(*pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }
    let mut w = pty_proxy_world("rebuildoldalive");
    let original = common::fixture("fake-claude");
    let stubborn = w.path("stubborn-claude");
    w.identity_entry = Some(stubborn.clone());
    let text = std::fs::read_to_string(&original).unwrap().replace(
        "start \"$source\"", "trap '' HUP TERM\nprintf '%s\\n' \"$source\" >> \"$CC_MOSS_FAKE_DIR/launches\"\nstart \"$source\"");
    std::fs::write(&stubborn, text).unwrap();
    std::fs::set_permissions(&stubborn, std::fs::Permissions::from_mode(0o700)).unwrap();
    let proxy = std::fs::read_to_string(&w.claude)
        .unwrap()
        .replace(original.to_str().unwrap(), stubborn.to_str().unwrap());
    std::fs::write(&w.claude, proxy).unwrap();
    w.new_session("api");
    w.session_id("api");
    let before = session(&w, "api");
    let pid = before["pid"].as_u64().unwrap();
    let mut cleanup = IsolatedPids(vec![pid as u32]);
    let (refusal, _) = restart(&w, &["api"]);
    let identity = refusal["results"][0]["rebuild_identity"].to_string();
    let pane = before["tmux"]["pane_id"].as_str().unwrap();
    let root = w.tmux(&["display-message", "-p", "-t", pane, "#{pane_pid}"]);
    let out = w
        .cc_moss()
        .env("CC_MOSS_RESTART_QUIT_MS", "300")
        .args([
            "restart",
            "--rebuild-pane",
            "--yes",
            "--rebuild-identity",
            &identity,
            "--json",
            "api",
        ])
        .output()
        .unwrap();
    let current = session(&w, "api")["pid"].as_u64().unwrap() as u32;
    if !cleanup.0.contains(&current) {
        cleanup.0.push(current);
    }
    assert!(alive(pid), "the stubborn old Claude still lives");
    let launches = std::fs::read_to_string(w.path("fifos/launches")).unwrap();
    assert_eq!(
        launches.lines().count(),
        1,
        "no second launch while old Claude lives: {launches}"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        !out.status.success() && v["results"][0]["reason"] == "quit-timeout",
        "{v}"
    );
    assert_eq!(
        w.tmux(&["display-message", "-p", "-t", pane, "#{pane_pid}"]),
        root
    );
    assert!(!v["results"][0]["message"]
        .as_str()
        .unwrap()
        .contains("--resume"));
}

#[test]
fn restart_terminal_failure_timeout_ends_daemon_queue_without_retry() {
    let w = World::new("rterminalfailure");
    new_with(&w, "api", "ignore");
    let sid = w.session_id("api");
    w.first_prompt(&sid, "api");
    wait_until("busy", || {
        (session(&w, "api")["busy"] == true).then_some(())
    });
    let old = session(&w, "api")["pid"].as_u64().unwrap();
    let (v, ok) = restart(&w, &["api"]);
    assert!(ok && v["results"][0]["result"] == "queued", "{v}");
    let _daemon = Daemon(
        w.cc_moss()
            .env("CC_MOSS_RESTART_QUIT_MS", "300")
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    w.send(&sid, "stop");
    wait_until("terminal failure event", || {
        let out = w.cc_moss().args(["events", "--json"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout)
            .contains("quit-timeout")
            .then_some(())
    });
    let queue = w.state().join("daemon/restart-queue.json");
    let entries = wait_until("worker result processed", || {
        let q: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(&queue).ok()?).ok()?;
        (q.is_empty() || q.iter().all(|q| q["executing"] == false)).then_some(q)
    });
    assert!(
        entries.is_empty(),
        "terminal timeout must not remain for automatic retry: {entries:?}"
    );
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    let claim: Option<i64> = db
        .query_row("SELECT restore_started_at FROM slots LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(claim, None);
    assert!(alive(old));
}

#[test]
fn restart_terminal_failure_pending_cancel_ignores_unowned_restore_marker() {
    let w = World::new("rcancelmarker");
    w.new_session("api");
    w.session_id("api");
    let before = session(&w, "api");
    let id = before["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET restore_started_at=?1",
        [moss_core::slot::now_ms()],
    )
    .unwrap();
    std::fs::create_dir_all(w.state().join("daemon")).unwrap();
    std::fs::write(w.state().join("daemon/restart-queue.json"), serde_json::json!([
        {"slot_id":id,"recheck":false,"attempts":1,"provider_default":true,"executing":false,"execution_until_ms":0}
    ]).to_string()).unwrap();
    let (v, ok) = restart(&w, &["--cancel", "api"]);
    assert!(ok && v["results"][0]["result"] == "cancelled", "{v}");
    let claim: Option<i64> = db
        .query_row("SELECT restore_started_at FROM slots LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(
        claim.is_some(),
        "cancellation must not clear someone else's marker"
    );
    assert!(alive(before["pid"].as_u64().unwrap()));
}

#[test]
fn rebuild_refuses_pane_moved_to_another_window_during_exit() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = pty_proxy_world("rebuildtopology");
    let original = common::fixture("fake-claude");
    let moving = w.path("moving-claude");
    w.identity_entry = Some(moving.clone());
    let script = std::fs::read_to_string(&original).unwrap().replace("start \"$source\"",
        "trap 'target=$(cat \"$CC_MOSS_FAKE_DIR/target-pane\"); tmux move-pane -s \"$TMUX_PANE\" -t \"$target\"; exit 0' TERM\nprintf '%s\\n' \"$source\" >> \"$CC_MOSS_FAKE_DIR/launches\"\nstart \"$source\"");
    std::fs::write(&moving, script).unwrap();
    std::fs::set_permissions(&moving, std::fs::Permissions::from_mode(0o700)).unwrap();
    let proxy = std::fs::read_to_string(&w.claude)
        .unwrap()
        .replace(original.to_str().unwrap(), moving.to_str().unwrap());
    std::fs::write(&w.claude, proxy).unwrap();
    w.new_session("api");
    w.session_id("api");
    let before = session(&w, "api");
    let target = w.tmux(&[
        "new-window",
        "-d",
        "-P",
        "-F",
        "#{pane_id}",
        "-t",
        before["tmux"]["session_name"].as_str().unwrap(),
        "sleep 30",
    ]);
    std::fs::write(w.path("fifos/target-pane"), target.trim()).unwrap();
    let (v, _) = restart(&w, &["api"]);
    let identity = v["results"][0]["rebuild_identity"].to_string();
    let (v, ok) = restart(
        &w,
        &[
            "--rebuild-pane",
            "--yes",
            "--rebuild-identity",
            &identity,
            "api",
        ],
    );
    assert!(
        !ok && v["results"][0]["reason"] == "confirmation-stale",
        "{v}"
    );
    assert!(!alive(before["pid"].as_u64().unwrap()));
    assert_eq!(
        std::fs::read_to_string(w.path("fifos/launches"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn restart_terminal_failure_direct_replacement_is_not_killed_or_relaunched() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = World::new("rdirectreplace");
    let text = std::fs::read_to_string(&w.claude).unwrap().replace("start \"$source\"",
        "trap 'touch \"$CC_MOSS_FAKE_DIR/term-seen\"; while :; do sleep 1; done' TERM\nprintf '%s\\n' \"$source\" >> \"$CC_MOSS_FAKE_DIR/launches\"\nstart \"$source\"");
    w.claude = w.path("slow-exit-claude");
    std::fs::write(&w.claude, text).unwrap();
    std::fs::set_permissions(&w.claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    w.new_session("api");
    w.session_id("api");
    let before = session(&w, "api");
    let pane = before["tmux"]["pane_id"].as_str().unwrap();
    let mut child = w
        .cc_moss()
        .args(["restart", "api", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while !w.path("fifos/term-seen").exists() {
        if Instant::now() >= until {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!("never entered exit wait: {out:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // A different controller replaces the pane while the old request waits.
    w.tmux(&["respawn-pane", "-k", "-t", pane, "sleep", "30"]);
    let out = child.wait_with_output().unwrap();
    let launches = std::fs::read_to_string(w.path("fifos/launches")).unwrap();
    assert_eq!(
        launches.lines().count(),
        1,
        "old request must not launch over the replacement: {launches}"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        !out.status.success() && v["results"][0]["reason"] == "confirmation-stale",
        "{v}"
    );
    assert_eq!(
        w.tmux(&[
            "display-message",
            "-p",
            "-t",
            pane,
            "#{pane_current_command}"
        ])
        .trim(),
        "sleep"
    );
    let new_pid: u64 = w
        .tmux(&["display-message", "-p", "-t", pane, "#{pane_pid}"])
        .trim()
        .parse()
        .unwrap();
    assert!(alive(new_pid));
}

#[test]
fn restart_terminal_failure_cancel_absent_queue_is_explicit_noop() {
    let w = World::new("rcancelabsent");
    w.new_session("api");
    w.session_id("api");
    let (v, ok) = restart(&w, &["--cancel", "api"]);
    assert!(ok, "{v}");
    assert_eq!(v["results"][0]["result"], "cancelled");
    assert_eq!(v["results"][0]["reason"], "not-queued");
}

#[test]
fn resume_identity_single_hop_restart_keeps_current_conversation() {
    let w = World::new("rboundresume");
    w.new_session("switch");
    let origin = w.session_id("switch");
    let before = session(&w, "switch");
    let target = uuid::Uuid::new_v4().to_string();
    w.send(&origin, &format!("resume {target}"));
    wait_until("same process switched", || {
        let r = moss_core::registry::Registry::open(&w.state().join("registry.db")).ok()?;
        r.slots().ok()?.into_iter().find(|s| {
            s.session_id == target
                && s.status == moss_core::slot::Status::Running
                && s.provider_started
                    .as_deref()
                    .and_then(moss_core::provider::Snapshot::from_json)
                    .is_some_and(|snapshot| {
                        snapshot.launch_source != moss_core::provider::LaunchSource::Unknown
                    })
        })
    });
    let (v, ok) = restart(&w, &[&target]);
    assert!(ok, "{v}");
    let after = restarted(&w, "switch", before["pid"].as_u64().unwrap());
    assert_eq!(after["session_id"], target);
    assert_eq!(after["tmux"]["pane_id"], before["tmux"]["pane_id"]);
    assert!(!alive(before["pid"].as_u64().unwrap()));
}

#[test]
fn resume_identity_restart_rejects_generation_replaced_after_claim() {
    let w = World::new("restart-generation-race");
    w.new_session("race");
    w.session_id("race");
    let before = session(&w, "race");
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute_batch("CREATE TRIGGER test_change_generation AFTER UPDATE OF restore_started_at ON slots WHEN NEW.restore_started_at IS NOT NULL
        BEGIN UPDATE process_bindings SET generation='replacement-generation' WHERE owner_slot=NEW.slot_id; END;").unwrap();
    let (v, ok) = restart(&w, &["race"]);
    assert!(!ok, "{v}");
    assert!(
        alive(before["pid"].as_u64().unwrap()),
        "no signal or respawn after generation changes"
    );
    assert_eq!(session(&w, "race")["pid"], before["pid"]);
}

#[test]
fn resume_identity_owner_transfers_during_term_without_relaunching_old_session() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = World::new("owner-transfer-term");
    let target = uuid::Uuid::new_v4().to_string();
    std::fs::write(w.path("fifos/transfer-target"), &target).unwrap();
    let fixture = std::fs::read_to_string(&w.claude).unwrap();
    let fixture = fixture.replace(
        "start \"$source\"",
        r#"transfer_on_term() {
  hook session-end "{\"session_id\":\"$sid\",\"reason\":\"resume\"}"
  sid=$(cat "$CC_MOSS_FAKE_DIR/transfer-target")
  start resume
  exit 0
}
trap transfer_on_term TERM
printf '%s\n' "$source" >> "$CC_MOSS_FAKE_DIR/launches"
start "$source""#,
    );
    w.claude = w.path("transfer-claude");
    std::fs::write(&w.claude, fixture).unwrap();
    std::fs::set_permissions(&w.claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    w.new_session("origin");
    let origin = w.session_id("origin");
    let before = session(&w, "origin");
    let pid = before["pid"].as_u64().unwrap();
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    let old_generation: String = db
        .query_row(
            "SELECT generation FROM process_bindings WHERE pid=?1",
            [pid as i64],
            |r| r.get(0),
        )
        .unwrap();
    let (result, ok) = restart(&w, &[&origin]);
    let (generation, current, hops): (String, String, i64) = db
        .query_row(
            "SELECT generation,current_session_id,hops FROM process_bindings WHERE pid=?1",
            [pid as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        generation, old_generation,
        "this is same-generation transfer, not replacement"
    );
    assert_eq!(current, target);
    assert_eq!(hops, 1);
    assert!(!alive(pid));
    let launches = std::fs::read_to_string(w.path("fifos/launches")).unwrap();
    assert_eq!(
        launches.lines().count(),
        1,
        "must not relaunch old A after ownership moved to B: {launches}; {result}"
    );
    assert!(
        !ok && result["results"][0]["reason"] == "confirmation-stale",
        "{result}"
    );
}

fn owner_transfer_fixture(w: &World) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let text = std::fs::read_to_string(common::fixture("fake-claude"))
        .unwrap()
        .replace(
            "start \"$source\"",
            r#"transfer_on_term() {
  hook session-end "{\"session_id\":\"$sid\",\"reason\":\"resume\"}"
  sid=$(cat "$CC_MOSS_FAKE_DIR/transfer-target")
  start resume
  exit 0
}
trap transfer_on_term TERM
printf '%s\n' "$source" >> "$CC_MOSS_FAKE_DIR/launches"
start "$source""#,
        );
    let path = w.path("transfer-claude");
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn resume_identity_owner_transfer_blocks_rebuild_and_shell_input() {
    for proxy in [true, false] {
        let mut w = if proxy {
            pty_proxy_world("owner-transfer-proxy")
        } else {
            World::new("owner-transfer-shell")
        };
        let target = uuid::Uuid::new_v4().to_string();
        std::fs::write(w.path("fifos/transfer-target"), &target).unwrap();
        let script = owner_transfer_fixture(&w);
        let origin = if proxy {
            let text = std::fs::read_to_string(&w.claude).unwrap().replace(
                common::fixture("fake-claude").to_str().unwrap(),
                script.to_str().unwrap(),
            );
            std::fs::write(&w.claude, text).unwrap();
            w.identity_entry = Some(script.clone());
            w.new_session("origin");
            w.session_id("origin")
        } else {
            w.claude = script.clone();
            w.tmux(&[
                "new-session",
                "-d",
                "-s",
                "shell",
                "-c",
                w.path("work").to_str().unwrap(),
                "bash",
                "--norc",
                "--noprofile",
            ]);
            let pane = w.tmux(&["display-message", "-p", "-t", "shell", "#{pane_id}"]);
            let exports=format!("export HOME='{}' CLAUDE_CONFIG_DIR='{}' CC_MOSS_STATE_DIR='{}' CC_MOSS_CONFIG='{}' CC_MOSS_CLAUDE_BIN='{}' CC_MOSS_BIN='{}' CC_MOSS_FAKE_DIR='{}' CC_MOSS_LANG=en",
                w.path("home").display(),w.path("claude").display(),w.state().display(),w.config_path().display(),script.display(),env!("CARGO_BIN_EXE_cc-moss"),w.path("fifos").display());
            w.tmux(&["send-keys", "-t", pane.trim(), "-l", &exports]);
            w.tmux(&["send-keys", "-t", pane.trim(), "Enter"]);
            let origin = uuid::Uuid::new_v4().to_string();
            let command = format!(
                "'{}' --session-id {} --name=origin",
                script.display(),
                origin
            );
            w.tmux(&["send-keys", "-t", pane.trim(), "-l", &command]);
            w.tmux(&["send-keys", "-t", pane.trim(), "Enter"]);
            w.session_id("origin");
            origin
        };
        let before = session(&w, "origin");
        let pid = before["pid"].as_u64().unwrap();
        let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
        let generation: String = db
            .query_row(
                "SELECT generation FROM process_bindings WHERE pid=?1",
                [pid as i64],
                |r| r.get(0),
            )
            .unwrap();
        let (result, ok) = if proxy {
            let (v, _) = restart(&w, &[&origin]);
            let identity = v["results"][0]["rebuild_identity"].to_string();
            restart(
                &w,
                &[
                    "--rebuild-pane",
                    "--yes",
                    "--rebuild-identity",
                    &identity,
                    &origin,
                ],
            )
        } else {
            restart(&w, &[&origin])
        };
        assert!(!ok, "proxy={proxy}: {result}");
        let (after, owner, hops): (String, String, i64) = db
            .query_row(
                "SELECT generation,current_session_id,hops FROM process_bindings WHERE pid=?1",
                [pid as i64],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(after, generation);
        assert_eq!(owner, target);
        assert_eq!(hops, 1);
        assert!(!alive(pid));
        assert_eq!(
            std::fs::read_to_string(w.path("fifos/launches"))
                .unwrap()
                .lines()
                .count(),
            1,
            "no relaunch for old owner"
        );
        if !proxy {
            let pane = before["tmux"]["pane_id"].as_str().unwrap();
            assert!(
                !w.tmux(&["capture-pane", "-p", "-t", pane])
                    .contains("--resume"),
                "no resume input to original shell"
            );
        }
    }
}
