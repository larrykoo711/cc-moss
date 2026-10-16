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
