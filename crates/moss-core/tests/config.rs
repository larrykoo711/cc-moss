use moss_core::config::Config;

#[test]
fn missing_file_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let c = Config::load_from(&dir.path().join("nope.toml")).unwrap();
    assert!(c.claude.default_args.is_empty());
    assert!(c.naming.host_prefix);
    assert_eq!(c.tmux.socket, None);
}

#[test]
fn partial_fields() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(
        &p,
        "[claude]\ndefault_args = [\"--dangerously-skip-permissions\"]\n",
    )
    .unwrap();
    let c = Config::load_from(&p).unwrap();
    assert_eq!(
        c.claude.default_args,
        vec!["--dangerously-skip-permissions"]
    );
    assert!(c.naming.host_prefix);
}

#[test]
fn invalid_toml_error_has_path() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(&p, "[claude\n").unwrap();
    let err = format!("{:#}", Config::load_from(&p).unwrap_err());
    assert!(err.contains("config.toml"), "{err}");
}
