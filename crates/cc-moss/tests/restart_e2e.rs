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
    let exports = format!(
        "export CC_MOSS_STATE_DIR='{}' CC_MOSS_BIN='{}' CC_MOSS_FAKE_DIR='{}' CC_MOSS_LANG=en",
        w.state().display(),
        env!("CARGO_BIN_EXE_cc-moss"),
        w.path("fifos").display()
    );
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
    assert_eq!(v["results"][0]["reason"], "failed", "{v}");
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
    let exports = format!(
        "export CC_MOSS_STATE_DIR='{}' CC_MOSS_BIN='{}' CC_MOSS_FAKE_DIR='{}' CC_MOSS_LANG=en",
        w.state().display(),
        env!("CARGO_BIN_EXE_cc-moss"),
        w.path("fifos").display()
    );
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
