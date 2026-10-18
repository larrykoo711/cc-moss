//! Helpers for driving the cc-moss binary against a throwaway state dir.

#![allow(dead_code)]

pub mod app;
pub mod releases;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cc-moss"));
    // Never inherit the caller's tmux or Claude session.
    for var in [
        "TMUX",
        "TMUX_PANE",
        "CLAUDE_PID",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_SESSION_ATTENDED",
        "CLAUDE_CONFIG_DIR",
    ] {
        cmd.env_remove(var);
    }
    // Never let a test reach the real ~/.zshrc, ~/.claude or ~/.config.
    cmd.env("HOME", "/nonexistent/cc-moss-test-home")
        .env_remove("ZDOTDIR")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("CC_MOSS_CONFIG")
        .env("CC_MOSS_CLAUDE_BIN", "/nonexistent/claude")
        // Never the machine's own managed Claude Code settings.
        .env(
            "CC_MOSS_CLAUDE_MANAGED_SETTINGS",
            "/nonexistent/cc-moss-test-managed-settings.json",
        )
        // Assertions are in English, whatever this machine's language is.
        .env("CC_MOSS_LANG", "en")
        // No test reaches the network; update tests turn checking back on
        // with a stub curl on PATH.
        .env("CC_MOSS_NO_UPDATE_CHECK", "1")
        // Never the real launchctl: a debug build panics if it gets there.
        .env("CC_MOSS_FORBID_LAUNCHCTL", "1")
        // Not root unless a test says so, and never the real /Applications.
        .env_remove("SUDO_USER")
        .env(
            "CC_MOSS_SYSTEM_APPLICATIONS",
            "/nonexistent/cc-moss-test-applications",
        );
    cmd
}

/// Tests that steer the binary with its debug-only switches (Moss's tmux
/// socket, shortened waits) refuse to run against a release build, which
/// ignores them and would act on the real thing.
pub fn debug_only() {
    if !cfg!(debug_assertions) {
        panic!("this test drives debug-only switches of cc-moss; run it without --release");
    }
}

pub fn hook(state: &Path, event: &str, stdin: &str, pid: Option<u32>) -> Output {
    let mut cmd = bin();
    cmd.env("CC_MOSS_STATE_DIR", state)
        .args(["hook", event])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(pid) = pid {
        cmd.env("CLAUDE_PID", pid.to_string());
    }
    let mut child = cmd.spawn().expect("spawn cc-moss hook");
    // A hook may exit without reading its input (an event it does not know):
    // the write then races the exit and can hit a closed pipe, which is fine.
    match child.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(e) => panic!("writing the hook's stdin: {e}"),
    }
    child.wait_with_output().unwrap()
}

pub fn start_json(session_id: &str, source: &str, title: Option<&str>) -> String {
    serde_json::json!({
        "session_id": session_id,
        "transcript_path": format!("/nonexistent/{session_id}.jsonl"),
        "cwd": "/work",
        "hook_event_name": "SessionStart",
        "source": source,
        "session_title": title,
    })
    .to_string()
}

pub fn end_json(session_id: &str, reason: &str) -> String {
    serde_json::json!({
        "session_id": session_id,
        "hook_event_name": "SessionEnd",
        "reason": reason,
    })
    .to_string()
}

pub fn ls_json(state: &Path, all: bool) -> serde_json::Value {
    let mut cmd = bin();
    cmd.env("CC_MOSS_STATE_DIR", state).args(["ls", "--json"]);
    if all {
        cmd.arg("--all");
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "ls failed: {out:?}");
    serde_json::from_slice(&out.stdout).expect("ls --json is valid JSON")
}

/// A pid that existed a moment ago and is now gone.
pub fn dead_pid() -> u32 {
    let mut child = Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// A stand-in for one Claude Code process: alive until dropped.
pub struct FakeProcess(std::process::Child);

impl FakeProcess {
    pub fn spawn() -> Self {
        FakeProcess(Command::new("sleep").arg("60").spawn().unwrap())
    }

    pub fn pid(&self) -> Option<u32> {
        Some(self.0.id())
    }
}

impl Drop for FakeProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One isolated world: tmux socket, state dir, config, fake claude.
pub struct World {
    pub socket: String,
    pub dir: tempfile::TempDir,
    /// Fake claude for `cc-moss new` / `restore`.
    pub claude: PathBuf,
}

impl World {
    pub fn new(test: &str) -> Self {
        let w = World {
            socket: format!("cc-moss-test-{}-{test}", std::process::id()),
            dir: tempfile::tempdir().unwrap(),
            claude: fixture("fake-claude"),
        };
        std::fs::create_dir_all(w.path("fifos")).unwrap();
        std::fs::create_dir_all(w.path("work")).unwrap();
        std::fs::write(
            w.path("config.toml"),
            "[claude]\ndefault_args = [\"--dangerously-skip-permissions\"]\n",
        )
        .unwrap();
        w
    }

    pub fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    pub fn cc_moss(&self) -> Command {
        let mut cmd = bin();
        cmd.env("CC_MOSS_STATE_DIR", self.path("state"))
            .env("CC_MOSS_CONFIG", self.path("config.toml"))
            .env("CC_MOSS_TMUX_SOCKET", &self.socket)
            .env("CC_MOSS_HOST_PREFIX", "host")
            .env("CC_MOSS_CLAUDE_BIN", &self.claude)
            .env("CC_MOSS_BIN", env!("CARGO_BIN_EXE_cc-moss"))
            .env("CC_MOSS_FAKE_DIR", self.path("fifos"));
        cmd
    }

    pub fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .args(["-L", &self.socket])
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn new_session(&self, name: &str) {
        let out = self
            .cc_moss()
            .args(["new", name, "--detach", "-C"])
            .arg(self.path("work"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }

    pub fn state(&self) -> PathBuf {
        self.path("state")
    }

    /// Session id of the slot named `name`, once its SessionStart arrived.
    pub fn session_id(&self, name: &str) -> String {
        wait_for(|| {
            ls_json(&self.state(), true)["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["name"] == name)
                .map(|s| s["session_id"].as_str().unwrap().to_string())
        })
    }

    pub fn send(&self, session_id: &str, line: &str) {
        let fifo = self.path("fifos").join(format!("{session_id}.fifo"));
        wait_for(|| fifo.exists().then_some(()));
        std::fs::write(fifo, format!("{line}\n")).unwrap();
    }

    /// Send the first prompt and wait until the fake claude has written its
    /// transcript: a session killed before that has nothing to restore.
    pub fn first_prompt(&self, session_id: &str, name: &str) {
        self.send(session_id, &format!("prompt {name}"));
        let transcript = self.path("fifos").join(format!("{session_id}.jsonl"));
        wait_for(|| transcript.exists().then_some(()));
    }

    pub fn session_names(&self) -> Vec<String> {
        self.tmux(&["list-sessions", "-F", "#{session_name}"])
            .lines()
            .map(String::from)
            .collect()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
        // tmux leaves the socket file behind, also when a test killed the
        // server itself; it lives at $TMUX_TMPDIR/tmux-<uid>/<name>.
        let uid = Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let base = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let _ = std::fs::remove_file(
            Path::new(&base)
                .join(format!("tmux-{uid}"))
                .join(&self.socket),
        );
    }
}

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting");
        std::thread::sleep(Duration::from_millis(50));
    }
}
