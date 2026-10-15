mod common;

use common::{hook, ls_json, start_json};
use std::os::unix::fs::PermissionsExt;

#[test]
fn startup_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let out = hook(
        dir.path(),
        "session-start",
        &start_json("U1", "startup", Some("em-a")),
        Some(std::process::id()),
    );
    assert!(out.status.success());
    let v = ls_json(dir.path(), true);
    assert_eq!(v["slots"][0]["name"], "em-a");
    assert_eq!(v["slots"][0]["status"], "running");
}

#[test]
fn invalid_json_exits_zero_and_logs() {
    let dir = tempfile::tempdir().unwrap();
    let out = hook(dir.path(), "session-start", "{not json", Some(1));
    assert!(out.status.success(), "hook must exit 0: {out:?}");
    assert!(out.stdout.is_empty());
    let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
    assert!(log.contains("session-start"), "log: {log}");
}

#[test]
fn unknown_event_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let out = hook(dir.path(), "made-up-event", "{}", Some(1));
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty());
}

#[test]
fn unwritable_state_dir_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    let out = hook(
        &locked.join("state"),
        "session-start",
        &start_json("U1", "startup", None),
        Some(1),
    );
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty());
}

#[test]
fn concurrent_hooks_no_lost_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let workers: Vec<_> = (0..8)
        .map(|w| {
            let path = path.clone();
            std::thread::spawn(move || {
                for i in 0..50 {
                    let id = format!("W{w}-{i}");
                    let out = hook(
                        &path,
                        "session-start",
                        &start_json(&id, "startup", None),
                        Some(10_000 + w * 100 + i),
                    );
                    assert!(out.status.success());
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let v = ls_json(&path, true);
    assert_eq!(v["slots"].as_array().unwrap().len(), 400);
    let log = std::fs::read_to_string(path.join("hook.log")).unwrap_or_default();
    assert!(!log.contains("locked"), "lock errors: {log}");
}

#[test]
fn non_interactive_sessions_are_ignored() {
    // F10: `claude -p` fires every hook and ends with reason=other.
    let dir = tempfile::tempdir().unwrap();
    for (key, val) in [
        ("CLAUDE_CODE_ENTRYPOINT", "sdk-cli"),
        ("CLAUDE_CODE_SESSION_ATTENDED", "0"),
    ] {
        let mut cmd = common::bin();
        cmd.env("CC_MOSS_STATE_DIR", dir.path())
            .env("CLAUDE_PID", "1")
            .env(key, val)
            .args(["hook", "session-start"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(start_json(key, "startup", None).as_bytes())
            .unwrap();
        assert!(child.wait_with_output().unwrap().status.success());
    }
    let v = ls_json(dir.path(), true);
    assert_eq!(v["slots"].as_array().unwrap().len(), 0, "{v}");
}

#[test]
fn missing_pid_is_logged() {
    let dir = tempfile::tempdir().unwrap();
    let out = hook(
        dir.path(),
        "session-start",
        &start_json("U1", "startup", None),
        None,
    );
    assert!(out.status.success());
    let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
    assert!(log.contains("CLAUDE_PID"), "{log}");
    assert_eq!(
        ls_json(dir.path(), true)["slots"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn inherited_entrypoint_does_not_drop_attended_sessions() {
    // Reviewer R3: CLAUDE_CODE_ENTRYPOINT is inherited from the environment
    // (e.g. a tmux server first started by an SDK agent); ATTENDED is not.
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = common::bin();
    cmd.env("CC_MOSS_STATE_DIR", dir.path())
        .env("CLAUDE_PID", "1")
        .env("CLAUDE_CODE_ENTRYPOINT", "sdk-cli")
        .env("CLAUDE_CODE_SESSION_ATTENDED", "1")
        .args(["hook", "session-start"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(start_json("U1", "startup", None).as_bytes())
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    let v = ls_json(dir.path(), true);
    assert_eq!(v["slots"].as_array().unwrap().len(), 1, "{v}");
}

#[test]
fn outside_tmux_warns_via_system_message() {
    let dir = tempfile::tempdir().unwrap();
    let out = hook(
        dir.path(),
        "session-start",
        &start_json("U1", "startup", None),
        Some(std::process::id()),
    );
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert!(v["systemMessage"].as_str().unwrap().contains("tmux"), "{v}");
    assert_eq!(v.as_object().unwrap().len(), 1, "only systemMessage: {v}");

    // Later events stay silent.
    let out = hook(
        dir.path(),
        "user-prompt-submit",
        &serde_json::json!({"session_id": "U1"}).to_string(),
        Some(std::process::id()),
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn tmux_failure_does_not_block_registry() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = common::bin();
    cmd.env("CC_MOSS_STATE_DIR", dir.path())
        .env("CLAUDE_PID", std::process::id().to_string())
        .env("TMUX", "/nonexistent/tmux-socket,1,0")
        .env("TMUX_PANE", "%0")
        .args(["hook", "session-start"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(start_json("U1", "startup", Some("x")).as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty(), "in tmux: no warning");
    let v = ls_json(dir.path(), true);
    assert_eq!(v["slots"][0]["name"], "x");
    assert_eq!(v["slots"][0]["tmux"]["socket"], "/nonexistent/tmux-socket");
}

#[test]
fn rename_collision_warns() {
    // T1.5: `/rename` into a name another open session already has.
    let dir = tempfile::tempdir().unwrap();
    let (pa, pb) = (common::FakeProcess::spawn(), common::FakeProcess::spawn());
    hook(
        dir.path(),
        "session-start",
        &start_json("A", "startup", Some("api")),
        pa.pid(),
    );
    hook(
        dir.path(),
        "session-start",
        &start_json("B", "startup", Some("web")),
        pb.pid(),
    );
    let out = hook(
        dir.path(),
        "user-prompt-submit",
        &serde_json::json!({"session_id": "B", "session_title": "api"}).to_string(),
        pb.pid(),
    );
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert!(
        v["systemMessage"]
            .as_str()
            .unwrap()
            .contains("already named"),
        "{v}"
    );

    // No collision, no output.
    let out = hook(
        dir.path(),
        "user-prompt-submit",
        &serde_json::json!({"session_id": "B", "session_title": "web2"}).to_string(),
        pb.pid(),
    );
    assert!(out.stdout.is_empty());
}
