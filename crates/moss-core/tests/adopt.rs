use std::collections::HashMap;

use moss_core::agent::claude::{is_auto_name, parse_agents};
use moss_core::launch_args::is_headless;
use moss_core::tmux::locate_in_panes;

#[test]
fn parses_interactive_sessions_and_drops_auto_names() {
    let json = r#"[
      {"id":"0b47","pid":10,"cwd":"/w/a","kind":"background","sessionId":"bg","name":"x","state":"blocked"},
      {"pid":11,"cwd":"/w/acme","kind":"interactive","sessionId":"S1","name":"acme-d9","status":"busy"},
      {"pid":12,"cwd":"/w/acme","kind":"interactive","sessionId":"S2","name":"job-api","status":"idle"},
      {"cwd":"/w/acme","kind":"interactive","sessionId":"S3","name":"no-pid","status":"idle"}
    ]"#;
    let live = parse_agents(json);
    assert_eq!(
        live.len(),
        2,
        "background and pid-less entries skipped: {live:?}"
    );
    assert_eq!(live[0].session_id, "S1");
    assert_eq!(live[0].name, None, "auto name dropped");
    assert!(live[0].busy);
    assert_eq!(live[1].name.as_deref(), Some("job-api"));
    assert!(!live[1].busy);
}

#[test]
fn auto_names_are_folder_plus_two_hex_digits() {
    assert!(is_auto_name("acme-app-d9", "/Users/u/Documents/acme-app"));
    assert!(is_auto_name("billing-b6", "/x/billing"));
    assert!(!is_auto_name("job-api", "/x/acme-app"));
    assert!(!is_auto_name("acme-app-dev", "/x/acme-app"));
    assert!(!is_auto_name("acme-app-abc", "/x/acme-app"));
    assert!(!is_auto_name("97-gateway-app", "/x/acme-app"));
    // Claude Code lowercases the folder name.
    assert!(is_auto_name("t-89", "/private/var/folders/xm/T"));
    assert!(is_auto_name("acme-app-d9", "/x/Acme-App"));
}

#[test]
fn headless_runs_are_told_apart() {
    assert!(is_headless(&["claude", "-p", "--model", "haiku"]));
    assert!(is_headless(&["/usr/local/bin/claude", "--print"]));
    assert!(!is_headless(&["claude", "--dangerously-skip-permissions"]));
    assert!(!is_headless(&["claude", "--resume", "abc"]));
    assert!(!is_headless(&["-p"]), "the program name is not a flag");
}

#[test]
fn finds_the_pane_a_process_runs_under() {
    // pane shell 100 -> zsh 200 -> claude 300
    let parents: HashMap<u32, u32> = [(300, 200), (200, 100), (100, 1)].into();
    let panes = vec![
        (50, "$0".to_string(), "other".to_string(), "%0".to_string()),
        (
            100,
            "$3".to_string(),
            "host_job-api".to_string(),
            "%7".to_string(),
        ),
    ];
    let loc = locate_in_panes(300, &parents, &panes, "/tmp/sock").unwrap();
    assert_eq!(loc.session_id.as_deref(), Some("$3"));
    assert_eq!(loc.pane_id, "%7");
    assert_eq!(loc.socket, "/tmp/sock");
    assert!(locate_in_panes(999, &parents, &panes, "/tmp/sock").is_none());
}
