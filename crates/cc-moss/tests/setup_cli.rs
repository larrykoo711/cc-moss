//! `cc-moss setup` against a settings.json in a temp dir, never the real one.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::bin;

/// A settings.json as Claude Code writes it, with a user hook on an event
/// cc-moss also uses and non-ASCII text that must survive unescaped.
const CLAUDE_FORMATTED: &str = r#"{
  "permissions": {
    "allow": [
      "Bash(ls:*)"
    ]
  },
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "~/bin/notify.sh 完成…"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit",
        "hooks": [
          {
            "type": "command",
            "command": "cargo fmt"
          }
        ]
      }
    ]
  },
  "model": "opus"
}
"#;

fn setup(dir: &Path, args: &[&str]) -> Output {
    let settings = dir.join("settings.json");
    bin()
        .env("HOME", dir)
        .arg("setup")
        .args(args)
        .arg("--settings")
        .arg(&settings)
        .output()
        .expect("run cc-moss setup")
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with("settings.json.cc-moss-") && n.ends_with(".bak")
        })
        .collect();
    v.sort();
    v
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn install_then_uninstall_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, CLAUDE_FORMATTED).unwrap();

    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "install: {}", stderr(&out));
    let installed = fs::read_to_string(&path).unwrap();
    for name in [
        "session-start",
        "user-prompt-submit",
        "stop",
        "session-end",
        "notification",
        "post-tool-use",
    ] {
        assert!(
            installed.contains(&format!("cc-moss hook {name}")),
            "{name}"
        );
    }
    assert!(
        installed.contains("~/bin/notify.sh 完成…"),
        "user hook kept"
    );

    let out = setup(dir.path(), &["--uninstall", "--yes"]);
    assert!(out.status.success(), "uninstall: {}", stderr(&out));
    assert_eq!(fs::read(&path).unwrap(), CLAUDE_FORMATTED.as_bytes());
}

#[test]
fn invalid_settings_refuses_to_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let broken = "{ \"model\": \"opus\", \n";
    fs::write(&path, broken).unwrap();

    for args in [&["--yes"][..], &["--uninstall", "--yes"][..]] {
        let out = setup(dir.path(), args);
        assert!(!out.status.success(), "{args:?} must fail");
        assert!(stderr(&out).contains("not valid JSON"), "{}", stderr(&out));
        assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        assert!(backups(dir.path()).is_empty());
    }
}

#[test]
fn requires_confirmation_or_yes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, CLAUDE_FORMATTED).unwrap();

    // stdin is not a terminal here, so nobody can answer a prompt.
    let out = setup(dir.path(), &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--yes"), "{}", stderr(&out));
    let shown = stdout(&out);
    assert!(
        shown
            .lines()
            .any(|l| l.starts_with('+') && l.contains("cc-moss hook stop")),
        "diff shown: {shown}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), CLAUDE_FORMATTED);

    let out = setup(dir.path(), &["--dry-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("cc-moss hook session-start"),
        "diff shown"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), CLAUDE_FORMATTED);
    assert!(backups(dir.path()).is_empty());

    // A dry run must not create a missing file either.
    let empty = tempfile::tempdir().unwrap();
    let out = setup(empty.path(), &["--dry-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!empty.path().join("settings.json").exists());
}

#[test]
fn writes_backup_before_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, CLAUDE_FORMATTED).unwrap();

    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let b = backups(dir.path());
    assert_eq!(b.len(), 1, "{b:?}");
    assert_eq!(fs::read_to_string(&b[0]).unwrap(), CLAUDE_FORMATTED);
    let installed = fs::read_to_string(&path).unwrap();

    // A second write within the same second must not clobber the first backup.
    let out = setup(dir.path(), &["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let b2 = backups(dir.path());
    assert_eq!(b2.len(), 2, "{b2:?}");
    let contents: Vec<String> = b2.iter().map(|p| fs::read_to_string(p).unwrap()).collect();
    assert!(contents.contains(&CLAUDE_FORMATTED.to_string()));
    assert!(contents.contains(&installed));
}

#[test]
fn second_setup_reports_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    // No settings.json yet: setup creates it, with nothing to back up.
    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(dir.path().join("settings.json").exists());
    assert!(backups(dir.path()).is_empty());

    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("up to date"), "{}", stdout(&out));
    assert!(backups(dir.path()).is_empty(), "no write, no backup");
}

#[test]
fn honours_claude_config_dir() {
    // Claude Code reads settings from $CLAUDE_CONFIG_DIR when it is set.
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let out = common::bin()
        .env("HOME", home.path())
        .env("CLAUDE_CONFIG_DIR", config_dir.path())
        .args(["setup", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(config_dir.path().join("settings.json").exists());
    assert!(!home.path().join(".claude/settings.json").exists());
}

/// The `.zshrc` block cc-moss 0.1 wrote for `t`.
const OLD_T_BLOCK: &str = "# >>> cc-moss >>>\n\
# Managed by `cc-moss setup`; remove with `cc-moss setup --uninstall`.\n\
unalias t 2>/dev/null\n\
function t { cc-moss pick \"$@\"; }\n\
# <<< cc-moss <<<\n";

/// `setup` with a temp HOME, PATH and settings file; stdin is never a terminal.
fn setup_in(home: &Path, args: &[&str]) -> Output {
    bin()
        .env("HOME", home)
        .env("SHELL", "/bin/sh")
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .arg("setup")
        .args(args)
        .arg("--settings")
        .arg(home.join("settings.json"))
        .output()
        .unwrap()
}

#[test]
fn alias_t_replaces_zsh_function() {
    let home = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(home.path()).unwrap();
    let zshrc = home.join(".zshrc");
    // The user's own `t` sits outside our block: never edited.
    let mine = "export A=1\n# >>> conda >>>\nconda init\n# <<< conda <<<\nalias t2='tmux'\n";
    let original = format!("{mine}{OLD_T_BLOCK}eval \"$(starship init zsh)\"\n");
    fs::write(&zshrc, &original).unwrap();
    let link = home.join(".local/bin/t");
    let config = home.join(".config/cc-moss/config.toml");

    // The plan is shown first; nothing is written without a yes.
    let dry = setup_in(&home, &["--dry-run"]);
    let shown = stdout(&dry);
    assert!(shown.contains("-function t { cc-moss pick"), "{shown}");
    assert!(shown.contains("+t = \"pick\""), "{shown}");
    assert!(
        shown.contains(&format!("+++ {} → ", link.display())),
        "{shown}"
    );
    let refused = setup_in(&home, &[]);
    assert!(!refused.status.success());
    assert_eq!(fs::read_to_string(&zshrc).unwrap(), original);
    assert!(
        fs::symlink_metadata(&link).is_err(),
        "no link without a yes"
    );
    assert!(!config.exists());

    let out = setup_in(&home, &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        fs::read_to_string(&zshrc).unwrap(),
        format!("{mine}eval \"$(starship init zsh)\"\n"),
        "only cc-moss's block is taken out"
    );
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "[aliases]\nt = \"pick\"\n"
    );
    assert_eq!(
        fs::read_link(&link).unwrap(),
        Path::new(env!("CARGO_BIN_EXE_cc-moss"))
    );

    let again = setup_in(&home, &["--yes"]);
    assert!(stdout(&again).contains("up to date"), "{}", stdout(&again));
    assert!(!stdout(&again).contains("+++"), "{}", stdout(&again));

    // Uninstall takes the link and the entry back out.
    let out = setup_in(&home, &["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(!config.exists());
}

#[test]
fn alias_t_keeps_the_users_own_choice() {
    let home = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(home.path()).unwrap();
    let config = home.join(".config/cc-moss/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    let text = "# mine\n[aliases]\nt = \"pick api\" # my t\nmycc = \"new\"\n";
    fs::write(&config, text).unwrap();
    let out = setup_in(&home, &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        text,
        "t was already set"
    );
    assert!(home.join(".local/bin/t").exists());

    // Uninstall removes the `t` link but keeps the user's own `t` entry.
    let out = setup_in(&home, &["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fs::read_to_string(&config).unwrap(), text);
    assert!(fs::symlink_metadata(home.join(".local/bin/t")).is_err());
}

#[test]
fn hooks_use_the_path_link_not_the_resolved_binary() {
    // A package manager's `bin/cc-moss` link survives upgrades; the versioned
    // file it points at does not.
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    let link = bin_dir.join("cc-moss");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cc-moss"), &link).unwrap();
    let settings = dir.path().join("settings.json");
    let out = bin()
        .env("HOME", dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
        .args(["setup", "--yes", "--settings"])
        .arg(&settings)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let text = fs::read_to_string(&settings).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let want = format!("[ -x {0} ] && {0} hook stop || true", link.display());
    assert_eq!(
        doc["hooks"]["Stop"][0]["hooks"][0]["command"],
        want.as_str()
    );
}

#[test]
fn setup_upgrades_unguarded_hooks_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let legacy = serde_json::json!({ "hooks": { "Stop": [{ "hooks": [{
        "type": "command",
        "command": "/old/place/cc-moss hook stop",
        "timeout": 10,
    }] }] } });
    fs::write(&path, legacy.to_string()).unwrap();
    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let stop = doc["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1, "{stop:?}");
    let command = stop[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(command.starts_with("[ -x "), "{command}");
    assert!(!command.contains("/old/place"), "{command}");
}

/// The settings a 0.1.x `setup` left: `CLAUDE_FORMATTED` plus the first four
/// cc-moss hooks, written the way Claude Code formats the file.
fn settings_from_0_1(dir: &Path) -> String {
    let path = dir.join("settings.json");
    fs::write(&path, CLAUDE_FORMATTED).unwrap();
    assert!(setup(dir, &["--yes"]).status.success());
    let mut doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let hooks = doc["hooks"].as_object_mut().unwrap();
    hooks.shift_remove("Notification");
    let post = hooks["PostToolUse"].as_array_mut().unwrap();
    post.retain(|g| g["matcher"] == "Edit");
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    fs::write(&path, &text).unwrap();
    for b in backups(dir) {
        fs::remove_file(b).unwrap();
    }
    text
}

#[test]
fn new_hooks_added_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let old = settings_from_0_1(dir.path());
    assert!(!old.contains("hook notification") && !old.contains("hook post-tool-use"));

    let dry = setup(dir.path(), &["--dry-run"]);
    let diff = stdout(&dry);
    let added: Vec<&str> = diff
        .lines()
        .filter(|l| l.starts_with('+') && l.contains("cc-moss hook"))
        .collect();
    assert_eq!(added.len(), 2, "only the two new hooks: {diff}");
    assert!(
        added.iter().any(|l| l.contains("hook notification")),
        "{diff}"
    );
    assert!(
        added.iter().any(|l| l.contains("hook post-tool-use")),
        "{diff}"
    );
    assert!(
        !diff
            .lines()
            .any(|l| l.starts_with('-') && l.contains("cc-moss hook")),
        "the four existing hooks stay as they are: {diff}"
    );

    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let again = setup(dir.path(), &["--yes"]);
    assert!(
        stdout(&again).contains("already up to date"),
        "{}",
        stdout(&again)
    );
    let installed = fs::read_to_string(&path).unwrap();
    for name in [
        "session-start",
        "user-prompt-submit",
        "stop",
        "session-end",
        "notification",
        "post-tool-use",
    ] {
        let n = installed.matches(&format!("cc-moss hook {name} ")).count();
        assert_eq!(n, 1, "{name}: {installed}");
    }
    let doc: serde_json::Value = serde_json::from_str(&installed).unwrap();
    let post = doc["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post[0]["matcher"], "Edit", "the user's group stays first");
    assert_eq!(post[1]["matcher"], "*");
    assert!(doc["hooks"]["Notification"][0].get("matcher").is_none());

    let out = setup(dir.path(), &["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fs::read(&path).unwrap(), CLAUDE_FORMATTED.as_bytes());
}

#[test]
fn alias_setup_puts_local_bin_on_path_for_zsh() {
    // A 0.1 user who only ever ran `setup`: no install, no PATH block.
    let home = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(home.path()).unwrap();
    fs::write(home.join(".zshrc"), format!("export A=1\n{OLD_T_BLOCK}")).unwrap();
    let run = |args: &[&str]| {
        bin()
            .env("HOME", &home)
            .env("SHELL", "/bin/zsh")
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::null())
            .arg("setup")
            .args(args)
            .arg("--settings")
            .arg(home.join("settings.json"))
            .output()
            .unwrap()
    };
    let dry = run(&["--dry-run"]);
    assert!(
        stdout(&dry).contains(".zshenv"),
        "shown in the diff: {}",
        stdout(&dry)
    );
    let out = run(&["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let zshenv = fs::read_to_string(home.join(".zshenv")).unwrap();
    assert!(zshenv.contains("# >>> cc-moss: path >>>"), "{zshenv}");

    // A new zsh finds `t` with nothing else on PATH.
    if Path::new("/bin/zsh").exists() || Path::new("/usr/bin/zsh").exists() {
        let found = std::process::Command::new("zsh")
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .args(["-c", "command -v t"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&found.stdout).trim(),
            home.join(".local/bin/t").to_str().unwrap()
        );
    }

    // setup --uninstall takes its own PATH block back out, and only `t`.
    let out = run(&["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !home.join(".zshenv").exists(),
        "the file held only our block"
    );
}

#[test]
fn alias_setup_uninstall_only_removes_t() {
    let home = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(home.path()).unwrap();
    assert!(setup_in(&home, &["--yes"]).status.success());
    let bin_dir = home.join(".local/bin");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cc-moss"), bin_dir.join("tn")).unwrap();
    let config = home.join(".config/cc-moss/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config).unwrap() + "tn = \"new\"\n",
    )
    .unwrap();
    let out = setup_in(&home, &["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(fs::symlink_metadata(bin_dir.join("t")).is_err());
    assert!(
        bin_dir.join("tn").exists(),
        "other aliases are install's business"
    );
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "[aliases]\ntn = \"new\"\n"
    );
}

/// T1.2: setup adds `skipDangerousModePermissionPrompt` only when the user has
/// not set it, records that in the state dir, and uninstall takes out only that.
#[test]
fn setup_owns_only_the_prompt_key_it_wrote() {
    const KEY: &str = "skipDangerousModePermissionPrompt";
    let record = |dir: &Path| dir.join(".local/state/cc-moss/claude-settings-owned.json");
    let read = |dir: &Path| -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
    };

    // Not set: cc-moss writes it, notes it, and takes it out again.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("settings.json"), CLAUDE_FORMATTED).unwrap();
    let out = setup(dir.path(), &["--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(dir.path())[KEY], true);
    let owned: Vec<String> =
        serde_json::from_str(&fs::read_to_string(record(dir.path())).unwrap()).unwrap();
    assert_eq!(owned, [KEY]);
    let out = setup(dir.path(), &["--uninstall", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(read(dir.path()).get(KEY).is_none());
    assert!(!record(dir.path()).exists());

    // The user's own value, either way: never touched, never claimed.
    for value in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let mut doc: serde_json::Value = serde_json::from_str(CLAUDE_FORMATTED).unwrap();
        doc[KEY] = value.into();
        let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
        fs::write(dir.path().join("settings.json"), &text).unwrap();
        let out = setup(dir.path(), &["--yes"]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(read(dir.path())[KEY], value);
        assert!(!record(dir.path()).exists(), "{value}");
        let out = setup(dir.path(), &["--uninstall", "--yes"]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            fs::read_to_string(dir.path().join("settings.json")).unwrap(),
            text
        );
    }
}
