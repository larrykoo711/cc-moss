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
    /// Between a prompt and the end of Claude's reply.
    pub busy: bool,
    /// Why the session waits for the user (`permission`, `question`,
    /// `input`), set by a Notification and cleared once it moves on.
    pub attention: Option<String>,
    /// First line of the last prompt, for previews.
    pub last_prompt: Option<String>,
    /// When `cc-moss restore` last started this slot; cleared by SessionStart.
    pub restore_started_at_ms: Option<i64>,
    /// What cc-moss runs this session as: `moss` for Moss, else none.
    pub role: Option<String>,
    pub created_at_ms: i64,
    pub last_active_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    /// When it was put away in the archive (out of the list, still
    /// restorable); none while it is in the list.
    pub archived_at_ms: Option<i64>,
    /// Who archived it: `auto` (the daily tidy-up), `you`, or `moss`.
    pub archived_by: Option<String>,
    /// The API provider it started with ([`crate::provider::Snapshot`] as
    /// JSON: key names, host, models; never a key's value).
    pub provider_started: Option<String>,
    /// Provider key names seen in the settings while it ran (JSON array).
    pub provider_seen_keys: Option<String>,
}

impl Slot {
    /// Whether the user ended it on purpose (`/exit`, closed from cc-moss,
    /// replaced by another copy): a session brought back from the archive
    /// shows again, but restoring everything leaves it out.
    pub fn ended_on_purpose(&self) -> bool {
        matches!(
            self.end_reason.as_deref(),
            Some(
                "closed-by-user"
                    | "prompt_input_exit"
                    | "logout"
                    | "resume"
                    | "switched"
                    | "superseded"
            )
        )
    }

    /// Whether it is in the archive.
    pub fn is_archived(&self) -> bool {
        self.archived_at_ms.is_some()
    }

    /// Whether this is Moss, the session cc-moss looks after itself.
    pub fn is_moss(&self) -> bool {
        self.role.as_deref() == Some(crate::moss::ROLE)
    }

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

/// What the picker shows as a session's last message: the first line that
/// has words in it, at most 200 characters. Claude Code wraps some prompts in
/// its own markup (messages from other sessions, slash commands); those tags
/// are taken off so the words show, and a line that is only a tag is skipped.
pub fn prompt_preview(prompt: &str) -> Option<String> {
    if is_notice(prompt) {
        return None;
    }
    prompt
        .lines()
        .map(strip_markup)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(200).collect())
}

/// Whether `prompt` is a notice Claude Code feeds the session itself (a
/// background task finished), not words you typed: its first line once the
/// tags are off is only the task's id.
pub fn is_notice(prompt: &str) -> bool {
    prompt.trim_start().starts_with("<task-notification>")
}

/// `line` without a Claude Code tag at its start or end. Those tags have a
/// hyphen in their name (`cross-session-message`, `command-name`); plain
/// HTML like `<div>` does not, so a question about HTML stays as written.
fn strip_markup(line: &str) -> &str {
    let mut s = line.trim();
    if let Some(end) = s.find('>').filter(|_| s.starts_with('<')) {
        if is_markup_tag(&s[1..end]) {
            s = s[end + 1..].trim_start();
        }
    }
    if let Some(start) = s.rfind("</").filter(|_| s.ends_with('>')) {
        if is_markup_tag(&s[start + 1..s.len() - 1]) {
            s = s[..start].trim_end();
        }
    }
    s
}

/// `name attr="…"` or `/name` where the name looks like Claude Code's markup.
fn is_markup_tag(inside: &str) -> bool {
    let name = inside.trim_start_matches('/');
    let name = name.split_whitespace().next().unwrap_or_default();
    name.contains('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}
