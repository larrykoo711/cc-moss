//! Command output in Chinese (`CC_MOSS_LANG=zh`).

mod common;

use common::{bin, hook, start_json, FakeProcess};

fn zh(state: &std::path::Path, args: &[&str]) -> String {
    let out = bin()
        .env("CC_MOSS_LANG", "zh")
        .env("CC_MOSS_STATE_DIR", state)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn commands_speak_chinese() {
    let dir = tempfile::tempdir().unwrap();
    assert!(zh(dir.path(), &["ls"]).contains("还没有会话"));
    assert!(zh(dir.path(), &["restore", "--dry-run"]).contains("全员健在"));

    let p = FakeProcess::spawn();
    hook(
        dir.path(),
        "session-start",
        &start_json("A", "startup", Some("api")),
        p.pid(),
    );
    let saved = zh(dir.path(), &["note", "api", "--state", "等上线窗口"]);
    assert!(saved.contains("记下了"), "{saved}");
    let table = zh(dir.path(), &["ls"]);
    assert!(
        table.contains("状态") && table.contains("运行中"),
        "{table}"
    );
    assert!(table.contains("等上线窗口"), "{table}");
}

#[test]
fn english_stays_the_default_for_tests() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .env("CC_MOSS_STATE_DIR", dir.path())
        .arg("ls")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("No sessions yet"));
}

#[test]
fn help_is_translated() {
    let dir = tempfile::tempdir().unwrap();
    let top = zh(dir.path(), &["--help"]);
    assert!(top.contains("用法：") && top.contains("列出会话"), "{top}");
    let sub = zh(dir.path(), &["restore", "--help"]);
    assert!(
        sub.contains("把被打断的会话恢复回来") && sub.contains("显示帮助"),
        "{sub}"
    );
}
