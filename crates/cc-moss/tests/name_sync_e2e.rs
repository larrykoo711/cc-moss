//! End to end on a real, isolated tmux server with a fake `claude`.

mod common;

use common::{fixture, ls_json, wait_for, World};

#[test]
fn new_creates_named_session() {
    let w = World::new("new");
    w.new_session("em-a");
    w.session_id("em-a");
    assert_eq!(w.session_names(), vec!["host_em-a"]);
    let cmd = w.tmux(&[
        "list-panes",
        "-t",
        "host_em-a",
        "-F",
        "#{pane_start_command}",
    ]);
    assert!(cmd.contains("--name=em-a --session-id"), "{cmd}");
    assert!(cmd.contains("--dangerously-skip-permissions"), "{cmd}");
    let slot = &ls_json(&w.state(), false)["slots"][0];
    assert_eq!(slot["tmux"]["session_name"], "host_em-a");
}

#[test]
fn prompt_title_renames_tmux_session() {
    let w = World::new("rename");
    w.new_session("em-a");
    let sid = w.session_id("em-a");

    // `/rename em-b` fires nothing; the next prompt carries the new title.
    w.send(&sid, "prompt em-b");
    wait_for(|| (w.session_names() == ["host_em-b"]).then_some(()));

    // A manual tmux rename is reverted on the next prompt.
    w.tmux(&["rename-session", "-t", "host_em-b", "manual"]);
    assert_eq!(w.session_names(), vec!["manual"]);
    w.send(&sid, "prompt em-b");
    wait_for(|| (w.session_names() == ["host_em-b"]).then_some(()));
}

#[test]
fn single_window_and_primary_slot_owns_name() {
    let w = World::new("single");
    w.new_session("em-a");
    let sid = w.session_id("em-a");
    let windows = w.tmux(&["list-windows", "-t", "host_em-a", "-F", "#{window_id}"]);
    assert_eq!(windows.lines().count(), 1, "one window per session (R9)");

    // A second claude started by hand in the same tmux session does not take the name.
    w.tmux(&[
        "new-window",
        "-t",
        "host_em-a",
        fixture("fake-claude").to_str().unwrap(),
        "-n",
        "other",
        "--session-id",
        "11111111-1111-1111-1111-111111111111",
    ]);
    w.session_id("other");
    assert_eq!(w.session_names(), vec!["host_em-a"]);

    // A session whose claude exits disappears with it.
    w.new_session("em-c");
    let sid_c = w.session_id("em-c");
    w.send(&sid_c, "exit");
    wait_for(|| (!w.session_names().iter().any(|n| n == "host_em-c")).then_some(()));
    let _ = sid;
}

#[test]
fn duplicate_names_get_a_suffix() {
    // `new` cannot create a second em-a (T1.5); a `/rename` into a taken name can.
    let w = World::new("dup");
    w.new_session("em-a");
    w.session_id("em-a");
    w.new_session("em-b");
    let b = w.session_id("em-b");
    w.send(&b, "prompt em-a");
    wait_for(|| {
        let mut names = w.session_names();
        names.sort();
        (names == ["host_em-a", "host_em-a-2"]).then_some(())
    });
}
