//! Where cc-moss keeps its state.

use std::path::PathBuf;

/// `$CC_MOSS_STATE_DIR`, else `$XDG_STATE_HOME/cc-moss`, else `~/.local/state/cc-moss`.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = non_empty_env("CC_MOSS_STATE_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = non_empty_env("XDG_STATE_HOME") {
        return PathBuf::from(dir).join("cc-moss");
    }
    let home = non_empty_env("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".local/state/cc-moss")
}

pub fn registry_path() -> PathBuf {
    state_dir().join("registry.db")
}

pub fn hook_log_path() -> PathBuf {
    state_dir().join("hook.log")
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}
