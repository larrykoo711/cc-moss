use moss_core::naming::{sanitize, session_name, unique};

#[test]
fn sanitize_cases() {
    // Keeps what people name things with (any script, case, emoji); only
    // what tmux or a shell can't take becomes `-` (2026-10-19,
    // notes/session-names.md).
    let cases = [
        ("job-a", "job-a"),
        ("JOB_A", "JOB_A"),
        ("接口评审", "接口评审"),
        ("接口 评审", "接口-评审"),
        ("中文 name", "中文-name"),
        ("API Review v1.2", "API-Review-v1-2"),
        ("v1.0.43", "v1-0-43"),
        ("a:b", "a-b"),
        ("hello\tworld", "hello-world"),
        ("修复#12", "修复-12"),
        ("it's \"done\" $now `x` !", "it-s-done-now-x"),
        ("a\\b", "a-b"),
        ("ship 🚀", "ship-🚀"),
        ("a...b", "a-b"),
        ("--x--", "x"),
        ("$HOME", "HOME"),
        ("@team", "team"),
        ("%1", "1"),
        ("=x", "x"),
        ("a\u{7}b", "ab"),
        ("", "session"),
        ("。。。", "。。。"),
        ("...", "session"),
    ];
    for (input, want) in cases {
        assert_eq!(sanitize(input), want, "input={input:?}");
    }
}

#[test]
fn long_names_are_cut_at_forty_characters() {
    let long = "很长的会话名字".repeat(10);
    let got = sanitize(&long);
    assert_eq!(got.chars().count(), 40);
    assert!(long.starts_with(&got));
    // A cut never leaves a trailing `-`.
    let spaced = format!("{} tail", "a".repeat(39));
    assert_eq!(sanitize(&spaced), "a".repeat(39));
}

#[test]
fn prefix_toggle() {
    // The machine's prefix stays lowercase, as before; the name keeps its case.
    assert_eq!(session_name(Some("Host-A"), "Job A"), "host-a_Job-A");
    assert_eq!(session_name(Some("host-a"), "接口评审"), "host-a_接口评审");
    assert_eq!(session_name(None, "Job A"), "Job-A");
}

#[test]
fn unique_name_cases() {
    let taken = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(unique("h_job", &taken(&[]), None), "h_job");
    assert_eq!(unique("h_job", &taken(&["h_job"]), None), "h_job-2");
    assert_eq!(
        unique("h_job", &taken(&["h_job", "h_job-2"]), None),
        "h_job-3"
    );
    // Our own current name does not count as taken.
    assert_eq!(unique("h_job", &taken(&["h_job"]), Some("h_job")), "h_job");
}

#[test]
fn a_missing_host_name_is_cached_too() {
    // Minimal images have no `hostname`: remember that for a day instead of
    // trying again on every prompt.
    use moss_core::naming::cached_host_prefix;
    std::env::remove_var("CC_MOSS_HOST_PREFIX");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("host_prefix"), "").unwrap();
    assert_eq!(cached_host_prefix(dir.path()), None);
    std::fs::write(dir.path().join("host_prefix"), "host-a\n").unwrap();
    assert_eq!(cached_host_prefix(dir.path()).as_deref(), Some("host-a"));
}
