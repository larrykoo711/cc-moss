//! Claude Code adapter. Field names verified against Claude Code 2.1.300
//! (see `.sdd/001-cli/notes/hook-probe.md`).

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::{Agent, HookKind};
use crate::event::{EndReason, Event, StartSource};

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
}

impl Agent for ClaudeAgent {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn parse_hook(&self, kind: HookKind, payload: &str) -> Result<Event> {
        let p: Payload = serde_json::from_str(payload).context("hook payload is not valid JSON")?;
        if p.session_id.is_empty() {
            bail!("hook payload has an empty session_id");
        }
        Ok(match kind {
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
        })
    }
}

/// A running interactive Claude Code session, as `claude agents --json` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub pid: u32,
    pub session_id: String,
    /// A name the user chose; Claude Code's automatic names are left out.
    pub name: Option<String>,
    pub cwd: String,
    pub busy: bool,
}

/// Interactive sessions with a pid from `claude agents --json` output.
pub fn parse_agents(json: &str) -> Vec<LiveSession> {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str::<serde_json::Value>(json)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|e| e.get("kind").and_then(|k| k.as_str()) == Some("interactive"))
        .filter_map(|e| {
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
            })
        })
        .collect()
}

/// Claude Code names unnamed sessions `<folder>-<two hex digits>`. Such a
/// name says nothing the folder does not, and must not overwrite a tmux
/// session name the user chose.
pub fn is_auto_name(name: &str, cwd: &str) -> bool {
    let folder = cwd.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    name.strip_prefix(folder)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|suffix| suffix.len() == 2 && suffix.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Running interactive sessions, from `<claude> agents --json`. Empty when
/// the command is missing, fails or is slow.
pub fn live_sessions(claude_bin: &str) -> Vec<LiveSession> {
    let out = crate::command::output_with_timeout(
        std::process::Command::new(claude_bin).args(["agents", "--json"]),
        std::time::Duration::from_secs(3),
    );
    match out {
        Some(o) if o.status.success() => parse_agents(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Where Claude Code keeps a session's transcript:
/// `<config dir>/projects/<any>/<session id>.jsonl`.
pub fn transcript_path(session_id: &str) -> Option<String> {
    let config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".claude"))
        })?;
    let file = format!("{session_id}.jsonl");
    std::fs::read_dir(config.join("projects"))
        .ok()?
        .flatten()
        .map(|d| d.path().join(&file))
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}
