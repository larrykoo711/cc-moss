//! End to end on a real, isolated tmux server with a fake `claude`.

mod common;

use common::{fixture, ls_json, wait_for, World};

#[test]
fn new_creates_named_session() {
    let w = World::new("new");
    w.new_session("job-a");
    w.session_id("job-a");
    assert_eq!(w.session_names(), vec!["host_job-a"]);
    let cmd = w.tmux(&[
        "list-panes",
        "-t",
        "host_job-a",
        "-F",
        "#{pane_start_command}",
    ]);
    assert!(cmd.contains("--name=job-a --session-id"), "{cmd}");
    assert!(cmd.contains("--dangerously-skip-permissions"), "{cmd}");
    let slot = &ls_json(&w.state(), false)["sessions"][0];
    assert_eq!(slot["tmux"]["session_name"], "host_job-a");
}

#[test]
fn prompt_title_renames_tmux_session() {
    let w = World::new("rename");
    w.new_session("job-a");
    let sid = w.session_id("job-a");

    // `/rename job-b` fires nothing; the next prompt carries the new title.
    w.send(&sid, "prompt job-b");
    wait_for(|| (w.session_names() == ["host_job-b"]).then_some(()));

    // A manual tmux rename is reverted on the next prompt.
    w.tmux(&["rename-session", "-t", "host_job-b", "manual"]);
    assert_eq!(w.session_names(), vec!["manual"]);
    w.send(&sid, "prompt job-b");
    wait_for(|| (w.session_names() == ["host_job-b"]).then_some(()));
}

#[test]
fn a_rename_without_a_prompt_shows_at_the_next_look() {
    // The panel renames an idle session with `/rename`, which fires no hook:
    // the next `ls` (the panel refreshing) picks it up from the transcript.
    let w = World::new("idle-rename");
    w.new_session("job-a");
    let sid = w.session_id("job-a");
    w.send(&sid, "prompt job-a");
    let ls = || {
        let out = w.cc_moss().args(["ls", "--json"]).output().unwrap();
        assert!(out.status.success(), "ls failed: {out:?}");
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["sessions"][0].clone()
    };
    wait_for(|| (ls()["name"] == "job-a").then_some(()));
    w.send(&sid, "rename job-z");
    wait_for(|| (ls()["name"] == "job-z" && w.session_names() == ["host_job-z"]).then_some(()));
}

#[test]
fn single_window_and_primary_slot_owns_name() {
    let w = World::new("single");
    w.new_session("job-a");
    let sid = w.session_id("job-a");
    let windows = w.tmux(&["list-windows", "-t", "host_job-a", "-F", "#{window_id}"]);
    assert_eq!(windows.lines().count(), 1, "one window per session (R9)");

    // A second claude started by hand in the same tmux session does not take the name.
    w.tmux(&[
        "new-window",
        "-t",
        "host_job-a",
        fixture("fake-claude").to_str().unwrap(),
        "-n",
        "other",
        "--session-id",
        "11111111-1111-1111-1111-111111111111",
    ]);
    w.session_id("other");
    assert_eq!(w.session_names(), vec!["host_job-a"]);

    // A session whose claude exits disappears with it.
    w.new_session("job-c");
    let sid_c = w.session_id("job-c");
    w.send(&sid_c, "exit");
    wait_for(|| (!w.session_names().iter().any(|n| n == "host_job-c")).then_some(()));
    let _ = sid;
}

#[test]
fn duplicate_names_get_a_suffix() {
    // `new` cannot create a second job-a (T1.5); a `/rename` into a taken name can.
    let w = World::new("dup");
    w.new_session("job-a");
    w.session_id("job-a");
    w.new_session("job-b");
    let b = w.session_id("job-b");
    w.send(&b, "prompt job-a");
    wait_for(|| {
        let mut names = w.session_names();
        names.sort();
        (names == ["host_job-a", "host_job-a-2"]).then_some(())
    });
}

#[test]
fn new_passes_claude_flags_without_double_dash() {
    let w = World::new("passthru");
    let out = w
        .cc_moss()
        .args(["new", "--model", "opus", "job-a", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    w.session_id("job-a");
    let cmd = w.tmux(&[
        "list-panes",
        "-t",
        "host_job-a",
        "-F",
        "#{pane_start_command}",
    ]);
    assert!(cmd.contains("--model opus"), "{cmd}");
    assert!(cmd.contains("--name=job-a"), "{cmd}");
}
