//! `cc-moss daemon`: one at a time, honest status, back after `kill -9`, and
//! the LaunchAgent install puts in and uninstall takes out. Everything runs
//! in a temporary state dir and home; launchctl is never called (the
//! `CC_MOSS_LAUNCHD_DIR` switch records the calls instead).

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::{bin, dead_pid, FakeProcess};

const LABEL: &str = "io.github.larrykoo711.cc-moss.daemon";

/// A `cc-moss daemon run` child, killed when dropped.
struct Daemon(Child);

impl Daemon {
    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon_cmd(state: &Path) -> Command {
    let mut cmd = bin();
    cmd.env("CC_MOSS_STATE_DIR", state)
        .args(["daemon", "run"])
        .stdin(Stdio::null());
    cmd
}

fn spawn(state: &Path, heartbeat_ms: Option<u64>) -> Daemon {
    let mut cmd = daemon_cmd(state);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(ms) = heartbeat_ms {
        cmd.env("CC_MOSS_DAEMON_HEARTBEAT_MS", ms.to_string());
    }
    Daemon(cmd.spawn().expect("spawn cc-moss daemon run"))
}

fn status(state: &Path) -> serde_json::Value {
    let out = bin()
        .env("CC_MOSS_STATE_DIR", state)
        .args(["daemon", "status", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("status --json is JSON")
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn wait_running(state: &Path, pid: u32) -> serde_json::Value {
    let mut last = serde_json::Value::Null;
    wait_until("the daemon to report running", || {
        last = status(state);
        last["running"] == true && last["pid"] == pid
    });
    last
}

fn status_file(state: &Path) -> PathBuf {
    state.join("daemon/status.json")
}

fn stored(state: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(status_file(state)).unwrap()).unwrap()
}

#[test]
fn second_daemon_exits() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let first = spawn(&state, None);
    wait_running(&state, first.pid());
    let before = stored(&state);

    let mut second = daemon_cmd(&state)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while second.try_wait().unwrap().is_none() {
        if Instant::now() > until {
            let _ = second.kill();
            let _ = second.wait();
            panic!("the second daemon kept running next to the first");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = second.wait_with_output().unwrap();
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("already running") && stderr.contains(&first.pid().to_string()),
        "{stderr}"
    );
    let after = stored(&state);
    assert_eq!(after["pid"], before["pid"]);
    assert_eq!(after["started_ms"], before["started_ms"]);
    assert_eq!(status(&state)["pid"], first.pid());
}

#[test]
fn status_reports_stale_pid() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");

    // Never started: no status file.
    assert_eq!(
        status(&state),
        serde_json::json!({"schema": 1, "running": false})
    );

    // Running.
    let d = spawn(&state, None);
    let r = wait_running(&state, d.pid());
    assert_eq!(r["schema"], 1);
    assert_eq!(r["version"], env!("CARGO_PKG_VERSION"));
    for key in ["started_at", "heartbeat_at"] {
        assert!(r[key].as_str().is_some_and(|s| s.contains('T')), "{r}");
    }
    drop(d);

    // Killed without a word: the file stays, and is not believed.
    std::fs::write(
        status_file(&state),
        serde_json::json!({
            "schema": 1, "pid": dead_pid(), "pid_start": null, "version": "0.2.2",
            "started_ms": 1_792_310_400_000_i64, "heartbeat_ms": 1_792_310_400_000_i64
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        status(&state),
        serde_json::json!({"schema": 1, "running": false})
    );

    // A pid that is alive but started at another time is someone else's.
    let other = FakeProcess::spawn();
    std::fs::write(
        status_file(&state),
        serde_json::json!({
            "schema": 1, "pid": other.pid().unwrap(), "pid_start": "Thu Jan  1 00:00:00 1970",
            "version": "0.2.2", "started_ms": 0, "heartbeat_ms": 0
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(status(&state)["running"], false);
}

#[test]
fn human_status_says_running_or_not() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let text = |state: &Path| {
        let out = bin()
            .env("CC_MOSS_STATE_DIR", state)
            .args(["daemon", "status"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert!(text(&state).contains("not running"), "{}", text(&state));
    let d = spawn(&state, None);
    wait_running(&state, d.pid());
    let said = text(&state);
    assert!(
        said.contains("is running") && said.contains(&d.pid().to_string()),
        "{said}"
    );
}

/// What launchd does with `KeepAlive`, in miniature: start the daemon again
/// whenever it is gone.
#[test]
fn killed_daemon_comes_back_under_a_watcher() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut d = spawn(&state, Some(200));
    let first = wait_running(&state, d.pid());

    // The heartbeat moves on.
    let beat = first["heartbeat_at"].clone();
    wait_until("a heartbeat", || status(&state)["heartbeat_at"] != beat);

    Command::new("kill")
        .args(["-9", &d.pid().to_string()])
        .status()
        .unwrap();
    let _ = d.0.wait();
    assert!(status_file(&state).exists(), "kill -9 leaves the file");
    assert_eq!(status(&state)["running"], false);

    // The watcher's turn: the lock went with the old process.
    d = spawn(&state, Some(200));
    let again = wait_running(&state, d.pid());
    assert_ne!(again["pid"], first["pid"]);
    assert_eq!(again["version"], first["version"]);
    assert_ne!(again["started_at"], first["started_at"]);
}

#[test]
fn sigterm_stops_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut d = spawn(&state, None);
    wait_running(&state, d.pid());
    Command::new("kill")
        .args(["-TERM", &d.pid().to_string()])
        .status()
        .unwrap();
    let code = d.0.wait().unwrap();
    assert!(code.success(), "{code:?}");
    assert!(!status_file(&state).exists());
    let log = std::fs::read_to_string(state.join("daemon/daemon.log")).unwrap();
    assert!(
        log.contains("started: pid") && log.contains("stopping"),
        "{log}"
    );
}

// ---------------------------------------------------------------- install

struct Mac {
    dir: tempfile::TempDir,
}

impl Mac {
    fn new() -> Mac {
        Mac {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn agents(&self) -> PathBuf {
        self.path("LaunchAgents")
    }

    fn plist(&self) -> PathBuf {
        self.agents().join(format!("{LABEL}.plist"))
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.agents().join("launchctl.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn run(&self, args: &[&str]) -> Output {
        let out = bin()
            .stdin(Stdio::null())
            .env("HOME", self.dir.path())
            .env("PATH", "/usr/bin:/bin")
            .env("SHELL", "/bin/zsh")
            .env("CC_MOSS_STATE_DIR", self.path("state"))
            .env("CC_MOSS_PLATFORM", "macos")
            .env("CC_MOSS_BUNDLED_APP", "")
            .env(
                "CC_MOSS_SYSTEM_APPLICATIONS",
                self.path("system-Applications"),
            )
            .env("CC_MOSS_LAUNCHD_DIR", self.agents())
            .env_remove("STARSHIP_CONFIG")
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        out
    }

    /// The daemon as the status file has it, running `version` in a live process.
    fn pretend_running(&self, version: &str) -> FakeProcess {
        let p = FakeProcess::spawn();
        std::fs::create_dir_all(self.path("state/daemon")).unwrap();
        std::fs::write(
            self.path("state/daemon/status.json"),
            serde_json::json!({
                "schema": 1, "pid": p.pid().unwrap(), "pid_start": null, "version": version,
                "started_ms": 0, "heartbeat_ms": 0
            })
            .to_string(),
        )
        .unwrap();
        p
    }
}

#[test]
fn install_places_and_starts_the_agent() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    let text = std::fs::read_to_string(mac.plist()).unwrap();
    assert!(
        text.contains(&format!("<string>{LABEL}</string>")),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "<string>{}</string>\n        <string>daemon</string>",
            env!("CARGO_BIN_EXE_cc-moss")
        )),
        "{text}"
    );
    assert!(text.contains(&format!(
        "<string>{}</string>",
        mac.path("state/daemon/stdout.log").display()
    )));
    assert_eq!(
        mac.calls(),
        [format!("bootstrap {}", mac.plist().display())]
    );
    // Nothing changed, nothing running: launchd is asked to start it.
    mac.run(&["install", "--yes"]);
    assert_eq!(mac.calls().last().unwrap(), "kickstart");
}

#[test]
fn install_kickstarts_after_upgrade() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    let _old = mac.pretend_running("0.0.1");
    mac.run(&["install", "--yes"]);
    assert_eq!(mac.calls().last().unwrap(), "kickstart -k");

    // Same version, running: nothing to do.
    let _current = mac.pretend_running(env!("CARGO_PKG_VERSION"));
    let n = mac.calls().len();
    mac.run(&["install", "--yes"]);
    assert_eq!(mac.calls().len(), n);

    // A different plist (another cc-moss path) is loaded afresh.
    std::fs::write(mac.plist(), "old").unwrap();
    mac.run(&["install", "--yes"]);
    assert_eq!(
        mac.calls()[n..],
        [
            "bootout".to_string(),
            format!("bootstrap {}", mac.plist().display())
        ]
    );
}

#[test]
fn install_respects_a_stopped_daemon() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    mac.run(&["daemon", "stop"]);
    assert_eq!(mac.calls()[1..], ["disable", "bootout"]);
    let out = mac.run(&["install", "--yes"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("stays stopped"));
    assert_eq!(mac.calls().len(), 3);
    mac.run(&["daemon", "start"]);
    assert_eq!(
        mac.calls()[3..],
        [
            "enable".to_string(),
            format!("bootstrap {}", mac.plist().display())
        ]
    );
}

#[test]
fn uninstall_removes_daemon_agent() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    assert!(mac.plist().exists());
    let out = mac.run(&["install", "--uninstall", "--yes"]);
    assert!(!mac.plist().exists(), "{out:?}");
    assert_eq!(mac.calls().last().unwrap(), "bootout");
    // Once more: nothing left, nothing called.
    let n = mac.calls().len();
    mac.run(&["install", "--uninstall", "--yes"]);
    assert_eq!(mac.calls().len(), n);
}

#[test]
fn dry_run_touches_nothing() {
    let mac = Mac::new();
    let out = mac.run(&["install", "--dry-run"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Would set up the cc-moss daemon"));
    assert!(!mac.plist().exists());
    assert!(mac.calls().is_empty());
}

#[test]
fn linux_gets_no_agent() {
    let mac = Mac::new();
    let out = bin()
        .env("HOME", mac.dir.path())
        .env("CC_MOSS_STATE_DIR", mac.path("state"))
        .env("CC_MOSS_PLATFORM", "other")
        .env("CC_MOSS_LAUNCHD_DIR", mac.agents())
        .args(["daemon", "start"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("macOS only"));
    assert!(mac.calls().is_empty());
}
