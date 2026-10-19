//! The resident `cc-moss daemon`: where its files live, its log, and the
//! parts that need no operating system (lock, status, restart ladder, plist).
//! Talking to launchd and handling signals is the binary's job.

pub mod ladder;
pub mod lock;
pub mod plist;
pub mod status;

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The launchd label. A protocol literal: install, doctor and the app use it.
pub const LABEL: &str = "io.github.larrykoo711.cc-moss.daemon";

/// How often the daemon refreshes its heartbeat.
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// Logs are started afresh past this size, as `hook.log` is.
pub const LOG_MAX_BYTES: u64 = 1 << 20;

/// The daemon's files, all under `<state>/daemon/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub dir: PathBuf,
    pub lock: PathBuf,
    pub status: PathBuf,
    pub log: PathBuf,
    /// launchd's standard output and error for the daemon.
    pub stdout: PathBuf,
    pub stderr: PathBuf,
}

impl Paths {
    pub fn new(state: &Path) -> Paths {
        let dir = state.join("daemon");
        Paths {
            lock: dir.join("lock"),
            status: dir.join("status.json"),
            log: dir.join("daemon.log"),
            stdout: dir.join("stdout.log"),
            stderr: dir.join("stderr.log"),
            dir,
        }
    }

    /// Under the state directory cc-moss uses now.
    pub fn current() -> Paths {
        Paths::new(&crate::store_dir::state_dir())
    }
}

/// Append one line to `path` in a single write, starting the file afresh
/// once it is past [`LOG_MAX_BYTES`].
pub fn append_log(path: &Path, line: &str) -> std::io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        // Another writer may have started it afresh a moment ago.
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    let mut text = line.trim_end_matches('\n').to_string();
    text.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?
        .write_all(text.as_bytes())
}

/// Empty a file someone else holds open for appending (launchd's output
/// files) once it is past [`LOG_MAX_BYTES`]. Removing it would leave the
/// writer appending to a file nobody can see.
pub fn truncate_if_large(path: &Path) -> std::io::Result<bool> {
    if !std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        return Ok(false);
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_len(0)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_live_under_state_daemon() {
        let p = Paths::new(Path::new("/s"));
        assert_eq!(p.dir, PathBuf::from("/s/daemon"));
        assert_eq!(p.lock, PathBuf::from("/s/daemon/lock"));
        assert_eq!(p.status, PathBuf::from("/s/daemon/status.json"));
        assert_eq!(p.log, PathBuf::from("/s/daemon/daemon.log"));
    }

    #[test]
    fn log_starts_afresh_past_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("daemon.log");
        std::fs::write(&log, vec![b'x'; LOG_MAX_BYTES as usize + 1]).unwrap();
        append_log(&log, "fresh").unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "fresh\n");
        append_log(&log, "again\n").unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "fresh\nagain\n");
    }

    #[test]
    fn large_output_files_are_emptied_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("stdout.log");
        std::fs::write(&out, "small").unwrap();
        assert!(!truncate_if_large(&out).unwrap());
        std::fs::write(&out, vec![b'x'; LOG_MAX_BYTES as usize + 1]).unwrap();
        assert!(truncate_if_large(&out).unwrap());
        assert_eq!(std::fs::metadata(&out).unwrap().len(), 0);
        assert!(!truncate_if_large(&dir.path().join("missing")).unwrap());
    }
}
