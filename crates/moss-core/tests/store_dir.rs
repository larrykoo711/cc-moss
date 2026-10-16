//! The state directory: private, and never relative to wherever a hook runs.

use std::os::unix::fs::PermissionsExt;

use moss_core::registry::Registry;

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn registry_files_are_private() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("a/state");
    let db = state.join("registry.db");
    let reg = Registry::open(&db).unwrap();
    drop(reg);
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(&db), 0o600);

    // A state dir left world-readable by an earlier version is tightened.
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    Registry::open(&db).unwrap();
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(&db), 0o600);
}

#[test]
fn relative_registry_path_is_refused() {
    let err = Registry::open(std::path::Path::new(".local/state/cc-moss/registry.db"))
        .err()
        .expect("refused");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("HOME") && msg.contains("CC_MOSS_STATE_DIR"),
        "{msg}"
    );
    assert!(!std::path::Path::new(".local/state/cc-moss").exists());
}
