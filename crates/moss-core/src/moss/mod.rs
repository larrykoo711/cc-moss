//! Moss: the resident Claude Code session the daemon keeps alive. Where it
//! lives, what it is called, and what the daemon records about it. Starting
//! it (tmux) is the binary's job; the rules are here.

pub mod guard;
pub mod home;
pub mod keeper;

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The tmux server Moss has to itself (`tmux -L`). A protocol literal.
pub const SOCKET: &str = "cc-moss-moss";

/// Its tmux session's name on that server, when it is started. Moss is
/// recognised by [`ROLE_OPTION`], not by this name: a session renamed by
/// hand is still Moss.
pub const SESSION: &str = "moss";

/// The tmux user option set on Moss's session, to [`ROLE`].
pub const ROLE_OPTION: &str = "@cc-moss-role";

/// Set for Moss's Claude Code to the session id the daemon started it
/// with: only the session with that id is recorded as Moss, whatever else
/// inherits [`ROLE_ENV`].
pub const SESSION_ENV: &str = "CC_MOSS_MOSS_SESSION";

/// The registry role of Moss's sessions, and `ls --json`'s `role`.
pub const ROLE: &str = "moss";

/// Set for Moss's Claude Code (and so for its hooks) to [`ROLE`].
pub const ROLE_ENV: &str = "CC_MOSS_ROLE";

/// `<state>/moss/`.
pub fn home_dir(state: &Path) -> PathBuf {
    state.join("moss")
}

/// `<state>/daemon/moss.json`: outside Moss's home, which Moss may write.
pub fn status_path(state: &Path) -> PathBuf {
    crate::daemon::Paths::new(state).dir.join("moss.json")
}

/// What the daemon says about the Moss it looks after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub schema: u32,
    /// When this Moss was started, Unix milliseconds.
    pub started_ms: i64,
    /// Restarts after it died, since it was switched on.
    pub restarts: u32,
    /// The Claude Code session id it was started with.
    pub session_id: String,
}

pub fn read(path: &Path) -> Option<Stored> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Replace the file in one step.
pub fn write(path: &Path, stored: &Stored) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(stored).map_err(io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    drop(file);
    std::fs::rename(&tmp, path)
}

/// `<state>/daemon/moss-error.txt`: why the last start failed, until one
/// succeeds or Moss is stopped.
pub fn error_path(state: &Path) -> PathBuf {
    crate::daemon::Paths::new(state).dir.join("moss-error.txt")
}

pub fn write_error(state: &Path, message: &str) -> io::Result<()> {
    let text: String = message
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(300)
        .collect();
    crate::moss::home::write_if_changed(&error_path(state), &format!("{text}\n")).map(|_| ())
}

pub fn read_error(state: &Path) -> Option<String> {
    std::fs::read_to_string(error_path(state))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub fn clear_error(state: &Path) {
    let _ = remove(&error_path(state));
}

pub fn remove(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// `moss status --json` (schema 1). A contract: the app decodes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub schema: u32,
    pub enabled: bool,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    pub restarts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Whether the cc-moss daemon that starts Moss runs.
    pub daemon_running: bool,
    /// Why the last start failed, while Moss is on and not running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// `since` in Unix milliseconds.
    #[serde(skip)]
    pub since_ms: Option<i64>,
}

impl Report {
    /// From the switch, whether it runs and what the daemon recorded.
    pub fn new(
        enabled: bool,
        running: bool,
        stored: Option<&Stored>,
        daemon_running: bool,
        last_error: Option<String>,
    ) -> Report {
        let stored = stored.filter(|_| running);
        Report {
            daemon_running,
            last_error: last_error.filter(|_| enabled && !running),
            schema: 1,
            enabled,
            running,
            since: stored.map(|s| crate::slot::rfc3339(s.started_ms)),
            restarts: stored.map_or(0, |s| s.restarts),
            session_id: stored.map(|s| s.session_id.clone()),
            since_ms: stored.map(|s| s.started_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("moss.json");
        assert_eq!(read(&path), None);
        let s = Stored {
            schema: 1,
            started_ms: 1_000,
            restarts: 2,
            session_id: "abc".into(),
        };
        write(&path, &s).unwrap();
        assert_eq!(read(&path), Some(s.clone()));
        let r = Report::new(true, true, Some(&s), true, Some("old".into()));
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({"schema":1,"enabled":true,"running":true,
                "since":"1970-01-01T00:00:01Z","restarts":2,"session_id":"abc",
                "daemon_running":true})
        );
        let down = Report::new(false, false, Some(&s), false, Some("old".into()));
        assert_eq!(
            serde_json::to_value(&down).unwrap(),
            serde_json::json!({"schema":1,"enabled":false,"running":false,"restarts":0,
                "daemon_running":false})
        );
        let failing = Report::new(true, false, None, true, Some("no claude".into()));
        assert_eq!(
            serde_json::to_value(&failing).unwrap()["last_error"],
            "no claude"
        );
        remove(&path).unwrap();
        remove(&path).unwrap();
    }

    #[test]
    fn start_errors_are_kept_until_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("daemon")).unwrap();
        assert_eq!(read_error(dir.path()), None);
        write_error(dir.path(), "claude is not on the PATH\nmore").unwrap();
        assert_eq!(
            read_error(dir.path()).as_deref(),
            Some("claude is not on the PATH")
        );
        clear_error(dir.path());
        assert_eq!(read_error(dir.path()), None);
    }

    #[test]
    fn files_live_under_state() {
        let s = Path::new("/s");
        assert_eq!(home_dir(s), PathBuf::from("/s/moss"));
        assert_eq!(status_path(s), PathBuf::from("/s/daemon/moss.json"));
    }
}
