//! `cc-moss new` end to end on an isolated tmux server with a fake `claude`:
//! sessions start in bypass mode unless asked not to (T1.2).

mod common;

use common::World;

const BYPASS: &str = "--dangerously-skip-permissions";

fn pane_command(w: &World, tmux_name: &str) -> String {
    w.tmux(&["list-panes", "-t", tmux_name, "-F", "#{pane_start_command}"])
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
