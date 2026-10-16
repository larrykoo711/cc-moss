//! `cc-moss doctor` on the hooks in settings.json, in a throwaway HOME.

mod common;

use std::fs;
use std::path::Path;
use std::process::Output;

use common::bin;

fn doctor(home: &Path, lang: &str) -> Output {
    bin()
        .env("HOME", home)
        .env("CC_MOSS_STATE_DIR", home.join("state"))
        .env("CC_MOSS_PLATFORM", "other")
        .env("CC_MOSS_LANG", lang)
        .env("PATH", "/usr/bin:/bin")
        .arg("doctor")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn write_settings(home: &Path, exe: &str) {
    let dir = home.join(".claude");
    fs::create_dir_all(&dir).unwrap();
    let hooks: serde_json::Map<String, serde_json::Value> = [
        ("SessionStart", "session-start"),
        ("UserPromptSubmit", "user-prompt-submit"),
        ("Stop", "stop"),
        ("SessionEnd", "session-end"),
    ]
    .into_iter()
    .map(|(event, name)| {
        let command = format!("[ -x {exe} ] && {exe} hook {name} || true");
        (
            event.to_string(),
            serde_json::json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 10 }] }]),
        )
    })
    .collect();
    let doc = serde_json::json!({ "hooks": hooks });
    fs::write(dir.join("settings.json"), doc.to_string()).unwrap();
}

#[test]
fn doctor_flags_hooks_whose_binary_is_gone() {
    let home = tempfile::tempdir().unwrap();
    let gone = home.path().join("Cellar/cc-moss/0.1.0/bin/cc-moss");
    write_settings(home.path(), gone.to_str().unwrap());
    let out = doctor(home.path(), "en");
    assert!(!out.status.success());
    let report = text(&out);
    let line = report
        .lines()
        .find(|l| l.contains(gone.to_str().unwrap()))
        .unwrap_or_else(|| panic!("names the path: {report}"));
    assert!(line.contains('✗') && line.contains("missing"), "{line}");
    assert!(report.contains("→ cc-moss setup"), "{report}");

    let report = text(&doctor(home.path(), "zh"));
    assert!(report.contains("不见了"), "{report}");
}

#[test]
fn doctor_flags_hooks_whose_binary_is_not_executable() {
    let home = tempfile::tempdir().unwrap();
    let plain = home.path().join("cc-moss");
    fs::write(&plain, "").unwrap();
    write_settings(home.path(), plain.to_str().unwrap());
    let report = text(&doctor(home.path(), "en"));
    let line = report
        .lines()
        .find(|l| l.contains(plain.to_str().unwrap()))
        .unwrap_or_else(|| panic!("names the path: {report}"));
    assert!(
        line.contains('✗') && line.contains("not executable"),
        "{line}"
    );
}

#[test]
fn doctor_accepts_hooks_from_setup() {
    let home = tempfile::tempdir().unwrap();
    let out = bin()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .args(["setup", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let report = text(&doctor(home.path(), "en"));
    let line = report
        .lines()
        .find(|l| l.contains("hooks"))
        .unwrap_or_else(|| panic!("{report}"));
    assert!(line.contains('✓'), "{line}");
}

#[test]
fn doctor_reports_sessions_recorded_without_a_pid() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join("state");
    let report = text(&doctor(home.path(), "en"));
    assert!(
        !report.contains("process id"),
        "nothing recorded yet: {report}"
    );

    for (id, pid) in [("U1", None), ("U2", None), ("U3", Some(std::process::id()))] {
        let out = common::hook(
            &state,
            "session-start",
            &common::start_json(id, "startup", Some(id)),
            pid,
        );
        assert!(out.status.success());
    }
    let report = text(&doctor(home.path(), "en"));
    let line = report
        .lines()
        .find(|l| l.contains("process id"))
        .unwrap_or_else(|| panic!("{report}"));
    assert!(
        line.contains("2 recent sessions were recorded without a process id")
            && line.contains("crash recovery cannot see them"),
        "{line}"
    );
    let report = text(&doctor(home.path(), "zh"));
    assert!(report.contains("2 个最近的会话没有记下进程号"), "{report}");
}
