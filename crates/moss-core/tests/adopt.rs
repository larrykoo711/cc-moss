use std::collections::HashMap;

use moss_core::agent::claude::{is_auto_name, parse_agents};
use moss_core::launch_args::is_headless;
use moss_core::tmux::locate_in_panes;

#[test]
fn parses_running_sessions_and_drops_auto_names() {
    let json = r#"[
      {"id":"0b47","cwd":"/w/a","kind":"background","sessionId":"done","name":"x","state":"blocked"},
      {"pid":10,"id":"99a0","cwd":"/w/a","kind":"background","sessionId":"BG","name":"db-tools","status":"busy"},
      {"pid":11,"cwd":"/w/acme","kind":"interactive","sessionId":"S1","name":"acme-d9","status":"busy"},
      {"pid":12,"cwd":"/w/acme","kind":"interactive","sessionId":"S2","name":"job-api","status":"idle"},
      {"cwd":"/w/acme","kind":"interactive","sessionId":"S3","name":"no-pid","status":"idle"},
      {"pid":13,"cwd":"/w/acme","kind":"something-new","sessionId":"S4","status":"idle"}
    ]"#;
    let live = parse_agents(json);
    assert_eq!(
        live.len(),
        3,
        "pid-less entries (a finished background job) and unknown kinds skipped: {live:?}"
    );
    assert_eq!(live[0].session_id, "BG");
    assert!(live[0].background, "run by Claude Code's supervisor");
    assert_eq!(live[0].name.as_deref(), Some("db-tools"));
    assert_eq!(live[1].session_id, "S1");
    assert!(!live[1].background);
    assert_eq!(live[1].name, None, "auto name dropped");
    assert!(live[1].busy);
    assert_eq!(live[2].name.as_deref(), Some("job-api"));
    assert!(!live[2].busy);
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
    // Exact words: a prompt that mentions -p is not the flag.
    assert!(!is_headless(&["claude", "explain what -p does"]));
    assert!(
        !is_headless(&["claude", "--", "-p"]),
        "after -- nothing is a flag"
    );
    assert!(is_headless(&["claude", "-cp"]), "continue and print");
    assert!(
        !is_headless(&["claude", "-pirate"]),
        "not a cluster of switches"
    );
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

#[test]
fn argv_reads_a_processs_words_exactly() {
    // One argument with spaces in it stays one. `; :` keeps the shell from
    // replacing itself with `sleep` (dash does), which would drop the words.
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "sleep 30; :", "summarize -p this"])
        .spawn()
        .unwrap();
    let mut words = None;
    for _ in 0..50 {
        words = moss_core::launch_args::argv(child.id());
        if words
            .as_ref()
            .is_some_and(|w| w.iter().any(|w| w == "summarize -p this"))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    let words = words.expect("its words");
    assert!(words.iter().any(|w| w == "summarize -p this"), "{words:?}");
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    assert!(!is_headless(&refs));
}

#[test]
fn procargs2_is_read_word_by_word() {
    let mut buf = 3i32.to_ne_bytes().to_vec();
    buf.extend_from_slice(b"/usr/local/bin/claude\0\0\0claude\0--resume\0a b\0HOME=/x\0");
    assert_eq!(
        moss_core::launch_args::parse_procargs2(&buf),
        Some(vec!["claude".into(), "--resume".into(), "a b".into()])
    );
    assert_eq!(moss_core::launch_args::parse_procargs2(&[1, 0]), None);
}
