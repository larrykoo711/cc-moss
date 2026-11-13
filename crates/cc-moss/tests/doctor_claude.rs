//! `cc-moss doctor` finds Claude Code the way sessions are started, not only
//! on the PATH (a throwaway HOME, a PATH without `~/.local/bin`).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::bin;

fn claude_line(home: &Path, pinned: Option<&str>) -> String {
    let mut cmd = bin();
    cmd.env("HOME", home)
        .env("CC_MOSS_STATE_DIR", home.join("state"))
        .env("CC_MOSS_PLATFORM", "other")
        .env("CC_MOSS_LANG", "en")
        .env("PATH", "/usr/bin:/bin")
        .env_remove("CC_MOSS_CLAUDE_BIN")
        .arg("doctor");
    if let Some(p) = pinned {
        cmd.env("CC_MOSS_CLAUDE_BIN", p);
    }
    let out = cmd.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.lines()
        .skip_while(|l| !l.contains("Claude Code"))
        .take(2)
        .collect::<Vec<_>>()
        .join("\n")
}

fn fake_claude(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "#!/bin/sh\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn claude_in_the_native_installer_spot_is_installed_though_not_on_path() {
    let home = tempfile::tempdir().unwrap();
    fake_claude(&home.path().join(".local/bin/claude"));
    let line = claude_line(home.path(), None);
    assert!(line.contains("✓"), "{line}");
    assert!(line.contains("installed: ~/.local/bin/claude"), "{line}");
}

#[test]
fn claude_nowhere_is_not_found() {
    let home = tempfile::tempdir().unwrap();
    let line = claude_line(home.path(), None);
    assert!(line.contains("✗"), "{line}");
    assert!(line.contains("not found"), "{line}");
}

#[test]
fn a_pinned_claude_is_installed_at_its_path() {
    let home = tempfile::tempdir().unwrap();
    let pinned = home.path().join("tools/claude");
    fake_claude(&pinned);
    let line = claude_line(home.path(), Some(pinned.to_str().unwrap()));
    assert!(line.contains("✓"), "{line}");
    assert!(line.contains("installed: ~/tools/claude"), "{line}");
}

#[test]
fn a_pinned_claude_that_is_not_executable_says_why() {
    let home = tempfile::tempdir().unwrap();
    let pinned = home.path().join("tools/claude");
    let line = claude_line(home.path(), Some(pinned.to_str().unwrap()));
    assert!(line.contains("✗"), "{line}");
    assert!(line.contains("CC_MOSS_CLAUDE_BIN"), "{line}");
}
