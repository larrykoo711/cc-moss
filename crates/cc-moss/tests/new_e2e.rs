//! `cc-moss new` end to end on an isolated tmux server with a fake `claude`:
//! sessions start in bypass mode unless asked not to (T1.2).

mod common;

use common::World;

const BYPASS: &str = "--dangerously-skip-permissions";

fn pane_command(w: &World, tmux_name: &str) -> String {
    w.tmux(&["list-panes", "-t", tmux_name, "-F", "#{pane_start_command}"])
}

/// The session's own name as cc-moss gave it to tmux, and its title.
fn display_name(w: &World, tmux_name: &str) -> (String, String) {
    let target = format!("={tmux_name}:");
    let name = w.tmux(&["display-message", "-p", "-t", &target, "#{@cc-moss-name}"]);
    let title = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        &target,
        "#{?#{@cc-moss-name},#{@cc-moss-name},#{session_name}}",
    ]);
    (name.trim().to_string(), title.trim().to_string())
}

#[test]
fn new_sessions_carry_their_own_name_for_the_tab_title() {
    let w = World::new("new-title");
    w.new_session("接口 评审");
    let tmux_name = "host_接口-评审";
    common::wait_for(|| {
        w.session_names()
            .contains(&tmux_name.to_string())
            .then_some(())
    });
    assert_eq!(
        display_name(&w, tmux_name),
        ("接口 评审".to_string(), "接口 评审".to_string())
    );
    // Renamed with /rename: the tmux name and the title follow.
    let sid = w.session_id("接口 评审");
    w.send(&sid, "prompt login page");
    common::wait_for(|| {
        w.session_names()
            .contains(&"host_login-page".to_string())
            .then_some(())
    });
    assert_eq!(display_name(&w, "host_login-page").0, "login page");
}

#[test]
fn bypass_in_pane() {
    let w = World::new("bypass-pane");
    // No `default_args`: bypass comes from cc-moss itself.
    std::fs::write(w.path("config.toml"), "").unwrap();
    std::fs::create_dir_all(w.path("home")).unwrap();
    for (name, extra) in [("job-a", None), ("job-b", Some("--safe"))] {
        let out = w
            .cc_moss()
            .env("HOME", w.path("home"))
            .args(["new", name, "--detach", "-C"])
            .arg(w.path("work"))
            .args(extra)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        // The fake claude started and checked in through the hooks.
        w.session_id(name);
    }
    let bypassed = pane_command(&w, "host_job-a");
    assert_eq!(bypassed.matches(BYPASS).count(), 1, "{bypassed}");
    let safe = pane_command(&w, "host_job-b");
    assert!(!safe.contains(BYPASS), "{safe}");
}
