mod common;

use common::{hook, ls_json, start_json};
use std::os::unix::fs::PermissionsExt;

#[test]
fn startup_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let out = hook(
        dir.path(),
        "session-start",
        &start_json("U1", "startup", Some("job-a")),
        Some(std::process::id()),
    );
    assert!(out.status.success());
    let v = ls_json(dir.path(), true);
    assert_eq!(v["sessions"][0]["name"], "job-a");
    assert_eq!(v["sessions"][0]["status"], "running");
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
    assert_eq!(v["sessions"].as_array().unwrap().len(), 400);
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
    assert_eq!(v["sessions"].as_array().unwrap().len(), 0, "{v}");
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
        ls_json(dir.path(), true)["sessions"]
            .as_array()
            .unwrap()
            .len(),
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
    assert_eq!(v["sessions"].as_array().unwrap().len(), 1, "{v}");
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
    assert_eq!(v["sessions"][0]["name"], "x");
    assert_eq!(
        v["sessions"][0]["tmux"]["socket"],
        "/nonexistent/tmux-socket"
    );
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

#[test]
fn name_comes_from_the_transcript_when_the_payload_lacks_it() {
    // After `--resume`, Claude Code leaves session_title out of the payload,
    // but the transcript records the last /rename.
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().join("t.jsonl");
    std::fs::write(
        &t,
        "{\"type\":\"user\"}\n{\"type\":\"custom-title\",\"customTitle\":\"job-final\",\"sessionId\":\"R\"}\n{\"type\":\"user\"}\n",
    )
    .unwrap();
    let p = common::FakeProcess::spawn();
    let payload = serde_json::json!({
        "session_id": "R", "cwd": "/work", "source": "resume", "transcript_path": t,
    })
    .to_string();
    assert!(hook(dir.path(), "session-start", &payload, p.pid())
        .status
        .success());
    assert_eq!(
        ls_json(dir.path(), true)["sessions"][0]["name"],
        "job-final"
    );
}

#[test]
fn ls_backfills_names_of_sessions_registered_earlier() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().join("t.jsonl");
    std::fs::write(&t, "{\"type\":\"user\"}\n").unwrap();
    let p = common::FakeProcess::spawn();
    let payload = serde_json::json!({
        "session_id": "R", "cwd": "/work", "source": "resume", "transcript_path": t,
    })
    .to_string();
    hook(dir.path(), "session-start", &payload, p.pid());
    assert!(ls_json(dir.path(), true)["sessions"][0]["name"].is_null());
    // Renamed later (before cc-moss saw a title in any payload); Claude
    // Code appends to the transcript.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(&t).unwrap();
    writeln!(
        f,
        "{{\"type\":\"custom-title\",\"customTitle\":\"job-late\",\"sessionId\":\"R\"}}"
    )
    .unwrap();
    assert_eq!(ls_json(dir.path(), true)["sessions"][0]["name"], "job-late");
}

/// Runs `cc-moss <args>` as a hook would, with an empty payload.
fn hook_args(state: &std::path::Path, args: &[&str]) -> std::process::Output {
    common::bin()
        .env("CC_MOSS_STATE_DIR", state)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn hook_with_unknown_arguments_exits_zero_silently() {
    // A newer cc-moss may write hook commands with flags this one does not
    // know; exit 2 would block the prompt in Claude Code.
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["hook", "stop", "--future-flag"][..],
        &["hook", "--future-flag", "stop"][..],
        &["hook"][..],
        &["hook", "stop", "extra", "words"][..],
    ] {
        let out = hook_args(dir.path(), args);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
        assert!(
            out.stdout.is_empty() && out.stderr.is_empty(),
            "{args:?}: {out:?}"
        );
    }
}

#[test]
fn hook_panic_is_silent_and_logged() {
    let dir = tempfile::tempdir().unwrap();
    let out = common::bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .env("CC_MOSS_HOOK_PANIC", "1")
        .args(["hook", "stop"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
    assert!(log.contains("panic"), "{log}");
}

#[test]
fn hook_log_is_private() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let out = hook(&state, "session-start", "{not json", Some(1));
    assert!(out.status.success());
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(&state.join("hook.log")), 0o600);
}

#[test]
fn hook_without_home_writes_nothing_relative() {
    let dir = tempfile::tempdir().unwrap();
    let out = common::bin()
        .env_remove("HOME")
        .env_remove("CC_MOSS_STATE_DIR")
        .env_remove("XDG_STATE_HOME")
        .current_dir(dir.path())
        .args(["hook", "session-start"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "nothing created in the working directory"
    );
}

#[test]
fn ls_without_home_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let out = common::bin()
        .env_remove("HOME")
        .env_remove("CC_MOSS_STATE_DIR")
        .env_remove("XDG_STATE_HOME")
        .current_dir(dir.path())
        .args(["ls"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("HOME") && err.contains("CC_MOSS_STATE_DIR"),
        "{err}"
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}
