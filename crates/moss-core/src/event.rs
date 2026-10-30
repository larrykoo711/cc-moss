//! Agent-neutral session events.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartSource {
    Startup,
    Resume,
    Clear,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    /// The user ended the session on purpose (`/exit`, logout).
    Closed(String),
    /// `/clear`: a new session id follows in the same process.
    Clear,
    /// Anything else: signals, crashes, unknown reasons.
    Abnormal(String),
}

impl EndReason {
    /// Map Claude Code's `SessionEnd.reason`. Only reasons the user chose count
    /// as closed; everything else, including values we have never seen, is
    /// treated as an interruption so the session stays recoverable.
    pub fn parse(raw: &str) -> Self {
        match raw {
            // `resume`: the user switched this process to another conversation (F11).
            "prompt_input_exit" | "logout" | "resume" => EndReason::Closed(raw.to_string()),
            "clear" => EndReason::Clear,
            other => EndReason::Abnormal(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Start {
        source: StartSource,
        session_id: String,
        cwd: String,
        title: Option<String>,
        transcript_path: Option<String>,
    },
    Prompt {
        session_id: String,
        title: Option<String>,
        /// What the user typed (UserPromptSubmit's `prompt`).
        prompt: Option<String>,
    },
    Stop {
        session_id: String,
    },
    End {
        session_id: String,
        reason: EndReason,
    },
    /// The session waits for the user: a permission prompt, a question, input.
    Attention {
        session_id: String,
        reason: AttentionReason,
        /// The agent's own words for it, if any.
        message: Option<String>,
    },
    /// A tool finished: whatever the session waited on was answered.
    ToolDone {
        session_id: String,
    },
}

impl Event {
    /// The session the event is about.
    pub fn session_id(&self) -> &str {
        match self {
            Event::Start { session_id, .. }
            | Event::Prompt { session_id, .. }
            | Event::Stop { session_id }
            | Event::End { session_id, .. }
            | Event::Attention { session_id, .. }
            | Event::ToolDone { session_id } => session_id,
        }
    }
}

/// Why a session waits for the user. Stored as `slots.attention` and shown
/// as `ls --json`'s `attention`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionReason {
    Permission,
    Question,
    Input,
}

impl AttentionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            AttentionReason::Permission => "permission",
            AttentionReason::Question => "question",
            AttentionReason::Input => "input",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxLoc {
    pub socket: String,
    pub session_id: Option<String>,
    pub session_name: Option<String>,
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub pid: Option<u32>,
    pub pid_start: Option<String>,
    pub tmux: Option<TmuxLoc>,
    /// Whitelisted command-line flags, captured on SessionStart only.
    pub launch_args: Option<Vec<String>>,
    pub now_ms: i64,
}
