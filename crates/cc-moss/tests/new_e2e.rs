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
fn a_chinese_tmux_name_matches_without_a_utf8_locale() {
    let w = World::new("new-locale");
    w.new_session("接口 评审");
    w.session_id("接口 评审");
    // The daemon and the menu bar app run outside tmux, often with no
    // locale at all: tmux must not hand them `_` for every Chinese letter.
    let out = w
        .cc_moss()
        .env_remove("LANG")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .args(["doctor", "--sessions", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let session = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "接口 评审")
        .unwrap_or_else(|| panic!("{v}"));
    assert_eq!(session["tmux_name_matches"], true, "{session}");
}

#[test]
fn ls_labels_sessions_that_have_no_title_yet() {
    let w = World::new("ls-title");
    w.new_session("api");
    common::wait_for(|| {
        w.session_names()
            .contains(&"host_api".to_string())
            .then_some(())
    });
    w.session_id("api");
    // A session from before cc-moss set titles: no name on it.
    w.tmux(&["set-option", "-u", "-t", "=host_api:", "@cc-moss-name"]);
    assert_eq!(display_name(&w, "host_api").0, "");
    common::ls_json(&w.state(), false);
    assert_eq!(display_name(&w, "host_api").0, "api");
}

#[test]
fn bypass_in_pane() {
    let w = World::new("bypass-pane");
    // No `default_args`: bypass comes from cc-moss itself.
    std::fs::write(w.config_path(), "").unwrap();
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

/// Who the session says manages it (`@managed-by`).
fn manager(w: &World, tmux_name: &str) -> String {
    let target = format!("={tmux_name}:");
    w.tmux(&["display-message", "-p", "-t", &target, "#{@managed-by}"])
        .trim()
        .to_string()
}

#[test]
fn new_sessions_tell_other_tools_who_manages_them() {
    let w = World::new("new-managed");
    w.new_session("api");
    common::wait_for(|| {
        w.session_names()
            .contains(&"host_api".to_string())
            .then_some(())
    });
    assert_eq!(manager(&w, "host_api"), "cc-moss");
    // Per session, never server-wide.
    let global = w.tmux(&["show-options", "-g", "-v", "-q", "@managed-by"]);
    assert_eq!(global.trim(), "");
}

#[test]
fn running_sessions_get_the_mark_when_listed() {
    let w = World::new("ls-managed");
    w.new_session("api");
    common::wait_for(|| {
        w.session_names()
            .contains(&"host_api".to_string())
            .then_some(())
    });
    w.session_id("api");
    // A session from before the mark.
    w.tmux(&["set-option", "-u", "-t", "=host_api:", "@managed-by"]);
    assert_eq!(manager(&w, "host_api"), "");
    common::ls_json(&w.state(), false);
    assert_eq!(manager(&w, "host_api"), "cc-moss");
}

#[test]
fn reused_session_ids_after_server_restart_are_not_marked_and_stale_marks_clear() {
    let w = World::new("reused-id");
    // Two sessions on the old server claim $0 and $1, then the server dies
    // without SessionEnd so both rows stay in the registry pointing at $0/$1.
    w.new_session("old-zero");
    w.session_id("old-zero");
    w.new_session("old-one");
    w.session_id("old-one");
    assert_eq!(
        w.tmux(&[
            "display-message",
            "-p",
            "-t",
            "=host_old-zero:",
            "#{session_id}"
        ])
        .trim(),
        "$0"
    );
    assert_eq!(
        w.tmux(&[
            "display-message",
            "-p",
            "-t",
            "=host_old-one:",
            "#{session_id}"
        ])
        .trim(),
        "$1"
    );

    // Simulate a tmux server restart: kill the server, then start an unrelated
    // session that reuses $0. Pass the test env so the restarted server's panes
    // still find the test fake-claude hooks when `fresh` starts below.
    w.tmux(&["kill-server"]);
    let status = std::process::Command::new("tmux")
        .args([
            "-u",
            "-L",
            &w.socket,
            "new-session",
            "-d",
            "-s",
            "other-app",
            "sleep",
            "60",
        ])
        .env("HOME", w.path("home"))
        .env("CC_MOSS_STATE_DIR", w.state())
        .env("CC_MOSS_CONFIG", w.config_path())
        .env("CC_MOSS_TMUX_SOCKET", &w.socket)
        .env("CC_MOSS_HOST_PREFIX", "host")
        .env("CC_MOSS_BIN", env!("CARGO_BIN_EXE_cc-moss"))
        .env("CC_MOSS_FAKE_DIR", w.path("fifos"))
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        w.tmux(&[
            "display-message",
            "-p",
            "-t",
            "=other-app:",
            "#{session_id}"
        ])
        .trim(),
        "$0"
    );
    // Reconcile must not stamp old-zero's title or @managed-by onto other-app ($0).
    common::ls_json(&w.state(), false);
    assert_eq!(manager(&w, "other-app"), "");
    assert_eq!(display_name(&w, "other-app").0, "");

    // Simulate stale marks left behind by an earlier cc-moss on $0, plus a
    // brand-new cc-moss session that reuses $1 (where old-one lived!) and a
    // third-party tool's session ($2) whose @managed-by must stay untouched.
    w.tmux(&["set-option", "-t", "=other-app:", "@managed-by", "cc-moss"]);
    w.tmux(&[
        "set-option",
        "-t",
        "=other-app:",
        "@cc-moss-name",
        "old-zero",
    ]);
    w.new_session("fresh");
    w.session_id("fresh");
    assert_eq!(
        w.tmux(&[
            "display-message",
            "-p",
            "-t",
            "=host_fresh:",
            "#{session_id}"
        ])
        .trim(),
        "$1"
    );
    w.tmux(&["new-session", "-d", "-s", "third-party", "sleep", "60"]);
    w.tmux(&[
        "set-option",
        "-t",
        "=third-party:",
        "@managed-by",
        "other-tool",
    ]);

    common::ls_json(&w.state(), false);
    assert_eq!(manager(&w, "other-app"), "");
    assert_eq!(display_name(&w, "other-app").0, "");
    assert_eq!(manager(&w, "third-party"), "other-tool");
    assert_eq!(manager(&w, "host_fresh"), "cc-moss");
    assert_eq!(display_name(&w, "host_fresh").0, "fresh");
}
