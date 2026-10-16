//! What the picker lists: tmux sessions joined with the Claude Code
//! registry, and interrupted sessions.

use moss_core::slot::{Slot, Status};
use moss_core::tmux::SessionInfo;

/// A Claude Code session as the picker shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRow {
    pub slot_id: String,
    pub session_id: String,
    pub name: Option<String>,
    pub note: Option<String>,
    pub cwd: String,
    pub last_active_ms: i64,
    /// Claude is working on a prompt right now.
    pub busy: bool,
    /// First line of the last prompt.
    pub preview: Option<String>,
}

impl SlotRow {
    pub fn from_slot(s: &Slot) -> Self {
        SlotRow {
            slot_id: s.slot_id.clone(),
            session_id: s.session_id.clone(),
            name: s.name.clone(),
            note: s.notes.state.clone().or_else(|| s.notes.note.clone()),
            cwd: s.cwd.clone(),
            last_active_ms: s.last_active_at_ms,
            busy: s.busy,
            // Older versions stored markup-only lines; tidy them on the way out.
            preview: s
                .last_prompt
                .as_deref()
                .and_then(moss_core::slot::prompt_preview),
        }
    }

    pub fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.session_id.chars().take(8).collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub tmux_id: String,
    pub tmux_name: String,
    pub attached: bool,
    pub activity_ms: i64,
    /// Start directory of the tmux session.
    pub path: String,
    /// The Claude Code session running in it, if any.
    pub claude: Option<SlotRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Start a new session here (named by the query, if any).
    New,
    Session(SessionRow),
    Interrupted(SlotRow),
}

impl Item {
    /// The text the fuzzy matcher sees: names, note, last message and the
    /// folder's own name. Never the full path: fuzzy matching a long path
    /// matches almost anything.
    pub fn search_text(&self) -> String {
        let folder = |p: &str| {
            p.rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("")
                .to_string()
        };
        let join = |parts: &[Option<&str>]| {
            parts
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
        };
        match self {
            Item::New => String::new(),
            Item::Session(s) => match &s.claude {
                Some(c) => join(&[
                    Some(&c.label()),
                    Some(&s.tmux_name),
                    c.note.as_deref(),
                    c.preview.as_deref(),
                    Some(&folder(&c.cwd)),
                ]),
                None => join(&[Some(&s.tmux_name), Some(&folder(&s.path))]),
            },
            Item::Interrupted(r) => join(&[
                Some(&r.label()),
                r.note.as_deref(),
                r.preview.as_deref(),
                Some(&folder(&r.cwd)),
            ]),
        }
    }

    /// The directory the item lives in.
    pub fn dir(&self) -> Option<&str> {
        match self {
            Item::New => None,
            Item::Session(s) => Some(
                s.claude
                    .as_ref()
                    .map_or(s.path.as_str(), |c| c.cwd.as_str()),
            ),
            Item::Interrupted(r) => Some(&r.cwd),
        }
    }

    /// The name shown on the first line.
    pub fn title(&self) -> String {
        match self {
            Item::New => tr!("New session", "新会话").into(),
            Item::Session(s) => s
                .claude
                .as_ref()
                .map_or_else(|| s.tmux_name.clone(), |c| c.label()),
            Item::Interrupted(r) => r.label(),
        }
    }
}

/// tmux sessions, most recently active first, each joined with the live
/// Claude Code slot recorded in it. `socket_path` is the server's socket, so
/// slots from another tmux server never match by accident.
pub fn sessions(
    tmux: &[SessionInfo],
    socket_path: Option<&str>,
    rows: &[(Status, Slot)],
) -> Vec<Item> {
    let mut out: Vec<SessionRow> = tmux
        .iter()
        .map(|t| {
            let claude = rows
                .iter()
                .filter(|(st, _)| *st == Status::Running)
                .map(|(_, s)| s)
                .find(|s| {
                    s.tmux.as_ref().is_some_and(|loc| {
                        loc.session_id.as_deref() == Some(t.id.as_str())
                            && socket_path.is_none_or(|p| p == loc.socket)
                    })
                })
                .map(SlotRow::from_slot);
            let activity_ms = claude
                .as_ref()
                .map_or(t.activity_ms, |c| c.last_active_ms.max(t.activity_ms));
            SessionRow {
                tmux_id: t.id.clone(),
                tmux_name: t.name.clone(),
                attached: t.attached,
                activity_ms,
                path: t.path.clone(),
                claude,
            }
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.activity_ms));
    out.into_iter().map(Item::Session).collect()
}

/// Interrupted sessions, most recently active first.
pub fn interrupted(rows: &[(Status, Slot)]) -> Vec<Item> {
    let mut out: Vec<SlotRow> = rows
        .iter()
        .filter(|(st, _)| *st == Status::Interrupted)
        .map(|(_, s)| SlotRow::from_slot(s))
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.last_active_ms));
    out.into_iter().map(Item::Interrupted).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::event::TmuxLoc;
    use moss_core::slot::Notes;

    fn slot(name: &str, tmux_id: &str, socket: &str) -> Slot {
        Slot {
            slot_id: format!("slot-{name}"),
            agent: "claude".into(),
            session_id: format!("{name}-id"),
            session_ids: vec![],
            name: Some(name.into()),
            cwd: "/w".into(),
            pid: Some(1),
            pid_start: None,
            tmux: Some(TmuxLoc {
                socket: socket.into(),
                session_id: Some(tmux_id.into()),
                session_name: Some(format!("h_{name}")),
                pane_id: "%0".into(),
            }),
            status: Status::Running,
            end_reason: None,
            notes: Notes {
                state: Some("waiting".into()),
                ..Notes::default()
            },
            launch_args: vec![],
            transcript_path: None,
            restore_started_at_ms: None,
            busy: false,
            last_prompt: None,
            created_at_ms: 0,
            last_active_at_ms: 5_000,
            ended_at_ms: None,
        }
    }

    fn info(id: &str, name: &str, activity_ms: i64) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            name: name.into(),
            activity_ms,
            attached: false,
            path: "/w".into(),
        }
    }

    #[test]
    fn sessions_merge_tmux_and_slots() {
        let tmux = [info("$0", "h_api", 1_000), info("$1", "scratch", 9_000)];
        let rows = vec![
            (Status::Running, slot("api", "$0", "/sock")),
            // Same tmux id on another server: must not match.
            (Status::Running, slot("other", "$1", "/elsewhere")),
        ];
        let items = sessions(&tmux, Some("/sock"), &rows);
        let rows: Vec<&SessionRow> = items
            .iter()
            .map(|i| match i {
                Item::Session(s) => s,
                _ => panic!("{i:?}"),
            })
            .collect();
        // Most recent first: scratch (9s) before api (max(1s, 5s)).
        assert_eq!(rows[0].tmux_name, "scratch");
        assert!(
            rows[0].claude.is_none(),
            "plain tmux session listed without Claude info"
        );
        assert_eq!(rows[1].claude.as_ref().unwrap().label(), "api");
        assert_eq!(
            rows[1].claude.as_ref().unwrap().note.as_deref(),
            Some("waiting")
        );
    }

    #[test]
    fn interrupted_lists_only_interrupted() {
        let rows = vec![
            (Status::Interrupted, slot("a", "$0", "/s")),
            (Status::Running, slot("b", "$1", "/s")),
            (Status::Closed, slot("c", "$2", "/s")),
        ];
        let items = interrupted(&rows);
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], Item::Interrupted(r) if r.label() == "a"));
    }
}
