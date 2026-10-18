//! `<state>/daemon/status.json`: what the running daemon says about itself,
//! and the report `cc-moss daemon status --json` prints from it.
//!
//! The file can outlive the daemon (`kill -9`), so it is never believed on
//! its own: the recorded pid must still be the same process.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::proc::Liveness;
use crate::slot::rfc3339;

/// What the daemon writes once it holds the lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub schema: u32,
    pub pid: u32,
    /// The process start time, to tell a reused pid from the daemon.
    pub pid_start: Option<String>,
    pub version: String,
    pub started_ms: i64,
    pub heartbeat_ms: i64,
}

/// `daemon status --json` (schema 1). A contract: the app decodes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub schema: u32,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_at: Option<String>,
    /// `started_at` in Unix milliseconds, for working out the uptime.
    #[serde(skip)]
    pub started_ms: Option<i64>,
}

impl Report {
    pub fn not_running() -> Report {
        Report {
            schema: 1,
            running: false,
            pid: None,
            version: None,
            started_at: None,
            heartbeat_at: None,
            started_ms: None,
        }
    }
}

/// The stored status, or `None` when there is none or it cannot be read
/// (half-written by an older version, say): either way, nothing to believe.
pub fn read(path: &Path) -> Option<Stored> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Replace the file in one step, so a reader never sees half of it.
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

/// Remove the file if it is still `pid`'s: a daemon on its way out never
/// deletes what a newer one wrote.
pub fn remove_if_ours(path: &Path, pid: u32) -> io::Result<()> {
    match read(path) {
        Some(s) if s.pid == pid => std::fs::remove_file(path),
        _ => Ok(()),
    }
}

/// The report for `stored`. A pid that is gone (or now belongs to another
/// process) is not running; one whose liveness cannot be told counts as
/// running: calling a live daemon dead would start a second one.
pub fn report(stored: Option<&Stored>, liveness: impl Fn(u32, Option<&str>) -> Liveness) -> Report {
    let Some(s) = stored else {
        return Report::not_running();
    };
    if liveness(s.pid, s.pid_start.as_deref()) == Liveness::Dead {
        return Report::not_running();
    }
    Report {
        schema: 1,
        running: true,
        pid: Some(s.pid),
        version: Some(s.version.clone()),
        started_at: Some(rfc3339(s.started_ms)),
        heartbeat_at: Some(rfc3339(s.heartbeat_ms)),
        started_ms: Some(s.started_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Stored {
        Stored {
            schema: 1,
            pid: 4242,
            pid_start: Some("Sat Oct 18 08:00:00 2026".into()),
            version: "0.2.2".into(),
            started_ms: 1_792_310_400_000,
            heartbeat_ms: 1_792_310_430_000,
        }
    }

    #[test]
    fn report_running_carries_the_details() {
        let r = report(Some(&stored()), |_, _| Liveness::Alive);
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["running"], true);
        assert_eq!(json["pid"], 4242);
        assert_eq!(json["version"], "0.2.2");
        assert_eq!(json["started_at"], rfc3339(1_792_310_400_000));
        assert_eq!(json["heartbeat_at"], rfc3339(1_792_310_430_000));
    }

    #[test]
    fn report_without_a_file_is_not_running() {
        let json = serde_json::to_value(report(None, |_, _| Liveness::Alive)).unwrap();
        assert_eq!(json, serde_json::json!({"schema": 1, "running": false}));
    }

    #[test]
    fn report_for_a_gone_pid_is_not_running() {
        let r = report(Some(&stored()), |_, _| Liveness::Dead);
        assert_eq!(r, Report::not_running());
    }

    #[test]
    fn report_for_an_unknown_pid_counts_as_running() {
        assert!(report(Some(&stored()), |_, _| Liveness::Unknown).running);
    }

    #[test]
    fn report_checks_the_recorded_start_time() {
        let seen = std::cell::RefCell::new(None);
        report(Some(&stored()), |pid, start| {
            *seen.borrow_mut() = Some((pid, start.map(str::to_string)));
            Liveness::Alive
        });
        assert_eq!(
            seen.into_inner(),
            Some((4242, Some("Sat Oct 18 08:00:00 2026".to_string())))
        );
    }

    #[test]
    fn write_then_read_round_trips_and_remove_spares_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        assert_eq!(read(&path), None);
        write(&path, &stored()).unwrap();
        assert_eq!(read(&path), Some(stored()));
        remove_if_ours(&path, 1).unwrap();
        assert!(path.exists(), "another pid's file stays");
        remove_if_ours(&path, 4242).unwrap();
        assert!(!path.exists());
        std::fs::write(&path, "{half").unwrap();
        assert_eq!(read(&path), None);
    }
}
