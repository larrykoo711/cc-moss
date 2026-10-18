//! Editing Claude Code's `settings.json`: only cc-moss entries ever change.

use moss_core::settings_json::{install, is_ours, to_claude_string, uninstall};
use serde_json::{json, Value};

const EXE: &str = "/usr/local/bin/cc-moss";
const EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "SessionEnd",
    "Notification",
    "PostToolUse",
    "PreToolUse",
    "PermissionRequest",
];
const NAMES: [&str; 8] = [
    "session-start",
    "user-prompt-submit",
    "stop",
    "session-end",
    "notification",
    "post-tool-use",
    "pre-tool-use",
    "permission-request",
];

fn ours(exe: &str, name: &str) -> Value {
    json!({ "hooks": [{ "type": "command", "command": guarded(exe, name), "timeout": 10 }] })
}

/// The command shape cc-moss writes: silent when the binary is gone.
fn guarded(exe: &str, name: &str) -> String {
    format!("[ -x {exe} ] && {exe} hook {name} || true")
}

/// The command shape written before the guard existed.
fn legacy(exe: &str, name: &str) -> Value {
    json!({ "hooks": [{ "type": "command", "command": format!("{exe} hook {name}"), "timeout": 10 }] })
}

/// PostToolUse takes a matcher; cc-moss wants every tool.
fn ours_for_all_tools(exe: &str, name: &str) -> Value {
    json!({ "matcher": "*", "hooks": [{ "type": "command", "command": guarded(exe, name), "timeout": 10 }] })
}

/// PreToolUse only runs before AskUserQuestion, and only takes note.
fn ours_before_questions(exe: &str) -> Value {
    json!({ "matcher": "AskUserQuestion", "hooks": [{ "type": "command",
        "command": guarded(exe, "pre-tool-use"), "timeout": 10 }] })
}

/// PermissionRequest may wait for an answer from outside: 600 s.
fn ours_waiting(exe: &str) -> Value {
    json!({ "matcher": "*", "hooks": [{ "type": "command",
        "command": guarded(exe, "permission-request"), "timeout": 600 }] })
}

fn expected_hooks(exe: &str) -> Value {
    json!({
        "SessionStart": [ours(exe, "session-start")],
        "UserPromptSubmit": [ours(exe, "user-prompt-submit")],
        "Stop": [ours(exe, "stop")],
        "SessionEnd": [ours(exe, "session-end")],
        "Notification": [ours(exe, "notification")],
        "PostToolUse": [ours_for_all_tools(exe, "post-tool-use")],
        "PreToolUse": [ours_before_questions(exe)],
        "PermissionRequest": [ours_waiting(exe)],
    })
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

fn foreign() -> Value {
    json!({ "hooks": [{ "type": "command", "command": "~/bin/notify.sh" }] })
}

#[test]
fn install_preserves_everything_else() {
    // A missing file is treated as `{}`.
    let got = install(&json!({}), EXE).unwrap();
    assert_eq!(got, json!({ "hooks": expected_hooks(EXE) }));
    assert_eq!(keys(&got["hooks"]), EVENTS);

    let before = json!({
        "permissions": { "allow": ["Bash(ls)"] },
        "model": "opus",
        "env": { "A": "1" },
    });
    let got = install(&before, EXE).unwrap();
    assert_eq!(keys(&got), ["permissions", "model", "env", "hooks"]);
    for k in ["permissions", "model", "env"] {
        assert_eq!(got[k], before[k], "{k} untouched");
    }
    assert_eq!(got["hooks"], expected_hooks(EXE));
}

#[test]
fn keeps_foreign_hooks_on_same_event() {
    let before = json!({ "hooks": { "Stop": [foreign()], "PostToolUse": [foreign()] } });
    let got = install(&before, EXE).unwrap();
    assert_eq!(got["hooks"]["Stop"], json!([foreign(), ours(EXE, "stop")]));
    assert_eq!(
        got["hooks"]["PostToolUse"],
        json!([foreign(), ours_for_all_tools(EXE, "post-tool-use")])
    );
    assert_eq!(
        keys(&got["hooks"]),
        [
            "Stop",
            "PostToolUse",
            "SessionStart",
            "UserPromptSubmit",
            "SessionEnd",
            "Notification",
            "PreToolUse",
            "PermissionRequest",
        ]
    );
}

#[test]
fn install_is_idempotent_and_upgrades_path() {
    let once = install(&json!({ "hooks": { "Stop": [foreign()] } }), EXE).unwrap();
    assert_eq!(install(&once, EXE).unwrap(), once);

    let new_exe = "/Users/me/My Tools/cc-moss";
    let upgraded = install(&once, new_exe).unwrap();
    for (event, name) in EVENTS.iter().zip(NAMES) {
        let groups = upgraded["hooks"][event].as_array().unwrap();
        let ours_here: Vec<&Value> = groups
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .filter(|h| is_ours(h))
            .collect();
        assert_eq!(ours_here.len(), 1, "{event}: replaced, not stacked");
        assert_eq!(
            ours_here[0]["command"],
            guarded(&format!("'{new_exe}'"), name),
            "path with a space is quoted"
        );
    }
    // Upgrading keeps our entry where it was, so the user's order is stable.
    assert_eq!(upgraded["hooks"]["Stop"][0], foreign());
}

#[test]
fn uninstall_only_removes_ours() {
    let before = json!({
        "model": "opus",
        "hooks": { "Stop": [foreign()], "PostToolUse": [foreign()] },
    });
    let installed = install(&before, EXE).unwrap();
    assert_eq!(uninstall(&installed).unwrap(), before);

    // Empty event arrays and the `hooks` key itself go away again.
    let bare = json!({ "model": "opus" });
    let installed = install(&bare, EXE).unwrap();
    assert_eq!(uninstall(&installed).unwrap(), bare);

    // A group that mixes a user command with ours keeps the user's command.
    let mixed = json!({ "hooks": { "Stop": [{ "hooks": [
        { "type": "command", "command": "~/bin/notify.sh" },
        { "type": "command", "command": guarded(EXE, "stop") },
    ] }] } });
    assert_eq!(
        uninstall(&mixed).unwrap(),
        json!({ "hooks": { "Stop": [foreign()] } })
    );
}

#[test]
fn is_ours_matches_quoted_and_bare_paths() {
    let cmd = |c: &str| json!({ "type": "command", "command": c });
    for yes in [
        "cc-moss hook stop",
        "/usr/local/bin/cc-moss hook session-start",
        "\"/Users/me/My Tools/cc-moss\" hook user-prompt-submit",
        "'/opt/cc moss/cc-moss' hook session-end",
        "[ -x /usr/local/bin/cc-moss ] && /usr/local/bin/cc-moss hook stop || true",
        "[ -x '/opt/cc moss/cc-moss' ] && '/opt/cc moss/cc-moss' hook session-end || true",
        "[ -x '/tmp/a'\\''b/cc-moss' ] && '/tmp/a'\\''b/cc-moss' hook stop || true",
    ] {
        assert!(is_ours(&cmd(yes)), "{yes}");
    }
    for no in [
        "/usr/local/bin/not-cc-moss hook stop",
        "cc-moss hook stop && rm -rf /",
        "cc-moss hook pre-compact",
        "~/bin/notify.sh",
        "cc-moss ls",
        "echo hi; /usr/local/bin/cc-moss hook stop",
        "[ -x /a/cc-moss ] && /b/cc-moss hook stop || true",
        "[ -x /a/cc-moss ] && /a/cc-moss hook stop || rm -rf /",
        "[ -x /a/cc-moss ] && /a/cc-moss hook stop",
    ] {
        assert!(!is_ours(&cmd(no)), "{no}");
    }
    assert!(!is_ours(&json!({ "type": "command" })));
}

#[test]
fn serialize_matches_claude_format() {
    // Byte layout of `JSON.stringify(x, null, 2) + "\n"`, key order as written.
    let text = "{\n  \"zeta\": \"中文…\",\n  \"alpha\": [\n    1,\n    true,\n    null\n  ],\n  \"empty\": {},\n  \"none\": []\n}\n";
    let doc: Value = serde_json::from_str(text).unwrap();
    assert_eq!(to_claude_string(&doc), text);
}

#[test]
fn hook_commands_quote_any_unsafe_path() {
    use moss_core::agent::HookKind;
    use moss_core::settings_json::hook_command;
    assert_eq!(
        hook_command("/usr/local/bin/cc-moss", HookKind::Stop),
        "[ -x /usr/local/bin/cc-moss ] && /usr/local/bin/cc-moss hook stop || true"
    );
    assert_eq!(
        hook_command("/tmp/a'b$(touch x);/cc-moss", HookKind::Stop),
        "[ -x '/tmp/a'\\''b$(touch x);/cc-moss' ] && '/tmp/a'\\''b$(touch x);/cc-moss' hook stop || true"
    );
}

/// Runs a hook command the way Claude Code does, through `sh -c`.
fn sh(command: &str) -> std::process::Output {
    std::process::Command::new("sh")
        .args(["-c", command])
        .output()
        .unwrap()
}

#[test]
fn hook_command_runs_the_binary_at_an_awkward_path() {
    use moss_core::agent::HookKind;
    use moss_core::settings_json::hook_command;
    let dir = tempfile::tempdir().unwrap();
    let odd = dir.path().join("a'b $(echo pwned)");
    std::fs::create_dir(&odd).unwrap();
    let exe = odd.join("cc-moss");
    std::fs::write(&exe, "#!/bin/sh\nprintf '%s ' \"$@\"\n").unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&exe)
        .status()
        .unwrap();
    let out = sh(&hook_command(exe.to_str().unwrap(), HookKind::Stop));
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hook stop ");
}

#[test]
fn hook_command_is_silent_when_the_binary_is_gone() {
    use moss_core::agent::HookKind;
    use moss_core::settings_json::hook_command;
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("Cellar/cc-moss/0.1.0/bin/cc-moss");
    for kind in HookKind::ALL {
        let out = sh(&hook_command(gone.to_str().unwrap(), kind));
        assert_eq!(out.status.code(), Some(0), "{kind:?}");
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{kind:?}");
    }
}

#[test]
fn setup_upgrades_unguarded_hooks_in_place() {
    let user = foreign();
    let old = json!({ "hooks": {
        "SessionStart": [legacy(EXE, "session-start")],
        "UserPromptSubmit": [legacy(EXE, "user-prompt-submit")],
        "Stop": [user.clone(), legacy(EXE, "stop")],
        "SessionEnd": [legacy(EXE, "session-end")],
        "Notification": [legacy(EXE, "notification")],
        "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{EXE} hook post-tool-use"), "timeout": 10 }] }],
    } });
    let upgraded = install(&old, EXE).unwrap();
    let mut want = json!({ "hooks": expected_hooks(EXE) });
    want["hooks"]["Stop"] = json!([user, ours(EXE, "stop")]);
    assert_eq!(upgraded, want, "one guarded entry per event, same place");
    assert_eq!(install(&upgraded, EXE).unwrap(), upgraded, "idempotent");

    // Uninstall removes both shapes, even mixed in one file.
    let mut mixed = upgraded.clone();
    mixed["hooks"]["SessionEnd"] = json!([legacy(EXE, "session-end")]);
    assert_eq!(
        uninstall(&mixed).unwrap(),
        json!({ "hooks": { "Stop": [foreign()] } })
    );
}

#[test]
fn hook_binaries_lists_the_path_of_every_entry() {
    use moss_core::settings_json::hook_binaries;
    let doc = json!({ "hooks": {
        "Stop": [foreign(), ours("'/opt/cc moss/cc-moss'", "stop")],
        "SessionEnd": [legacy("/old/bin/cc-moss", "session-end")],
    } });
    assert_eq!(
        hook_binaries(&doc),
        ["/opt/cc moss/cc-moss", "/old/bin/cc-moss"]
    );
    assert!(hook_binaries(&json!({})).is_empty());
}

#[test]
fn hooks_exe_prefers_the_path_entry_for_the_same_file() {
    // Hooks and alias links share one helper.
    use moss_core::aliases::stable_entry as stable_exe;
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let cellar = dir.path().join("Cellar/cc-moss/0.1.0/bin");
    let bin = dir.path().join("bin");
    let other = dir.path().join("other");
    for d in [&cellar, &bin, &other] {
        std::fs::create_dir_all(d).unwrap();
    }
    let real = cellar.join("cc-moss");
    std::fs::write(&real, "").unwrap();
    symlink(&real, bin.join("cc-moss")).unwrap();
    std::fs::write(other.join("cc-moss"), "").unwrap();

    let path = std::env::join_paths([&other, &bin]).unwrap();
    assert_eq!(
        stable_exe(&real, Some(&path)),
        bin.join("cc-moss"),
        "the PATH link, unresolved; a different cc-moss earlier on PATH is skipped"
    );
    // Not on PATH: the prefix's `bin` next to the Cellar.
    let path = std::env::join_paths([&other]).unwrap();
    assert_eq!(stable_exe(&real, Some(&path)), bin.join("cc-moss"));
    assert_eq!(stable_exe(&real, None), bin.join("cc-moss"));
    let relative = std::ffi::OsString::from("bin");
    assert_eq!(
        stable_exe(&real, Some(&relative)),
        bin.join("cc-moss"),
        "relative PATH entries are ignored"
    );
    // Nothing stable at all: the real file, never a link it was started by.
    std::fs::remove_file(bin.join("cc-moss")).unwrap();
    let started_by = other.join("t");
    symlink(&real, &started_by).unwrap();
    assert_eq!(
        stable_exe(&started_by, None),
        std::fs::canonicalize(&real).unwrap()
    );
}

#[test]
fn new_hooks_added_to_an_older_install() {
    use moss_core::agent::HookKind;
    use moss_core::settings_json::missing_hooks;
    // What 0.1.x wrote: the first four hooks only.
    let mut old = install(&json!({ "hooks": { "Stop": [foreign()] } }), EXE).unwrap();
    let hooks = old["hooks"].as_object_mut().unwrap();
    hooks.shift_remove("Notification");
    hooks.shift_remove("PostToolUse");
    assert_eq!(
        missing_hooks(&old),
        [HookKind::Notification, HookKind::PostToolUse]
    );

    let new = install(&old, EXE).unwrap();
    assert!(missing_hooks(&new).is_empty());
    for event in ["SessionStart", "UserPromptSubmit", "Stop", "SessionEnd"] {
        assert_eq!(
            new["hooks"][event], old["hooks"][event],
            "{event} untouched"
        );
    }
    assert_eq!(
        new["hooks"]["Notification"],
        json!([ours(EXE, "notification")])
    );
    assert_eq!(
        new["hooks"]["PostToolUse"],
        json!([ours_for_all_tools(EXE, "post-tool-use")])
    );
    assert_eq!(missing_hooks(&json!({})).len(), HookKind::ALL.len());
    // A user's own Notification hook does not count as ours.
    let users = json!({ "hooks": { "Notification": [foreign()] } });
    assert!(missing_hooks(&users).contains(&HookKind::Notification));
}

#[test]
fn ask_hooks_added_to_an_install_from_before_them() {
    use moss_core::agent::HookKind;
    use moss_core::settings_json::missing_hooks;
    let user_pre =
        json!({ "matcher": "Bash", "hooks": [{ "type": "command", "command": "~/bin/guard.sh" }] });
    let mut old = install(
        &json!({ "hooks": { "PreToolUse": [user_pre.clone()] } }),
        EXE,
    )
    .unwrap();
    let hooks = old["hooks"].as_object_mut().unwrap();
    hooks.insert("PreToolUse".into(), json!([user_pre.clone()]));
    hooks.shift_remove("PermissionRequest");
    assert_eq!(
        missing_hooks(&old),
        [HookKind::PreToolUse, HookKind::PermissionRequest]
    );

    let new = install(&old, EXE).unwrap();
    assert!(missing_hooks(&new).is_empty());
    assert_eq!(
        new["hooks"]["PreToolUse"],
        json!([user_pre.clone(), ours_before_questions(EXE)])
    );
    assert_eq!(
        new["hooks"]["PermissionRequest"],
        json!([ours_waiting(EXE)])
    );

    let gone = uninstall(&new).unwrap();
    assert_eq!(gone["hooks"]["PreToolUse"], json!([user_pre]));
    assert!(gone["hooks"].get("PermissionRequest").is_none());
    assert!(moss_core::settings_json::hook_binaries(&gone).is_empty());
}

mod bypass_prompt {
    use moss_core::settings_json::{
        accept_bypass_prompt, added_keys, disables_bypass, forget_owned, removed_keys,
        BYPASS_PROMPT_KEY,
    };
    use serde_json::json;

    #[test]
    fn install_accepts_bypass_prompt_once() {
        let before = json!({ "model": "opus" });
        let after = accept_bypass_prompt(&before).unwrap();
        assert_eq!(after, json!({ "model": "opus", BYPASS_PROMPT_KEY: true }));
        let owned = added_keys(&before, &after);
        assert_eq!(owned, [BYPASS_PROMPT_KEY]);
        // A second install changes nothing and claims nothing new.
        let again = accept_bypass_prompt(&after).unwrap();
        assert_eq!(again, after);
        assert!(added_keys(&after, &again).is_empty());
        // Uninstall takes out what it wrote, and only that.
        let undone = forget_owned(&again, &owned).unwrap();
        assert_eq!(undone, before);
        assert_eq!(removed_keys(&again, &undone), [BYPASS_PROMPT_KEY]);
    }

    #[test]
    fn keeps_users_own_prompt_setting() {
        for value in [true, false] {
            let users = json!({ BYPASS_PROMPT_KEY: value, "model": "opus" });
            let after = accept_bypass_prompt(&users).unwrap();
            assert_eq!(after, users, "{value}");
            assert!(added_keys(&users, &after).is_empty(), "{value}");
            // Not ours: uninstall leaves it.
            assert_eq!(forget_owned(&after, &[]).unwrap(), users, "{value}");
        }
    }

    #[test]
    fn a_key_the_user_changed_after_install_stays() {
        let owned = vec![BYPASS_PROMPT_KEY.to_string()];
        let changed = json!({ BYPASS_PROMPT_KEY: false });
        assert_eq!(forget_owned(&changed, &owned).unwrap(), changed);
    }

    #[test]
    fn policy_can_disable_bypass() {
        assert!(disables_bypass(
            &json!({ "permissions": { "disableBypassPermissionsMode": "disable" } })
        ));
        assert!(!disables_bypass(&json!({ "permissions": { "allow": [] } })));
        assert!(!disables_bypass(&json!({})));
    }
}
