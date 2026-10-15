//! Agent adapters turn an agent's hook payloads into neutral events.

pub mod claude;

use anyhow::Result;

use crate::event::Event;

/// The hook entry points cc-moss registers with an agent.
/// These strings are a protocol literal: `cc-moss setup` writes them into the
/// agent's settings (see T1.2) and `cc-moss hook <event>` parses them back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    SessionStart,
    UserPromptSubmit,
    Stop,
    SessionEnd,
}

impl HookKind {
    pub const ALL: [HookKind; 4] = [
        HookKind::SessionStart,
        HookKind::UserPromptSubmit,
        HookKind::Stop,
        HookKind::SessionEnd,
    ];

    pub fn cli_name(self) -> &'static str {
        match self {
            HookKind::SessionStart => "session-start",
            HookKind::UserPromptSubmit => "user-prompt-submit",
            HookKind::Stop => "stop",
            HookKind::SessionEnd => "session-end",
        }
    }

    pub fn from_cli_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.cli_name() == name)
    }
}

pub trait Agent {
    /// Stable identifier stored in the registry, e.g. `"claude"`.
    fn name(&self) -> &'static str;

    /// Parse one hook payload (the JSON the agent writes to the hook's stdin).
    fn parse_hook(&self, kind: HookKind, payload: &str) -> Result<Event>;
}
