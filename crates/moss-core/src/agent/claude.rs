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
