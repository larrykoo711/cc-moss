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
        sub.contains("把中断的会话恢复回来") && sub.contains("显示帮助"),
        "{sub}"
    );
    // One word for an interrupted session: 中断.
    for help in [top, zh(dir.path(), &["ls", "--help"])] {
        assert!(help.contains("中断") && !help.contains("打断"), "{help}");
    }
}

fn en(args: &[&str]) -> String {
    let out = bin().args(args).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn top_level_help_says_what_bare_cc_moss_does() {
    let dir = tempfile::tempdir().unwrap();
    let top = en(&["--help"]);
    assert!(
        top.contains("Run `cc-moss` without a command to open the session picker"),
        "{top}"
    );
    let top = zh(dir.path(), &["--help"]);
    assert!(top.contains("不带命令直接运行 `cc-moss`"), "{top}");
}

#[test]
fn positional_names_are_described() {
    for sub in ["attach", "close", "note"] {
        let help = en(&[sub, "--help"]);
        assert!(help.contains("Session name"), "{sub}: {help}");
    }
}

#[test]
fn zh_help_uses_full_width_colons_and_plain_words() {
    let dir = tempfile::tempdir().unwrap();
    let top = zh(dir.path(), &["--help"]);
    assert!(top.contains("命令：") && top.contains("选项："), "{top}");
    assert!(!top.contains("命令:") && !top.contains("选项:"), "{top}");
    assert!(top.contains("显示版本") && !top.contains("标志"), "{top}");
    let new = zh(dir.path(), &["new", "--help"]);
    assert!(new.contains("参数："), "{new}");
    // Arguments come first, in their own order, like the English help.
    let (args, name, extra, opts) = (
        new.find("参数：").unwrap(),
        new.find("[NAME]  ").unwrap(),
        new.find("[CLAUDE_ARGS]").unwrap(),
        new.find("选项：").unwrap(),
    );
    assert!(args < name && name < extra && extra < opts, "{new}");
}
