//! Crash and restore on a real, isolated tmux server with a fake `claude`.

mod common;

use common::{ls_json, wait_for, World};

fn slot_field(w: &World, name: &str, field: &str) -> Option<String> {
    ls_json(&w.state(), true)["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .and_then(|s| s[field].as_str().map(String::from))
}

/// Kill the tmux server: every pane process dies, no SessionEnd is sent.
fn crash(w: &World) {
    w.tmux(&["kill-server"]);
    wait_for(|| (slot_status(w, "job-b").as_deref() == Some("interrupted")).then_some(()));
}

fn slot_status(w: &World, name: &str) -> Option<String> {
    slot_field(w, name, "status")
}

fn restore(w: &World) -> String {
    let out = w.cc_moss().args(["restore", "--yes"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn crash_then_restore_full_cycle() {
    let w = World::new("cycle");
    w.new_session("job-a");
    let first_id = w.session_id("job-a");
    w.send(&first_id, "prompt job-b");
    wait_for(|| (w.session_names() == ["host_job-b"]).then_some(()));

    // /clear rotates the id; the restore must use the new one.
    w.send(&first_id, "clear");
    let latest = wait_for(|| slot_field(&w, "job-b", "session_id").filter(|id| *id != first_id));
    w.send(&latest, "prompt job-b");

    for round in 1..=2 {
        crash(&w);
        let out = restore(&w);
        assert!(out.contains("Started job-b"), "round {round}: {out}");
        let cmd = w.tmux(&[
            "list-panes",
            "-t",
            "host_job-b",
            "-F",
            "#{pane_start_command}",
        ]);
        assert!(
            cmd.contains(&format!("--resume {latest} --name=job-b")),
            "round {round}: {cmd}"
        );
        assert!(
            cmd.contains("--dangerously-skip-permissions"),
            "round {round}: {cmd}"
        );
        // The resumed process reports in and the slot is live again.
        wait_for(|| (slot_status(&w, "job-b").as_deref() == Some("running")).then_some(()));
        assert_eq!(
            ls_json(&w.state(), true)["sessions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn restore_replays_only_whitelisted_arguments() {
    let w = World::new("planted-args");
    w.new_session("job-p");
    let sid = w.session_id("job-p");
    w.send(&sid, "prompt job-p");
    wait_for(|| (w.session_names() == ["host_job-p"]).then_some(()));
    w.tmux(&["kill-server"]);
    wait_for(|| (slot_status(&w, "job-p").as_deref() == Some("interrupted")).then_some(()));
    // Arguments written straight into the registry, as something with write
    // access to it could: only the whitelist is replayed.
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET launch_args = ?1",
        [r#"["--dangerously-skip-permissions","--mcp-config","/tmp/evil.json","--settings","/tmp/x.json","--model","opus"]"#],
    )
    .unwrap();
    drop(db);
    let out = restore(&w);
    assert!(out.contains("Started job-p"), "{out}");
    let cmd = w.tmux(&[
        "list-panes",
        "-t",
        "host_job-p",
        "-F",
        "#{pane_start_command}",
    ]);
    assert!(
        !cmd.contains("--mcp-config") && !cmd.contains("evil"),
        "{cmd}"
    );
    assert!(!cmd.contains("--settings"), "{cmd}");
    assert!(cmd.contains("--model opus"), "{cmd}");
    assert!(cmd.contains("--dangerously-skip-permissions"), "{cmd}");
}

#[test]
fn empty_session_is_not_restored() {
    let w = World::new("empty");
    w.new_session("job-x");
    w.session_id("job-x");
    w.tmux(&["kill-server"]);
    wait_for(|| (slot_status(&w, "job-x").as_deref() == Some("interrupted")).then_some(()));

    let out = restore(&w);
    assert!(out.contains("no messages"), "{out}");
    assert!(!out.contains("Started"), "{out}");
    assert!(w.session_names().is_empty());
}
