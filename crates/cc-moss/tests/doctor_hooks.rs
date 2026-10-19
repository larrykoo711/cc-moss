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
fn missing_new_hooks() {
    // What a 0.1.x setup wrote: four hooks, all pointing at a working binary.
    // Notification and PostToolUse came in 0.2, the two ask hooks after.
    let home = tempfile::tempdir().unwrap();
    write_settings(home.path(), env!("CARGO_BIN_EXE_cc-moss"));
    let out = doctor(home.path(), "en");
    assert!(!out.status.success());
    let report = text(&out);
    let line = report
        .lines()
        .find(|l| l.contains("hooks"))
        .unwrap_or_else(|| panic!("{report}"));
    assert!(
        line.contains('✗')
            && line.contains(
                "4 hook(s) missing (Notification, PostToolUse, PreToolUse, PermissionRequest)"
            ),
        "{line}"
    );
    assert!(report.contains("→ cc-moss setup"), "{report}");
    let report = text(&doctor(home.path(), "zh"));
    assert!(report.contains("少了 4 个钩子"), "{report}");
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

/// `doctor` (with `args`) for a home whose aliases live in `.local/bin`,
/// on a PATH that has it, with bash as the login shell.
fn doctor_with(home: &Path, args: &[&str], lang: &str) -> Output {
    bin()
        .env("HOME", home)
        .env("CC_MOSS_STATE_DIR", home.join("state"))
        .env("CC_MOSS_PLATFORM", "other")
        .env("CC_MOSS_LANG", lang)
        .env("SHELL", "/bin/bash")
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", home.join(".local/bin").display()),
        )
        .stdin(std::process::Stdio::null())
        .arg("doctor")
        .args(args)
        .output()
        .unwrap()
}

fn line_for<'a>(report: &'a str, name: &str) -> &'a str {
    report
        .lines()
        .find(|l| {
            let mut words = l.split_whitespace();
            words.next(); // the mark
            words.next() == Some(name)
        })
        .unwrap_or_else(|| panic!("no line for {name}: {report}"))
}

fn alias_home(aliases: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config/cc-moss/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(config, format!("[aliases]\n{aliases}")).unwrap();
    fs::create_dir_all(home.path().join(".local/bin")).unwrap();
    home
}

#[test]
fn alias_broken_link_reported() {
    let home = alias_home("t = \"pick\"\ntn = \"new\"\n");
    let bin_dir = home.path().join(".local/bin");
    // Homebrew upgraded: the versioned path the links pointed at is gone.
    let gone = home.path().join("Cellar/cc-moss/0.1.0/bin/cc-moss");
    std::os::unix::fs::symlink(&gone, bin_dir.join("t")).unwrap();
    // tn has no link at all.
    let out = doctor_with(home.path(), &[], "en");
    assert!(!out.status.success(), "a broken link is broken");
    let report = text(&out);
    let t = line_for(&report, "t");
    assert!(t.contains('✗') && t.contains("cc-moss that is gone"), "{t}");
    let tn = line_for(&report, "tn");
    assert!(tn.contains('!') && tn.contains("no link yet"), "{tn}");
    assert!(report.contains("→ cc-moss doctor --fix"), "{report}");
    let zh = text(&doctor_with(home.path(), &[], "zh"));
    assert!(zh.contains("链接指向的 cc-moss 不见了"), "{zh}");

    let report = text(&doctor_with(home.path(), &["--fix"], "en"));
    assert_eq!(report.matches("Fixed: ").count(), 2, "{report}");
    for name in ["t", "tn"] {
        assert_eq!(
            fs::read_link(bin_dir.join(name)).unwrap(),
            Path::new(env!("CARGO_BIN_EXE_cc-moss"))
        );
        assert!(line_for(&report, name).contains('✓'), "{report}");
    }
    let again = text(&doctor_with(home.path(), &["--fix"], "en"));
    assert!(again.contains("No alias links to fix."), "{again}");
}

#[test]
fn alias_fix_never_touches_what_is_not_ours() {
    let home = alias_home("mine = \"pick\"\nbad = \"rm -rf /\"\n");
    let bin_dir = home.path().join(".local/bin");
    let mine = bin_dir.join("mine");
    fs::write(&mine, "#!/bin/sh\necho mine\n").unwrap();
    let report = text(&doctor_with(home.path(), &["--fix"], "en"));
    assert!(report.contains("No alias links to fix."), "{report}");
    assert_eq!(fs::read_to_string(&mine).unwrap(), "#!/bin/sh\necho mine\n");
    assert!(
        fs::symlink_metadata(bin_dir.join("bad")).is_err(),
        "no link for a bad alias"
    );
    let line = line_for(&report, "mine");
    assert!(
        line.contains('✗') && line.contains("is not cc-moss's"),
        "{line}"
    );
    let line = line_for(&report, "bad");
    assert!(
        line.contains('✗') && line.contains("not a cc-moss command"),
        "{line}"
    );
}

#[test]
fn alias_shadowed_and_old_function_reported() {
    let home = alias_home("t = \"pick\"\nmycc = \"new\"\n");
    let bin_dir = home.path().join(".local/bin");
    for name in ["t", "mycc"] {
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cc-moss"), bin_dir.join(name)).unwrap();
    }
    fs::write(home.path().join(".bashrc"), "mycc() { echo theirs; }\n").unwrap();
    let report = text(&doctor_with(home.path(), &[], "en"));
    let line = line_for(&report, "mycc");
    assert!(
        line.contains('!') && line.contains("hidden by a shell function"),
        "{line}"
    );
    assert!(line_for(&report, "t").contains('✓'), "{report}");

    // cc-moss 0.1's zsh function for `t` is still in .zshrc.
    fs::write(
        home.path().join(".zshrc"),
        "# >>> cc-moss >>>\nfunction t { cc-moss pick \"$@\"; }\n# <<< cc-moss <<<\n",
    )
    .unwrap();
    let report = text(&doctor_with(home.path(), &[], "en"));
    assert!(
        report.contains("still the old zsh function from cc-moss 0.1"),
        "{report}"
    );
}

#[test]
fn alias_path_missing_reported() {
    let home = alias_home("t = \"pick\"\n");
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_cc-moss"),
        home.path().join(".local/bin/t"),
    )
    .unwrap();
    let out = bin()
        .env("HOME", home.path())
        .env("CC_MOSS_STATE_DIR", home.path().join("state"))
        .env("CC_MOSS_PLATFORM", "other")
        .env("SHELL", "/bin/bash")
        .env("PATH", "/usr/bin:/bin")
        .arg("doctor")
        .output()
        .unwrap();
    let report = text(&out);
    assert!(report.contains("is not on PATH"), "{report}");
    assert!(
        report.contains("export PATH=\"$HOME/.local/bin:$PATH\""),
        "{report}"
    );
}

#[test]
fn alias_non_string_entry_is_named_not_fatal() {
    let home = alias_home("t = \"pick\"\nn = 3\n");
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_cc-moss"),
        home.path().join(".local/bin/t"),
    )
    .unwrap();
    let report = text(&doctor_with(home.path(), &[], "en"));
    assert!(
        line_for(&report, "t").contains('✓'),
        "the rest still works: {report}"
    );
    let line = line_for(&report, "aliases");
    assert!(
        line.contains('!') && line.contains("skipped") && line.contains(": n"),
        "{line}"
    );
}
