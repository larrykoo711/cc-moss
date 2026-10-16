//! `cc-moss send`: text typed into a session's own input, on an isolated tmux.

mod common;

use common::{wait_for, World};

fn screen(w: &World) -> String {
    w.tmux(&["capture-pane", "-p", "-t", "host_job-a"])
}

#[test]
fn single_line_reaches_the_session() {
    let w = World::new("send-one");
    w.new_session("job-a");
    w.session_id("job-a");
    let out = w
        .cc_moss()
        .args(["send", "job-a", "--", "look at the error codes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    wait_for(|| screen(&w).contains("look at the error codes").then_some(()));
}

#[test]
fn several_lines_arrive_whole() {
    let w = World::new("send-many");
    w.new_session("job-a");
    w.session_id("job-a");
    let out = w
        .cc_moss()
        .args(["send", "job-a", "--", "first line\nsecond line"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    wait_for(|| {
        let s = screen(&w);
        (s.contains("first line") && s.contains("second line")).then_some(())
    });
}

#[test]
fn control_characters_are_refused() {
    let w = World::new("send-ctrl");
    w.new_session("job-a");
    w.session_id("job-a");
    let out = w
        .cc_moss()
        .args(["send", "job-a", "--", "oops\u{3}"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("control"),
        "{out:?}"
    );
}

#[test]
fn a_session_outside_tmux_is_refused() {
    let w = World::new("send-none");
    let out = w
        .cc_moss()
        .args(["send", "nobody", "--", "hi"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
