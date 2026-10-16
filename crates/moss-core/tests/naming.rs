use moss_core::naming::{sanitize, session_name, unique};

#[test]
fn sanitize_cases() {
    let cases = [
        ("job-a", "job-a"),
        ("JOB_A", "job_a"),
        ("v1.0.43", "v1-0-43"),
        ("a:b", "a-b"),
        ("hello world", "hello-world"),
        ("中文 name", "name"),
        ("a...b", "a-b"),
        ("--x--", "x"),
        ("", "session"),
        ("。。。", "session"),
    ];
    for (input, want) in cases {
        assert_eq!(sanitize(input), want, "input={input:?}");
    }
}

#[test]
fn prefix_toggle() {
    assert_eq!(session_name(Some("Host-A"), "Job A"), "host-a_job-a");
    assert_eq!(session_name(None, "Job A"), "job-a");
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
