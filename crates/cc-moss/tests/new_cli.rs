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

/// Permission mode (T1.2): the command line `new` gives Claude Code.
mod permission_mode {
    use super::common::World;
    use std::path::Path;

    const BYPASS: &str = "--dangerously-skip-permissions";

    /// A world whose config is `config` and whose HOME is its own directory.
    fn world(test: &str, config: &str) -> World {
        let w = World::new(test);
        std::fs::write(w.config_path(), config).unwrap();
        std::fs::create_dir_all(w.path("home")).unwrap();
        w
    }

    /// Start `name` with `args` after `new <name>`; the pane's command line.
    fn started(w: &World, name: &str, args: &[&str]) -> String {
        let out = w
            .cc_moss()
            .env("HOME", w.path("home"))
            .args(["new", name, "--detach", "-C"])
            .arg(w.path("work"))
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        w.tmux(&[
            "list-panes",
            "-t",
            &format!("host_{name}"),
            "-F",
            "#{pane_start_command}",
        ])
    }

    fn disable_bypass_in(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            r#"{ "permissions": { "disableBypassPermissionsMode": "disable" } }"#,
        )
        .unwrap();
    }

    #[test]
    fn new_defaults_to_bypass() {
        let w = world("bypass-default", "");
        let cmd = started(&w, "job-a", &[]);
        assert_eq!(cmd.matches(BYPASS).count(), 1, "{cmd}");
    }

    #[test]
    fn new_safe_skips_bypass() {
        let w = world("bypass-safe", "");
        let cmd = started(&w, "job-a", &["--safe"]);
        assert!(!cmd.contains(BYPASS), "{cmd}");
        // `permission_mode = "default"` does the same for every session.
        let w = world("bypass-off", "[claude]\npermission_mode = \"default\"\n");
        let cmd = started(&w, "job-b", &[]);
        assert!(!cmd.contains(BYPASS), "{cmd}");
    }

    #[test]
    fn new_safe_strips_bypass_from_default_args() {
        let config = "[claude]\ndefault_args = [\"--dangerously-skip-permissions\", \"--model\", \"opus\", \"--permission-mode\", \"acceptEdits\", \"--permission-mode=plan\"]\n";
        let w = world("bypass-strip", config);
        let cmd = started(&w, "job-a", &["--safe"]);
        assert!(!cmd.contains(BYPASS), "{cmd}");
        assert!(!cmd.contains("--permission-mode"), "{cmd}");
        assert!(cmd.contains("--model opus"), "{cmd}");
        // Only this run: the config is unchanged and the next session has it again.
        assert_eq!(std::fs::read_to_string(w.config_path()).unwrap(), config);
        let cmd = started(&w, "job-b", &[]);
        assert_eq!(cmd.matches(BYPASS).count(), 1, "{cmd}");
    }

    #[test]
    fn new_does_not_repeat_permission_args() {
        // Already in default_args (the old way to get bypass).
        let w = world(
            "bypass-args",
            "[claude]\ndefault_args = [\"--dangerously-skip-permissions\"]\n",
        );
        let cmd = started(&w, "job-a", &[]);
        assert_eq!(cmd.matches(BYPASS).count(), 1, "{cmd}");
        // On the command line, either flag.
        let w = world("bypass-args2", "");
        let cmd = started(&w, "job-b", &[BYPASS]);
        assert_eq!(cmd.matches(BYPASS).count(), 1, "{cmd}");
        let w = world("bypass-mode", "");
        let cmd = started(&w, "job-c", &["--permission-mode", "plan"]);
        assert!(!cmd.contains(BYPASS), "{cmd}");
        assert!(cmd.contains("--permission-mode plan"), "{cmd}");
    }

    #[test]
    fn bypass_disabled_by_policy() {
        // In the user's own settings.json.
        let w = world("bypass-policy", "");
        disable_bypass_in(&w.path("home/.claude/settings.json"));
        let cmd = started(&w, "job-a", &[]);
        assert!(!cmd.contains(BYPASS), "{cmd}");
        let out = w
            .cc_moss()
            .env("HOME", w.path("home"))
            .env("PATH", "/usr/bin:/bin")
            .arg("doctor")
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&out.stdout);
        let line = report
            .lines()
            .find(|l| l.contains("turned off by a policy"))
            .unwrap_or_else(|| panic!("{report}"));
        assert!(line.contains("new sessions"), "{line}");
        assert!(line.contains(".claude/settings.json"), "{line}");
        assert!(line.contains("normal mode"), "{line}");

        // In the managed settings an administrator installs.
        let w = world("bypass-managed", "");
        let managed = w.path("managed/managed-settings.json");
        disable_bypass_in(&managed);
        let out = w
            .cc_moss()
            .env("HOME", w.path("home"))
            .env("CC_MOSS_CLAUDE_MANAGED_SETTINGS", &managed)
            .args(["new", "job-b", "--detach", "-C"])
            .arg(w.path("work"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let cmd = w.tmux(&[
            "list-panes",
            "-t",
            "host_job-b",
            "-F",
            "#{pane_start_command}",
        ]);
        assert!(!cmd.contains(BYPASS), "{cmd}");
    }

    #[test]
    fn permission_mode_rejects_unknown() {
        // In the file: `new` refuses and names the key.
        let w = world("bypass-bad", "[claude]\npermission_mode = \"yolo\"\n");
        let out = w
            .cc_moss()
            .args(["new", "job-a", "--detach", "-C"])
            .arg(w.path("work"))
            .output()
            .unwrap();
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("permission_mode"), "{err}");
        assert!(w.session_names().is_empty());
        // Through `config set`: refused before the file is touched.
        let w = world("bypass-bad-set", "");
        let out = w
            .cc_moss()
            .args(["config", "set", "claude.permission_mode", "yolo"])
            .output()
            .unwrap();
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("claude.permission_mode"), "{err}");
        assert!(err.contains("bypass, default"), "{err}");
        assert_eq!(std::fs::read_to_string(w.config_path()).unwrap(), "");
    }
}
