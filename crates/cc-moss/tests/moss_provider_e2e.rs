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
    common::debug_only();
    let mut cmd = w.cc_moss();
    cmd.env("CC_MOSS_MOSS_SOCKET", &w.socket)
        .env("CC_MOSS_MOSS_CHECK_MS", "200")
        .env("CC_MOSS_MOSS_STEPS_MS", "300,600")
        .env("CC_MOSS_DAEMON_HEARTBEAT_MS", "500");
    if let Some(claude) = claude {
        cmd.env("CC_MOSS_CLAUDE_BIN", claude);
    }
    if trust {
        cmd.env("CC_MOSS_FAKE_TRUST", "1");
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
        let config = w.path("config.toml");
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

fn moss_env(w: &World) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(moss_file(w)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Every file under `dir`, with its bytes.
fn all_files(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).unwrap();
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
    let env = moss_env(&w).expect("Moss has its provider file");
    assert_eq!(
        env,
        serde_json::json!({"env": {
            "ANTHROPIC_BASE_URL": "https://a.example/api",
            "ANTHROPIC_AUTH_TOKEN": SECRET,
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
    let second = wait_until("a fresh Moss", || running_moss(&w, Some(&first)));
    assert_ne!(second, first);
    assert!(log(&w).contains("starting Moss again"), "{}", log(&w));
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
    set_provider(&w, r#"{}"#);
    wait_until("a fresh Moss", || running_moss(&w, Some(&first)));
    // Only Moss's provider file holds the token: not its other files, the
    // registry, the logs, the facts, nor what the commands print.
    let mut files = Vec::new();
    all_files(&w.state(), &mut files);
    all_files(&w.path("fifos"), &mut files);
    assert!(files.iter().any(|(p, _)| p.ends_with("registry.db")));
    for (path, bytes) in &files {
        if path.ends_with(".claude/settings.local.json") {
            continue;
        }
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
