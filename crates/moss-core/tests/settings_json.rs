//! Editing Claude Code's `settings.json`: only cc-moss entries ever change.

use moss_core::settings_json::{install, is_ours, to_claude_string, uninstall};
use serde_json::{json, Value};

const EXE: &str = "/usr/local/bin/cc-moss";
const EVENTS: [&str; 4] = ["SessionStart", "UserPromptSubmit", "Stop", "SessionEnd"];

fn ours(exe: &str, name: &str) -> Value {
    json!({ "hooks": [{ "type": "command", "command": format!("{exe} hook {name}"), "timeout": 10 }] })
}

fn expected_hooks(exe: &str) -> Value {
    json!({
        "SessionStart": [ours(exe, "session-start")],
        "UserPromptSubmit": [ours(exe, "user-prompt-submit")],
        "Stop": [ours(exe, "stop")],
        "SessionEnd": [ours(exe, "session-end")],
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
    assert_eq!(got["hooks"]["PostToolUse"], json!([foreign()]));
    assert_eq!(
        keys(&got["hooks"]),
        [
            "Stop",
            "PostToolUse",
            "SessionStart",
            "UserPromptSubmit",
            "SessionEnd"
        ]
    );
}

#[test]
fn install_is_idempotent_and_upgrades_path() {
    let once = install(&json!({ "hooks": { "Stop": [foreign()] } }), EXE).unwrap();
    assert_eq!(install(&once, EXE).unwrap(), once);

    let new_exe = "/Users/me/My Tools/cc-moss";
    let upgraded = install(&once, new_exe).unwrap();
    for (event, name) in
        EVENTS
            .iter()
            .zip(["session-start", "user-prompt-submit", "stop", "session-end"])
    {
        let groups = upgraded["hooks"][event].as_array().unwrap();
        let ours_here: Vec<&Value> = groups
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .filter(|h| is_ours(h))
            .collect();
        assert_eq!(ours_here.len(), 1, "{event}: replaced, not stacked");
        assert_eq!(
            ours_here[0]["command"],
            format!("\"{new_exe}\" hook {name}"),
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
        { "type": "command", "command": format!("{EXE} hook stop") },
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
    ] {
        assert!(is_ours(&cmd(yes)), "{yes}");
    }
    for no in [
        "/usr/local/bin/not-cc-moss hook stop",
        "cc-moss hook stop && rm -rf /",
        "cc-moss hook pre-tool-use",
        "~/bin/notify.sh",
        "cc-moss ls",
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
