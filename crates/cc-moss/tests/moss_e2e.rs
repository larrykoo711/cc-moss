//! Moss under a real `cc-moss daemon run`: it comes up as `role: moss`,
//! comes back fresh after its tmux server is killed, and stays down once
//! switched off. Everything runs on a test-only tmux socket (killed at the
//! end), a temporary state dir and config, and the fake claude; the waits
//! are shortened through debug-only switches.

mod common;

use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{ls_json, World};

/// A `cc-moss daemon run` child, killed when dropped (before the world,
/// which then ends the test's tmux server and Moss with it).
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The world's own socket doubles as Moss's: never the real `cc-moss-moss`.
fn moss_cmd(w: &World) -> std::process::Command {
    let mut cmd = w.cc_moss();
    cmd.env("CC_MOSS_MOSS_SOCKET", &w.socket)
        .env("CC_MOSS_MOSS_CHECK_MS", "200")
        .env("CC_MOSS_MOSS_STEPS_MS", "300,600")
        .env("CC_MOSS_DAEMON_HEARTBEAT_MS", "500");
    cmd
}

fn daemon(w: &World) -> Daemon {
    let child = moss_cmd(w)
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

/// The running Moss session in `ls --json`, once there is one.
fn running_moss(w: &World, not: Option<&str>) -> Option<serde_json::Value> {
    ls_json(&w.state(), false)["sessions"]
        .as_array()?
        .iter()
        .find(|s| {
            s["role"] == "moss"
                && s["status"] == "running"
                && not.is_none_or(|old| s["session_id"] != old)
        })
        .cloned()
}

/// Moss's session id once it runs and the daemon counts it as started
/// (a Moss killed within its first half second counts as a failed start).
fn moss_up(w: &World) -> String {
    let s = wait_until("Moss to come up", 15, || running_moss(w, None));
    let sid = s["session_id"].as_str().unwrap().to_string();
    wait_until("the daemon to record the start", 10, || {
        (status(w)["session_id"] == sid.as_str()).then_some(())
    });
    sid
}

fn status(w: &World) -> serde_json::Value {
    let out = moss_cmd(w)
        .args(["moss", "status", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("moss status --json is JSON")
}

fn has_moss(w: &World) -> bool {
    std::process::Command::new("tmux")
        .args(["-L", &w.socket, "has-session", "-t", "=moss"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn log(w: &World) -> String {
    std::fs::read_to_string(w.state().join("daemon/daemon.log")).unwrap_or_default()
}

#[test]
fn moss_comes_up_with_role() {
    let w = World::new("moss-up");
    let _d = daemon(&w);
    let sid = moss_up(&w);
    assert!(has_moss(&w), "a tmux session named moss on the test socket");
    let home = w.state().join("moss");
    // Started in its home, with its files laid out.
    let s = running_moss(&w, None).unwrap();
    let cwd = std::path::PathBuf::from(s["cwd"].as_str().unwrap());
    assert_eq!(cwd.canonicalize().unwrap(), home.canonicalize().unwrap());
    for f in [
        "CLAUDE.md",
        "SECURITY.md",
        "IDENTITY.md",
        "SOUL.md",
        "USER.md",
        ".claude/settings.json",
    ] {
        assert!(home.join(f).is_file(), "{f}");
    }
    let settings = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
    assert!(settings.contains("hook moss-guard || exit 2"), "{settings}");
    wait_until("SESSIONS.md", 10, || {
        home.join("SESSIONS.md").exists().then_some(())
    });
    let st = status(&w);
    assert_eq!(st["schema"], 1);
    assert_eq!(st["enabled"], true);
    assert_eq!(st["running"], true);
    assert_eq!(st["restarts"], 0);
    assert_eq!(st["session_id"], sid.as_str());
    assert!(st["since"].is_string(), "{st}");
    // Other sessions carry no role.
    let tmux_name = s["tmux"]["session_name"].clone();
    assert_eq!(tmux_name, "moss", "never renamed after its Claude name");
}

#[test]
fn moss_comes_back_after_kill() {
    let w = World::new("moss-back");
    let _d = daemon(&w);
    let first = moss_up(&w);
    let out = std::process::Command::new("tmux")
        .args(["-L", &w.socket, "kill-server"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let back = wait_until("Moss to come back", 15, || running_moss(&w, Some(&first)));
    let second = back["session_id"].as_str().unwrap().to_string();
    assert_ne!(first, second, "a fresh session, not a resume");
    // The old record is closed: never offered for restore.
    let all = ls_json(&w.state(), true);
    let old = all["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == first.as_str())
        .expect("the old Moss is still listed with --all");
    assert_eq!(old["status"], "closed", "{old}");
    assert_eq!(old["end_reason"], "moss-restarted", "{old}");
    let interrupted = ls_json(&w.state(), false)["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["status"] == "interrupted")
        .count();
    assert_eq!(interrupted, 0);
    let st = wait_until("the restart to be counted", 5, || {
        let st = status(&w);
        (st["session_id"] == second.as_str()).then_some(st)
    });
    assert_eq!(st["restarts"], 1, "{st}\n{}", log(&w));
    assert!(log(&w).contains("moss: started again"), "{}", log(&w));
}

#[test]
fn moss_stop_keeps_it_down() {
    let w = World::new("moss-stop");
    let _d = daemon(&w);
    let first = moss_up(&w);
    let out = moss_cmd(&w).args(["moss", "stop"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Stopped Moss"),
        "{out:?}"
    );
    assert!(!has_moss(&w));
    // Several checks and the whole (shortened) ladder later, still down.
    std::thread::sleep(Duration::from_millis(2_000));
    assert!(!has_moss(&w), "the daemon left it off");
    assert!(running_moss(&w, None).is_none());
    let st = status(&w);
    assert_eq!(
        (st["enabled"].clone(), st["running"].clone()),
        (false.into(), false.into()),
        "{st}"
    );
    let config = std::fs::read_to_string(w.path("config.toml")).unwrap();
    assert!(config.contains("[moss]\nenabled = false"), "{config}");
    let old = ls_json(&w.state(), true)["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == first.as_str())
        .cloned()
        .unwrap();
    assert_eq!(old["status"], "closed", "{old}");

    // On again: the daemon brings a fresh one up.
    let out = moss_cmd(&w).args(["moss", "start"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let back = wait_until("Moss to start again", 15, || running_moss(&w, Some(&first)));
    assert_ne!(back["session_id"], first.as_str());
    assert_eq!(
        status(&w)["restarts"],
        0,
        "switching on starts the count afresh"
    );
}

#[test]
fn a_restarted_daemon_adopts_the_running_moss() {
    let w = World::new("moss-adopt");
    let d = daemon(&w);
    let first = moss_up(&w);
    drop(d);
    let _again = daemon(&w);
    wait_until("the new daemon to look after Moss", 10, || {
        log(&w)
            .contains("already running; looking after it")
            .then_some(())
    });
    std::thread::sleep(Duration::from_millis(500));
    let s = running_moss(&w, None).unwrap();
    assert_eq!(
        s["session_id"],
        first.as_str(),
        "the same Moss, not a second one"
    );
    let ids: Vec<String> = ls_json(&w.state(), true)["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["role"] == "moss")
        .map(|s| s["session_id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, [first]);
}
