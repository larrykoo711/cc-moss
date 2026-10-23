use moss_core::launch_args::whitelist;

fn args(line: &str) -> Vec<String> {
    whitelist(&line.split_whitespace().collect::<Vec<_>>())
}

#[test]
fn shell_mode_prompt_words_cannot_bypass_directory_guard() {
    for word in ["config", "completion", "uninstall", "help"] {
        assert_eq!(
            moss_core::launch_args::shell_mode(&[word.into()]).as_str(),
            "fresh",
            "{word}"
        );
    }
}

#[test]
fn shell_mode_unknown_target_is_blocked_not_fresh_or_unguarded() {
    for words in [
        vec!["attach", "old"],
        vec!["respawn", "old"],
        vec!["agents"],
        vec!["--unknown", "--help"],
        vec!["--worktree", "name"],
    ] {
        let args: Vec<String> = words.into_iter().map(str::to_owned).collect();
        assert_eq!(
            moss_core::launch_args::shell_mode(&args).as_str(),
            "blocked",
            "{args:?}"
        );
    }
}

#[test]
fn shell_launch_modes_follow_argument_arity() {
    use moss_core::launch_args::{shell_mode, ShellMode};
    for (words, want) in [
        (vec![], ShellMode::Fresh),
        (vec!["--model", "opus"], ShellMode::Fresh),
        (vec!["--name", "session", "--debug"], ShellMode::Fresh),
        (vec!["--debug=hooks,mcp"], ShellMode::Fresh),
        (vec!["-d", "hooks", "--model", "opus"], ShellMode::Fresh),
        (vec!["--debug", "--help"], ShellMode::NonSession),
        (
            vec!["--plugin-dir", "/tmp/plugin", "--brief"],
            ShellMode::Fresh,
        ),
        (vec!["--append-system-prompt", "--help"], ShellMode::Fresh),
        (vec!["--append-system-prompt", "--resume"], ShellMode::Fresh),
        (vec!["--resume", "abc"], ShellMode::Native),
        (vec!["--continue"], ShellMode::Native),
        (vec!["--print", "hello"], ShellMode::Native),
        (vec!["--unknown", "--help"], ShellMode::Blocked),
        (vec!["--help"], ShellMode::NonSession),
        (vec!["--version"], ShellMode::NonSession),
        (vec!["mcp", "list"], ShellMode::NonSession),
        (
            vec!["agents", "--json", "--all", "--cwd", "/tmp/project"],
            ShellMode::NonSession,
        ),
        (vec!["agents", "--cwd", "--json"], ShellMode::Blocked),
        (vec!["--verbose", "attach", "old"], ShellMode::Blocked),
        (vec!["--resume"], ShellMode::Native),
        (vec!["-C", "/tmp/project", "--model=opus"], ShellMode::Fresh),
    ] {
        let args: Vec<String> = words.into_iter().map(str::to_owned).collect();
        assert_eq!(shell_mode(&args), want, "{args:?}");
    }
}

#[test]
fn fresh_launch_arguments_cannot_override_receipt_identity() {
    for words in [
        vec!["--session-id", "fixed"],
        vec!["--resume", "old"],
        vec!["--continue"],
        vec!["--name=other"],
        vec!["-n", "other"],
        vec!["--json"],
    ] {
        let args: Vec<String> = words.into_iter().map(str::to_owned).collect();
        assert!(!moss_core::launch_args::fresh_arguments(&args), "{args:?}");
    }
}

#[test]
fn jarvis_name_alias_becomes_fresh_name_without_duplicate_claude_name() {
    let args = ["-n", "project", "--debug", "--model", "opus", "--detach"].map(str::to_owned);
    assert_eq!(
        moss_core::launch_args::normalize_new(&args),
        ["project", "--detach", "--", "--debug", "--model", "opus"]
    );
}

#[test]
fn new_json_is_owned_by_moss_not_forwarded_to_claude() {
    let args = ["--json", "--model", "opus", "project", "--detach"].map(str::to_owned);
    assert_eq!(
        moss_core::launch_args::normalize_new(&args),
        ["--json", "project", "--detach", "--", "--model", "opus"]
    );
}

#[test]
fn whitelist_cases() {
    let cases: [(&str, &[&str]); 9] = [
        ("claude", &[]),
        (
            "claude --dangerously-skip-permissions",
            &["--dangerously-skip-permissions"],
        ),
        (
            "claude --model opus --effort high",
            &["--model", "opus", "--effort", "high"],
        ),
        ("claude --model=opus", &["--model", "opus"]),
        (
            "claude --add-dir /a --add-dir /b",
            &["--add-dir", "/a", "--add-dir", "/b"],
        ),
        (
            "claude -n job --session-id U1 --resume U0 --fork-session -p hi",
            &[],
        ),
        (
            "claude --permission-mode plan --agent reviewer",
            &["--permission-mode", "plan", "--agent", "reviewer"],
        ),
        // Values that look like credentials are dropped with their flag.
        ("claude --model sk-ant-123 --agent my_token_bot", &[]),
        // A whitelisted flag missing its value is dropped.
        ("claude --model", &[]),
    ];
    for (line, want) in cases {
        assert_eq!(args(line), want, "line={line:?}");
    }
}

mod new_args {
    use moss_core::launch_args::normalize_new;

    fn n(args: &[&str]) -> Vec<String> {
        normalize_new(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn claude_flags_move_behind_double_dash() {
        assert_eq!(
            n(&["--dangerously-skip-permissions", "login-page"]),
            ["login-page", "--", "--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn claude_flags_with_values_keep_their_value() {
        assert_eq!(
            n(&["job-a", "--model", "opus", "--detach", "--effort=high"]),
            [
                "job-a",
                "--detach",
                "--",
                "--model",
                "opus",
                "--effort=high"
            ]
        );
    }

    #[test]
    fn own_flags_stay_and_take_their_values() {
        assert_eq!(
            n(&["-C", "/work", "--yes", "job-a", "--dir", "/w2"]),
            ["-C", "/work", "--yes", "job-a", "--dir", "/w2"]
        );
    }

    #[test]
    fn explicit_double_dash_is_kept_after_moved_flags() {
        assert_eq!(
            n(&["--model", "opus", "job-a", "--", "--add-dir", "/x"]),
            ["job-a", "--", "--model", "opus", "--add-dir", "/x"]
        );
    }

    #[test]
    fn help_is_ours() {
        assert_eq!(n(&["--help"]), ["--help"]);
        assert_eq!(n(&["-h"]), ["-h"]);
    }

    #[test]
    fn nothing_to_move_leaves_args_alone() {
        assert_eq!(n(&["job-a", "--detach"]), ["job-a", "--detach"]);
    }
}

mod permissions {
    use moss_core::launch_args::{normalize_new, sets_permissions, without_permissions};

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn recognises_both_permission_flags() {
        assert!(sets_permissions(&s(&["--dangerously-skip-permissions"])));
        assert!(sets_permissions(&s(&[
            "--model",
            "opus",
            "--permission-mode",
            "plan"
        ])));
        assert!(sets_permissions(&s(&["--permission-mode=acceptEdits"])));
        assert!(!sets_permissions(&s(&["--model", "opus"])));
        assert!(!sets_permissions(&[]));
    }

    #[test]
    fn strips_every_permission_flag() {
        assert_eq!(
            without_permissions(&s(&[
                "--dangerously-skip-permissions",
                "--model",
                "opus",
                "--permission-mode",
                "plan",
                "--permission-mode=acceptEdits",
                "--effort=high",
            ])),
            ["--model", "opus", "--effort=high"]
        );
    }

    #[test]
    fn safe_stays_with_new() {
        assert_eq!(
            normalize_new(&s(&["x", "--safe", "--model", "opus"])),
            ["x", "--safe", "--", "--model", "opus"]
        );
    }
}
