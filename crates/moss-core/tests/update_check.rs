use moss_core::config::Config;
use moss_core::update::{latest_from_headers, Cache, Version, CACHE_TTL_SECS};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap_or_else(|| panic!("{s} parses"))
}

#[test]
fn versions_parse_with_or_without_a_v() {
    assert_eq!(v("1.2.3"), v("v1.2.3"));
    assert_eq!(v(" v0.10.0\n").to_string(), "0.10.0");
    for bad in [
        "",
        "1.2",
        "1.2.3.4",
        "x.y.z",
        "v",
        "1..3",
        "1.2.3-rc.1",
        "v2.0.0-beta",
    ] {
        assert_eq!(Version::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn versions_compare_by_number_not_text() {
    assert!(v("0.10.0") > v("0.9.9"));
    assert!(v("1.0.0") > v("0.99.99"));
    assert!(v("0.1.1") > v("0.1.0"));
    assert!(v("0.1.0") <= v("0.1.0"));
    assert!(v("9.9.9").is_newer_than(&v("0.1.0")));
    assert!(!v("0.1.0").is_newer_than(&v("0.1.0")));
}

#[test]
fn reads_the_tag_from_the_location_header() {
    let headers = "HTTP/2 302 \r\ndate: Fri, 16 Oct 2026 08:00:00 GMT\r\n\
                   location: https://github.com/larrykoo711/cc-moss/releases/tag/v0.2.0\r\n\r\n";
    assert_eq!(latest_from_headers(headers), Some(v("0.2.0")));
    // Header names are case-insensitive; the tag may lack the v.
    let upper = "HTTP/1.1 302 Found\nLOCATION: /larrykoo711/cc-moss/releases/tag/1.4.2\n";
    assert_eq!(latest_from_headers(upper), Some(v("1.4.2")));
    let mixed = "HTTP/1.1 302 Found\nLocation:https://example.invalid/releases/tag/v3.0.1/\n";
    assert_eq!(latest_from_headers(mixed), Some(v("3.0.1")));
}

#[test]
fn prereleases_and_missing_tags_are_not_new_versions() {
    let pre = "HTTP/2 302\nlocation: https://github.com/o/r/releases/tag/v1.0.0-rc.1\n";
    assert_eq!(latest_from_headers(pre), None);
    // No release yet: GitHub sends you to the releases list.
    let none = "HTTP/2 302\nlocation: https://github.com/o/r/releases\n";
    assert_eq!(latest_from_headers(none), None);
    assert_eq!(
        latest_from_headers("HTTP/2 200\ncontent-type: text/html\n"),
        None
    );
    // A body mentioning a tag is not a header.
    let body = "HTTP/2 200\nx-note: see location: /releases/tag/v9.0.0\n";
    assert_eq!(latest_from_headers(body), None);
}

#[test]
fn the_cache_is_fresh_for_a_day() {
    let c = Cache {
        checked_at: 1_000_000,
        latest: Some("0.2.0".into()),
    };
    assert!(c.is_fresh(1_000_000));
    assert!(c.is_fresh(1_000_000 + CACHE_TTL_SECS - 1));
    assert!(!c.is_fresh(1_000_000 + CACHE_TTL_SECS));
    // A clock that went backwards does not keep a cache alive for ever.
    assert!(!c.is_fresh(1_000_000 - 3600));
    assert_eq!(c.latest_version(), Some(v("0.2.0")));
}

#[test]
fn the_cache_round_trips_and_tolerates_junk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state/update.json");
    assert_eq!(Cache::load(&path), None);
    let c = Cache {
        checked_at: 42,
        latest: None,
    };
    c.save(&path).unwrap();
    assert_eq!(Cache::load(&path), Some(c));
    std::fs::write(&path, "{not json").unwrap();
    assert_eq!(Cache::load(&path), None);
}

#[test]
fn checking_is_on_unless_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    let none = |_: &str| None::<String>;
    let config = Config::load_from(&p).unwrap();
    assert!(config.update.check);
    assert!(moss_core::update::enabled(&config, &none));
    let off = |k: &str| (k == "CC_MOSS_NO_UPDATE_CHECK").then(|| "1".to_string());
    assert!(!moss_core::update::enabled(&config, &off));
    let empty = |k: &str| (k == "CC_MOSS_NO_UPDATE_CHECK").then(String::new);
    assert!(moss_core::update::enabled(&config, &empty));
    let zero = |k: &str| (k == "CC_MOSS_NO_UPDATE_CHECK").then(|| "0".to_string());
    assert!(moss_core::update::enabled(&config, &zero));
    std::fs::write(&p, "[update]\ncheck = false\n").unwrap();
    let config = Config::load_from(&p).unwrap();
    assert!(!moss_core::update::enabled(&config, &none));
}
