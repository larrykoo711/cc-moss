mod common;

#[test]
fn invalid_config_reports_path() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("broken.toml");
    std::fs::write(&cfg, "[claude\n").unwrap();
    let out = common::bin()
        .env("CC_MOSS_CONFIG", &cfg)
        .env("CC_MOSS_TMUX_SOCKET", "cc-moss-test-never-started")
        .args(["new", "x", "--detach"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("broken.toml"), "{err}");
}

use std::process::Command;

use common::World;

/// An executable script in the world's directory.
fn script(w: &World, name: &str, body: &str) -> std::path::PathBuf {
    let p = w.path(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    Command::new("chmod").arg("+x").arg(&p).status().unwrap();
    p
}

/// The real tmux, for a PATH that does not have its directory.
fn real_tmux() -> std::path::PathBuf {
    let out = Command::new("which").arg("tmux").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().into()
}

#[test]
fn claude_is_found_on_our_path_not_the_tmux_servers() {
    let w = World::new("serverpath");
    // `claude` lives in a directory only cc-moss's PATH has.
    let bin = w.path("user-bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&w.claude, bin.join("claude")).unwrap();
    // A server started from a context without that directory. (Recent tmux
    // also hands a detached client's PATH to the pane; older ones do not,
    // so cc-moss passes an absolute path either way.)
    let started = Command::new(real_tmux())
        .env_remove("TMUX")
        .env("PATH", "/usr/bin:/bin")
        .env("CC_MOSS_STATE_DIR", w.state())
        .env("CC_MOSS_BIN", env!("CARGO_BIN_EXE_cc-moss"))
        .env("CC_MOSS_FAKE_DIR", w.path("fifos"))
        .args([
            "-L",
            &w.socket,
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "other",
        ])
        .status()
        .unwrap();
    assert!(started.success());
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = w
        .cc_moss()
        .env_remove("CC_MOSS_CLAUDE_BIN")
        .env("PATH", path)
        .args(["new", "api-gw", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    // The fake claude started and checked in.
    w.session_id("api-gw");
}

#[test]
fn missing_claude_is_reported_before_tmux_starts() {
    let w = World::new("noclaude");
    let bin = w.path("only-tmux");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(real_tmux(), bin.join("tmux")).unwrap();
    let out = w
        .cc_moss()
        .env_remove("CC_MOSS_CLAUDE_BIN")
        .env("HOME", w.path("home"))
        .env("PATH", &bin)
        .args(["new", "api-gw", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Claude Code (`claude`) was not found"),
        "{err}"
    );
    assert!(w.session_names().is_empty());
}

#[test]
fn a_claude_that_exits_at_once_is_reported() {
    let w = World::new("diesfast");
    let dying = script(&w, "dying-claude", "echo 'unknown option' >&2; exit 1");
    let out = w
        .cc_moss()
        .env("CC_MOSS_CLAUDE_BIN", &dying)
        .env("CC_MOSS_STARTUP_GRACE_MS", "5000")
        .args(["new", "api-gw", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("exited right after it started"), "{err}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Started"));
}

#[test]
fn missing_tmux_says_so_in_both_languages() {
    let w = World::new("notmux");
    let empty = w.path("empty-bin");
    std::fs::create_dir_all(&empty).unwrap();
    for (lang, want) in [("en", "tmux is not installed"), ("zh", "没找到 tmux")] {
        let out = w
            .cc_moss()
            .env("PATH", &empty)
            .env("CC_MOSS_LANG", lang)
            .args(["new", "api-gw", "--detach", "-C"])
            .arg(w.path("work"))
            .output()
            .unwrap();
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains(want) && err.contains("cc-moss doctor"),
            "{err}"
        );
    }
}
