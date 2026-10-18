//! Where cc-moss keeps its state.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Why a relative state path is refused. It only comes up when `HOME` is
/// unset, and would otherwise scatter state into whatever directory a hook
/// happens to run in.
pub const NOT_ABSOLUTE: &str = "cannot find the cc-moss state directory: HOME is not set; \
     set HOME or CC_MOSS_STATE_DIR to an absolute path";

/// `$CC_MOSS_STATE_DIR`, else `$XDG_STATE_HOME/cc-moss`, else `~/.local/state/cc-moss`.
/// Relative when `HOME` is unset; [`ensure_private_dir`] and the registry
/// refuse such a path.
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

/// The keys cc-moss added to Claude Code's `settings.json` (a JSON array of
/// names), so uninstall takes out only those.
pub fn claude_settings_owned_path() -> PathBuf {
    state_dir().join("claude-settings-owned.json")
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Create `dir` (and missing parents) readable by the owner only. The
/// registry holds prompt previews and notes, so an existing directory left
/// open to others by an earlier version is tightened too.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    if !dir.is_absolute() {
        return Err(std::io::Error::other(NOT_ABSOLUTE));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    make_private(dir, 0o700);
    Ok(())
}

/// Drop group and other access from `path` if it has any. Best effort: a
/// file someone else owns is left as it is.
pub fn make_private(path: &Path, mode: u32) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.permissions().mode() & 0o077 != 0 {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
        }
    }
}
