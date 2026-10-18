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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

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
        self.run_with(args, &[])
    }

    /// `run` with more variables (or, with an empty value, fewer).
    fn run_with(&self, args: &[&str], env: &[(&str, &Path)]) -> Output {
        let out = self.command(args, env).output().unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        out
    }

    fn command(&self, args: &[&str], env: &[(&str, &Path)]) -> Command {
        let mut cmd = bin();
        cmd.stdin(Stdio::null())
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
            .args(args);
        for (k, v) in env {
            if v.as_os_str().is_empty() {
                cmd.env_remove(k);
            } else {
                cmd.env(k, v);
            }
        }
        cmd
    }

    /// The daemon as the status file has it, running `version` in a live
    /// process with a fresh heartbeat.
    fn pretend_running(&self, version: &str) -> FakeProcess {
        self.pretend_running_in(&self.path("state"), version)
    }

    fn pretend_running_in(&self, state: &Path, version: &str) -> FakeProcess {
        let p = FakeProcess::spawn();
        std::fs::create_dir_all(state.join("daemon")).unwrap();
        std::fs::write(
            state.join("daemon/status.json"),
            serde_json::json!({
                "schema": 1, "pid": p.pid().unwrap(), "pid_start": null, "version": version,
                "started_ms": now_ms(), "heartbeat_ms": now_ms()
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

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A shell with its own CC_MOSS_STATE_DIR sees the daemon where its
/// LaunchAgent puts it, and `start` there does not move it.
#[test]
fn status_and_start_follow_the_installed_agent() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    let _d = mac.pretend_running(env!("CARGO_PKG_VERSION"));
    let elsewhere = mac.path("elsewhere");
    let env = [("CC_MOSS_STATE_DIR", elsewhere.as_path())];
    let out = mac.run_with(&["daemon", "status", "--json"], &env);
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["running"], true, "{r}");

    let plist = std::fs::read_to_string(mac.plist()).unwrap();
    let n = mac.calls().len();
    let out = mac.run_with(&["daemon", "start"], &env);
    assert!(
        stdout(&out).contains("Kept it that way"),
        "{}",
        stdout(&out)
    );
    assert_eq!(std::fs::read_to_string(mac.plist()).unwrap(), plist);
    assert!(
        !mac.calls()[n..].iter().any(|c| c == "bootout"),
        "{:?}",
        mac.calls()
    );
}

#[test]
fn daemon_run_refuses_root() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut child = daemon_cmd(&state)
        .env("SUDO_USER", "someone")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the daemon kept running under sudo");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("without sudo"),
        "{out:?}"
    );
    assert!(!state.join("daemon").exists(), "root left files behind");
}

/// A home that is not this account's (every test's) gets the plist but no
/// launchd, and install and uninstall say so.
#[test]
fn a_home_launchd_never_reads_is_told() {
    let mac = Mac::new();
    let none = [("CC_MOSS_LAUNCHD_DIR", Path::new(""))];
    let out = mac.run_with(&["install", "--yes"], &none);
    let plist = mac
        .path("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"));
    assert!(plist.exists());
    assert!(
        stdout(&out).contains("did not start the cc-moss daemon"),
        "{}",
        stdout(&out)
    );
    let out = mac.run_with(&["install", "--uninstall", "--yes"], &none);
    assert!(!plist.exists());
    assert!(
        stdout(&out).contains("launchd was not asked to stop the daemon"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn uninstall_under_sudo_leaves_the_daemon_alone() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    let n = mac.calls().len();
    let out = mac.run_with(
        &["install", "--uninstall", "--yes"],
        &[("SUDO_USER", Path::new("someone"))],
    );
    assert!(stdout(&out).contains("Running as root"), "{}", stdout(&out));
    assert!(mac.plist().exists());
    assert_eq!(mac.calls().len(), n);
}

#[test]
fn install_goes_on_when_the_daemon_cannot_be_set_up() {
    let mac = Mac::new();
    // A state directory that is not absolute: the LaunchAgent cannot pin it.
    let out = mac.run_with(
        &["install", "--yes"],
        &[("CC_MOSS_STATE_DIR", Path::new("relative/state"))],
    );
    let said = stdout(&out);
    assert!(said.contains("Left the cc-moss daemon out"), "{said}");
    assert!(
        said.contains("All set") || said.contains("Almost there"),
        "the rest of install ran: {said}"
    );
    assert!(!mac.plist().exists());
    assert!(
        !Path::new("relative").exists(),
        "nothing was laid out relative to where install ran (Moss's home)"
    );
}

#[test]
fn uninstall_goes_on_when_launchd_will_not_stop_it() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    let out = mac
        .command(&["install", "--uninstall", "--yes"], &[])
        .env("CC_MOSS_LAUNCHCTL_FAIL", "bootout")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let said = stdout(&out);
    assert!(said.contains("Could not stop the cc-moss daemon"), "{said}");
    assert!(!mac.plist().exists(), "the plist goes anyway");
    assert!(
        said.contains("Your session records stay"),
        "later steps ran: {said}"
    );
}

#[test]
fn uninstall_forgets_a_stop() {
    let mac = Mac::new();
    mac.run(&["install", "--yes"]);
    mac.run(&["daemon", "stop"]);
    mac.run(&["install", "--uninstall", "--yes"]);
    assert_eq!(mac.calls().last().unwrap(), "enable");
    let out = mac.run(&["install", "--yes"]);
    assert!(!stdout(&out).contains("stays stopped"), "{}", stdout(&out));
    assert_eq!(
        mac.calls().last().unwrap(),
        &format!("bootstrap {}", mac.plist().display())
    );
}

/// What launchd's copy does while a `daemon run` in a terminal holds the
/// lock: wait before exiting (launchd starts it again right after), and
/// write to daemon.log only, not to launchd's stderr.log.
#[test]
fn a_launchd_copy_waits_quietly_while_another_runs() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let first = spawn(&state, None);
    wait_running(&state, first.pid());
    let started = Instant::now();
    let out = daemon_cmd(&state)
        .env("XPC_SERVICE_NAME", LABEL)
        .env("CC_MOSS_DAEMON_BUSY_WAIT_MS", "400")
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(started.elapsed() >= Duration::from_millis(400), "it waited");
    assert!(out.stderr.is_empty(), "{out:?}");
    let log = std::fs::read_to_string(state.join("daemon/daemon.log")).unwrap();
    assert!(log.contains("another daemon is running"), "{log}");
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
