use moss_core::naming::{sanitize, session_name, unique};

#[test]
fn sanitize_cases() {
    let cases = [
        ("em-a", "em-a"),
        ("EM_A", "em_a"),
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
    assert_eq!(session_name(Some("Host-A"), "Em A"), "host-a_em-a");
    assert_eq!(session_name(None, "Em A"), "em-a");
}

#[test]
fn unique_name_cases() {
    let taken = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(unique("h_em", &taken(&[]), None), "h_em");
    assert_eq!(unique("h_em", &taken(&["h_em"]), None), "h_em-2");
    assert_eq!(unique("h_em", &taken(&["h_em", "h_em-2"]), None), "h_em-3");
    // Our own current name does not count as taken.
    assert_eq!(unique("h_em", &taken(&["h_em"]), Some("h_em")), "h_em");
}
