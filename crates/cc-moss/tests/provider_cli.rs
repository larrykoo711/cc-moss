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
    // `/model` (top-level `model`) is the default for new sessions: no
    // restart for it.
    s.settings(
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://a.example","ANTHROPIC_AUTH_TOKEN":"x",
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"glm-5"},"apiKeyHelper":"x","model":"opus"}"#,
    );
    let ls = s.json(&["ls", "--json"]);
    assert_eq!(
        ls["sessions"][0]["provider_restart"],
        serde_json::Value::Null
    );
    // The provider's fixed model is read only at start.
    s.settings(
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://a.example","ANTHROPIC_AUTH_TOKEN":"x",
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"glm-5","ANTHROPIC_MODEL":"kimi"},"apiKeyHelper":"x"}"#,
    );
    let ls = s.json(&["ls", "--json"]);
    assert_eq!(ls["sessions"][0]["provider_restart"], "model");
}

#[test]
fn provider_follows_the_settings_each_session_reads() {
    // The session's Claude Code reads its own folder; cc-moss looks at
    // another one.
    let s = Setup::new();
    s.settings(&third_party("a.example"));
    let theirs = s.dir.path().join("theirs");
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(theirs.join("settings.json"), third_party("t.example")).unwrap();
    let p = FakeProcess::spawn();
    let mut child = s
        .cmd()
        .env("CLAUDE_CONFIG_DIR", &theirs)
        .env("CLAUDE_PID", p.pid().unwrap().to_string())
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
        .write_all(common::start_json("own", "startup", Some("own")).as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        serde_json::Value::Null
    );
    // Back to the subscription in the folder cc-moss looks at: not theirs.
    s.settings(r#"{}"#);
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        serde_json::Value::Null
    );
    // In their own folder: that one counts.
    std::fs::write(theirs.join("settings.json"), "{}").unwrap();
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "removed"
    );
    // And doctor says the folders differ.
    let out = s.cmd().arg("doctor").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("another Claude Code settings folder"),
        "{text}"
    );
}

#[test]
fn provider_model_old_session_has_uncertain_safe_plan() {
    let s = Setup::new();
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"route-b","ANTHROPIC_AUTH_TOKEN":"private-provider-credential"}}"#);
    let p = FakeProcess::spawn();
    s.start("old-model", p.pid());
    let db = rusqlite::Connection::open(s.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET provider_started=NULL, provider_seen_keys=NULL",
        [],
    )
    .unwrap();
    let v = s.json(&["ls", "--json"]);
    let plan = &v["sessions"][0]["provider_model"];
    assert_eq!(plan["model"], "route-b");
    assert_eq!(plan["source"], "env");
    assert_eq!(plan["uncertain"], true);
    assert!(plan["revision"]
        .as_str()
        .is_some_and(|revision| !revision.is_empty()));
    assert!(!v.to_string().contains("private-provider-credential"));
    assert_eq!(
        v["sessions"][0]["provider_restart"],
        serde_json::Value::Null
    );
}

#[test]
fn provider_model_invalid_configuration_is_unknown_without_echoing_values() {
    let s = Setup::new();
    s.settings("{}");
    let p = FakeProcess::spawn();
    s.start("bad-model", p.pid());
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"sk-private-provider-credential"}}"#);
    let v = s.json(&["ls", "--json"]);
    assert_eq!(v["sessions"][0]["provider_model"]["source"], "unknown");
    assert_eq!(
        v["sessions"][0]["provider_model"]["model"],
        serde_json::Value::Null
    );
    assert!(!v.to_string().contains("sk-private-provider-credential"));
}

#[test]
fn provider_model_known_baseline_is_not_uncertain() {
    let s = Setup::new();
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"route-b"}}"#);
    let p = FakeProcess::spawn();
    s.start("known-model", p.pid());
    let v = s.json(&["ls", "--json"]);
    assert_eq!(v["sessions"][0]["provider_model"]["uncertain"], false);
}

fn model_evidence_setup() -> (Setup, FakeProcess, std::path::PathBuf) {
    let s = Setup::new();
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"old-model","ANTHROPIC_AUTH_TOKEN":"private-a"}}"#);
    let process = FakeProcess::spawn();
    let path = s.dir.path().join("history.jsonl");
    std::fs::write(&path, b"").unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&common::start_json(
        "model-evidence",
        "startup",
        Some("model-evidence"),
    ))
    .unwrap();
    payload["transcript_path"] = path.to_str().unwrap().into();
    payload["cwd"] = s.dir.path().to_str().unwrap().into();
    let mut child = s
        .cmd()
        .env("CLAUDE_CONFIG_DIR", s.dir.path().join("claude"))
        .env("CLAUDE_PID", process.pid().unwrap().to_string())
        .args(["hook", "session-start"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"new-model","ANTHROPIC_AUTH_TOKEN":"private-a"}}"#);
    (s, process, path)
}
fn model_evidence_response(path: &Path, timestamp: Option<i64>) {
    let timestamp = timestamp.map(moss_core::slot::rfc3339);
    writeln!(std::fs::OpenOptions::new().append(true).open(path).unwrap(),"{}",serde_json::json!({"type":"assistant","sessionId":"model-evidence","timestamp":timestamp,"message":{"model":"new-model","stop_reason":"tool_use","content":"private response body"}})).unwrap();
}
#[test]
fn model_evidence_new_success_suppresses_only_current_epoch_model_prompt() {
    let (s, _process, path) = model_evidence_setup();
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "model"
    );
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
    let registry = moss_core::registry::Registry::open(&s.state().join("registry.db")).unwrap();
    let evidence = serde_json::to_string(&registry.model_evidence().unwrap()).unwrap();
    assert!(!evidence.contains("private-a") && !evidence.contains("private response body"));
    let started: serde_json::Value = serde_json::from_str(
        registry.slots().unwrap()[0]
            .provider_started
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(started["env_model"], "old-model");
    let stale = s.json(&["restart", "--stale", "--json"]);
    assert!(stale["results"].as_array().unwrap().is_empty());
    // 明确手动请求仍进入目标处理；本fixture不在tmux中，因此安全拒绝而非被悄悄过滤。
    let explicit = s
        .cmd()
        .args([
            "restart",
            "--provider-default",
            "--json",
            "--",
            "model-evidence",
        ])
        .output()
        .unwrap();
    let explicit: serde_json::Value = serde_json::from_slice(&explicit.stdout).unwrap();
    assert_eq!(explicit["results"].as_array().unwrap().len(), 1);
    assert_eq!(explicit["results"][0]["reason"], "not-in-tmux");
    // 认证值不读取或存储，但文件stat变化使旧模型证据失效。
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"new-model","ANTHROPIC_AUTH_TOKEN":"private-b"}}"#);
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "model"
    );
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"new-model"}}"#);
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "removed"
    );
}
#[test]
fn model_evidence_cache_loss_invalid_time_and_failure_keep_prompt() {
    let (s, _process, path) = model_evidence_setup();
    s.json(&["ls", "--json"]);
    model_evidence_response(&path, None);
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "model"
    );
    model_evidence_response(&path, Some(moss_core::slot::now_ms() + 60_000));
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "model"
    );
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
    std::fs::write(s.state().join("request-scan.json"), b"broken cache").unwrap();
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "model"
    );
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
    writeln!(std::fs::OpenOptions::new().append(true).open(&path).unwrap(),"{}",serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 500 failed"}})).unwrap();
    let result = s.json(&["ls", "--json"]);
    assert_eq!(result["sessions"][0]["provider_restart"], "model");
    assert!(result["sessions"][0]["api_failure"].is_object());
}

#[test]
fn model_evidence_daemon_collects_success_without_listing_and_aba_invalidates_it() {
    let (s, _process, path) = model_evidence_setup();
    let _daemon = Daemon(
        s.cmd()
            .args(["daemon", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_until("model evidence fence", || {
        let cache: serde_json::Value =
            serde_json::from_slice(&std::fs::read(s.state().join("request-scan.json")).ok()?)
                .ok()?;
        cache
            .as_object()?
            .values()
            .any(|entry| entry["fence_ms"].is_i64())
            .then_some(())
    });
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    wait_until("model evidence from daemon", || {
        let reg = moss_core::registry::Registry::open(&s.state().join("registry.db")).ok()?;
        (!reg.model_evidence().ok()?.is_empty()).then_some(())
    });
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"third-model","ANTHROPIC_AUTH_TOKEN":"private-a"}}"#);
    s.json(&["ls", "--json"]);
    s.settings(r#"{"env":{"ANTHROPIC_MODEL":"new-model","ANTHROPIC_AUTH_TOKEN":"private-a"}}"#);
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "model"
    );
}

#[test]
fn model_evidence_rechecks_automatic_queue_but_keeps_explicit_request() {
    let (s, _process, path) = model_evidence_setup();
    s.json(&["ls", "--json"]);
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    let listing = s.json(&["ls", "--json"]);
    assert!(listing["sessions"][0]["provider_restart"].is_null());
    let id = listing["sessions"][0]["id"].as_str().unwrap();
    let queue = s.state().join("daemon/restart-queue.json");
    std::fs::create_dir_all(queue.parent().unwrap()).unwrap();
    let automatic =
        serde_json::json!([{"slot_id":id,"recheck":true,"attempts":0,"provider_default":true}]);
    std::fs::write(&queue, automatic.to_string()).unwrap();
    let _daemon = Daemon(
        s.cmd()
            .args(["daemon", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_until("automatic stale queue cleared", || {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&queue).ok()?).ok()?;
        value.as_array()?.is_empty().then_some(())
    });
    let manual =
        serde_json::json!([{"slot_id":id,"recheck":false,"attempts":0,"provider_default":true}]);
    std::fs::write(&queue, manual.to_string()).unwrap();
    std::thread::sleep(Duration::from_millis(1250));
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&queue).unwrap()).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 1);
    assert_eq!(value[0]["attempts"], 0);
}

#[test]
fn model_evidence_file_replacement_starts_a_new_watermark() {
    let (s, _process, path) = model_evidence_setup();
    s.json(&["ls", "--json"]);
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
    let replacement = s.dir.path().join("new-history.jsonl");
    std::fs::write(&replacement, b"").unwrap();
    model_evidence_response(&replacement, Some(moss_core::slot::now_ms()));
    std::fs::rename(&replacement, &path).unwrap();
    assert_eq!(
        s.json(&["ls", "--json"])["sessions"][0]["provider_restart"],
        "model"
    );
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
}

#[test]
fn model_evidence_non_result_appends_preserve_proof_but_api_failure_revokes_it() {
    let (s, _process, path) = model_evidence_setup();
    s.json(&["ls", "--json"]);
    std::thread::sleep(Duration::from_millis(5));
    model_evidence_response(&path, Some(moss_core::slot::now_ms()));
    assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
    for record in [
        serde_json::json!({"type":"user","message":{"content":"continue"}}),
        serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","content":"done"}]}}),
        serde_json::json!({"type":"system","subtype":"turn_duration"}),
    ] {
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap(),
            "{record}"
        )
        .unwrap();
        assert!(s.json(&["ls", "--json"])["sessions"][0]["provider_restart"].is_null());
        let registry = moss_core::registry::Registry::open(&s.state().join("registry.db")).unwrap();
        assert_eq!(registry.model_evidence().unwrap().len(), 1);
    }
    writeln!(std::fs::OpenOptions::new().append(true).open(&path).unwrap(),"{}",serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 400 unknown provider for model"}})).unwrap();
    let listing = s.json(&["ls", "--json"]);
    assert_eq!(listing["sessions"][0]["provider_restart"], "model");
    assert_eq!(
        listing["sessions"][0]["api_failure"]["kind"],
        "model_unavailable"
    );
    let registry = moss_core::registry::Registry::open(&s.state().join("registry.db")).unwrap();
    assert!(registry.model_evidence().unwrap().is_empty());
    assert_eq!(registry.api_failures().unwrap().len(), 1);
}
