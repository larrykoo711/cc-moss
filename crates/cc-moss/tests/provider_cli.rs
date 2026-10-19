//! The API provider (T2.6): what a session started with, the daemon's
//! `provider-changed` event, `ls --json` and, above all, that no token or
//! key value is ever written or printed. Temporary settings, state and HOME.

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{bin, FakeProcess};

const SECRET: &str = "sk-ant-api03-SECRETVALUE-never-recorded";
const PASSWORD: &str = "hunter2-SECRETPASS";

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Setup {
    dir: tempfile::TempDir,
}

impl Setup {
    fn new() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("claude")).unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                "[claude]\nconfig_dir = \"{}\"\n",
                dir.path().join("claude").display()
            ),
        )
        .unwrap();
        Setup { dir }
    }

    fn state(&self) -> std::path::PathBuf {
        self.dir.path().join("state")
    }

    fn settings(&self, text: &str) {
        // Whole, as cc-switch writes it.
        let tmp = self.dir.path().join("claude/settings.json.tmp");
        std::fs::write(&tmp, text).unwrap();
        std::fs::rename(&tmp, self.dir.path().join("claude/settings.json")).unwrap();
    }

    fn cmd(&self) -> std::process::Command {
        let mut cmd = bin();
        cmd.env("CC_MOSS_STATE_DIR", self.state())
            .env("CC_MOSS_CONFIG", self.dir.path().join("config.toml"));
        cmd
    }

    fn start(&self, sid: &str, pid: Option<u32>) {
        let mut child = self
            .cmd()
            .env("CLAUDE_CONFIG_DIR", self.dir.path().join("claude"))
            .env("CLAUDE_PID", pid.unwrap().to_string())
            .args(["hook", "session-start"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(common::start_json(sid, "startup", Some(sid)).as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }

    fn out(&self, args: &[&str]) -> String {
        let out = self.cmd().args(args).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        serde_json::from_str(&self.out(args)).unwrap()
    }
}

fn third_party(host: &str) -> String {
    format!(
        r#"{{"env":{{"ANTHROPIC_BASE_URL":"https://user:{PASSWORD}@{host}:8443/api",
            "ANTHROPIC_AUTH_TOKEN":"{SECRET}","ANTHROPIC_DEFAULT_SONNET_MODEL":"glm-5"}},
            "apiKeyHelper":"echo {SECRET}","theme":"dark"}}"#
    )
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

/// Every byte under `dir`, file by file.
fn all_files(dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            all_files(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push((path.display().to_string(), bytes));
        }
    }
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn provider_changed(s: &Setup, after: i64) -> Option<serde_json::Value> {
    s.json(&["events", "--json", "--since", &after.to_string()])["events"]
        .as_array()?
        .iter()
        .find(|e| e["kind"] == "provider-changed")
        .cloned()
}

fn last_seq(s: &Setup) -> i64 {
    s.json(&["events", "--json"])["events"]
        .as_array()
        .and_then(|e| e.last())
        .and_then(|e| e["seq"].as_i64())
        .unwrap_or(0)
}

#[test]
fn provider_secrets_never_recorded() {
    let s = Setup::new();
    s.settings(&third_party("a.example"));
    let p = FakeProcess::spawn();
    s.start("api", p.pid());
    let ls = s.json(&["ls", "--json"]);
    assert_eq!(ls["provider"]["host"], "a.example");
    assert_eq!(
        ls["sessions"][0]["provider_restart"],
        serde_json::Value::Null
    );
    assert_eq!(ls["sessions"][0]["restart_queued"], false);

    let _daemon = Daemon(
        s.cmd()
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    // The daemon's first look is its baseline.
    std::thread::sleep(Duration::from_millis(1500));
    // Another third party: the next message uses it.
    let seq = last_seq(&s);
    s.settings(&third_party("b.example"));
    let e = wait_until("provider-changed", || provider_changed(&s, seq));
    assert_eq!(e["slot_id"], "");
    assert_eq!(e["detail"]["host"], "b.example");
    assert_eq!(e["detail"]["live"], true);
    assert_eq!(e["detail"]["restart_needed"], serde_json::json!([]));
    // Back to the subscription: the session keeps the old keys.
    let seq = last_seq(&s);
    s.settings(r#"{"theme":"dark"}"#);
    let e = wait_until("provider-changed", || provider_changed(&s, seq));
    assert_eq!(e["detail"]["live"], false);
    let ls = s.json(&["ls", "--json"]);
    let slot = ls["sessions"][0]["id"].clone();
    assert_eq!(e["detail"]["restart_needed"], serde_json::json!([slot]));
    assert_eq!(ls["sessions"][0]["provider_restart"], "removed");
    assert_eq!(ls["provider"]["host"], serde_json::Value::Null);

    // Nothing anywhere holds a token, a key helper's command or a URL's
    // password: not the registry, the logs, the daemon's files, nor what
    // the commands print.
    let mut printed = vec![
        s.out(&["ls", "--json"]),
        s.out(&["ls", "--json", "--all"]),
        s.out(&["events", "--json", "--since", "0"]),
        s.out(&["doctor", "--sessions", "--json"]),
    ];
    printed.push(String::from_utf8_lossy(&s.cmd().arg("doctor").output().unwrap().stdout).into());
    for text in &printed {
        assert!(!text.contains(SECRET) && !text.contains(PASSWORD), "{text}");
    }
    let mut files = Vec::new();
    all_files(&s.state(), &mut files);
    assert!(files.iter().any(|(p, _)| p.ends_with("registry.db")));
    for (path, bytes) in files {
        assert!(
            !contains(&bytes, SECRET) && !contains(&bytes, PASSWORD) && !contains(&bytes, "SECRET"),
            "{path} holds a secret"
        );
    }
}

#[test]
fn provider_model_change_needs_a_restart() {
    let s = Setup::new();
    s.settings(&third_party("a.example"));
    let p = FakeProcess::spawn();
    s.start("api", p.pid());
    s.settings(
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://a.example","ANTHROPIC_AUTH_TOKEN":"x",
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"glm-5"},"apiKeyHelper":"x","model":"opus"}"#,
    );
    let ls = s.json(&["ls", "--json"]);
    assert_eq!(ls["sessions"][0]["provider_restart"], "model");
}
