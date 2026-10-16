use moss_core::launch_args::whitelist;

fn args(line: &str) -> Vec<String> {
    whitelist(&line.split_whitespace().collect::<Vec<_>>())
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
