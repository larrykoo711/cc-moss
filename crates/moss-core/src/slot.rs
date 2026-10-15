//! A slot is one agent process lifetime; its session id may rotate.

use crate::event::TmuxLoc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Closed,
    Interrupted,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Closed => "closed",
            Status::Interrupted => "interrupted",
        }
    }

    /// Unknown values read back as interrupted: recoverable is the safe default.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "running" => Status::Running,
            "closed" => Status::Closed,
            _ => Status::Interrupted,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    pub goal: Option<String>,
    pub state: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub slot_id: String,
    pub agent: String,
    pub session_id: String,
    pub session_ids: Vec<String>,
    pub name: Option<String>,
    pub cwd: String,
    pub pid: Option<u32>,
    pub pid_start: Option<String>,
    pub tmux: Option<TmuxLoc>,
    pub status: Status,
    pub end_reason: Option<String>,
    pub notes: Notes,
    pub launch_args: Vec<String>,
    /// Transcript of the current session id, as reported by the agent.
    pub transcript_path: Option<String>,
    /// When `cc-moss restore` last started this slot; cleared by SessionStart.
    pub restore_started_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub last_active_at_ms: i64,
    pub ended_at_ms: Option<i64>,
}

impl Slot {
    /// The status to show: a slot whose process is gone (SIGKILL, power loss)
    /// is an interruption, and one whose process still runs is running. Without a pid we cannot prove
    /// the process died, so it stays running: restoring a live session would
    /// start a duplicate. `is_alive` is injected so the rule is testable.
    pub fn effective_status(&self, is_alive: impl Fn(u32, Option<&str>) -> bool) -> Status {
        self.effective_status_since(is_alive, crate::proc::boot_time_ms())
    }

    /// [`Slot::effective_status`] with an explicit boot time. A pid recorded
    /// before the machine booted belongs to a process that cannot exist any
    /// more, whatever now runs under that number.
    pub fn effective_status_since(
        &self,
        is_alive: impl Fn(u32, Option<&str>) -> bool,
        boot_ms: Option<i64>,
    ) -> Status {
        let before_boot = boot_ms.is_some_and(|b| self.last_active_at_ms < b);
        match (self.status, self.pid) {
            (Status::Running | Status::Interrupted, Some(_)) if before_boot => Status::Interrupted,
            // Not closed and a pid on record: the process decides, whatever the
            // last event said (a SessionEnd(other) process may have survived).
            (Status::Running | Status::Interrupted, Some(pid)) => {
                if is_alive(pid, self.pid_start.as_deref()) {
                    Status::Running
                } else {
                    Status::Interrupted
                }
            }
            (status, _) => status,
        }
    }
}

/// Milliseconds since the Unix epoch as RFC 3339 UTC.
pub fn rfc3339(ms: i64) -> String {
    let t = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub fn now_ms() -> i64 {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}
