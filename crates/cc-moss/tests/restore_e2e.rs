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
fn config_source_unknown_refuses_without_launch() {
    let w = World::new("restore-source-unknown");
    w.new_session("job-b");
    let sid = w.session_id("job-b");
    w.first_prompt(&sid, "job-b");
    crash(&w);
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute("UPDATE slots SET provider_started=NULL", [])
        .unwrap();
    let out = w
        .cc_moss()
        .args(["restore", "--yes", "--json", "job-b"])
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["ok"], false, "{result}");
    assert_eq!(result["results"][0]["reason"], "config_source_unknown");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("config source is unknown"),
        "{out:?}"
    );
    assert!(w.session_names().is_empty());
    let claim: Option<i64> = db
        .query_row("SELECT restore_started_at FROM slots", [], |r| r.get(0))
        .unwrap();
    assert!(claim.is_none());
}

fn legacy_config_source(w: &World, markers: &[&str]) -> String {
    w.new_session("job-b");
    let sid = w.session_id("job-b");
    w.first_prompt(&sid, "job-b");
    crash(w);
    let settings = w.dir.path().join("legacy/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "{}").unwrap();
    let snapshot = serde_json::json!({
        "keys": [], "host": null, "env_model": null, "model": null,
        "source": settings, "unreplayable_args": markers,
    })
    .to_string();
    rusqlite::Connection::open(w.state().join("registry.db"))
        .unwrap()
        .execute("UPDATE slots SET provider_started=?1", [&snapshot])
        .unwrap();
    snapshot
}

fn assert_config_source_refusal(w: &World, options: &[&str], expected: &str, original: &str) {
    let out = w
        .cc_moss()
        .args(["restore", "--yes", "--json"])
        .args(options)
        .args(["--", "job-b"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["reason"], expected, "{result}");
    assert!(w.session_names().is_empty());
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    let (snapshot, claim): (String, Option<i64>) = db
        .query_row(
            "SELECT provider_started, restore_started_at FROM slots",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(snapshot, original);
    assert!(claim.is_none());
}

#[test]
fn config_source_existing_settings_hint_does_not_authorize_restore() {
    let w = World::new("source-hint");
    let original = legacy_config_source(&w, &[]);
    assert_config_source_refusal(&w, &[], "config_source_unknown", &original);
}

#[test]
fn config_source_default_explicitly_unsets_inherited_source() {
    let w = World::new("source-default");
    legacy_config_source(&w, &[]);
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", w.dir.path().join("wrong"))
        .args([
            "restore",
            "--yes",
            "--json",
            "--config-source",
            "default",
            "--",
            "job-b",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["ok"], true, "{result}");
    let cmd = w.tmux(&[
        "list-panes",
        "-t",
        "host_job-b",
        "-F",
        "#{pane_start_command}",
    ]);
    assert!(cmd.contains("env -u CLAUDE_CONFIG_DIR"), "{cmd}");
}

#[test]
fn config_source_directory_is_one_explicit_argument() {
    let w = World::new("source-directory");
    legacy_config_source(&w, &[]);
    let directory = w.dir.path().join("config with spaces");
    std::fs::create_dir(&directory).unwrap();
    let out = w
        .cc_moss()
        .args(["restore", "--yes", "--json", "--config-dir"])
        .arg(&directory)
        .args(["--", "job-b"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["ok"], true, "{result}");
    let cmd = w.tmux(&[
        "list-panes",
        "-t",
        "host_job-b",
        "-F",
        "#{pane_start_command}",
    ]);
    assert!(
        cmd.contains(&format!("CLAUDE_CONFIG_DIR={}", directory.display())),
        "{cmd}"
    );
}

#[test]
fn restore_source_plan_known_missing_directory_is_not_ready() {
    let w = World::new("plan-missing");
    legacy_config_source(&w, &[]);
    let missing = w.dir.path().join("deleted-source");
    let snapshot = moss_core::provider::Snapshot {
        launch_source: moss_core::provider::LaunchSource::Explicit(
            missing.to_string_lossy().into_owned(),
        ),
        ..Default::default()
    }
    .to_json();
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute("UPDATE slots SET provider_started=?1", [&snapshot])
        .unwrap();
    let out = w.cc_moss().args(["ls", "--json"]).output().unwrap();
    let list: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let plan = &list["sessions"][0]["restore_source"];
    assert_eq!(plan["status"], "needs_location", "{list}");
    assert_eq!(plan["issue"], "config_directory_missing", "{list}");
    assert_config_source_refusal(&w, &[], "config_directory_missing", &snapshot);
}

fn source_plan(w: &World) -> serde_json::Value {
    let out = w.cc_moss().args(["ls", "--json"]).output().unwrap();
    let list: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    list["sessions"][0]["restore_source"].clone()
}

fn set_recorded_source(w: &World, source: moss_core::provider::LaunchSource) -> String {
    let snapshot = moss_core::provider::Snapshot {
        launch_source: source,
        ..Default::default()
    }
    .to_json();
    rusqlite::Connection::open(w.state().join("registry.db"))
        .unwrap()
        .execute("UPDATE slots SET provider_started=?1", [&snapshot])
        .unwrap();
    snapshot
}

#[test]
fn restore_source_plan_unknown_hint_is_not_a_resolved_path() {
    let w = World::new("plan-unknown");
    legacy_config_source(&w, &[]);
    let plan = source_plan(&w);
    assert_eq!(plan["kind"], "unknown");
    assert_eq!(plan["issue"], "config_source_unknown");
    assert!(plan["display_path"].is_null());
    assert_eq!(
        plan["default_path"],
        w.path("home")
            .canonicalize()
            .unwrap()
            .join(".claude")
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn restore_source_plan_ready_recorded_directory_rechecks_after_deletion() {
    let w = World::new("plan-delete");
    legacy_config_source(&w, &[]);
    let dir = w.path("original-config");
    std::fs::create_dir(&dir).unwrap();
    let original = set_recorded_source(
        &w,
        moss_core::provider::LaunchSource::Explicit(dir.to_string_lossy().into_owned()),
    );
    let plan = source_plan(&w);
    assert_eq!(plan["status"], "ready");
    std::fs::remove_dir(&dir).unwrap();
    assert_config_source_refusal(&w, &[], "config_directory_missing", &original);
}

#[test]
fn restore_source_plan_revision_guards_original_source_before_replacement() {
    let w = World::new("plan-revision");
    legacy_config_source(&w, &[]);
    let dir = w.path("original-config");
    std::fs::create_dir(&dir).unwrap();
    let settings = dir.join("settings.json");
    std::fs::write(&settings, "[]").unwrap();
    let original = set_recorded_source(
        &w,
        moss_core::provider::LaunchSource::Explicit(dir.to_string_lossy().into_owned()),
    );
    let plan = source_plan(&w);
    assert_eq!(plan["issue"], "config_settings_invalid");
    let revision = plan["revision"].as_str().unwrap();
    // 同时带两种前提时，两者都必须满足。
    assert_config_source_refusal(
        &w,
        &[
            "--config-source",
            "default",
            "--expected-source-revision",
            revision,
            "--require-unknown-config",
        ],
        "config_source_changed",
        &original,
    );
    std::fs::write(&settings, "{\"restored\":true}").unwrap();
    assert_config_source_refusal(
        &w,
        &[
            "--config-source",
            "default",
            "--expected-source-revision",
            revision,
        ],
        "config_source_changed",
        &original,
    );
    let current = source_plan(&w);
    let out = w
        .cc_moss()
        .args([
            "restore",
            "--yes",
            "--json",
            "--config-source",
            "default",
            "--expected-source-revision",
            current["revision"].as_str().unwrap(),
            "--",
            "job-b",
        ])
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["ok"], true, "{result}");
}

#[test]
fn restore_source_plan_recorded_ready_preserves_env_and_pins_target_home() {
    for explicit in [false, true] {
        let w = World::new(if explicit {
            "plan-explicit"
        } else {
            "plan-default"
        });
        legacy_config_source(&w, &[]);
        let dir = w.path("home/.claude");
        std::fs::create_dir(&dir).unwrap();
        set_recorded_source(
            &w,
            if explicit {
                moss_core::provider::LaunchSource::Explicit(dir.to_string_lossy().into_owned())
            } else {
                moss_core::provider::LaunchSource::Unset
            },
        );
        let plan = source_plan(&w);
        assert_eq!(plan["status"], "ready");
        let expected_dir = if explicit {
            dir.clone()
        } else {
            dir.canonicalize().unwrap()
        };
        assert_eq!(
            plan["display_path"],
            expected_dir.to_string_lossy().as_ref()
        );
        w.new_session("polluted-home");
        w.session_id("polluted-home");
        w.tmux(&[
            "set-environment",
            "-g",
            "HOME",
            "/nonexistent/polluted-tmux-home",
        ]);
        let out = w
            .cc_moss()
            .args(["restore", "--yes", "--json", "job-b"])
            .output()
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(result["results"][0]["ok"], true, "{result}");
        let cmd = w.tmux(&[
            "list-panes",
            "-t",
            "host_job-b",
            "-F",
            "#{pane_start_command}",
        ]);
        assert!(
            cmd.contains(&format!(
                "HOME={}",
                w.path("home").canonicalize().unwrap().display()
            )),
            "{cmd}"
        );
        if explicit {
            assert!(cmd.contains("CLAUDE_CONFIG_DIR="), "{cmd}");
        } else {
            assert!(cmd.contains("-u CLAUDE_CONFIG_DIR"), "{cmd}");
        }
        let expected_settings = expected_dir
            .join("settings.json")
            .to_string_lossy()
            .into_owned();
        wait_for(|| {
            let db = rusqlite::Connection::open(w.state().join("registry.db")).ok()?;
            let raw: String = db
                .query_row(
                    "SELECT provider_started FROM slots WHERE name='job-b'",
                    [],
                    |r| r.get(0),
                )
                .ok()?;
            let snapshot = moss_core::provider::Snapshot::from_json(&raw)?;
            (snapshot.source.as_deref() == Some(expected_settings.as_str())).then_some(())
        });
    }
}

#[test]
fn restore_source_plan_budget_oversize_blocks_listing_and_restore_without_claim() {
    let w = World::new("plan-budget");
    legacy_config_source(&w, &[]);
    let dir = w.path("large-config");
    std::fs::create_dir(&dir).unwrap();
    let snapshot = set_recorded_source(
        &w,
        moss_core::provider::LaunchSource::Explicit(dir.to_string_lossy().into_owned()),
    );
    std::fs::write(
        dir.join("settings.json"),
        format!("{{\"large\":\"{}\"}}", "x".repeat(1024 * 1024)),
    )
    .unwrap();
    let plan = source_plan(&w);
    assert_eq!(plan["issue"], "config_source_unavailable");
    assert_eq!(plan["status"], "blocked");
    assert!(plan["default_path"].is_null());
    assert_eq!(plan["revision"], "");
    assert_config_source_refusal(&w, &[], "config_source_unavailable", &snapshot);
    assert_config_source_refusal(
        &w,
        &[
            "--config-source",
            "default",
            "--expected-source-revision",
            "old-plan",
        ],
        "config_source_unavailable",
        &snapshot,
    );
}

#[test]
fn restore_source_plan_output_never_includes_settings_values() {
    let w = World::new("plan-secrets");
    legacy_config_source(&w, &[]);
    let dir = w.path("source-with-settings");
    std::fs::create_dir(&dir).unwrap();
    let snapshot = set_recorded_source(
        &w,
        moss_core::provider::LaunchSource::Explicit(dir.to_string_lossy().into_owned()),
    );
    let secret = "test-secret-never-emit-this-value";
    std::fs::write(
        dir.join("settings.json"),
        format!("{{\"arbitrary\":\"{secret}\"}}"),
    )
    .unwrap();
    let plan = source_plan(&w);
    assert_eq!(plan["status"], "ready");
    assert!(!plan.to_string().contains(secret));
    std::fs::write(
        dir.join("settings.json"),
        format!("{{\"arbitrary\":\"{secret}\""),
    )
    .unwrap();
    let plan = source_plan(&w);
    assert_eq!(plan["issue"], "config_settings_invalid");
    assert!(!plan.to_string().contains(secret));
    assert_config_source_refusal(&w, &[], "config_settings_invalid", &snapshot);
}

#[test]
fn config_source_invalid_directory_does_not_claim_or_change_snapshot() {
    let w = World::new("source-invalid");
    let original = legacy_config_source(&w, &[]);
    for path in ["relative", "/tmp/control\npath"] {
        assert_config_source_refusal(
            &w,
            &["--config-dir", path],
            "invalid_config_source",
            &original,
        );
    }
    let file = w.dir.path().join("not-a-directory");
    std::fs::write(&file, "{}").unwrap();
    assert_config_source_refusal(
        &w,
        &["--config-dir", file.to_str().unwrap()],
        "config_not_directory",
        &original,
    );
}

#[test]
fn config_source_choice_never_bypasses_unsupported_arguments() {
    let w = World::new("source-unsupported");
    let original = legacy_config_source(&w, &["--mcp-config"]);
    assert_config_source_refusal(
        &w,
        &["--config-source", "default"],
        "config_source_unsupported",
        &original,
    );
    assert_config_source_refusal(
        &w,
        &["--config-dir", w.dir.path().to_str().unwrap()],
        "config_source_unsupported",
        &original,
    );
}

#[test]
fn config_source_ambiguous_target_never_launches() {
    let w = World::new("source-ambiguous");
    w.new_session("job-a");
    let sid = w.session_id("job-a");
    w.first_prompt(&sid, "job-a");
    legacy_config_source(&w, &[]);
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute("UPDATE slots SET name='same-name'", []).unwrap();
    let out = w
        .cc_moss()
        .args([
            "restore",
            "--yes",
            "--json",
            "--config-source",
            "default",
            "--",
            "same-name",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["reason"], "ambiguous", "{result}");
    assert!(w.session_names().is_empty());
    let claims: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM slots WHERE restore_started_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(claims, 0);
}

#[test]
fn config_source_other_preflight_is_not_tmux_failed() {
    let w = World::new("source-preflight");
    let original = legacy_config_source(&w, &[]);
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute(
        "UPDATE slots SET launch_args=?1",
        [r#"["--effort","bad\nvalue"]"#],
    )
    .unwrap();
    assert_config_source_refusal(
        &w,
        &["--config-source", "default"],
        "resume_preflight_failed",
        &original,
    );
}

#[test]
fn config_source_real_tmux_launch_failure_keeps_its_reason() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new("source-tmux-failed");
    let original = legacy_config_source(&w, &[]);
    let shim = w.dir.path().join("shim");
    std::fs::create_dir(&shim).unwrap();
    let real = std::process::Command::new("which")
        .arg("tmux")
        .output()
        .unwrap();
    let real = String::from_utf8(real.stdout).unwrap();
    let script = shim.join("tmux");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" new-session \"*) exit 1;; esac\nexec '{}' \"$@\"\n",
            real.trim()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = w
        .cc_moss()
        .env(
            "PATH",
            format!("{}:{}", shim.display(), std::env::var("PATH").unwrap()),
        )
        .args([
            "restore",
            "--yes",
            "--json",
            "--config-source",
            "default",
            "--",
            "job-b",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["reason"], "tmux_failed", "{result}");
    assert!(w.session_names().is_empty());
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    let snapshot: String = db
        .query_row("SELECT provider_started FROM slots", [], |r| r.get(0))
        .unwrap();
    assert_eq!(snapshot, original);
}

#[test]
fn config_source_guard_refuses_known_but_explicit_cli_choice_still_works() {
    let w = World::new("source-changed");
    legacy_config_source(&w, &[]);
    let known = moss_core::provider::Snapshot {
        launch_source: moss_core::provider::LaunchSource::Unset,
        ..Default::default()
    }
    .to_json();
    let db = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    db.execute("UPDATE slots SET provider_started=?1", [&known])
        .unwrap();
    assert_config_source_refusal(
        &w,
        &["--config-source", "default", "--require-unknown-config"],
        "config_source_changed",
        &known,
    );
    let out = w
        .cc_moss()
        .args([
            "restore",
            "--yes",
            "--json",
            "--config-source",
            "default",
            "--",
            "job-b",
        ])
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["ok"], true, "{result}");
}

#[test]
fn config_source_guard_allows_unknown_choice() {
    let w = World::new("source-guard-unknown");
    legacy_config_source(&w, &[]);
    let out = w
        .cc_moss()
        .args([
            "restore",
            "--yes",
            "--json",
            "--config-source",
            "default",
            "--require-unknown-config",
            "--",
            "job-b",
        ])
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["results"][0]["ok"], true, "{result}");
}

#[test]
fn config_source_rejects_batch_and_conflicting_options() {
    for args in [
        vec!["--config-source", "default"],
        vec!["--require-unknown-config", "job-a"],
        vec!["--config-source", "default", "job-a", "job-b"],
        vec![
            "--config-source",
            "default",
            "--config-dir",
            "/tmp",
            "job-a",
        ],
        vec!["--config-source", "default", "--provider-default", "job-a"],
        vec!["--config-dir", "/tmp", "--provider-default", "job-a"],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let out = common::bin()
            .env("CC_MOSS_STATE_DIR", dir.path())
            .args(["restore", "--yes", "--json"])
            .args(&args)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?}: {out:?}");
        assert!(!dir.path().join("registry.db").exists());
    }
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
    assert!(
        !cmd.contains("--model"),
        "ordinary resume must not force an old launch selection: {cmd}"
    );
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
