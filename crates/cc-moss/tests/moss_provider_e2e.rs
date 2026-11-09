//! Moss follows the user's API provider (F24): its home's
//! `.claude/settings.local.json` carries the provider from the user's
//! settings, a switch reaches it, a switch back to the subscription starts
//! a fresh Moss, and a fresh machine's folder-trust question is answered.
//! Test-only tmux socket, temporary state, config and HOME, fake claude.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{fixture, ls_json, World};

const SECRET: &str = "sk-ant-api03-MOSS-SECRETVALUE";

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon(w: &World, claude: Option<&Path>, trust: bool) -> Daemon {
    daemon_with(w, claude, trust.then_some("1"), &[])
}

/// [`daemon`] with the fake's trust question in `trust` mode (see
/// `fixtures/moss-claude`) and more environment.
fn daemon_with(
    w: &World,
    claude: Option<&Path>,
    trust: Option<&str>,
    env: &[(&str, &str)],
) -> Daemon {
    common::debug_only();
    let mut cmd = w.cc_moss();
    cmd.env("CC_MOSS_MOSS_SOCKET", &w.socket)
        .env("CC_MOSS_MOSS_CHECK_MS", "200")
        // The real crash ladder (30 s first): a provider restart must not
        // wait on it.
        .env("CC_MOSS_DAEMON_HEARTBEAT_MS", "500");
    if let Some(claude) = claude {
        cmd.env("CC_MOSS_CLAUDE_BIN", claude);
    }
    if let Some(mode) = trust {
        cmd.env("CC_MOSS_FAKE_TRUST", mode);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    Daemon(
        cmd.args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn wait_until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn running_moss(w: &World, not: Option<&str>) -> Option<String> {
    ls_json(&w.state(), false)["sessions"]
        .as_array()?
        .iter()
        .find(|s| {
            s["role"] == "moss"
                && s["status"] == "running"
                && not.is_none_or(|old| s["session_id"] != old)
        })
        .map(|s| s["session_id"].as_str().unwrap().to_string())
}

fn log(w: &World) -> String {
    std::fs::read_to_string(w.state().join("daemon/daemon.log")).unwrap_or_default()
}

/// The user's Claude Code settings, where the daemon is told to look.
fn user_settings(w: &World) -> PathBuf {
    let dir = w.path("claude");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).unwrap();
        let config = w.config_path();
        let mut text = std::fs::read_to_string(&config).unwrap();
        text = text.replacen(
            "[claude]\n",
            &format!("[claude]\nconfig_dir = \"{}\"\n", dir.display()),
            1,
        );
        std::fs::write(&config, text).unwrap();
    }
    dir.join("settings.json")
}

fn set_provider(w: &World, text: &str) {
    let path = user_settings(w);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

fn third_party(host: &str) -> String {
    format!(
        r#"{{"env":{{"ANTHROPIC_BASE_URL":"https://{host}/api","ANTHROPIC_AUTH_TOKEN":"{SECRET}",
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"glm-5","HTTPS_PROXY":"http://proxy"}},
            "theme":"dark","model":"opus"}}"#
    )
}

fn moss_file(w: &World) -> PathBuf {
    w.state().join("moss/.claude/settings.local.json")
}

/// Moss's local settings without the bypass answer, which is always
/// there (F59).
fn moss_env(w: &World) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(moss_file(w)).ok()?;
    let mut v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let answer = v.as_object_mut()?.remove(BYPASS_KEY);
    (answer == Some(serde_json::Value::Bool(true))).then_some(v)
}

const BYPASS_KEY: &str = "skipDangerousModePermissionPrompt";

/// Every file under `dir`, with its bytes.
fn all_files(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Gone meanwhile (a file being replaced): nothing to look at.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            all_files(&path, out);
        } else if meta.is_file() {
            out.push((path.clone(), std::fs::read(&path).unwrap_or_default()));
        }
    }
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn moss_follows_the_provider() {
    let w = World::new("moss-prov");
    set_provider(&w, &third_party("a.example"));
    let _d = daemon(&w, None, false);
    let first = wait_until("Moss to come up", || running_moss(&w, None));
    // It started with the provider: the same keys and values, nothing else.
    // No token: Claude Code asks cc-moss for it (`moss provider-key`).
    let mut env = moss_env(&w).expect("Moss has its provider file");
    let helper = env["apiKeyHelper"].as_str().unwrap_or_default().to_string();
    assert!(helper.ends_with(" moss provider-key"), "{env}");
    env.as_object_mut().unwrap().remove("apiKeyHelper");
    assert_eq!(
        env,
        serde_json::json!({"env": {
            "ANTHROPIC_BASE_URL": "https://a.example/api",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5"
        }})
    );
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(moss_file(&w))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let started = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        "=moss:",
        "#{pane_start_command}",
    ]);
    assert!(started.contains("project,local"), "{started}");

    // Another third party: the file follows, Moss keeps running (Claude
    // Code takes it in by itself).
    set_provider(&w, &third_party("b.example"));
    wait_until("Moss's copy to follow", || {
        (moss_env(&w)?["env"]["ANTHROPIC_BASE_URL"] == "https://b.example/api").then_some(())
    });
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(running_moss(&w, None).as_deref(), Some(first.as_str()));

    // Back to the subscription: the keys go, and a fresh Moss starts.
    set_provider(&w, r#"{"theme":"dark"}"#);
    wait_until("the keys to go", || {
        (moss_env(&w)? == serde_json::json!({})).then_some(())
    });
    let switched = Instant::now();
    let second = wait_until("a fresh Moss", || running_moss(&w, Some(&first)));
    assert_ne!(second, first);
    assert!(
        switched.elapsed() < Duration::from_secs(15),
        "started right away, not after the crash ladder's wait"
    );
    assert!(log(&w).contains("starting Moss again"), "{}", log(&w));
    // Not a crash: the restart count stays.
    let out = w
        .cc_moss()
        .env("CC_MOSS_MOSS_SOCKET", &w.socket)
        .args(["moss", "status", "--json"])
        .output()
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(status["restarts"], 0, "{status}");
}

#[test]
fn provider_secrets_never_recorded_by_moss() {
    let w = World::new("moss-secret");
    set_provider(&w, &third_party("a.example"));
    let _d = daemon(&w, None, false);
    let first = wait_until("Moss to come up", || running_moss(&w, None));
    set_provider(&w, &third_party("b.example"));
    wait_until("Moss's copy to follow", || {
        (moss_env(&w)?["env"]["ANTHROPIC_BASE_URL"] == "https://b.example/api").then_some(())
    });
    // While a third party is on, and again after the switch back.
    no_token_anywhere(&w);
    set_provider(&w, r#"{}"#);
    wait_until("a fresh Moss", || running_moss(&w, Some(&first)));
    no_token_anywhere(&w);
}

/// No file holds the token, Moss's own included (Claude Code would hand
/// its settings env to Moss's shell commands): not the registry, the logs,
/// the facts, nor what the commands print or Moss was started with.
fn no_token_anywhere(w: &World) {
    let mut files = Vec::new();
    all_files(&w.state(), &mut files);
    all_files(&w.path("fifos"), &mut files);
    assert!(files.iter().any(|(p, _)| p.ends_with("registry.db")));
    // Moss's provider file included: Claude Code would hand its env to
    // Moss's shell commands.
    assert!(files
        .iter()
        .any(|(p, _)| p.ends_with(".claude/settings.local.json")));
    for (path, bytes) in &files {
        assert!(
            !contains(bytes, SECRET),
            "{} holds the token",
            path.display()
        );
    }
    for args in [
        &["ls", "--json", "--all"][..],
        &["events", "--json", "--since", "0"],
        &["moss", "status", "--json"],
        &["doctor", "--sessions", "--json"],
    ] {
        let out = w.cc_moss().args(args).output().unwrap();
        assert!(!contains(&out.stdout, SECRET), "{args:?}");
        assert!(!contains(&out.stderr, SECRET), "{args:?}");
    }
    let started = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        "=moss:",
        "#{pane_start_command}",
    ]);
    assert!(!started.contains(SECRET), "never in its command line");
}

#[test]
fn moss_provider_key_hands_over_the_token_and_nothing_else() {
    let w = World::new("moss-key");
    set_provider(&w, &third_party("a.example"));
    let key = |w: &World| {
        w.cc_moss()
            .env("HOME", w.path("home"))
            .args(["moss", "provider-key"])
            .output()
            .unwrap()
    };
    std::fs::create_dir_all(w.path("home")).unwrap();
    let out = key(&w);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{SECRET}\n"));
    // The user's own helper runs in their home folder.
    set_provider(&w, r#"{"apiKeyHelper":"pwd > where; echo from-helper"}"#);
    let out = key(&w);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "from-helper\n");
    let ran_in = std::fs::read_to_string(w.path("home/where")).unwrap();
    assert_eq!(
        std::fs::canonicalize(ran_in.trim()).unwrap(),
        std::fs::canonicalize(w.path("home")).unwrap()
    );
    // No key: it fails, saying nothing on stdout.
    set_provider(&w, r#"{"theme":"dark"}"#);
    let out = key(&w);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    // Settings it can't read (Moss's shell, in its sandbox): it fails.
    set_provider(&w, &third_party("a.example"));
    use std::os::unix::fs::PermissionsExt;
    let settings = user_settings(&w);
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o000)).unwrap();
    let out = key(&w);
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o600)).unwrap();
    // (root reads it anyway.)
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        assert!(!out.status.success(), "{out:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn moss_answers_the_folder_trust_question() {
    let w = World::new("moss-trust");
    let _d = daemon(&w, Some(&fixture("moss-claude")), true);
    wait_until("the question to be answered", || {
        w.path("fifos/trusted").exists().then_some(())
    });
    let sid = wait_until("Moss to come up", || running_moss(&w, None));
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(
        running_moss(&w, None).as_deref(),
        Some(sid.as_str()),
        "no restart loop"
    );
    assert_eq!(
        log(&w).matches("trust Moss's folder").count(),
        1,
        "{}",
        log(&w)
    );
}

fn trust_keys(w: &World) -> String {
    std::fs::read_to_string(w.path("fifos/trust-keys.txt")).unwrap_or_default()
}

fn moss_status(w: &World) -> serde_json::Value {
    let out = w
        .cc_moss()
        .env("CC_MOSS_MOSS_SOCKET", &w.socket)
        .args(["moss", "status", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// F64: Claude Code 2.1.319 selects "No, exit" first. The daemon moves to
/// Yes and confirms only once Yes shows; Enter never lands on No.
#[test]
fn moss_answers_the_trust_question_with_no_selected() {
    let w = World::new("moss-trust-no");
    let _d = daemon_with(&w, Some(&fixture("moss-claude")), Some("no"), &[]);
    wait_until("the question to be answered", || {
        w.path("fifos/trusted").exists().then_some(())
    });
    wait_until("Moss to come up", || running_moss(&w, None));
    assert!(!w.path("fifos/enter-on-no").exists(), "Enter on No");
    let keys = trust_keys(&w);
    assert!(keys.contains("[B"), "a Down went in: {keys}");
    assert!(
        log(&w).contains("trust Moss's folder; answered yes"),
        "{}",
        log(&w)
    );
}

/// F64: a trust question Yes can't be selected on is left alone (never
/// Enter on No) and, once its window is over, reported as stuck.
#[test]
fn moss_status_reports_trust_blocked() {
    let w = World::new("moss-trust-stuck");
    let _d = daemon_with(
        &w,
        Some(&fixture("moss-claude")),
        Some("stuck"),
        &[("CC_MOSS_TRUST_WINDOW_MS", "1500")],
    );
    let st = wait_until("Moss to be reported blocked", || {
        let st = moss_status(&w);
        (st["activity"] == "blocked").then_some(st)
    });
    assert_eq!(st["blocked"]["kind"], "trust_folder", "{st}");
    let keys = trust_keys(&w);
    assert!(
        !keys.contains("$'\\r'") && !keys.contains("$'\\n'"),
        "no Enter: {keys}"
    );
    assert!(!w.path("fifos/enter-on-no").exists());
    assert!(!w.path("fifos/trusted").exists());
    // Nothing more is pressed while it waits for you.
    let before = trust_keys(&w);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(trust_keys(&w), before);
    assert_eq!(moss_status(&w)["activity"], "blocked");
}

/// F64: the trust question in wording that is neither yes nor no: no key
/// at all, and reported as stuck once the trust window is over.
#[test]
fn moss_reports_a_trust_question_it_cannot_read() {
    let w = World::new("moss-trust-odd");
    let _d = daemon_with(
        &w,
        Some(&fixture("moss-claude")),
        Some("odd"),
        &[("CC_MOSS_TRUST_WINDOW_MS", "1500")],
    );
    let st = wait_until("Moss to be reported blocked", || {
        let st = moss_status(&w);
        (st["activity"] == "blocked").then_some(st)
    });
    assert_eq!(st["blocked"]["kind"], "trust_folder", "{st}");
    assert_eq!(trust_keys(&w), "", "no key pressed");
}
