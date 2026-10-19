//! The files of Moss's action channel (T2.4): a request Moss leaves in
//! `<home>/actions/<id>.json`, and the daemon's answer next to it in
//! `<id>.result.json`. The binary checks and carries them out.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub fn dir(home: &Path) -> PathBuf {
    home.join("actions")
}

pub fn result_path(home: &Path, id: &str) -> PathBuf {
    dir(home).join(format!("{id}.result.json"))
}

/// `<state>/audit.jsonl`: one line per action, whatever became of it.
pub fn audit_path(state: &Path) -> PathBuf {
    state.join("audit.jsonl")
}

/// The audit log is kept to this size; the one before is kept as `.1`.
const AUDIT_MAX_BYTES: u64 = 1 << 20;

/// Add one line to the audit log, starting a new one (and keeping the last
/// as `audit.jsonl.1`) past [`AUDIT_MAX_BYTES`].
pub fn append_audit(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if std::fs::metadata(path).is_ok_and(|m| m.len() > AUDIT_MAX_BYTES) {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        std::fs::rename(path, old)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?
        .write_all(format!("{}\n", line.trim_end()).as_bytes())
}

/// What Moss asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub schema: u32,
    pub id: String,
    pub at: String,
    pub action: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub because: Option<String>,
}

/// What became of it: `done`, `refused`, `skipped` or `failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub schema: u32,
    pub id: String,
    pub outcome: String,
    pub message: String,
}
