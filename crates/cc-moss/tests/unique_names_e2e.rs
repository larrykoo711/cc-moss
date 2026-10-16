//! One open session per name (T1.5): `new` enters or restores instead of duplicating.

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
fn new_enters_running_session_with_same_name() {
    let w = World::new("u-running");
    w.new_session("job-a");
    w.session_id("job-a");
    let out = new(&w, "job-a", &[]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("already running"));
    assert_eq!(w.session_names(), vec!["host_job-a"]);
    assert_eq!(slots(&w).len(), 1);
}

#[test]
fn new_restores_interrupted_session_with_same_name() {
    let w = World::new("u-restore");
    let id = interrupted(&w, "job-a", true);
    let out = new(&w, "job-a", &["--yes"]);
    assert!(out.status.success(), "{out:?}");
    let cmd = w.tmux(&["list-panes", "-a", "-F", "#{pane_start_command}"]);
    assert!(cmd.contains(&format!("--resume {id}")), "{cmd}");
    assert_eq!(cmd.lines().count(), 1, "{cmd}");
    wait_for(|| (slots(&w).len() == 1 && slots(&w)[0]["status"] == "running").then_some(()));
}

#[test]
fn new_without_yes_refuses_when_interrupted() {
    let w = World::new("u-refuse");
    interrupted(&w, "job-a", true);
    let out = new(&w, "job-a", &[]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cc-moss restore job-a"),
        "{out:?}"
    );
    assert!(w.session_names().is_empty());
}

#[test]
fn new_replaces_empty_interrupted_session() {
    let w = World::new("u-empty");
    let old = interrupted(&w, "job-x", false);
    let out = new(&w, "job-x", &[]);
    assert!(out.status.success(), "{out:?}");
    wait_for(|| (slots(&w).len() == 2).then_some(()));
    let s = slots(&w);
    let old_slot = s.iter().find(|s| s["session_id"] == old.as_str()).unwrap();
    assert_eq!(old_slot["status"], "closed");
    assert!(s
        .iter()
        .any(|s| s["session_id"] != old.as_str() && s["status"] == "running"));
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
