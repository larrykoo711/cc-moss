//! Probe (F59, run by hand): does the real Claude Code skip its bypass
//! warning when `skipDangerousModePermissionPrompt` is in the local
//! settings, as Moss is started (`--dangerously-skip-permissions
//! --setting-sources project,local`)? A control run puts the key in the
//! project settings only, where Claude Code ignores it.
//!
//! Its own tmux socket, a temporary HOME and working folder, a clean
//! environment (no provider, no `CLAUDE_CONFIG_DIR`). No key is ever sent
//! to the pane. Claude Code is `CC_MOSS_PROBE_CLAUDE`, else `claude` on the
//! PATH, else `~/.local/bin/claude` (only run from there).
//!
//! `cargo test -p cc-moss --test moss_bypass_probe -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const SOCKET: &str = "cc-moss-test-a-probe";
const WARNING: &str = "Bypass Permissions mode";
const KEY: &str = "skipDangerousModePermissionPrompt";
const WAIT: Duration = Duration::from_secs(20);

fn claude() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CC_MOSS_PROBE_CLAUDE") {
        return Some(PathBuf::from(p));
    }
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("claude"))
            .find(|p| p.is_file())
    });
    on_path.or_else(|| {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(home.join(".local/bin/claude")).filter(|p| p.exists())
    })
}

fn tmux(args: &[&str]) -> String {
    let out = Command::new("tmux")
        .args(["-L", SOCKET, "-f", "/dev/null"])
        .args(args)
        .output()
        .expect("tmux runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Server;

impl Drop for Server {
    fn drop(&mut self) {
        tmux(&["kill-server"]);
    }
}

/// What the pane shows: whether the warning came up within [`WAIT`], and
/// the last screen.
fn run(claude: &Path, case: &str, file: &str) -> (bool, String) {
    let dir = tempfile::tempdir().unwrap();
    // Claude Code keys the trust by the real path (`/private/var/…`).
    let root = dir.path().canonicalize().unwrap();
    let home = root.join("home");
    let work = root.join("work");
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::create_dir_all(work.join(".claude")).unwrap();
    std::fs::write(
        work.join(".claude").join(file),
        format!("{{\"{KEY}\": true}}\n"),
    )
    .unwrap();
    // A fresh HOME: onboarding done and the folder trusted, so the first
    // screen can be the warning; no account in it.
    let trusted = serde_json::json!({
        "hasCompletedOnboarding": true,
        "theme": "dark",
        "projects": { work.to_string_lossy(): { "hasTrustDialogAccepted": true } }
    });
    std::fs::write(home.join(".claude.json"), trusted.to_string()).unwrap();
    let command = format!(
        "env -i HOME='{}' PATH=/usr/bin:/bin TERM=xterm-256color '{}' \
         --dangerously-skip-permissions --strict-mcp-config --setting-sources project,local",
        home.display(),
        claude.display()
    );
    tmux(&[
        "new-session",
        "-d",
        "-x",
        "160",
        "-y",
        "50",
        "-s",
        case,
        "-c",
        &work.to_string_lossy(),
        &command,
    ]);
    let until = Instant::now() + WAIT;
    let mut screen = String::new();
    while Instant::now() < until {
        screen = tmux(&["capture-pane", "-p", "-t", &format!("={case}:")]);
        if screen.contains(WARNING) {
            tmux(&["kill-session", "-t", &format!("={case}")]);
            return (true, screen);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    tmux(&["kill-session", "-t", &format!("={case}")]);
    (false, screen)
}

#[test]
#[ignore = "starts the real Claude Code; run by hand"]
fn moss_bypass_probe() {
    let Some(claude) = claude() else {
        panic!("no Claude Code found: set CC_MOSS_PROBE_CLAUDE");
    };
    let version = Command::new(&claude)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("Claude Code: {version}");
    let _server = Server;
    tmux(&["kill-server"]);
    let (control, control_screen) = run(&claude, "probe-project", "settings.json");
    println!("--- key in project settings only: warning shown = {control}\n{control_screen}");
    let (local, local_screen) = run(&claude, "probe-local", "settings.local.json");
    println!("--- key in local settings: warning shown = {local}\n{local_screen}");
    assert!(
        control,
        "inconclusive: the control run never reached the warning (see the screen above)"
    );
    assert!(!local, "the warning came up with the key in local settings");
}
