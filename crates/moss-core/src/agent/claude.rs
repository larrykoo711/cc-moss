//! Claude Code adapter. Field names verified against Claude Code 2.1.300
//! (see `.sdd/001-cli/notes/hook-probe.md`) and, for Notification and
//! PostToolUse, 2.1.301 (`.sdd/002-menubar/notes/hook-probe-2.md`).

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::{Agent, HookKind};
use crate::event::{AttentionReason, EndReason, Event, StartSource};

pub struct ClaudeAgent;

#[derive(Deserialize)]
struct Payload {
    session_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    transcript_path: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    session_title: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    notification_type: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Notification types cc-moss knows and leaves alone (idle reminders,
/// completions, account notices).
const IGNORED_NOTIFICATIONS: &[&str] = &["idle_prompt", "agent_completed", "auth_success"];

impl Agent for ClaudeAgent {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn parse_hook(&self, kind: HookKind, payload: &str) -> Result<Option<Event>> {
        let p: Payload = serde_json::from_str(payload).context("hook payload is not valid JSON")?;
        if p.session_id.is_empty() {
            bail!("hook payload has an empty session_id");
        }
        Ok(Some(match kind {
            HookKind::SessionStart => Event::Start {
                source: match p.source.as_deref() {
                    Some("resume") => StartSource::Resume,
                    Some("clear") => StartSource::Clear,
                    Some("compact") => StartSource::Compact,
                    _ => StartSource::Startup,
                },
                session_id: p.session_id,
                cwd: p.cwd.unwrap_or_default(),
                title: p.session_title,
                transcript_path: p.transcript_path,
            },
            HookKind::UserPromptSubmit => Event::Prompt {
                session_id: p.session_id,
                title: p.session_title,
                prompt: p.prompt,
            },
            HookKind::Stop => Event::Stop {
                session_id: p.session_id,
            },
            HookKind::SessionEnd => Event::End {
                session_id: p.session_id,
                reason: EndReason::parse(p.reason.as_deref().unwrap_or("")),
            },
            // Questions waiting for an answer are kept by `crate::ask`, not
            // the registry.
            HookKind::PreToolUse | HookKind::PermissionRequest => return Ok(None),
            HookKind::PostToolUse => Event::ToolDone {
                session_id: p.session_id,
            },
            HookKind::Notification => {
                // 2.1.301 reports AskUserQuestion as `permission_prompt` too
                // (`.sdd/002-menubar/notes/hook-probe-2.md` N3).
                let reason = match p.notification_type.as_deref().unwrap_or("") {
                    "permission_prompt" => AttentionReason::Permission,
                    "elicitation_dialog" | "elicitation_url_dialog" => AttentionReason::Question,
                    "agent_needs_input" => AttentionReason::Input,
                    t if IGNORED_NOTIFICATIONS.contains(&t) => return Ok(None),
                    other => bail!("ignored unknown notification type `{other}`"),
                };
                Event::Attention {
                    session_id: p.session_id,
                    reason,
                    message: p.message.filter(|m| !m.is_empty()),
                }
            }
        }))
    }
}

/// A running Claude Code session, as `claude agents --json` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub pid: u32,
    pub session_id: String,
    /// A name the user chose; Claude Code's automatic names are left out.
    pub name: Option<String>,
    pub cwd: String,
    pub busy: bool,
    /// Run by Claude Code's own background supervisor rather than in a
    /// terminal: `/resume` and `claude agents` start sessions there (2.1.314).
    pub background: bool,
}

/// Sessions with a pid from `claude agents --json` output: interactive ones
/// and background ones. A background entry without a pid is a finished job
/// Claude Code keeps listed, not a running process.
pub fn parse_agents(json: &str) -> Vec<LiveSession> {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str::<serde_json::Value>(json)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|e| {
            let background = match e.get("kind")?.as_str()? {
                "interactive" => false,
                "background" => true,
                _ => return None,
            };
            let cwd = e.get("cwd")?.as_str()?.to_string();
            let name = e
                .get("name")
                .and_then(|n| n.as_str())
                .filter(|n| !n.is_empty() && !is_auto_name(n, &cwd))
                .map(String::from);
            Some(LiveSession {
                pid: u32::try_from(e.get("pid")?.as_u64()?).ok()?,
                session_id: e.get("sessionId")?.as_str()?.to_string(),
                name,
                busy: e.get("status").and_then(|s| s.as_str()) == Some("busy"),
                cwd,
                background,
            })
        })
        .collect()
}

/// Claude Code names unnamed sessions `<folder>-<two hex digits>`. Such a
/// name says nothing the folder does not, and must not overwrite a tmux
/// session name the user chose.
pub fn is_auto_name(name: &str, cwd: &str) -> bool {
    // Claude Code lowercases the folder: `T` becomes `t-89`.
    let folder = cwd.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    name.get(..folder.len())
        .filter(|head| head.eq_ignore_ascii_case(folder))
        .and_then(|_| name[folder.len()..].strip_prefix('-'))
        .is_some_and(|suffix| suffix.len() == 2 && suffix.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Running sessions, from `<claude> agents --json`. None when the command
/// is missing, fails or is slow: nothing is known then.
pub fn live_sessions(claude_bin: &str) -> Option<Vec<LiveSession>> {
    let out = crate::command::output_with_timeout(
        std::process::Command::new(claude_bin).args(["agents", "--json"]),
        std::time::Duration::from_secs(3),
    )?;
    out.status
        .success()
        .then(|| parse_agents(&String::from_utf8_lossy(&out.stdout)))
}

/// Claude Code's config dir: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
fn config_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".claude")))
}

/// What Claude Code records about one of its running processes, in
/// `<config dir>/sessions/<pid>.json` (fields verified against 2.1.314).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// The background job this process runs, for `claude attach`.
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub parked_job_id: Option<String>,
    #[serde(default)]
    proc_start: Option<String>,
}

impl ProcessEntry {
    /// Run by Claude Code's background supervisor, not in a terminal.
    pub fn is_background(&self) -> bool {
        self.kind.as_deref() == Some("bg")
    }

    /// A terminal whose `/resume` handed its own conversation to a
    /// background job: it now only shows a session that runs elsewhere.
    pub fn parked(&self) -> bool {
        self.parked_job_id.as_deref().is_some_and(|j| !j.is_empty())
    }
}

/// Claude Code's record of process `pid`, when the file is that very
/// process's: it names the pid and, where both start times can be compared,
/// the same start (a file a dead process left behind says nothing about a
/// new process given its pid).
pub fn process_entry(pid: u32, start: Option<&str>) -> Option<ProcessEntry> {
    let path = config_dir()?.join("sessions").join(format!("{pid}.json"));
    entry_from(&std::fs::read(path).ok()?, pid, start)
}

fn entry_from(bytes: &[u8], pid: u32, start: Option<&str>) -> Option<ProcessEntry> {
    let entry: ProcessEntry = serde_json::from_slice(bytes).ok()?;
    let same_start = match (start, entry.proc_start.as_deref()) {
        // `ps` pads single-digit days; Claude Code may not.
        (Some(ours), Some(theirs)) if !ours.starts_with("proc:") => {
            ours.split_whitespace().eq(theirs.split_whitespace())
        }
        _ => true,
    };
    (entry.pid == pid && same_start).then_some(entry)
}

/// Whether process `pid` still runs a conversation itself: alive, and not a
/// terminal that handed its conversation to a background job.
pub fn runs_a_session(pid: u32, start: Option<&str>) -> bool {
    crate::proc::is_alive(pid, start) && !process_entry(pid, start).is_some_and(|e| e.parked())
}

/// Which of `wanted` a live Claude Code process runs right now, by Claude
/// Code's own records: interactive ones and background ones, terminals that
/// handed theirs away left out.
pub fn running_among(
    wanted: &std::collections::HashSet<&str>,
) -> std::collections::HashSet<String> {
    let Some(files) = config_dir().and_then(|c| std::fs::read_dir(c.join("sessions")).ok()) else {
        return Default::default();
    };
    files
        .flatten()
        .filter_map(|f| {
            let name = f.file_name();
            let pid = name.to_str()?.strip_suffix(".json")?.parse::<u32>().ok()?;
            let bytes = std::fs::read(f.path()).ok()?;
            // Only the files that matter cost a process lookup.
            let id = entry_from(&bytes, pid, None)?.session_id?;
            if !wanted.contains(id.as_str()) {
                return None;
            }
            let start = crate::proc::start_time(pid)?;
            let entry = entry_from(&bytes, pid, Some(&start))?;
            (!entry.parked()).then_some(id)
        })
        .collect()
}

/// Where Claude Code keeps a session's transcript:
/// `<config dir>/projects/<any>/<session id>.jsonl`.
pub fn transcript_path(session_id: &str) -> Option<String> {
    let config = config_dir()?;
    let file = format!("{session_id}.jsonl");
    std::fs::read_dir(config.join("projects"))
        .ok()?
        .flatten()
        .map(|d| d.path().join(&file))
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BG: &str = r#"{"pid":66788,"sessionId":"a85b","kind":"bg","jobId":"99a04088",
        "procStart":"Fri Oct 30 03:53:58 2026","status":"busy"}"#;

    #[test]
    fn a_process_record_speaks_for_its_own_process_only() {
        let start = Some("Fri Oct 30 03:53:58 2026");
        let e = entry_from(BG.as_bytes(), 66788, start).unwrap();
        assert!(e.is_background());
        assert!(!e.parked());
        assert_eq!(e.job_id.as_deref(), Some("99a04088"));
        assert_eq!(e.session_id.as_deref(), Some("a85b"));
        assert!(entry_from(BG.as_bytes(), 1, start).is_none(), "another pid");
        assert!(
            entry_from(BG.as_bytes(), 66788, Some("Sat Oct 31 03:53:58 2026")).is_none(),
            "a process that reused the pid"
        );
        assert!(
            entry_from(BG.as_bytes(), 66788, None).is_some(),
            "start unknown"
        );
        assert!(
            entry_from(BG.as_bytes(), 66788, Some("proc:12345")).is_some(),
            "a /proc start time cannot be compared"
        );
        assert!(entry_from(b"not json", 66788, start).is_none());
    }

    #[test]
    fn start_times_compare_whatever_the_padding() {
        let padded = br#"{"pid":7,"kind":"bg","procStart":"Sat Oct  3 01:02:03 2026"}"#;
        assert!(entry_from(padded, 7, Some("Sat Oct 3 01:02:03 2026")).is_some());
    }

    #[test]
    fn a_terminal_that_handed_its_conversation_away_is_parked() {
        let parked =
            br#"{"pid":15646,"sessionId":"9558","kind":"interactive","parkedJobId":"585fd743"}"#;
        let e = entry_from(parked, 15646, None).unwrap();
        assert!(e.parked());
        assert!(!e.is_background());
        let plain = br#"{"pid":15646,"kind":"interactive","parkedJobId":null}"#;
        assert!(!entry_from(plain, 15646, None).unwrap().parked());
    }
}
