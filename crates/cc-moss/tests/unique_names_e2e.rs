//! 普通 new 始终创建独立会话；编号不改变已有会话的身份或状态。

mod common;

use common::{ls_json, wait_for, World};

fn slots(w: &World) -> Vec<serde_json::Value> {
    ls_json(&w.state(), true)["sessions"]
        .as_array()
        .unwrap()
        .clone()
}

fn new(w: &World, name: &str, extra: &[&str]) -> std::process::Output {
    w.cc_moss()
        .args(["new", name, "--detach", "-C"])
        .arg(w.path("work"))
        .args(extra)
        .output()
        .unwrap()
}

/// `name` exists, has a transcript, and was killed without a SessionEnd.
fn interrupted(w: &World, name: &str, with_message: bool) -> String {
    w.new_session(name);
    let id = w.session_id(name);
    if with_message {
        w.send(&id, &format!("prompt {name}"));
        wait_for(|| {
            w.path("fifos")
                .join(format!("{id}.jsonl"))
                .exists()
                .then_some(())
        });
    }
    w.tmux(&["kill-server"]);
    wait_for(|| {
        slots(w)
            .iter()
            .any(|s| s["name"] == name && s["status"] == "interrupted")
            .then_some(())
    });
    id
}

#[test]
fn new_creates_numbered_session_without_entering_running_one() {
    let w = World::new("u-running");
    w.new_session("job-a");
    w.session_id("job-a");
    let out = new(&w, "job-a", &[]);
    assert!(out.status.success(), "{out:?}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("already running"));
    let fresh = w.session_id("job-a-2");
    assert_ne!(fresh, w.session_id("job-a"));
    let mut names = w.session_names();
    names.sort();
    assert_eq!(names, ["host_job-a", "host_job-a-2"]);
    assert_eq!(slots(&w).len(), 2);
}

#[test]
fn new_keeps_interrupted_session_and_creates_fresh_uuid() {
    let w = World::new("u-restore");
    let id = interrupted(&w, "job-a", true);
    let out = new(&w, "job-a", &["--yes"]);
    assert!(out.status.success(), "{out:?}");
    let cmd = w.tmux(&["list-panes", "-a", "-F", "#{pane_start_command}"]);
    assert!(!cmd.contains("--resume"), "{cmd}");
    assert_eq!(cmd.lines().count(), 1, "{cmd}");
    assert_ne!(w.session_id("job-a-2"), id);
    let all = slots(&w);
    assert_eq!(
        all.iter().find(|s| s["session_id"] == id).unwrap()["status"],
        "interrupted"
    );
}

#[test]
fn new_without_yes_is_fresh_when_interrupted() {
    let w = World::new("u-refuse");
    interrupted(&w, "job-a", true);
    let out = new(&w, "job-a", &[]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(w.session_names(), ["host_job-a-2"]);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("restore"));
}

#[test]
fn new_does_not_close_empty_interrupted_session() {
    let w = World::new("u-empty");
    let old = interrupted(&w, "job-x", false);
    let out = new(&w, "job-x", &[]);
    assert!(out.status.success(), "{out:?}");
    wait_for(|| (slots(&w).len() == 2).then_some(()));
    let s = slots(&w);
    let old_slot = s.iter().find(|s| s["session_id"] == old.as_str()).unwrap();
    assert_eq!(old_slot["status"], "interrupted");
    assert_ne!(w.session_id("job-x-2"), old);
    assert!(s
        .iter()
        .any(|s| s["session_id"] != old.as_str() && s["status"] == "running"));
}

#[test]
fn new_receipt_returns_actual_name_uuid_and_canonical_cwd() {
    let w = World::new("u-receipt");
    let out = new(&w, "receipt", &["--json"]);
    assert!(out.status.success(), "{out:?}");
    let receipt: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("new必须返回JSON，不透传--json");
    assert_eq!(receipt["schema"], 1);
    assert_eq!(receipt["result"], "starting");
    assert_eq!(receipt["name"], "receipt");
    assert!(uuid::Uuid::parse_str(receipt["session_id"].as_str().unwrap()).is_ok());
    assert_eq!(
        receipt["cwd"],
        w.path("work").canonicalize().unwrap().to_str().unwrap()
    );
    assert!(receipt["slot_id"].is_null());
    assert!(receipt["tmux"]["session_id"]
        .as_str()
        .unwrap()
        .starts_with('$'));
    assert_eq!(receipt["session_id"], w.session_id("receipt"));
    let argv = w.tmux(&["list-panes", "-a", "-F", "#{pane_start_command}"]);
    assert!(!argv.contains("--json"), "{argv}");
}

#[test]
fn new_concurrent_same_seed_reserves_two_distinct_sessions() {
    let w = World::new("u-concurrent");
    let first = w
        .cc_moss()
        .args(["new", "project", "--detach", "--json", "-C"])
        .arg(w.path("work"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let second = new(&w, "project", &["--json"]);
    let first = first.wait_with_output().unwrap();
    assert!(first.status.success(), "{first:?}");
    assert!(second.status.success(), "{second:?}");
    let a: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let b: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_ne!(a["session_id"], b["session_id"]);
    assert_ne!(a["tmux"]["session_id"], b["tmux"]["session_id"]);
    let mut names = [a["name"].as_str().unwrap(), b["name"].as_str().unwrap()];
    names.sort();
    assert_eq!(names, ["project", "project-2"]);
}

#[test]
fn new_delayed_hooks_still_reserve_display_name() {
    let mut w = World::new("u-no-hook");
    w.claude = common::fixture("silent-claude");
    let a = new(&w, "project", &["--json"]);
    let b = new(&w, "project", &["--json"]);
    assert!(a.status.success(), "{a:?}");
    assert!(b.status.success(), "{b:?}");
    let a: serde_json::Value = serde_json::from_slice(&a.stdout).unwrap();
    let b: serde_json::Value = serde_json::from_slice(&b.stdout).unwrap();
    assert_eq!(a["name"], "project");
    assert_eq!(b["name"], "project-2");
    assert_ne!(a["session_id"], b["session_id"]);
    assert!(a["slot_id"].is_null() && b["slot_id"].is_null());
}

#[test]
fn new_distinct_directories_with_same_basename_are_fresh() {
    let w = World::new("u-cwds");
    let mut receipts = Vec::new();
    for parent in ["one", "two"] {
        let dir = w.path(&format!("{parent}/project"));
        std::fs::create_dir_all(&dir).unwrap();
        let out = w
            .cc_moss()
            .args(["new", "--detach", "--json", "-C"])
            .arg(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        receipts.push(serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap());
    }
    assert_eq!(receipts[0]["name"], "project");
    assert_eq!(receipts[1]["name"], "project-2");
    assert_ne!(receipts[0]["session_id"], receipts[1]["session_id"]);
    assert_ne!(receipts[0]["cwd"], receipts[1]["cwd"]);
}

#[test]
fn new_sanitize_collisions_keep_number_in_every_surface() {
    let w = World::new("u-sanitize");
    for (seed, expected) in [("a:b", "a:b"), ("a.b", "a.b-2")] {
        let out = new(&w, seed, &["--json"]);
        assert!(out.status.success(), "{out:?}");
        let receipt: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(receipt["name"], expected);
        assert_eq!(receipt["session_id"], w.session_id(expected));
        let id = receipt["tmux"]["session_id"].as_str().unwrap();
        assert_eq!(
            w.tmux(&["show-option", "-v", "-t", id, "@cc-moss-name"])
                .trim(),
            expected
        );
    }
}

fn stub_tmux(w: &World) -> String {
    use std::os::unix::fs::PermissionsExt;
    let dir = w.path("stub");
    std::fs::create_dir_all(&dir).unwrap();
    let script = r#"#!/bin/sh
case " $* " in
  *" -V "*) printf 'tmux 3.5\n'; exit 0 ;;
  *" list-sessions "*) exit 1 ;;
  *" new-session "*)
    name='' sid=''
    while [ $# -gt 0 ]; do
      case "$1" in -s) name="$2"; shift;; --session-id) sid="$2"; shift;; esac
      shift
    done
    printf '%s %s\n' "$name" "$sid" >> "$CC_MOSS_CREATE_LOG"
    count=$(wc -l < "$CC_MOSS_CREATE_LOG" | tr -d ' ')
    case "$CC_MOSS_CREATE_MODE" in
      duplicate) printf 'duplicate session: %s\n' "$name" >&2; exit 1 ;;
      collide-twice) if [ "$count" -lt 3 ]; then printf 'duplicate session: %s\n' "$name" >&2; exit 1; fi ;;
      timeout) sleep 5; exit 0 ;;
      failure) printf 'permission denied (not a duplicate session)\n' >&2; exit 1 ;;
    esac
    printf '111|:|$7|:|%%9|:|fixture\n'; exit 0 ;;
  *" attach-session "*|*" switch-client "*) printf '%s\n' "$@" > "$CC_MOSS_ATTACH_LOG"; exit 0 ;;
  *" socket_path "*) printf '/tmp/fixture-socket\n' ;;
  *) printf '0\n' ;;
esac
"#;
    std::fs::write(dir.join("tmux"), script).unwrap();
    std::fs::set_permissions(dir.join("tmux"), std::fs::Permissions::from_mode(0o755)).unwrap();
    format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[test]
fn new_retries_only_confirmed_duplicates_with_same_uuid_and_bound() {
    for (mode, count, success) in [
        ("collide-twice", 3, true),
        ("duplicate", 32, false),
        ("failure", 1, false),
    ] {
        let w = World::new(&format!("u-{mode}"));
        let log = w.path("creates");
        let out = w
            .cc_moss()
            .env("PATH", stub_tmux(&w))
            .env("CC_MOSS_CREATE_LOG", &log)
            .env("CC_MOSS_CREATE_MODE", mode)
            .args(["new", "project", "--detach", "--json", "-C"])
            .arg(w.path("work"))
            .output()
            .unwrap();
        assert_eq!(out.status.success(), success, "{out:?}");
        let launches = std::fs::read_to_string(log).unwrap();
        let lines: Vec<_> = launches.lines().collect();
        assert_eq!(lines.len(), count, "{out:?}");
        let ids: std::collections::HashSet<_> = lines
            .iter()
            .map(|line| line.split_whitespace().nth(1).unwrap())
            .collect();
        assert_eq!(ids.len(), 1);
        if success {
            let receipt: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(receipt["name"], "project-3");
            assert_eq!(receipt["session_id"], *ids.iter().next().unwrap());
        }
    }
}

#[test]
fn new_timeout_returns_unconfirmed_uuid_without_second_launch() {
    let w = World::new("u-timeout");
    let log = w.path("creates");
    let out = w
        .cc_moss()
        .env("PATH", stub_tmux(&w))
        .env("CC_MOSS_CREATE_LOG", &log)
        .env("CC_MOSS_CREATE_MODE", "timeout")
        .env("CC_MOSS_NEW_TIMEOUT_MS", "1500")
        .args(["new", "project", "--detach", "--json", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let receipt: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["result"], "unconfirmed");
    let launches = std::fs::read_to_string(log).unwrap();
    assert_eq!(launches.lines().count(), 1);
    assert!(launches.contains(receipt["session_id"].as_str().unwrap()));
    assert!(receipt["tmux"].is_null());
}

#[test]
fn new_attaches_by_returned_id_and_never_switches_another_server() {
    for cross_socket in [false, true] {
        let w = World::new(if cross_socket {
            "u-cross-socket"
        } else {
            "u-id-attach"
        });
        let attach = w.path("attach");
        let mut cmd = w.cc_moss();
        cmd.env("PATH", stub_tmux(&w))
            .env("CC_MOSS_CREATE_LOG", w.path("creates"))
            .env("CC_MOSS_CREATE_MODE", "success")
            .env("CC_MOSS_ATTACH_LOG", &attach);
        if cross_socket {
            cmd.env("TMUX", "/tmp/other-test-server,1,1");
        }
        let out = cmd
            .args(["new", "project", "-C"])
            .arg(w.path("work"))
            .output()
            .unwrap();
        if cross_socket {
            assert!(!out.status.success(), "{out:?}");
            assert!(!attach.exists());
        } else {
            assert!(out.status.success(), "{out:?}");
            let words = std::fs::read_to_string(attach).unwrap();
            assert!(words.contains("attach-session\n-t\n$7\n"), "{words}");
            assert!(!words.contains("host_project"));
        }
    }
}

#[test]
fn new_rejects_root_before_tmux_or_registry_side_effects() {
    let w = World::new("u-root");
    let log = w.path("creates");
    let out = w
        .cc_moss()
        .env("PATH", stub_tmux(&w))
        .env("CC_MOSS_CREATE_LOG", &log)
        .args(["new", "project", "--detach", "--json", "-C", "/"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(!log.exists());
    assert!(!w.state().join("registry.db").exists());
}

#[test]
fn new_long_unicode_suffix_survives_hook_name_sync() {
    let w = World::new("u-unicode");
    let seed = "长名字".repeat(25);
    for ordinal in [1, 2] {
        let out = new(&w, &seed, &["--json"]);
        assert!(out.status.success(), "{out:?}");
        let receipt: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let expected = moss_core::naming::fresh_name(&seed, ordinal);
        assert_eq!(receipt["name"], expected);
        assert_eq!(receipt["session_id"], w.session_id(&expected));
        let physical = moss_core::naming::session_name(Some("host"), &expected);
        assert!(w.session_names().contains(&physical));
        if ordinal == 2 {
            assert!(physical.ends_with("-2"));
        }
    }
}

#[test]
fn new_accepts_jarvis_name_alias_without_overriding_fresh_receipt() {
    let w = World::new("u-name-alias");
    for expected in ["named", "named-2"] {
        let out = w
            .cc_moss()
            .args(["new", "-n", "named", "--detach", "--json", "--debug", "-C"])
            .arg(w.path("work"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let receipt: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(receipt["name"], expected);
        let id = receipt["tmux"]["session_id"].as_str().unwrap();
        let argv = w.tmux(&["list-panes", "-t", id, "-F", "#{pane_start_command}"]);
        assert_eq!(argv.matches("--name=").count(), 1, "{argv}");
        assert!(argv.contains(&format!("--name={expected}")), "{argv}");
        assert!(!argv.contains("--resume"));
    }
}

#[test]
fn new_ignores_closed_session_with_same_name() {
    let w = World::new("u-closed");
    w.new_session("job-c");
    let id = w.session_id("job-c");
    w.send(&id, "exit");
    wait_for(|| (slots(&w)[0]["status"] == "closed").then_some(()));
    let out = new(&w, "job-c", &[]);
    assert!(out.status.success(), "{out:?}");
    wait_for(|| (slots(&w).len() == 2).then_some(()));
}
