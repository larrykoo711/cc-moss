//! Talking to Moss under a real `cc-moss daemon run`: `moss say` drops a
//! message, the daemon pastes it into Moss's terminal, takes the reply from
//! Moss's transcript into chat.jsonl, and tells Moss when another session
//! drops. Moss is a fake (`tests/fixtures/moss-claude`) that answers in its
//! transcript and runs the Stop hook from its settings, like Claude Code.
//! Everything runs on a test-only tmux socket, a temporary state dir and
//! config; the waits are shortened through debug-only switches.

mod common;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{fixture, ls_json, World};

/// A `cc-moss daemon run` child, killed when dropped.
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
        .env("CC_MOSS_MOSS_REGISTRY_MS", "200")
        .env("CC_MOSS_MOSS_LOOK_MS", "200")
        .env("CC_MOSS_MOSS_SETTLE_MS", "500")
        .env("CC_MOSS_MOSS_TURN_MS", "1500")
        .env("CC_MOSS_DIGEST_WINDOW_MS", "500");
    cmd
}

fn daemon(w: &World) -> Daemon {
    let child = moss_cmd(w)
        .env("CC_MOSS_CLAUDE_BIN", fixture("moss-claude"))
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cc-moss daemon run");
    Daemon(child)
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

fn json(w: &World, args: &[&str]) -> serde_json::Value {
    let out = moss_cmd(w).args(args).output().unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{args:?}: {e}: {out:?}"))
}

fn status(w: &World) -> serde_json::Value {
    json(w, &["moss", "status", "--json"])
}

fn messages(w: &World) -> Vec<serde_json::Value> {
    let chat = json(w, &["moss", "chat", "--json"]);
    assert_eq!(chat["schema"], 1);
    chat["messages"].as_array().unwrap().clone()
}

fn say(w: &World, text: &str) -> String {
    let v = json(w, &["moss", "say", "--json", "--", text]);
    v["id"].as_str().unwrap().to_string()
}

fn log(w: &World) -> String {
    std::fs::read_to_string(w.state().join("daemon/daemon.log")).unwrap_or_default()
}

/// Moss runs, its first turn is over, and the daemon sees it idle.
fn moss_idle(w: &World) {
    // Its first turn (the boot prompt) came in.
    wait_until("Moss to hear its boot prompt", 20, || {
        (!heard(w).is_empty()).then_some(())
    });
    wait_until("Moss to be idle", 20, || {
        (status(w)["activity"] == "idle").then_some(())
    });
}

fn reply_to(w: &World, id: &str) -> serde_json::Value {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(m) = messages(w)
            .into_iter()
            .find(|m| m["reply_to"] == id && m["from"] == "moss")
        {
            return m;
        }
        if Instant::now() > until {
            panic!(
                "no reply to {id}\nchat: {:?}\nlog:\n{}\nscreen:\n{}",
                messages(w),
                log(w),
                w.tmux(&["capture-pane", "-p", "-t", "=moss:"])
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn release(w: &World) {
    std::fs::write(w.path("fifos").join("release"), "").unwrap();
}

fn heard(w: &World) -> String {
    std::fs::read_to_string(w.path("fifos").join("moss-heard.txt")).unwrap_or_default()
}

#[test]
fn say_gets_a_reply() {
    let w = World::new("chat-reply");
    let _d = daemon(&w);
    moss_idle(&w);
    let id = say(&w, "hello moss, buttons please");
    assert_eq!(id.len(), 20, "{id}");
    let reply = reply_to(&w, &id);
    assert_eq!(reply["text"], "echo: hello moss, buttons please", "{reply}");
    assert_eq!(reply["buttons"], serde_json::json!(["Yes", "No"]));
    assert_eq!(reply["fallback"], false);
    let mine = messages(&w)
        .into_iter()
        .find(|m| m["id"] == id.as_str())
        .unwrap();
    assert_eq!(mine["from"], "you");
    assert_eq!(mine["state"], "replied", "{mine}");

    // Two lines arrive as one message; two messages go in one after the other.
    let first = say(&w, "line one\nline two");
    let second = say(&w, "and then this");
    assert_eq!(reply_to(&w, &first)["text"], "echo: line one\nline two");
    assert_eq!(reply_to(&w, &second)["text"], "echo: and then this");
    let ids: Vec<String> = messages(&w)
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    let at = |id: &str| ids.iter().position(|i| i == id).unwrap();
    assert!(at(&first) < at(&second));
    let st = status(&w);
    assert_eq!(st["queued"], 0, "{st}");
    assert!(st["last_reply_at"].is_string(), "{st}");
    assert!(heard(&w).contains("line one\nline two\n"), "{}", heard(&w));
    assert!(!log(&w).contains("did not go in"), "{}", log(&w));
}

#[test]
fn moss_hears_a_disconnect() {
    let w = World::new("chat-digest");
    let _d = daemon(&w);
    moss_idle(&w);
    // An ordinary session, then its process is killed.
    w.new_session("api");
    let sid = w.session_id("api");
    let pid = wait_until("the session's pid", 10, || {
        ls_json(&w.state(), true)["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["session_id"] == sid.as_str())?["pid"]
            .as_u64()
    });
    let started = Instant::now();
    let out = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let news = wait_until("the news in the chat", 30, || {
        messages(&w).into_iter().find(|m| {
            m["from"] == "events"
                && m["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("api dropped"))
        })
    });
    assert_eq!(news["text"], "[Update] api dropped", "{news}");
    wait_until("Moss to hear it", 30, || {
        heard(&w).contains("[Update] api dropped").then_some(())
    });
    assert!(started.elapsed() < Duration::from_secs(30));
    // Its answer was silent: nothing from Moss shows for it.
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        !messages(&w)
            .iter()
            .any(|m| m["from"] == "moss" && m["reply_to"] == news["id"]),
        "{:?}",
        messages(&w)
    );
    // Moss itself is never news.
    assert_eq!(heard(&w).matches("[Update]").count(), 1, "{}", heard(&w));
}

#[test]
fn moss_status_json_activity() {
    let w = World::new("chat-activity");
    // On, but no daemon: offline. Switched off: off.
    let st = status(&w);
    assert_eq!(
        (st["activity"].clone(), st["queued"].clone()),
        ("offline".into(), 0.into()),
        "{st}"
    );
    let refused = moss_cmd(&w)
        .args(["moss", "say", "--", "hi"])
        .output()
        .unwrap();
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("daemon"),
        "{refused:?}"
    );
    assert!(moss_cmd(&w)
        .args(["moss", "stop"])
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(status(&w)["activity"], "off");
    let off = moss_cmd(&w)
        .args(["moss", "say", "--", "hi"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&off.stderr).contains("Moss is off"),
        "{off:?}"
    );
    assert!(moss_cmd(&w)
        .args(["moss", "start"])
        .output()
        .unwrap()
        .status
        .success());

    let _d = daemon(&w);
    moss_idle(&w);
    // A long turn: thinking, and what comes meanwhile waits in the inbox.
    let slow = say(&w, "a slow one");
    wait_until("thinking", 20, || {
        (status(&w)["activity"] == "thinking").then_some(())
    });
    let next = say(&w, "the next one");
    std::thread::sleep(Duration::from_millis(700));
    let st = status(&w);
    assert_eq!(st["queued"], 1, "{st}");
    assert_eq!(st["activity"], "thinking", "{st}");
    release(&w);
    reply_to(&w, &slow);
    reply_to(&w, &next);
    moss_idle(&w);

    // Stopped at a confirm prompt past the turn limit: waiting, no Esc.
    let confirm = say(&w, "confirm this");
    let waiting = w.state().join("moss/waiting");
    wait_until("waiting", 20, || {
        (status(&w)["activity"] == "waiting").then_some(())
    });
    assert!(waiting.exists());
    release(&w);
    assert_eq!(reply_to(&w, &confirm)["text"], "echo: confirm this");
    moss_idle(&w);
    assert!(!waiting.exists());
}

#[test]
fn moss_stop_hook_marks_the_turn() {
    let w = World::new("chat-stop-hook");
    let home: PathBuf = w.state().join("moss");
    let out = w
        .cc_moss()
        .args(["hook", "moss-stop", "--home"])
        .arg(&home)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    let marks = std::fs::read_dir(home.join("stop")).unwrap().count();
    assert_eq!(marks, 1);
    // Without a home it does nothing, and still exits 0.
    let out = w
        .cc_moss()
        .args(["hook", "moss-stop"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}
