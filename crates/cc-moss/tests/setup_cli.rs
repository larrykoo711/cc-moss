//! `cc-moss setup` against a settings.json in a temp dir, never the real one.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::bin;

/// A settings.json as Claude Code writes it, with a user hook on an event
/// cc-moss also uses and non-ASCII text that must survive unescaped.
const CLAUDE_FORMATTED: &str = r#"{
  "permissions": {
    "allow": [
      "Bash(ls:*)"
    ]
  },
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "~/bin/notify.sh 完成…"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit",
        "hooks": [
          {
            "type": "command",
            "command": "cargo fmt"
          }
        ]
      }
    ]
  },
  "model": "opus"
}
"#;

fn setup(dir: &Path, args: &[&str]) -> Output {
    let settings = dir.join("settings.json");
    bin()
        .env("HOME", dir)
        .arg("setup")
        .args(args)
        .arg("--settings")
        .arg(&settings)
        .output()
        .expect("run cc-moss setup")
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with("settings.json.cc-moss-") && n.ends_with(".bak")
        })
        .collect();
    v.sort();
    v
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn install_then_uninstall_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, CLAUDE_FORMATTED).unwrap();

    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "install: {}", stderr(&out));
    let installed = fs::read_to_string(&path).unwrap();
    for name in ["session-start", "user-prompt-submit", "stop", "session-end"] {
        assert!(
            installed.contains(&format!("cc-moss hook {name}")),
            "{name}"
        );
    }
    assert!(
        installed.contains("~/bin/notify.sh 完成…"),
        "user hook kept"
    );

    let out = setup(dir.path(), &["--uninstall", "--yes"]);
    assert!(out.status.success(), "uninstall: {}", stderr(&out));
    assert_eq!(fs::read(&path).unwrap(), CLAUDE_FORMATTED.as_bytes());
}

#[test]
fn invalid_settings_refuses_to_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let broken = "{ \"model\": \"opus\", \n";
    fs::write(&path, broken).unwrap();

    for args in [&["--yes"][..], &["--uninstall", "--yes"][..]] {
        let out = setup(dir.path(), args);
        assert!(!out.status.success(), "{args:?} must fail");
        assert!(stderr(&out).contains("not valid JSON"), "{}", stderr(&out));
        assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        assert!(backups(dir.path()).is_empty());
    }
}

#[test]
fn requires_confirmation_or_yes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, CLAUDE_FORMATTED).unwrap();

    // stdin is not a terminal here, so nobody can answer a prompt.
    let out = setup(dir.path(), &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--yes"), "{}", stderr(&out));
    let shown = stdout(&out);
    assert!(
        shown
            .lines()
            .any(|l| l.starts_with('+') && l.contains("cc-moss hook stop")),
        "diff shown: {shown}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), CLAUDE_FORMATTED);

    let out = setup(dir.path(), &["--dry-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("cc-moss hook session-start"),
        "diff shown"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), CLAUDE_FORMATTED);
    assert!(backups(dir.path()).is_empty());

    // A dry run must not create a missing file either.
    let empty = tempfile::tempdir().unwrap();
    let out = setup(empty.path(), &["--dry-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!empty.path().join("settings.json").exists());
}

#[test]
fn writes_backup_before_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, CLAUDE_FORMATTED).unwrap();

    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let b = backups(dir.path());
    assert_eq!(b.len(), 1, "{b:?}");
    assert_eq!(fs::read_to_string(&b[0]).unwrap(), CLAUDE_FORMATTED);
    let installed = fs::read_to_string(&path).unwrap();

    // A second write within the same second must not clobber the first backup.
    let out = setup(dir.path(), &["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let b2 = backups(dir.path());
    assert_eq!(b2.len(), 2, "{b2:?}");
    let contents: Vec<String> = b2.iter().map(|p| fs::read_to_string(p).unwrap()).collect();
    assert!(contents.contains(&CLAUDE_FORMATTED.to_string()));
    assert!(contents.contains(&installed));
}

#[test]
fn second_setup_reports_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    // No settings.json yet: setup creates it, with nothing to back up.
    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(dir.path().join("settings.json").exists());
    assert!(backups(dir.path()).is_empty());

    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("up to date"), "{}", stdout(&out));
    assert!(backups(dir.path()).is_empty(), "no write, no backup");
}

#[test]
fn honours_claude_config_dir() {
    // Claude Code reads settings from $CLAUDE_CONFIG_DIR when it is set.
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let out = common::bin()
        .env("HOME", home.path())
        .env("CLAUDE_CONFIG_DIR", config_dir.path())
        .args(["setup", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(config_dir.path().join("settings.json").exists());
    assert!(!home.path().join(".claude/settings.json").exists());
}
