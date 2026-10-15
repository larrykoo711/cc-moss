mod common;

#[test]
fn invalid_config_reports_path() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("broken.toml");
    std::fs::write(&cfg, "[claude\n").unwrap();
    let out = common::bin()
        .env("CC_MOSS_CONFIG", &cfg)
        .env("CC_MOSS_TMUX_SOCKET", "cc-moss-test-never-started")
        .args(["new", "x", "--detach"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("broken.toml"), "{err}");
}
