//! Moss from the command line without tmux: the hook records its role,
//! `close` will not end it, and its guard hook says no when in doubt.

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

use common::{bin, dead_pid, ls_json, start_json};

/// A SessionStart from Moss's Claude Code (`CC_MOSS_ROLE=moss` and the
/// session id the daemon gave it), or from an ordinary session.
fn start(state: &Path, sid: &str, title: &str, moss: bool) {
    let env: &[(&str, &str)] = if moss {
        &[("CC_MOSS_ROLE", "moss"), ("CC_MOSS_MOSS_SESSION", sid)]
    } else {
        &[]
    };
    start_with(state, sid, title, env);
}

fn start_with(state: &Path, sid: &str, title: &str, env: &[(&str, &str)]) {
    let mut cmd = bin();
    cmd.env("CC_MOSS_STATE_DIR", state)
        .env("CLAUDE_PID", dead_pid().to_string())
        .args(["hook", "session-start"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(start_json(sid, "startup", Some(title)).as_bytes())
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
}

fn session<'a>(ls: &'a serde_json::Value, sid: &str) -> &'a serde_json::Value {
    ls["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == sid)
        .unwrap_or_else(|| panic!("{sid} not listed in {ls}"))
}

#[test]
fn moss_reads_sessions_but_changes_none() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    start(&state, "work-0000-1111", "work", false);
    let as_moss = |args: &[&str]| {
        bin()
            .env("CC_MOSS_STATE_DIR", &state)
            .env("CC_MOSS_ROLE", "moss")
            .args(args)
            .output()
            .unwrap()
    };
    // ls reads the daemon's copy in its home (its sandbox can't open the
    // registry); none yet, and it says why.
    let out = as_moss(&["ls", "--json", "--all"]);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("the daemon writes it"),
        "{out:?}"
    );
    // The checkup's facts come from the daemon's copy in its home; none yet.
    let out = as_moss(&["doctor", "--sessions", "--json"]);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("the daemon writes it"),
        "{out:?}"
    );
    // Changing does not, and says so plainly.
    let out = as_moss(&["close", "work"]);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("moss act"),
        "{out:?}"
    );
    // Nothing was written: no repair recorded the dead process.
    let events = bin()
        .env("CC_MOSS_STATE_DIR", &state)
        .args(["events", "--json"])
        .output()
        .unwrap();
    let events: serde_json::Value = serde_json::from_slice(&events.stdout).unwrap();
    assert!(
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["kind"] != "interrupted"),
        "{events}"
    );
    assert_ne!(
        session(&ls_json(&state, true), "work-0000-1111")["status"],
        "closed"
    );
}

#[test]
fn moss_lists_sessions_from_its_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let home = state.join("moss");
    std::fs::create_dir_all(&home).unwrap();
    let row = |sid: &str, name: &str, status: &str, archived: Option<&str>| {
        serde_json::json!({
            "id": sid, "name": name, "status": status, "session_id": sid,
            "cwd": "/work", "tmux": null, "notes": {"goal": null, "state": null, "note": null},
            "last_active_at": "2026-10-20T10:00:00Z", "archived_at": archived,
        })
    };
    let snapshot = serde_json::json!({
        "schema": 1,
        "provider": {"host": null},
        "sessions": [
            row("run-0000-1111", "api", "running", None),
            row("old-0000-2222", "docs", "closed", None),
            row("arc-0000-3333", "fizzy-1153", "closed", Some("2026-10-19T10:00:00Z")),
        ],
    });
    std::fs::write(home.join("sessions.json"), snapshot.to_string()).unwrap();
    let as_moss = |args: &[&str]| {
        let out = bin()
            .env("CC_MOSS_STATE_DIR", &state)
            .env("CC_MOSS_ROLE", "moss")
            .env("CC_MOSS_LANG", "en")
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    let names = |args: &[&str]| -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(&as_moss(args)).unwrap();
        v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap().to_string())
            .collect()
    };
    // The same choices as `ls` over the registry.
    assert_eq!(names(&["ls", "--archived", "--json"]), ["fizzy-1153"]);
    assert_eq!(names(&["ls", "--json"]), ["api", "fizzy-1153"]);
    assert_eq!(
        names(&["ls", "--all", "--json"]),
        ["api", "docs", "fizzy-1153"]
    );
    let table = as_moss(&["ls", "--all"]);
    assert!(table.contains("api") && table.contains("docs"), "{table}");
    assert!(
        !table.contains("fizzy-1153"),
        "archived stay out of the table: {table}"
    );
}

#[test]
fn moss_profile_reads_only_from_its_home() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let home = state.join("moss");
    std::fs::create_dir_all(home.join("drafts")).unwrap();
    let outside = dir.path().join("history.jsonl");
    std::fs::write(&outside, "other sessions' words\n").unwrap();
    let run = |from: &Path| {
        bin()
            .env("CC_MOSS_STATE_DIR", &state)
            .args(["moss", "profile", "soul", "--from"])
            .arg(from)
            .output()
            .unwrap()
    };
    let out = run(&outside);
    assert!(!out.status.success(), "{out:?}");
    assert!(!home.join("SOUL.md").exists(), "nothing copied in");
    // Through a link from the home, too.
    std::os::unix::fs::symlink(&outside, home.join("drafts/link.md")).unwrap();
    assert!(!run(&home.join("drafts/link.md")).status.success());
    assert!(!home.join("SOUL.md").exists());
    // A folder on the way that leads out, and `..`.
    let away = dir.path().join("away");
    std::fs::create_dir_all(&away).unwrap();
    std::fs::write(away.join("soul.md"), "other sessions' words\n").unwrap();
    std::os::unix::fs::symlink(&away, home.join("drafts/away")).unwrap();
    assert!(!run(&home.join("drafts/away/soul.md")).status.success());
    assert!(!run(&home.join("drafts/../../away/soul.md"))
        .status
        .success());
    assert!(!home.join("SOUL.md").exists());
    // A draft in its home is fine.
    std::fs::write(home.join("drafts/soul.md"), "# Soul\nCalm.\n").unwrap();
    let out = run(&home.join("drafts/soul.md"));
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(home.join("SOUL.md")).unwrap(),
        "# Soul\nCalm.\n"
    );
    // The old one is kept in .snapshots, never through a link put there.
    std::fs::remove_dir_all(home.join(".snapshots")).ok();
    std::os::unix::fs::symlink(&away, home.join(".snapshots")).unwrap();
    std::fs::write(home.join("drafts/soul.md"), "# Soul\nBold.\n").unwrap();
    let out = run(&home.join("drafts/soul.md"));
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_dir(&away).unwrap().count(),
        1,
        "nothing written out"
    );
    let kept: Vec<_> = std::fs::read_dir(home.join(".snapshots"))
        .unwrap()
        .collect();
    assert_eq!(kept.len(), 1);
}

#[test]
fn moss_reads_the_checkup_facts_from_its_home() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let home = state.join("moss");
    std::fs::create_dir_all(&home).unwrap();
    // What the daemon wrote for it: Moss's sandbox can't read the registry.
    let facts = r#"{"schema":1,"sessions":[{"name":"api"}],"install":{}}"#;
    std::fs::write(home.join("facts.json"), facts).unwrap();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", &state)
        .env("CC_MOSS_ROLE", "moss")
        .args(["doctor", "--sessions", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["sessions"][0]["name"], "api", "{v}");
}

#[test]
fn hook_records_the_moss_role() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    start(&state, "moss-0000-1111", "moss", true);
    start(&state, "work-0000-1111", "work", false);
    // The role alone does not make a session Moss: a window opened in
    // Moss's tmux, or `claude --resume` of Moss elsewhere, inherits it.
    start_with(
        &state,
        "intruder-0000-1111",
        "intruder",
        &[
            ("CC_MOSS_ROLE", "moss"),
            ("CC_MOSS_MOSS_SESSION", "moss-0000-1111"),
        ],
    );
    start_with(
        &state,
        "bare-0000-1111",
        "bare",
        &[("CC_MOSS_ROLE", "moss")],
    );
    let ls = ls_json(&state, true);
    assert_eq!(session(&ls, "moss-0000-1111")["role"], "moss");
    for other in ["work-0000-1111", "intruder-0000-1111", "bare-0000-1111"] {
        assert!(session(&ls, other)["role"].is_null(), "{other}");
    }
}

#[test]
fn close_refuses_moss() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    // Both died (dead pids): interrupted, which `close` would normally end.
    start(&state, "moss-0000-1111", "moss", true);
    start(&state, "work-0000-1111", "work", false);
    let out = bin()
        .env("CC_MOSS_STATE_DIR", &state)
        .args(["close", "moss", "work"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Moss is looked after by cc-moss"), "{err}");
    assert!(err.contains("Settings"), "{err}");
    let ls = ls_json(&state, true);
    assert_ne!(session(&ls, "moss-0000-1111")["status"], "closed");
    assert_eq!(
        session(&ls, "work-0000-1111")["status"],
        "closed",
        "the others are still closed"
    );
}

fn guard(home: &Path, payload: &str) -> Output {
    let mut child = bin()
        // Claude Code's project folder is not where the guard looks.
        .env("CLAUDE_PROJECT_DIR", "/")
        .args(["hook", "moss-guard", "--home"])
        .arg(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
    child.wait_with_output().unwrap()
}

#[test]
fn guard_hook_blocks_with_exit_2_and_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("moss");
    std::fs::create_dir_all(&home).unwrap();
    let tool = |name: &str, input: serde_json::Value| {
        serde_json::json!({ "tool_name": name, "cwd": home, "tool_input": input }).to_string()
    };
    let shell_cmd = tool(
        "Bash",
        serde_json::json!({ "command": "echo hi >> journal/today.md" }),
    );
    // The shell, in its sandbox, is how Moss touches files.
    let shell = guard(&home, &shell_cmd);
    assert_eq!(shell.status.code(), Some(0), "{shell:?}");
    assert!(shell.stdout.is_empty() && shell.stderr.is_empty());

    // Claude Code's file tools are off, inside the home too: a link swapped
    // in after the check would lead them anywhere.
    for (name, input) in [
        (
            "Write",
            serde_json::json!({ "file_path": "journal/today.md", "content": "x" }),
        ),
        (
            "Write",
            serde_json::json!({ "file_path": "/etc/hosts", "content": "x" }),
        ),
        ("Edit", serde_json::json!({ "file_path": "memory/a.md" })),
        ("Read", serde_json::json!({ "file_path": "SESSIONS.md" })),
        ("Grep", serde_json::json!({ "pattern": "x", "path": "/" })),
        ("Glob", serde_json::json!({ "pattern": "**/*" })),
    ] {
        let out = guard(&home, &tool(name, input));
        assert_eq!(out.status.code(), Some(2), "{name}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("with the shell"),
            "{name}: {out:?}"
        );
    }

    // An MCP server's write tool never gets past it.
    let mcp = tool(
        "mcp__fs__write_file",
        serde_json::json!({ "path": "/etc/hosts", "content": "x" }),
    );
    assert_eq!(guard(&home, &mcp).status.code(), Some(2));

    // Without --home (settings of an older cc-moss): no, even for a
    // shell command that would otherwise go through.
    let mut child = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .env("CLAUDE_PROJECT_DIR", &home)
        .args(["hook", "moss-guard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(shell_cmd.as_bytes());
    assert_eq!(child.wait().unwrap().code(), Some(2));

    // Unreadable input: no.
    for junk in ["", "not json", "{\"tool_input\":{}}"] {
        let out = guard(&home, junk);
        assert_eq!(out.status.code(), Some(2), "{junk:?}: {out:?}");
    }
}

#[test]
fn moss_status_reports_off_after_stop_without_a_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let config = dir.path().join("config.toml");
    let run = |args: &[&str]| {
        bin()
            .env("CC_MOSS_STATE_DIR", &state)
            .env("CC_MOSS_CONFIG", &config)
            .args(args)
            .output()
            .unwrap()
    };
    let st: serde_json::Value =
        serde_json::from_slice(&run(&["moss", "status", "--json"]).stdout).unwrap();
    assert_eq!(
        st,
        serde_json::json!({"schema":1,"enabled":true,"running":false,"restarts":0,
            "daemon_running":false,"activity":"offline","queued":0,
            "account":st["account"].clone()})
    );
    // The account kind comes along (this test's HOME has no Claude Code).
    let account = st["account"].as_str().unwrap_or_default().to_string();
    assert!(
        ["subscription", "api", "cloud", "unknown"].contains(&account.as_str()),
        "{st}"
    );
    // The daemon's view wins: it is what Moss runs with.
    std::fs::create_dir_all(state.join("daemon")).unwrap();
    std::fs::write(state.join("daemon/moss-account.txt"), "api\n").unwrap();
    let st: serde_json::Value =
        serde_json::from_slice(&run(&["moss", "status", "--json"]).stdout).unwrap();
    assert_eq!(st["account"], "api", "{st}");
    // Text stuck in Moss's input is said, not silent.
    std::fs::create_dir_all(state.join("moss")).unwrap();
    std::fs::write(state.join("moss/input-busy"), "").unwrap();
    let st: serde_json::Value =
        serde_json::from_slice(&run(&["moss", "status", "--json"]).stdout).unwrap();
    assert_eq!(st["input_busy"], true, "{st}");
    std::fs::remove_file(state.join("moss/input-busy")).unwrap();
    start(&state, "moss-0000-1111", "moss", true);
    let out = run(&["moss", "stop"]);
    assert!(out.status.success(), "{out:?}");
    let st: serde_json::Value =
        serde_json::from_slice(&run(&["moss", "status", "--json"]).stdout).unwrap();
    assert_eq!(st["enabled"], false);
    // Not on Moss's tmux server (this test has none): not Moss's to close.
    assert_ne!(
        session(&ls_json(&state, true), "moss-0000-1111")["status"],
        "closed"
    );
    let on = run(&["moss", "start"]);
    assert!(on.status.success(), "{on:?}");
    assert!(
        String::from_utf8_lossy(&on.stdout).contains("daemon start"),
        "says the daemon is not running: {on:?}"
    );
}

/// A tmux server name in a socket folder tmux won't use (anyone may write
/// to it): tmux neither connects nor says that nothing runs there, so
/// whether Moss runs is unknown. (A plain file at the socket's path won't
/// do: Linux tmux reads the refused connection as "no server running".)
struct UnclearTmux {
    dir: tempfile::TempDir,
    name: String,
}

impl UnclearTmux {
    fn new() -> UnclearTmux {
        common::debug_only();
        let dir = tempfile::tempdir().unwrap();
        let uid = String::from_utf8(
            std::process::Command::new("id")
                .arg("-u")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let sockets = dir.path().join(format!("tmux-{}", uid.trim()));
        std::fs::create_dir_all(&sockets).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sockets, std::fs::Permissions::from_mode(0o777)).unwrap();
        let name = format!("cc-moss-test-{}-unclear", std::process::id());
        UnclearTmux { dir, name }
    }
}

#[test]
fn moss_stop_says_so_when_tmux_cannot_tell() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let config = dir.path().join("config.toml");
    let tmux = UnclearTmux::new();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", &state)
        .env("CC_MOSS_CONFIG", &config)
        .env("TMUX_TMPDIR", tmux.dir.path())
        .env("CC_MOSS_MOSS_SOCKET", &tmux.name)
        .args(["moss", "stop"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "a stop that may not have happened: {out:?}"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("could not make sure it stopped"), "{err}");
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("Stopped Moss"),
        "{out:?}"
    );
    // The switch is off all the same, for the daemon to act on.
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("enabled = false"), "{text}");
}

#[test]
fn moss_switch_lives_where_the_daemon_reads_it() {
    // A shell with its own XDG_CONFIG_HOME: without a LaunchAgent pinning
    // the path, CC_MOSS_CONFIG decides, and status reads the same file.
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("pinned.toml");
    let run = |args: &[&str]| {
        bin()
            .env("CC_MOSS_STATE_DIR", dir.path().join("state"))
            .env("CC_MOSS_CONFIG", &config)
            .env("XDG_CONFIG_HOME", dir.path().join("elsewhere"))
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["moss", "stop"]).status.success());
    assert!(std::fs::read_to_string(&config)
        .unwrap()
        .contains("enabled = false"));
    assert!(!dir.path().join("elsewhere").exists());
    let st: serde_json::Value =
        serde_json::from_slice(&run(&["moss", "status", "--json"]).stdout).unwrap();
    assert_eq!(st["enabled"], false);
}
