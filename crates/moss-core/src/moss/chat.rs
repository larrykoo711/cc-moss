//! The conversation with Moss: `<state>/moss/chat.jsonl`, one JSON object a
//! line, append only. Writers are `moss say` (your messages) and the daemon
//! (Moss's replies, the session updates it passed on, and what became of
//! each of your messages). `moss chat --json` folds it for the app.

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Past this many lines the daemon starts a new file and keeps one old one.
pub const ROTATE_AT: usize = 2000;

/// `moss chat --json` without `--limit`.
pub const DEFAULT_LIMIT: usize = 200;

pub fn path(home: &Path) -> PathBuf {
    home.join("chat.jsonl")
}

fn old_path(home: &Path) -> PathBuf {
    home.join("chat.jsonl.1")
}

/// Who a message is from.
pub const FROM_YOU: &str = "you";
pub const FROM_MOSS: &str = "moss";
pub const FROM_EVENTS: &str = "events";

/// One line of chat.jsonl: a message, or (with `state`) what became of an
/// earlier message of yours. Unknown fields are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub schema: u32,
    pub id: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fallback: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub silent: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<String>,
    /// `sent` or `failed`, for an earlier message of yours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// A Claude Code command's result: which command (`/usage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// That command is not run from the panel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refused: bool,
}

impl Line {
    pub fn message(id: &str, at: &str, from: &str, text: &str) -> Line {
        Line {
            schema: 1,
            id: id.to_string(),
            at: at.to_string(),
            from: Some(from.to_string()),
            text: Some(text.to_string()),
            ..Line::default()
        }
    }

    pub fn state(id: &str, at: &str, state: &str, error: Option<&str>) -> Line {
        Line {
            schema: 1,
            id: id.to_string(),
            at: at.to_string(),
            state: Some(state.to_string()),
            error: error.map(String::from),
            ..Line::default()
        }
    }
}

/// Add one line, in one write (`O_APPEND`), so writers never interleave.
pub fn append(home: &Path, line: &Line) -> io::Result<()> {
    let mut text = serde_json::to_string(line).map_err(io::Error::other)?;
    text.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path(home))?
        .write_all(text.as_bytes())
}

/// Start a new file once this one is past [`ROTATE_AT`] lines; the old one
/// becomes `chat.jsonl.1` (replacing the one before). True when it did.
pub fn rotate_if_long(home: &Path) -> io::Result<bool> {
    let text = match std::fs::read(path(home)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if text.iter().filter(|b| **b == b'\n').count() <= ROTATE_AT {
        return Ok(false);
    }
    std::fs::rename(path(home), old_path(home))?;
    Ok(true)
}

fn read_lines(file: &Path) -> Vec<Line> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<Line>(l).ok())
        .filter(|l| !l.id.is_empty())
        .collect()
}

/// A message as `moss chat --json` gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shown {
    pub id: String,
    pub at: String,
    pub from: String,
    pub text: String,
    /// Your messages only: queued, sent, failed or replied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    pub fallback: bool,
    pub buttons: Vec<String>,
    /// A Claude Code command's result (its screen, or why it was not run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub refused: bool,
}

/// The last `limit` messages, oldest first, with the state of yours folded
/// in. Silent replies are left out (they still mark yours as replied).
pub fn fold(home: &Path, limit: usize) -> Vec<Shown> {
    let mut lines = read_lines(&path(home));
    let messages = lines.iter().filter(|l| l.state.is_none()).count();
    if messages < limit {
        let mut old = read_lines(&old_path(home));
        old.append(&mut lines);
        lines = old;
    }
    fold_lines(&lines, limit)
}

fn fold_lines(lines: &[Line], limit: usize) -> Vec<Shown> {
    let mut shown: Vec<Shown> = Vec::new();
    let mut replied: Vec<String> = Vec::new();
    let mut states: std::collections::HashMap<String, (String, Option<String>)> =
        std::collections::HashMap::new();
    for l in lines {
        if let Some(state) = &l.state {
            states.insert(l.id.clone(), (state.clone(), l.error.clone()));
            continue;
        }
        let (Some(from), Some(text)) = (&l.from, &l.text) else {
            continue;
        };
        if let Some(to) = &l.reply_to {
            replied.push(to.clone());
        }
        if l.silent || shown.iter().any(|s| s.id == l.id) {
            continue;
        }
        shown.push(Shown {
            id: l.id.clone(),
            at: l.at.clone(),
            from: from.clone(),
            text: text.clone(),
            state: None,
            error: None,
            reply_to: l.reply_to.clone(),
            fallback: l.fallback,
            buttons: l.buttons.clone(),
            command: l.command.clone(),
            refused: l.refused,
        });
    }
    for s in shown.iter_mut().filter(|s| s.from == FROM_YOU) {
        let (state, error) = match states.get(&s.id) {
            _ if replied.contains(&s.id) => ("replied".to_string(), None),
            Some((state, error)) => (state.clone(), error.clone()),
            None => ("queued".to_string(), None),
        };
        s.state = Some(state);
        s.error = error;
    }
    let skip = shown.len().saturating_sub(limit);
    shown.into_iter().skip(skip).collect()
}

/// Your messages that went in and got no reply (state `sent`).
pub fn unanswered(home: &Path) -> Vec<String> {
    fold(home, usize::MAX)
        .into_iter()
        .filter(|m| m.from == FROM_YOU && m.state.as_deref() == Some("sent"))
        .map(|m| m.id)
        .collect()
}

/// When Moss last replied (shown or silent), as written.
pub fn last_reply_at(home: &Path) -> Option<String> {
    let mut lines = read_lines(&path(home));
    if !lines.iter().any(is_reply) {
        lines = read_lines(&old_path(home));
    }
    lines
        .iter()
        .rev()
        .find(|l| is_reply(l))
        .map(|l| l.at.clone())
}

fn is_reply(l: &Line) -> bool {
    l.state.is_none() && l.from.as_deref() == Some(FROM_MOSS)
}

/// The JSON `moss chat --json` prints.
pub fn to_json(messages: &[Shown]) -> Value {
    serde_json::json!({ "schema": 1, "messages": messages })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moss(id: &str, to: &str, text: &str) -> Line {
        Line {
            reply_to: Some(to.into()),
            ..Line::message(id, "t", FROM_MOSS, text)
        }
    }

    #[test]
    fn chat_folds_states_and_replies() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        for l in [
            Line::message("1", "t", FROM_YOU, "hi"),
            Line::message("2", "t", FROM_YOU, "second"),
            Line::message("3", "t", FROM_YOU, "third"),
            Line::message("4", "t", FROM_YOU, "fourth"),
            Line::state("1", "t", "sent", None),
            Line::state("3", "t", "failed", Some("no echo")),
            Line::state("4", "t", "sent", None),
            moss("m1", "1", "hello"),
            Line {
                silent: true,
                ..moss("m4", "4", "shh")
            },
            Line::message("e1", "t", FROM_EVENTS, "[动态] api 断了"),
        ] {
            append(h, &l).unwrap();
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(path(h))
            .unwrap()
            .write_all(b"not json\n{\"schema\":1,\"id\":\"x\",\"at\":\"t\",\"from\":\"you\",\"text\":\"ok\",\"future\":1}\n")
            .unwrap();
        let shown = fold(h, 100);
        let state = |id: &str| {
            shown
                .iter()
                .find(|s| s.id == id)
                .and_then(|s| s.state.clone())
        };
        assert_eq!(state("1").as_deref(), Some("replied"));
        assert_eq!(state("2").as_deref(), Some("queued"));
        assert_eq!(state("3").as_deref(), Some("failed"));
        assert_eq!(
            state("4").as_deref(),
            Some("replied"),
            "a silent reply still answers"
        );
        assert_eq!(
            state("x").as_deref(),
            Some("queued"),
            "unknown fields are ignored"
        );
        assert!(
            shown.iter().all(|s| s.id != "m4"),
            "silent replies are left out"
        );
        let ids: Vec<&str> = shown.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3", "4", "m1", "e1", "x"]);
        assert_eq!(fold(h, 2).len(), 2);
        assert_eq!(fold(h, 2)[1].id, "x");
        assert_eq!(last_reply_at(h).as_deref(), Some("t"));
        assert!(fold(&h.join("none"), 10).is_empty());
    }

    #[test]
    fn chat_passes_commands_through() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        append(h, &Line::message("1", "t", FROM_YOU, "/usage")).unwrap();
        append(
            h,
            &Line {
                command: Some("/usage".into()),
                ..moss("m1", "1", "  Usage")
            },
        )
        .unwrap();
        append(h, &Line::message("2", "t", FROM_YOU, "/login")).unwrap();
        append(
            h,
            &Line {
                command: Some("/login".into()),
                refused: true,
                ..moss("m2", "2", "needs the terminal")
            },
        )
        .unwrap();
        let v = to_json(&fold(h, 10));
        let m = v["messages"].as_array().unwrap();
        assert_eq!(m[0]["state"], "replied");
        assert_eq!(m[1]["command"], "/usage");
        assert!(m[1].get("refused").is_none());
        assert_eq!(m[3]["command"], "/login");
        assert_eq!(m[3]["refused"], true);
        assert!(m[0].get("command").is_none());
    }

    #[test]
    fn chat_rotates_and_reads_the_old_file_for_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        for i in 0..=ROTATE_AT {
            append(h, &Line::message(&format!("{i:05}"), "t", FROM_YOU, "x")).unwrap();
        }
        assert!(rotate_if_long(h).unwrap());
        assert!(!rotate_if_long(h).unwrap());
        append(h, &Line::message("new", "t", FROM_YOU, "y")).unwrap();
        let shown = fold(h, 3);
        let ids: Vec<&str> = shown.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["01999", "02000", "new"]);
    }
}
