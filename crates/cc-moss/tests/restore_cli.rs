mod common;

use common::{fixture, ls_json, wait_for, World};

/// A world with one interrupted session `job-a` that has a transcript.
fn interrupted_world(test: &str) -> (World, String) {
    let w = World::new(test);
    w.new_session("job-a");
    let id = w.session_id("job-a");
    w.first_prompt(&id, "job-a");
    w.tmux(&["kill-server"]);
    wait_for(|| {
        (ls_json(&w.state(), false)["sessions"][0]["status"] == "interrupted").then_some(())
    });
    (w, id)
}

fn run(w: &World, args: &[&str]) -> std::process::Output {
    w.cc_moss().args(args).output().unwrap()
}

#[test]
fn refuses_without_yes() {
    let (w, _) = interrupted_world("refuse");
    let out = run(&w, &["restore"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(w.session_names().is_empty());
}

#[test]
fn dry_run_changes_nothing() {
    let (w, _) = interrupted_world("dry");
    let out = run(&w, &["restore", "--dry-run"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Dry run"));
    assert!(w.session_names().is_empty());
    // Still a candidate afterwards.
    let again = run(&w, &["restore", "--dry-run"]);
    assert!(String::from_utf8_lossy(&again.stdout).contains("would restore 1"));
}

#[test]
fn second_restore_is_refused_while_pending() {
    let (mut w, _) = interrupted_world("pending");
    // A claude that never reports back keeps the restore in flight.
    w.claude = fixture("silent-claude");
    assert!(run(&w, &["restore", "--yes"]).status.success());
    // The restored process is recorded and alive, so it is simply running.
    let out = run(&w, &["restore", "--yes"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Nothing to restore"), "{text}");
    assert_eq!(w.session_names().len(), 1, "restored exactly once");
    assert_eq!(
        ls_json(&w.state(), false)["sessions"][0]["status"],
        "running"
    );
}

#[test]
fn note_round_trip() {
    let (w, _) = interrupted_world("note");
    assert!(run(&w, &["note", "job-a", "--state", "waiting for key"])
        .status
        .success());
    let ls = String::from_utf8_lossy(&run(&w, &["ls"]).stdout).into_owned();
    assert!(ls.contains("waiting for key"), "{ls}");
    assert_eq!(
        ls_json(&w.state(), false)["sessions"][0]["notes"]["state"],
        "waiting for key"
    );

    assert!(run(&w, &["note", "job-a", "--goal", "ship"])
        .status
        .success());
    let notes = &ls_json(&w.state(), false)["sessions"][0]["notes"];
    assert_eq!(notes["goal"], "ship");
    assert_eq!(
        notes["state"], "waiting for key",
        "unspecified fields are kept"
    );

    assert!(run(&w, &["note", "job-a", "--clear"]).status.success());
    assert!(ls_json(&w.state(), false)["sessions"][0]["notes"]["state"].is_null());
}

#[test]
fn close_removes_candidate() {
    let (w, _) = interrupted_world("close");
    assert!(run(&w, &["close", "job-a"]).status.success());
    let out = run(&w, &["restore", "--dry-run"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Nothing to restore"));
    assert_eq!(
        ls_json(&w.state(), true)["sessions"][0]["end_reason"],
        "closed-by-user"
    );
}

#[test]
fn unknown_name_is_an_error() {
    let (w, _) = interrupted_world("unknown");
    let out = run(&w, &["note", "nope", "x"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no session named"));
}

#[test]
fn a_claude_that_exits_at_once_is_not_counted_as_restored() {
    let (mut w, _) = interrupted_world("diesfast");
    let dying = w.path("dying-claude");
    std::fs::write(&dying, "#!/bin/sh\nexit 1\n").unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&dying)
        .status()
        .unwrap();
    w.claude = dying;
    // Exiting "at once" can take longer than the default grace on a busy machine.
    let out = w
        .cc_moss()
        .env("CC_MOSS_STARTUP_GRACE_MS", "5000")
        .args(["restore", "--yes"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("exited right after it started"), "{err}");
    // Still offered for restore.
    let again = run(&w, &["restore", "--dry-run"]);
    assert!(String::from_utf8_lossy(&again.stdout).contains("would restore 1"));
}

fn restore_json(w: &World, names: &[&str]) -> serde_json::Value {
    let out = w
        .cc_moss()
        .args(["restore", "--yes", "--json"])
        .args(names)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("restore --json is valid JSON")
}

#[test]
fn json_reports_each_result() {
    let (w, _) = interrupted_world("json-each");
    let id = ls_json(&w.state(), false)["sessions"][0]["id"].clone();
    let v = restore_json(&w, &[]);
    assert_eq!(v["schema"], 1);
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{v}");
    assert_eq!(results[0]["id"], id);
    assert_eq!(results[0]["name"], "job-a");
    assert_eq!(results[0]["ok"], true);
    assert!(results[0].get("reason").is_none(), "{v}");
    assert_eq!(w.session_names().len(), 1);

    // Named and now running: reported, not restored twice.
    wait_for(|| (ls_json(&w.state(), false)["sessions"][0]["status"] == "running").then_some(()));
    let again = restore_json(&w, &["job-a"]);
    assert_eq!(again["results"][0]["ok"], false, "{again}");
    assert_eq!(again["results"][0]["reason"], "already_running");
    assert_eq!(w.session_names().len(), 1);
}

#[test]
fn json_empty_session_has_reason_empty() {
    let w = World::new("json-empty");
    w.new_session("job-x");
    w.session_id("job-x");
    w.tmux(&["kill-server"]);
    wait_for(|| {
        (ls_json(&w.state(), false)["sessions"][0]["status"] == "interrupted").then_some(())
    });
    let v = restore_json(&w, &[]);
    let r = &v["results"][0];
    assert_eq!(r["name"], "job-x");
    assert_eq!(r["ok"], false);
    assert_eq!(r["reason"], "empty", "{v}");
    assert!(r["message"].as_str().is_some_and(|m| !m.is_empty()));
    assert!(w.session_names().is_empty());
}

#[test]
fn json_exit_at_once_and_missing_dir_have_codes() {
    let (mut w, _) = interrupted_world("json-dies");
    let dying = w.path("dying-claude");
    std::fs::write(&dying, "#!/bin/sh\nexit 1\n").unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&dying)
        .status()
        .unwrap();
    w.claude = dying;
    let out = w
        .cc_moss()
        .env("CC_MOSS_STARTUP_GRACE_MS", "5000")
        .args(["restore", "--yes", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["results"][0]["reason"], "exited_at_once", "{v}");

    std::fs::remove_dir_all(w.path("work")).unwrap();
    let v = restore_json(&w, &[]);
    assert_eq!(v["results"][0]["reason"], "missing_dir", "{v}");
}

#[test]
fn json_restores_all_after_a_crash() {
    let w = World::new("json-crash");
    for name in ["job-a", "job-b"] {
        w.new_session(name);
        let id = w.session_id(name);
        w.first_prompt(&id, name);
    }
    w.tmux(&["kill-server"]);
    let all = |status: &str| {
        ls_json(&w.state(), false)["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == status)
    };
    wait_for(|| all("interrupted").then_some(()));
    // The very command the app runs for "Bring All Back": every recently
    // interrupted session by name, after `--`.
    let v = restore_json(&w, &["--", "job-a", "job-b"]);
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "{v}");
    assert!(results.iter().all(|r| r["ok"] == true), "{v}");
    wait_for(|| all("running").then_some(()));
    assert_eq!(w.session_names().len(), 2);
}

#[test]
fn json_leaves_text_output_unchanged() {
    let (w, _) = interrupted_world("json-text");
    let out = run(&w, &["restore", "--yes"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Started job-a"), "{text}");
    assert!(!text.trim_start().starts_with('{'), "{text}");
}

#[test]
fn json_needs_yes() {
    let (w, _) = interrupted_world("json-yes");
    let out = run(&w, &["restore", "--json"]);
    assert!(!out.status.success());
    assert!(w.session_names().is_empty());
}
