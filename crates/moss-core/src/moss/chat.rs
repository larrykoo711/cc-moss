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
    /// What Moss pins on the panel's quick bar (`[[quick:]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quick: Vec<String>,
    /// Sessions Moss points at (`[[sessions:]]`), as slot ids it checked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<String>,
    /// What a yes to this message would let Moss do (`[[confirm:]]`, checked
    /// by the daemon): only these, for these sessions, with these buttons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<Proposal>,
    /// What each of those was doing when Moss said it, by slot id.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub session_states: std::collections::BTreeMap<String, String>,
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
    // Never through a link at chat.jsonl.
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path(home))?
        .write_all(text.as_bytes())
}

/// Start a new file once this one is past [`ROTATE_AT`] lines; the old one
/// becomes `chat.jsonl.1` (replacing the one before). True when it did.
pub fn rotate_if_long(home: &Path) -> io::Result<bool> {
    // Cheap first: a file this small can't hold that many lines.
    if std::fs::metadata(path(home)).map_or(0, |m| m.len()) < (ROTATE_AT as u64) * 32 {
        return Ok(false);
    }
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

/// An action Moss asked the user to agree to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub action: String,
    pub slot_id: String,
    /// The buttons that mean yes.
    pub yes: Vec<String>,
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
    /// Pinned on the quick bar; left out when none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub quick: Vec<String>,
    /// Session cards: slot ids; left out when none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<String>,
    /// Each card's session state when Moss said it, by slot id.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub session_states: std::collections::BTreeMap<String, String>,
    /// A Claude Code command's result (its screen, or why it was not run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub refused: bool,
}

/// The last `limit` messages, oldest first, with the state of yours folded
/// in. Silent replies are left out (they still mark yours as replied).
pub fn fold(home: &Path, limit: usize) -> Vec<Shown> {
    fold_lines(&window(home, limit), limit)
}

/// The lines read for the last `limit` messages: chat.jsonl, and the old
/// file before it when that is not enough.
fn window(home: &Path, limit: usize) -> Vec<Line> {
    let mut lines = read_lines(&path(home));
    let messages = lines.iter().filter(|l| l.state.is_none()).count();
    if messages < limit {
        let mut old = read_lines(&old_path(home));
        old.append(&mut lines);
        lines = old;
    }
    lines
}

fn fold_lines(lines: &[Line], limit: usize) -> Vec<Shown> {
    let mut shown: Vec<Shown> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut replied: std::collections::HashSet<String> = std::collections::HashSet::new();
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
            replied.insert(to.clone());
        }
        if l.silent || !seen.insert(l.id.clone()) {
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
            quick: l.quick.clone(),
            sessions: l.sessions.clone(),
            session_states: l.session_states.clone(),
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

/// Every line of the conversation kept (the old file's first), unfolded.
pub fn lines(home: &Path) -> Vec<Line> {
    let mut all = read_lines(&old_path(home));
    all.extend(read_lines(&path(home)));
    all
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

/// `moss chat --json`: the last `limit` messages, and what Moss pinned last.
/// The pin is the newest Moss line that carried a `quick` list, shown or
/// silent: `{"items":[…],"at":…}`, absent when there is none.
pub fn chat_json(home: &Path, limit: usize) -> Value {
    chat_json_with(home, limit, &|_, _, _| None)
}

/// What a session's news was, from the registry's events for slot `slot`
/// between two times (Unix ms): `needs-you-<reason>` or `dropped`.
pub type ToldLookup<'a> = dyn Fn(&str, i64, i64) -> Option<String> + 'a;

/// [`chat_json`], with Moss's answers to session news recorded before the
/// cards kept the news's state (they hold the state at reply time) set
/// right from `told`: a card that says neither needs-you nor dropped is
/// looked up in the window before the update (merge window plus 5 s). The
/// file is not rewritten.
pub fn chat_json_with(home: &Path, limit: usize, told: &ToldLookup) -> Value {
    let lines = window(home, limit);
    let mut shown = fold_lines(&lines, limit);
    let news: std::collections::HashMap<&str, i64> = lines
        .iter()
        .filter(|l| l.state.is_none() && l.from.as_deref() == Some(FROM_EVENTS))
        .filter_map(|l| Some((l.id.as_str(), crate::moss::reply::parse_ms(&l.at)?)))
        .collect();
    let before = i64::try_from(super::digest::WINDOW.as_millis()).unwrap_or(0) + 5_000;
    for m in shown.iter_mut().filter(|m| m.from == FROM_MOSS) {
        let Some(at) = m.reply_to.as_deref().and_then(|r| news.get(r)) else {
            continue;
        };
        for (slot, state) in m.session_states.iter_mut() {
            if state.starts_with("needs-you") || state == "dropped" {
                continue;
            }
            if let Some(then) = told(slot, at - before, *at) {
                *state = then;
            }
        }
    }
    let mut v = to_json(&shown);
    let pin = lines
        .iter()
        .rev()
        .find(|l| l.state.is_none() && l.from.as_deref() == Some(FROM_MOSS) && !l.quick.is_empty());
    if let Some(pin) = pin {
        v["quick"] = serde_json::json!({ "items": pin.quick, "at": pin.at });
    }
    v
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
        append(
            h,
            &Line {
                quick: vec!["/usage".into(), "stuck".into()],
                sessions: vec!["SLOT1".into()],
                session_states: [("SLOT1".to_string(), "needs-you-permission".to_string())].into(),
                ..moss("m3", "2", "pinned")
            },
        )
        .unwrap();
        let v = to_json(&fold(h, 10));
        let m = v["messages"].as_array().unwrap();
        assert_eq!(m[4]["quick"], serde_json::json!(["/usage", "stuck"]));
        assert_eq!(m[4]["sessions"], serde_json::json!(["SLOT1"]));
        assert_eq!(
            m[4]["session_states"],
            serde_json::json!({"SLOT1": "needs-you-permission"})
        );
        assert!(m[1].get("session_states").is_none());
        assert!(m[1].get("sessions").is_none());
        assert!(m[1].get("quick").is_none());
        assert_eq!(m[0]["state"], "replied");
        assert_eq!(m[1]["command"], "/usage");
        assert!(m[1].get("refused").is_none());
        assert_eq!(m[3]["command"], "/login");
        assert_eq!(m[3]["refused"], true);
        assert!(m[0].get("command").is_none());
    }

    #[test]
    fn chat_reports_the_latest_pin_even_when_silent() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        assert!(chat_json(h, 10).get("quick").is_none(), "none yet");
        append(h, &Line::message("1", "t1", FROM_YOU, "hi")).unwrap();
        append(
            h,
            &Line {
                quick: vec!["/usage".into()],
                ..moss("m1", "1", "seen")
            },
        )
        .unwrap();
        append(
            h,
            &Line {
                quick: vec!["stuck".into(), "today".into()],
                silent: true,
                at: "t2".into(),
                ..moss("m2", "e1", "quiet")
            },
        )
        .unwrap();
        // A later moss line without a pin leaves the last pin standing.
        append(h, &moss("m3", "1", "no pin")).unwrap();
        let v = chat_json(h, 10);
        assert_eq!(
            v["quick"],
            serde_json::json!({ "items": ["stuck", "today"], "at": "t2" })
        );
        // Per message, only on what is shown.
        let m = v["messages"].as_array().unwrap();
        assert_eq!(m[1]["quick"], serde_json::json!(["/usage"]));
        assert!(m.iter().all(|x| x["id"] != "m2"));
    }

    #[test]
    fn chat_shows_old_answers_to_news_as_the_news_was() {
        // The real case: needs_you at 10:40:18, the update at 10:40:38, Moss
        // answering at 10:42:00 with the card's state at that time: idle.
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        append(
            h,
            &Line::message(
                "e1",
                "2026-10-19T10:40:38Z",
                FROM_EVENTS,
                "[动态] api-review 需要你（权限）",
            ),
        )
        .unwrap();
        let mut states = std::collections::BTreeMap::new();
        states.insert("SLOT".to_string(), "idle".to_string());
        append(
            h,
            &Line {
                sessions: vec!["SLOT".into(), "OTHER".into()],
                session_states: [
                    ("SLOT".to_string(), "idle".to_string()),
                    ("OTHER".to_string(), "working".to_string()),
                ]
                .into(),
                reply_to: Some("e1".into()),
                ..Line::message(
                    "m1",
                    "2026-10-19T10:42:00Z",
                    FROM_MOSS,
                    "api-review 又在等你批一个权限。",
                )
            },
        )
        .unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let lookup = |slot: &str, from: i64, to: i64| {
            asked.borrow_mut().push((slot.to_string(), from, to));
            (slot == "SLOT").then(|| "needs-you-permission".to_string())
        };
        let v = chat_json_with(h, 10, &lookup);
        let m = &v["messages"][1];
        assert_eq!(m["session_states"]["SLOT"], "needs-you-permission", "{m}");
        assert_eq!(
            m["session_states"]["OTHER"], "working",
            "not in the news: as it was"
        );
        // Looked up by slot, in [update − window − 5 s, update].
        let at = crate::moss::reply::parse_ms("2026-10-19T10:40:38Z").unwrap();
        assert!(asked
            .borrow()
            .contains(&("SLOT".to_string(), at - 25_000, at)));
        // Plain answers to you: never looked up.
        asked.borrow_mut().clear();
        append(
            h,
            &Line::message("y1", "2026-10-19T10:43:00Z", FROM_YOU, "hi"),
        )
        .unwrap();
        append(
            h,
            &Line {
                sessions: vec!["SLOT".into()],
                session_states: [("SLOT".to_string(), "idle".to_string())].into(),
                reply_to: Some("y1".into()),
                ..Line::message("m2", "2026-10-19T10:43:05Z", FROM_MOSS, "ok")
            },
        )
        .unwrap();
        let v = chat_json_with(h, 10, &lookup);
        assert_eq!(v["messages"][3]["session_states"]["SLOT"], "idle");
        assert!(asked.borrow().iter().all(|(_, _, to)| *to == at));
    }

    #[test]
    fn chat_folds_many_lines_quickly() {
        let lines: Vec<Line> = (0..20_000)
            .map(|i| Line::message(&format!("{i:06}"), "t", FROM_YOU, "x"))
            .collect();
        let started = std::time::Instant::now();
        let shown = fold_lines(&lines, 200);
        assert_eq!(shown.len(), 200);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
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
