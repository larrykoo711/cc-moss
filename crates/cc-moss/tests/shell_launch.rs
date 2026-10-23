//! 仓内启动包装仅在临时 HOME、zsh -f 和 stub binary 下验证。

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

struct Shell {
    home: tempfile::TempDir,
}

impl Shell {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("bin")).unwrap();
        std::fs::create_dir(home.path().join("project")).unwrap();
        std::fs::write(
            home.path().join("launch.zsh"),
            include_str!("../assets/zsh/launch.zsh"),
        )
        .unwrap();
        executable(
            &home.path().join("bin/cc-moss"),
            r#"#!/bin/sh
verb="$1"; shift
case "$verb" in
 launch-mode) exec "$REAL_CC_MOSS" launch-mode "$@" ;;
 check-directory)
   printf '%s\000' "$@" > "$HOME/guard.args"
   [ "$GUARD_FAIL" = 1 ] && { printf 'directory refused\n' >&2; exit 23; }
   exit 0 ;;
 new) printf '%s\000' "$@" > "$HOME/new.args"; printf 'new\n'; exit 0 ;;
 *) exit 99 ;;
esac
"#,
        );
        executable(
            &home.path().join("bin/claude"),
            r#"#!/bin/sh
printf '%s\000' "$@" > "$HOME/native.args"
/bin/cat > "$HOME/native.stdin"
printf 'native\n'
exit "${NATIVE_EXIT:-0}"
"#,
        );
        Self { home }
    }

    fn run(&self, wrapper: &str, args: &[&str], guard_fail: bool, native_exit: u8) -> Output {
        let mut command = self.command();
        command
            .args([
                "-c",
                "source \"$HOME/launch.zsh\"; entry=$1; shift; \"$entry\" \"$@\"",
                "shell",
                wrapper,
            ])
            .args(args)
            .env("GUARD_FAIL", if guard_fail { "1" } else { "0" })
            .env("NATIVE_EXIT", native_exit.to_string());
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"untouched input\nsecond line\n")
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new("zsh");
        command
            .arg("-f")
            .env_clear()
            .env("HOME", self.home.path())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.home.path().join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("REAL_CC_MOSS", env!("CARGO_BIN_EXE_cc-moss"))
            .env("CC_MOSS_STATE_DIR", self.home.path().join("state"))
            .env("CC_MOSS_CONFIG", self.home.path().join("config.toml"))
            .current_dir(self.home.path().join("project"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn words(&self, file: &str) -> Vec<String> {
        std::fs::read(self.home.path().join(file))
            .unwrap()
            .split(|b| *b == 0)
            .filter(|word| !word.is_empty())
            .map(|word| String::from_utf8(word.to_vec()).unwrap())
            .collect()
    }
}

fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn shell_claude_stays_native_and_preserves_argv_stdin_and_exit() {
    let shell = Shell::new();
    let args = [
        "--append-system-prompt",
        "--help",
        "--model",
        "opus",
        "prompt with spaces",
    ];
    let out = shell.run("claude", &args, false, 7);
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    assert_eq!(shell.words("native.args"), args);
    assert_eq!(
        std::fs::read(shell.home.path().join("native.stdin")).unwrap(),
        b"untouched input\nsecond line\n"
    );
    assert!(shell.home.path().join("guard.args").exists());
    assert!(!shell.home.path().join("new.args").exists());
}

#[test]
fn shell_claude_refused_directory_never_executes_native() {
    let shell = Shell::new();
    let out = shell.run("claude", &["--resume", "existing-id"], true, 0);
    assert_eq!(out.status.code(), Some(23), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(!shell.home.path().join("native.args").exists());
}

#[test]
fn shell_help_and_version_do_not_require_session_directory() {
    for argument in ["--help", "--version"] {
        let shell = Shell::new();
        let out = shell.run("claude", &[argument], true, 0);
        assert!(out.status.success(), "{out:?}");
        assert!(!shell.home.path().join("guard.args").exists());
        assert_eq!(shell.words("native.args"), [argument]);
    }
}

#[test]
fn shell_jarvis_ordinary_flags_are_fresh_and_keep_target_directory() {
    let shell = Shell::new();
    let args = [
        "--model",
        "opus",
        "-C",
        "/tmp/target project",
        "--append-system-prompt",
        "--resume",
    ];
    let out = shell.run("jarvis", &args, true, 0);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(shell.words("new.args"), args);
    assert!(!shell.home.path().join("native.args").exists());
    assert!(
        !shell.home.path().join("guard.args").exists(),
        "最终-C由new同policy验证，不检查调用方PWD"
    );
}

#[test]
fn shell_jarvis_explicit_resume_continue_and_print_remain_native() {
    for args in [
        vec!["--resume", "existing-id"],
        vec!["--continue"],
        vec!["--print", "question"],
    ] {
        let shell = Shell::new();
        let out = shell.run("jarvis", &args, false, 0);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(shell.words("native.args"), args);
        assert!(!shell.home.path().join("new.args").exists());
    }
}

#[test]
fn shell_jarvis_name_alias_is_fresh_but_claude_name_stays_native() {
    let shell = Shell::new();
    let out = shell.run("jarvis", &["-n", "named", "--debug"], false, 0);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(shell.words("new.args"), ["-n", "named", "--debug"]);
    assert!(!shell.home.path().join("native.args").exists());
    let shell = Shell::new();
    let out = shell.run("claude", &["-n", "named", "--debug"], false, 0);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(shell.words("native.args"), ["-n", "named", "--debug"]);
    assert!(!shell.home.path().join("new.args").exists());
}

#[test]
fn shell_prompt_words_never_bypass_guard() {
    for word in ["config", "completion", "uninstall", "help"] {
        let shell = Shell::new();
        let out = shell.run("claude", &[word], true, 0);
        assert_eq!(out.status.code(), Some(23), "{word}: {out:?}");
        assert!(!shell.home.path().join("native.args").exists());
    }
}

#[test]
fn shell_unverifiable_native_targets_are_not_new_sessions() {
    for entry in ["claude", "jarvis"] {
        for args in [
            vec!["attach", "old"],
            vec!["respawn", "old"],
            vec!["agents"],
            vec!["--unknown", "--help"],
        ] {
            let shell = Shell::new();
            let out = shell.run(entry, &args, false, 0);
            assert_eq!(out.status.code(), Some(2), "{out:?}");
            assert!(!shell.home.path().join("native.args").exists());
            assert!(!shell.home.path().join("new.args").exists());
            assert!(String::from_utf8_lossy(&out.stderr).contains("attach/view"));
        }
    }
}

#[test]
fn shell_explicit_cli_pin_ignores_old_path_entry() {
    let shell = Shell::new();
    let pinned = shell.home.path().join("verified-cc-moss");
    std::fs::copy(shell.home.path().join("bin/cc-moss"), &pinned).unwrap();
    executable(
        &shell.home.path().join("bin/cc-moss"),
        "#!/bin/sh\nprintf wrong > \"$HOME/wrong-cli\"\nexit 91\n",
    );
    let out = shell
        .command()
        .env("CC_MOSS_LAUNCH_CLI", pinned)
        .args(["-c", "source \"$HOME/launch.zsh\"; jarvis --model opus"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(shell.words("new.args"), ["--model", "opus"]);
    assert!(!shell.home.path().join("wrong-cli").exists());
}

#[test]
fn shell_invalid_cli_pin_never_falls_back_to_path() {
    let shell = Shell::new();
    let nonexec = shell.home.path().join("not-executable");
    std::fs::write(&nonexec, "#!/bin/sh\nexit 0\n").unwrap();
    for pin in [
        String::new(),
        "relative/cc-moss".into(),
        shell.home.path().join("missing").display().to_string(),
        nonexec.display().to_string(),
        shell.home.path().display().to_string(),
    ] {
        let out = shell
            .command()
            .env("CC_MOSS_LAUNCH_CLI", pin)
            .args(["-c", "source \"$HOME/launch.zsh\"; jarvis --model opus"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(!shell.home.path().join("new.args").exists());
        assert!(!shell.home.path().join("native.args").exists());
    }
}

#[test]
fn shell_keeps_existing_custom_wrappers() {
    let shell = Shell::new();
    let out = shell.command().args(["-c", "function claude { print custom-claude; }; function jarvis { print custom-jarvis; }; source \"$HOME/launch.zsh\"; claude; jarvis"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "custom-claude\ncustom-jarvis\n"
    );
    assert!(!shell.home.path().join("native.args").exists());
    assert!(!shell.home.path().join("new.args").exists());
}
