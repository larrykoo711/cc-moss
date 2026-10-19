//! Moss's hands (T2.4) under a real `cc-moss daemon run`: the test plays
//! Moss and asks with `cc-moss moss act … --wait`, as Moss does from its
//! sandbox; the daemon checks, carries out and audits. Sessions are fakes
//! on a test-only tmux socket; state, config and HOME are temporary.

mod common;

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{dead_pid, fixture, ls_json, World};

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn moss_cmd(w: &World) -> Command {
    common::debug_only();
    let mut cmd = w.cc_moss();
    cmd.env("CC_MOSS_MOSS_SOCKET", &w.socket)
        .env("CC_MOSS_MOSS_CHECK_MS", "200")
        .env("CC_MOSS_MOSS_STEPS_MS", "300,600")
        .env("CC_MOSS_DAEMON_HEARTBEAT_MS", "500")
        .env("CC_MOSS_MOSS_REGISTRY_MS", "200");
    cmd
}

fn daemon_with(w: &World, home: Option<&Path>) -> Daemon {
    let mut cmd = moss_cmd(w);
    cmd.env("CC_MOSS_CLAUDE_BIN", fixture("moss-claude"));
    if let Some(home) = home {
        cmd.env("HOME", home);
    }
    let child = cmd
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cc-moss daemon run");
    Daemon(child)
}

fn daemon(w: &World) -> Daemon {
    daemon_with(w, None)
}

fn wait_until<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn session<'a>(all: &'a serde_json::Value, sid: &str) -> Option<&'a serde_json::Value> {
    all["sessions"]
        .as_array()?
        .iter()
        .find(|s| s["session_id"] == sid)
}

/// What `moss act … --wait` printed, and whether it exited 0.
fn act(w: &World, args: &[&str]) -> (serde_json::Value, bool) {
    let out = moss_cmd(w)
        .args(["moss", "act"])
        .args(args)
        .arg("--wait")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let v = serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    (v, out.status.success())
}

fn audit(w: &World) -> Vec<serde_json::Value> {
    std::fs::read_to_string(w.state().join("audit.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// A dropped session, registered by its hooks; its process is gone.
fn dropped(w: &World, sid: &str, cwd: &Path) {
    std::fs::create_dir_all(cwd).unwrap();
    // It said something before it dropped: there is a conversation to resume.
    let transcript = w.path("fifos").join(format!("{sid}.jsonl"));
    std::fs::write(&transcript, "{}\n").unwrap();
    let start = serde_json::json!({
        "session_id": sid,
        "transcript_path": transcript,
        "cwd": cwd,
        "hook_event_name": "SessionStart",
        "source": "startup",
        "session_title": sid.split('-').next().unwrap(),
    });
    let out = common::hook(
        &w.state(),
        "session-start",
        &start.to_string(),
        Some(dead_pid()),
    );
    assert!(out.status.success(), "{out:?}");
    wait_until("the session to show as dropped", 10, || {
        let all = ls_json(&w.state(), true);
        (session(&all, sid)?["status"] == "interrupted").then_some(())
    });
}

/// RFC 3339 for `ms` after the epoch (UTC).
fn rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60,
        ms.rem_euclid(1000)
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Lines of the conversation, as the daemon and `moss say` write them.
fn chat(w: &World, lines: &[serde_json::Value]) {
    let home = w.state().join("moss");
    std::fs::create_dir_all(&home).unwrap();
    let mut text = String::new();
    for l in lines {
        text.push_str(&l.to_string());
        text.push('\n');
    }
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("chat.jsonl"))
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

fn moss_asked(id: &str, at: i64, buttons: &[&str]) -> serde_json::Value {
    serde_json::json!({"schema":1,"id":id,"at":rfc3339(at),"from":"moss",
        "text":"4 个断了的会话，要找回吗？","buttons":buttons})
}

fn you_said(id: &str, at: i64) -> serde_json::Value {
    you_said_text(id, at, "全部找回")
}

fn you_said_text(id: &str, at: i64, text: &str) -> serde_json::Value {
    serde_json::json!({"schema":1,"id":id,"at":rfc3339(at),"from":"you","text":text})
}

/// A Moss question proposing to restore `slot` on 全部找回, as the daemon
/// records it from [[confirm:]].
fn moss_proposed(id: &str, at: i64, slot: &str) -> serde_json::Value {
    let mut v = moss_asked(id, at, &["全部找回", "都不要了"]);
    v["proposals"] = serde_json::json!([{"action":"restore","slot_id":slot,"yes":["全部找回"]}]);
    v
}

fn slot_id(w: &World, sid: &str) -> String {
    session(&ls_json(&w.state(), true), sid).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn moss_up(w: &World) {
    wait_until("Moss to run", 20, || {
        let all = ls_json(&w.state(), false);
        all["sessions"]
            .as_array()?
            .iter()
            .any(|s| s["role"] == "moss" && s["status"] == "running")
            .then_some(())
    });
}

#[test]
fn act_names_a_nameless_session() {
    let w = World::new("act-name");
    let _d = daemon(&w);
    moss_up(&w);
    // A session started by hand, without a name, in the test's tmux.
    let sid = "a1b2c3d4-0000-4000-8000-000000000001";
    let cmd = format!("{} --session-id {sid}", fixture("moss-claude").display());
    let work = w.path("work").to_string_lossy().into_owned();
    w.tmux(&["new-session", "-d", "-s", "raw", "-c", &work, &cmd]);
    wait_until("the session to be recorded", 10, || {
        let all = ls_json(&w.state(), false);
        session(&all, sid).map(|_| ())
    });
    std::thread::sleep(Duration::from_millis(500));
    let (out, ok) = act(&w, &["name", &sid[..8], "work"]);
    assert!(ok, "{out}");
    assert_eq!(out["outcome"], "done", "{out}");
    let all = ls_json(&w.state(), false);
    assert_eq!(session(&all, sid).unwrap()["name"], "work");
    assert!(
        w.session_names().contains(&"host_work".to_string()),
        "{:?}",
        w.session_names()
    );
    let commands = std::fs::read_to_string(w.path("fifos/moss-commands.txt")).unwrap_or_default();
    assert!(commands.contains("/rename work"), "{commands}");
    let audit = audit(&w);
    let line = audit
        .iter()
        .find(|l| l["action"] == "name")
        .expect("audited");
    assert_eq!(line["outcome"], "done");
    assert_eq!(line["by"], "moss");
}

#[test]
fn act_syncs_names() {
    let w = World::new("act-sync-names");
    let _d = daemon(&w);
    moss_up(&w);
    w.new_session("api");
    wait_until("its tmux name", 10, || {
        w.session_names()
            .contains(&"host_api".to_string())
            .then_some(())
    });
    // Renamed in tmux by hand; Moss's own stays as it is.
    w.tmux(&["rename-session", "-t", "=host_api", "scratch"]);
    let (out, ok) = act(&w, &["sync-names"]);
    assert!(ok, "{out}");
    assert_eq!(out["outcome"], "done", "{out}");
    let names = w.session_names();
    assert!(names.contains(&"host_api".to_string()), "{names:?}");
    assert!(names.contains(&"moss".to_string()), "{names:?}");
    assert!(!names.contains(&"scratch".to_string()), "{names:?}");
    assert_eq!(audit(&w)[0]["action"], "sync-names");
}

#[test]
fn act_forgets_a_gone_folder() {
    let w = World::new("act-forget-gone");
    let _d = daemon(&w);
    moss_up(&w);
    let gone = "b1b2b3b4-0000-4000-8000-000000000002";
    let there = "c1c2c3c4-0000-4000-8000-000000000003";
    dropped(&w, gone, &w.path("gone-folder"));
    dropped(&w, there, &w.path("work"));
    std::fs::remove_dir_all(w.path("gone-folder")).unwrap();
    let (out, ok) = act(&w, &["forget-gone", gone]);
    assert!(ok, "{out}");
    assert_eq!(out["outcome"], "done");
    let all = ls_json(&w.state(), true);
    assert_eq!(session(&all, gone).unwrap()["status"], "closed");
    // A folder still there: not gone, nothing done.
    let (out, ok) = act(&w, &["forget-gone", there]);
    assert!(!ok);
    assert_eq!(out["outcome"], "skipped", "{out}");
    let all = ls_json(&w.state(), true);
    assert_eq!(session(&all, there).unwrap()["status"], "interrupted");
    assert_eq!(audit(&w).len(), 2);
}

#[test]
fn act_setup_fix() {
    let w = World::new("act-setup");
    let home = w.path("home");
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let _d = daemon_with(&w, Some(&home));
    moss_up(&w);
    let (out, ok) = act(&w, &["setup-fix"]);
    assert!(ok, "{out}");
    assert_eq!(out["outcome"], "done", "{out}");
    let settings = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
    assert!(settings.contains("hook session-start"), "{settings}");
    assert_eq!(audit(&w)[0]["action"], "setup-fix");
}

#[test]
fn act_refuses_consent_actions_without_your_reply() {
    let w = World::new("act-consent");
    let _d = daemon(&w);
    moss_up(&w);
    let sid = "d1d2d3d4-0000-4000-8000-000000000004";
    dropped(&w, sid, &w.path("work"));
    // No reply at all: the command says so before asking the daemon.
    let out = moss_cmd(&w)
        .args(["moss", "act", "restore", sid, "--wait"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("go-ahead"),
        "{out:?}"
    );
    // And the daemon refuses a request without one, written by hand.
    let home = w.state().join("moss");
    std::fs::create_dir_all(home.join("actions")).unwrap();
    std::fs::write(
        home.join("actions/0000000000001-aaaaaa.json"),
        format!(r#"{{"schema":1,"id":"0000000000001-aaaaaa","at":"x","action":"restore","args":["{sid}"]}}"#),
    )
    .unwrap();
    let result = home.join("actions/0000000000001-aaaaaa.result.json");
    let out: serde_json::Value = wait_until("the daemon's answer", 10, || {
        serde_json::from_str(&std::fs::read_to_string(&result).ok()?).ok()
    });
    assert_eq!(out["outcome"], "refused", "{out}");
    let now = now_ms();
    chat(
        &w,
        &[
            moss_asked("m-old", now - 12 * 60_000, &["全部找回", "都不要了"]),
            you_said("y-old", now - 11 * 60_000),
            moss_asked("m-plain", now - 30_000, &[]),
            you_said("y-plain", now - 20_000),
        ],
    );
    for (because, what) in [
        ("1792000000000-ffffff", "a made-up id"),
        ("y-old", "a reply over ten minutes old"),
        ("y-plain", "a reply to a message without options"),
        ("m-plain", "Moss's own message"),
    ] {
        let (out, ok) = act(&w, &["restore", sid, "--because", because]);
        assert!(!ok, "{what}");
        assert_eq!(out["outcome"], "refused", "{what}: {out}");
    }
    let all = ls_json(&w.state(), true);
    assert_eq!(
        session(&all, sid).unwrap()["status"],
        "interrupted",
        "nothing was restored"
    );
    assert!(audit(&w).iter().all(|l| l["outcome"] == "refused"));
}

#[test]
fn act_restores_with_your_reply() {
    let w = World::new("act-restore");
    let _d = daemon(&w);
    moss_up(&w);
    let sid = "e1e2e3e4-0000-4000-8000-000000000005";
    dropped(&w, sid, &w.path("work"));
    let other = "e9e9e9e9-0000-4000-8000-000000000006";
    dropped(&w, other, &w.path("work"));
    let slot = slot_id(&w, sid);
    let now = now_ms();
    chat(
        &w,
        &[
            moss_proposed("m0", now - 70_000, &slot),
            you_said_text("n0", now - 65_000, "都不要了"),
            moss_proposed("m1", now - 60_000, &slot),
            you_said("y1", now - 5_000),
        ],
    );
    // "No" is not a yes; a yes for one session is not one for another.
    let (out, ok) = act(&w, &["restore", sid, "--because", "n0"]);
    assert!(!ok && out["outcome"] == "refused", "{out}");
    let (out, ok) = act(&w, &["restore", other, "--because", "y1"]);
    assert!(!ok && out["outcome"] == "refused", "{out}");
    let (out, ok) = act(&w, &["restore", sid, "--because", "y1"]);
    assert!(ok, "{out}");
    assert_eq!(out["outcome"], "done", "{out}");
    wait_until("the session to run again", 15, || {
        let all = ls_json(&w.state(), false);
        (session(&all, sid)?["status"] == "running").then_some(())
    });
    // Restored meanwhile: a second go-ahead finds nothing to do.
    let (out, _) = act(&w, &["restore", sid, "--because", "y1"]);
    assert_eq!(out["outcome"], "skipped", "{out}");
    let line = audit(&w)
        .into_iter()
        .find(|l| l["outcome"] == "done")
        .unwrap();
    assert_eq!(line["because"], "y1");
}
