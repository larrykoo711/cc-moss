//! Questions a session is waiting on, kept as files so anyone can answer.
//!
//! `<state>/asks/` holds, per question `<id>`:
//! - `<id>.json`: the question ([`Ask`]), written whole with a rename;
//! - `<id>.answer.json`: the answer ([`Answer`]), created with `O_EXCL`, so
//!   the first one to answer wins and everyone after is told so;
//! - `<id>.closed.json`: why the question no longer stands ([`Closed`]),
//!   also `O_EXCL`, so the first reason sticks.
//!
//! `takers/<name>.alive` are heartbeats: the hook only waits for an answer
//! when one was touched in the last [`TAKER_FRESH_SECS`] seconds.
//!
//! These names and shapes are a protocol literal shared with the menubar
//! app, the daemon and Slack (spec T1.3 § contract).

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::slot::rfc3339;

pub const SCHEMA: u32 = 1;
/// A taker whose heartbeat is older than this does not count.
pub const TAKER_FRESH_SECS: u64 = 90;
/// How often a waiting hook looks for an answer or a withdrawal.
pub const POLL: Duration = Duration::from_millis(150);
pub const DEFAULT_WAIT_SECS: u64 = 300;
/// Claude Code kills the hook at 600 s; this leaves it time to finish.
pub const MAX_WAIT_SECS: u64 = 590;
/// Longest tool preview, in characters.
pub const PREVIEW_MAX: usize = 300;
/// Closed and expired questions stay this long, so an app can show what
/// became of them, then they are swept.
const KEEP_MS: i64 = 60 * 60 * 1000;
/// A PermissionRequest takes over the question its PreToolUse wrote if that
/// one is at most this old.
const ADOPT_WITHIN_MS: i64 = 10_000;

/// The tool whose questions cc-moss can answer.
pub const ASK_TOOL: &str = "AskUserQuestion";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// AskUserQuestion: one or more multiple-choice questions.
    Question,
    /// A tool waiting for permission (sessions started with `--safe`).
    Permission,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub preview: String,
}

/// One question a session is waiting on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ask {
    pub schema: u32,
    pub id: String,
    pub kind: Kind,
    pub session_id: String,
    #[serde(default)]
    pub prompt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(default)]
    pub cwd: String,
    pub created_at: String,
    pub expires_at: String,
    /// `tool_input.questions` exactly as the session sent it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<Tool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    Cli,
    Desktop,
    Slack,
    Moss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Behavior {
    Allow,
    Deny,
}

/// What someone answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub schema: u32,
    pub by: By,
    /// Question text → chosen label, labels joined with `", "`, or free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<Behavior>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// The hook handed an answer to the session.
    Answered,
    /// The session moved on by itself: answered in its terminal, or the
    /// dialog was dismissed.
    AnsweredInSession,
    Timeout,
    SessionEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    pub reason: Reason,
}

/// Why an answer was not taken.
#[derive(Debug)]
pub enum AnswerError {
    NotFound,
    /// Withdrawn or expired.
    Closed,
    AlreadyAnswered,
    Io(std::io::Error),
}

impl std::fmt::Display for AnswerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnswerError::NotFound => write!(f, "no such question"),
            AnswerError::Closed => write!(f, "the question is no longer open"),
            AnswerError::AlreadyAnswered => write!(f, "someone already answered"),
            AnswerError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AnswerError {}

/// `<state>/asks`.
pub fn dir(state: &Path) -> PathBuf {
    state.join("asks")
}

/// A question id: creation time and pid in hex, unique per machine and
/// sortable by age.
pub fn new_id(now_ms: i64, pid: u32) -> String {
    format!("{:011x}{:05x}", now_ms.max(0), pid & 0xf_ffff)
}

/// Whether `name` is usable as a taker name (`[a-z0-9_-]`, 1–64 chars).
pub fn valid_taker_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Milliseconds since the epoch from an RFC 3339 time; `None` when unreadable.
pub fn parse_time(s: &str) -> Option<i64> {
    let t = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(t.unix_timestamp_nanos() / 1_000_000).ok()
}

impl Ask {
    pub fn expires_ms(&self) -> Option<i64> {
        parse_time(&self.expires_at)
    }

    fn created_ms(&self) -> Option<i64> {
        parse_time(&self.created_at)
    }

    /// The question's tool: `AskUserQuestion` or the tool asking permission.
    pub fn tool_name(&self) -> &str {
        match self.kind {
            Kind::Question => ASK_TOOL,
            Kind::Permission => self.tool.as_ref().map_or("", |t| t.name.as_str()),
        }
    }
}

/// The files under `<state>/asks`.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(state: &Path) -> Self {
        Store { dir: dir(state) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: &str, suffix: &str) -> PathBuf {
        self.dir.join(format!("{id}{suffix}"))
    }

    fn takers(&self) -> PathBuf {
        self.dir.join("takers")
    }

    /// Write a new question. The file appears whole or not at all.
    pub fn create(&self, ask: &Ask) -> std::io::Result<()> {
        crate::store_dir::ensure_private_dir(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(ask).map_err(std::io::Error::other)?;
        let target = self.path(&ask.id, ".json");
        let tmp = self
            .dir
            .join(format!(".{}.{}.tmp", ask.id, std::process::id()));
        let result = (|| {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            std::fs::rename(&tmp, &target)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    /// The question `id`, whether or not it is still open.
    pub fn get(&self, id: &str) -> Option<Ask> {
        if !valid_id(id) {
            return None;
        }
        let text = std::fs::read(self.path(id, ".json")).ok()?;
        serde_json::from_slice(&text).ok()
    }

    /// Every question on file, oldest first, open or not.
    pub fn all(&self) -> Vec<Ask> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut ids: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let id = name.strip_suffix(".json")?;
                valid_id(id).then(|| id.to_string())
            })
            .collect();
        ids.sort();
        ids.iter().filter_map(|id| self.get(id)).collect()
    }

    pub fn is_closed(&self, id: &str) -> bool {
        self.path(id, ".closed.json").exists()
    }

    pub fn closed(&self, id: &str) -> Option<Closed> {
        let text = std::fs::read(self.path(id, ".closed.json")).ok()?;
        serde_json::from_slice(&text).ok()
    }

    pub fn is_answered(&self, id: &str) -> bool {
        self.path(id, ".answer.json").exists()
    }

    /// Whether `ask` still waits for an answer at `now_ms`: not withdrawn,
    /// not answered, not expired.
    pub fn is_open(&self, ask: &Ask, now_ms: i64) -> bool {
        !self.is_closed(&ask.id)
            && !self.is_answered(&ask.id)
            && ask.expires_ms().is_some_and(|e| e > now_ms)
    }

    /// The questions waiting for an answer, oldest first.
    pub fn open(&self, now_ms: i64) -> Vec<Ask> {
        self.all()
            .into_iter()
            .filter(|a| self.is_open(a, now_ms))
            .collect()
    }

    /// The open question whose id is `id` or starts with it.
    pub fn find_open(&self, id: &str, now_ms: i64) -> Result<Ask, AnswerError> {
        if let Some(ask) = self.get(id) {
            return self.check_open(ask, now_ms);
        }
        let matches: Vec<Ask> = self
            .all()
            .into_iter()
            .filter(|a| !id.is_empty() && a.id.starts_with(id))
            .collect();
        match <[Ask; 1]>::try_from(matches) {
            Ok([ask]) => self.check_open(ask, now_ms),
            Err(_) => Err(AnswerError::NotFound),
        }
    }

    fn check_open(&self, ask: Ask, now_ms: i64) -> Result<Ask, AnswerError> {
        if self.is_answered(&ask.id) {
            Err(AnswerError::AlreadyAnswered)
        } else if self.is_open(&ask, now_ms) {
            Ok(ask)
        } else {
            Err(AnswerError::Closed)
        }
    }

    /// Record `answer` for question `id`. Only the first answer is kept:
    /// the file is created with `O_EXCL`, so two answers at once cannot both
    /// succeed.
    pub fn answer(&self, id: &str, answer: &Answer) -> Result<(), AnswerError> {
        if !valid_id(id) || !self.path(id, ".json").exists() {
            return Err(AnswerError::NotFound);
        }
        if self.is_closed(id) {
            return Err(AnswerError::Closed);
        }
        let bytes = serde_json::to_vec(answer).map_err(|e| AnswerError::Io(e.into()))?;
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.path(id, ".answer.json"))
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(AnswerError::AlreadyAnswered)
            }
            Err(e) => return Err(AnswerError::Io(e)),
        };
        f.write_all(&bytes)
            .and_then(|()| f.sync_all())
            .map_err(AnswerError::Io)
    }

    /// The answer to `id`, once it is there and complete.
    pub fn read_answer(&self, id: &str) -> Option<Answer> {
        let text = std::fs::read(self.path(id, ".answer.json")).ok()?;
        serde_json::from_slice(&text).ok()
    }

    /// Withdraw question `id`. `false` when it was already closed (the
    /// first reason stays).
    pub fn close(&self, id: &str, reason: Reason) -> std::io::Result<bool> {
        let bytes = serde_json::to_vec(&Closed { reason }).map_err(std::io::Error::other)?;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.path(id, ".closed.json"))
        {
            Ok(mut f) => f.write_all(&bytes).map(|()| true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Withdraw the open questions of `session_id`; with `tool`, only that
    /// tool's. Returns how many were withdrawn.
    pub fn withdraw_session(
        &self,
        session_id: &str,
        tool: Option<&str>,
        reason: Reason,
        now_ms: i64,
    ) -> std::io::Result<usize> {
        let mut n = 0;
        for ask in self.open(now_ms) {
            if ask.session_id != session_id || tool.is_some_and(|t| t != ask.tool_name()) {
                continue;
            }
            if self.close(&ask.id, reason)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Whether any question file is on disk at all; cheap, for the hooks
    /// that run on every event and usually have nothing to withdraw.
    pub fn any(&self) -> bool {
        std::fs::read_dir(&self.dir).is_ok_and(|mut entries| {
            entries.any(|e| {
                e.ok()
                    .and_then(|e| e.file_name().into_string().ok())
                    .and_then(|n| n.strip_suffix(".json").map(valid_id))
                    .unwrap_or(false)
            })
        })
    }

    /// The question `session_id` wrote from PreToolUse for these
    /// `questions` moments ago, still open: PermissionRequest takes it over.
    pub fn recent_question(
        &self,
        session_id: &str,
        prompt_id: &str,
        questions: &[Value],
        now_ms: i64,
    ) -> Option<Ask> {
        self.open(now_ms).into_iter().rev().find(|a| {
            a.kind == Kind::Question
                && a.session_id == session_id
                && a.prompt_id == prompt_id
                && a.questions.as_deref() == Some(questions)
                && a.created_ms()
                    .is_some_and(|c| now_ms - c <= ADOPT_WITHIN_MS)
        })
    }

    /// Delete questions that expired more than an hour ago, with their
    /// answers and withdrawals. Best effort.
    pub fn sweep(&self, now_ms: i64) {
        for ask in self.all() {
            let stale = ask.expires_ms().is_none_or(|e| e + KEEP_MS < now_ms);
            if stale {
                for suffix in [".answer.json", ".closed.json", ".json"] {
                    let _ = std::fs::remove_file(self.path(&ask.id, suffix));
                }
            }
        }
    }

    /// Say that taker `name` is present (create or refresh its heartbeat).
    pub fn take(&self, name: &str) -> std::io::Result<()> {
        if !valid_taker_name(name) {
            return Err(std::io::Error::other("taker names use a-z, 0-9, - and _"));
        }
        let dir = self.takers();
        crate::store_dir::ensure_private_dir(&self.dir)?;
        crate::store_dir::ensure_private_dir(&dir)?;
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join(format!("{name}.alive")))?;
        f.set_modified(SystemTime::now())
    }

    /// Whether some taker's heartbeat is fresh at `now`.
    pub fn has_taker(&self, now: SystemTime) -> bool {
        let Ok(entries) = std::fs::read_dir(self.takers()) else {
            return false;
        };
        entries.flatten().any(|e| {
            let alive = e.file_name().to_string_lossy().ends_with(".alive");
            alive
                && e.metadata().and_then(|m| m.modified()).is_ok_and(|t| {
                    match now.duration_since(t) {
                        Ok(age) => age.as_secs() < TAKER_FRESH_SECS,
                        // Touched "in the future" (clock skew): present.
                        Err(_) => true,
                    }
                })
        })
    }
}

/// A new question as of `now_ms`, waiting `wait_secs`.
pub struct NewAsk<'a> {
    pub id: String,
    pub session_id: &'a str,
    pub prompt_id: &'a str,
    pub tool_use_id: Option<&'a str>,
    pub session_name: Option<String>,
    pub cwd: &'a str,
    pub now_ms: i64,
    pub wait_secs: u64,
}

impl NewAsk<'_> {
    fn build(self, kind: Kind, questions: Option<Vec<Value>>, tool: Option<Tool>) -> Ask {
        let wait_ms = i64::try_from(self.wait_secs.saturating_mul(1000)).unwrap_or(i64::MAX);
        Ask {
            schema: SCHEMA,
            id: self.id,
            kind,
            session_id: self.session_id.to_string(),
            prompt_id: self.prompt_id.to_string(),
            tool_use_id: self.tool_use_id.map(String::from),
            session_name: self.session_name,
            cwd: self.cwd.to_string(),
            created_at: rfc3339(self.now_ms),
            expires_at: rfc3339(self.now_ms.saturating_add(wait_ms)),
            questions,
            tool,
        }
    }

    /// An AskUserQuestion. `None` when `tool_input` has no questions.
    pub fn question(self, tool_input: &Value) -> Option<Ask> {
        let questions = tool_input.get("questions")?.as_array()?.clone();
        if questions.is_empty() {
            return None;
        }
        Some(self.build(Kind::Question, Some(questions), None))
    }

    /// A permission request for `tool_name`.
    pub fn permission(self, tool_name: &str, tool_input: &Value) -> Ask {
        let tool = Tool {
            name: tool_name.to_string(),
            preview: preview(tool_input),
        };
        self.build(Kind::Permission, None, Some(tool))
    }
}

/// What a tool is about to do, in at most [`PREVIEW_MAX`] characters: the
/// command, file or address when there is one, else the input as JSON.
pub fn preview(tool_input: &Value) -> String {
    let text = ["command", "file_path", "url", "path", "pattern"]
        .iter()
        .find_map(|k| tool_input.get(*k).and_then(Value::as_str))
        .map(String::from)
        .unwrap_or_else(|| tool_input.to_string());
    truncate(&text, PREVIEW_MAX)
}

fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        None => text.to_string(),
        Some((at, _)) => {
            let mut s = text[..at].to_string();
            s.pop();
            s.push('…');
            s
        }
    }
}

fn question_text(q: &Value) -> Option<&str> {
    q.get("question").and_then(Value::as_str)
}

fn labels(q: &Value) -> Vec<&str> {
    q.get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|o| o.get("label").and_then(Value::as_str))
        .collect()
}

fn multi(q: &Value) -> bool {
    q.get("multiSelect").and_then(Value::as_bool) == Some(true)
}

/// Why choices do not fit the questions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceError {
    /// This label is not an option where it was given.
    NotAnOption(String),
    /// Question (1-based) got no answer.
    Unanswered(usize),
    /// Free text only works for a single question.
    TextNeedsOneQuestion,
    /// A single-choice question got several labels.
    OneOnly(usize),
}

/// `answers` for `questions` from labels given in question order (several
/// in a row for a multi-select question), or from free text when there is
/// one question. Multi-select labels are joined with `", "`.
pub fn answers_from(
    questions: &[Value],
    choices: &[String],
    text: Option<&str>,
) -> Result<BTreeMap<String, String>, ChoiceError> {
    let mut out = BTreeMap::new();
    if let Some(text) = text {
        let [q] = questions else {
            return Err(ChoiceError::TextNeedsOneQuestion);
        };
        let key = question_text(q).ok_or(ChoiceError::Unanswered(1))?;
        out.insert(key.to_string(), text.to_string());
        return Ok(out);
    }
    let mut rest = choices;
    for (i, q) in questions.iter().enumerate() {
        let key = question_text(q).ok_or(ChoiceError::Unanswered(i + 1))?;
        let options = labels(q);
        let last = i + 1 == questions.len();
        let take = if last {
            rest.len()
        } else if multi(q) {
            rest.iter()
                .take_while(|c| options.contains(&c.as_str()))
                .count()
        } else {
            usize::from(!rest.is_empty())
        };
        let (mine, tail) = rest.split_at(take);
        rest = tail;
        if mine.is_empty() {
            return Err(ChoiceError::Unanswered(i + 1));
        }
        if !multi(q) && mine.len() > 1 {
            return Err(ChoiceError::OneOnly(i + 1));
        }
        if let Some(bad) = mine.iter().find(|c| !options.contains(&c.as_str())) {
            return Err(ChoiceError::NotAnOption(bad.clone()));
        }
        out.insert(key.to_string(), mine.join(", "));
    }
    Ok(out)
}

/// What the PermissionRequest hook prints to hand `answer` to the session.
/// `None` when the answer does not fit the question (wrong kind, or a
/// question left unanswered): the hook then keeps waiting.
pub fn hook_output(ask: &Ask, answer: &Answer) -> Option<Value> {
    let decision = match ask.kind {
        Kind::Question => {
            let questions = ask.questions.as_ref()?;
            let given = answer.answers.as_ref()?;
            let mut answers = Map::new();
            for q in questions {
                let key = question_text(q)?;
                answers.insert(key.to_string(), Value::String(given.get(key)?.clone()));
            }
            json!({
                "behavior": "allow",
                "updatedInput": { "questions": questions, "answers": answers },
            })
        }
        Kind::Permission => match answer.behavior? {
            Behavior::Allow => json!({ "behavior": "allow" }),
            Behavior::Deny => match answer.message.as_deref().filter(|m| !m.is_empty()) {
                Some(m) => json!({ "behavior": "deny", "message": m }),
                None => json!({ "behavior": "deny" }),
            },
        },
    };
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": decision,
        }
    }))
}

/// `wait_secs` from the config, kept between 1 and [`MAX_WAIT_SECS`].
pub fn clamp_wait(secs: u64) -> u64 {
    secs.clamp(1, MAX_WAIT_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000_000;

    fn pick() -> Value {
        json!({ "question": "Pick one?", "header": "Pick",
            "options": [{"label": "A", "description": "a"}, {"label": "B", "description": "b"}],
            "multiSelect": false })
    }

    fn many() -> Value {
        json!({ "question": "Which ones?", "header": "Many",
            "options": [{"label": "X", "description": ""}, {"label": "Y", "description": ""},
                        {"label": "Z", "description": ""}],
            "multiSelect": true })
    }

    fn new_ask<'a>(id: &str, session: &'a str, now_ms: i64) -> NewAsk<'a> {
        NewAsk {
            id: id.into(),
            session_id: session,
            prompt_id: "p1",
            tool_use_id: Some("toolu_1"),
            session_name: Some("api".into()),
            cwd: "/work",
            now_ms,
            wait_secs: 300,
        }
    }

    fn question(store: &Store, id: &str, session: &str) -> Ask {
        let ask = new_ask(id, session, NOW)
            .question(&json!({ "questions": [pick()] }))
            .unwrap();
        store.create(&ask).unwrap();
        ask
    }

    fn cli(answers: &[(&str, &str)]) -> Answer {
        Answer {
            schema: SCHEMA,
            by: By::Cli,
            answers: Some(
                answers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
            behavior: None,
            message: None,
        }
    }

    #[test]
    fn writes_the_contract_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = question(&store, "a1", "S");
        let on_disk: Value =
            serde_json::from_slice(&std::fs::read(tmp.path().join("asks/a1.json")).unwrap())
                .unwrap();
        assert_eq!(on_disk["schema"], 1);
        assert_eq!(on_disk["kind"], "question");
        assert_eq!(on_disk["session_id"], "S");
        assert_eq!(on_disk["prompt_id"], "p1");
        assert_eq!(on_disk["tool_use_id"], "toolu_1");
        assert_eq!(on_disk["session_name"], "api");
        assert_eq!(on_disk["cwd"], "/work");
        assert_eq!(on_disk["questions"], json!([pick()]));
        assert!(on_disk.get("tool").is_none());
        assert_eq!(
            ask.expires_ms().unwrap() - parse_time(&ask.created_at).unwrap(),
            300_000
        );
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(tmp.path().join("asks"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        assert_eq!(store.open(NOW).len(), 1);
    }

    #[test]
    fn permission_preview_is_short() {
        let ask = new_ask("p", "S", NOW).permission(
            "Bash",
            &json!({ "command": "x".repeat(1000), "description": "d" }),
        );
        let tool = ask.tool.unwrap();
        assert_eq!(tool.name, "Bash");
        assert_eq!(tool.preview.chars().count(), PREVIEW_MAX);
        assert_eq!(
            preview(&json!({ "a": 1 })),
            "{\"a\":1}",
            "no known field: the input as JSON"
        );
    }

    #[test]
    fn first_answer_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let label = if i % 2 == 0 { "A" } else { "B" };
                    barrier.wait();
                    store
                        .answer("a1", &cli(&[("Pick one?", label)]))
                        .map(|()| label)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let won: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(won.len(), 1, "exactly one answer is taken");
        assert!(results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| matches!(e, AnswerError::AlreadyAnswered)));
        let kept = store.read_answer("a1").unwrap();
        assert_eq!(kept.answers.unwrap()["Pick one?"], *won[0]);
    }

    #[test]
    fn answer_needs_an_open_question() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        assert!(matches!(
            store.answer("nope", &cli(&[])),
            Err(AnswerError::NotFound)
        ));
        question(&store, "a1", "S");
        assert!(store.close("a1", Reason::AnsweredInSession).unwrap());
        assert!(matches!(
            store.answer("a1", &cli(&[("Pick one?", "A")])),
            Err(AnswerError::Closed)
        ));
        assert!(matches!(
            store.find_open("a1", NOW),
            Err(AnswerError::Closed)
        ));
    }

    #[test]
    fn first_withdrawal_reason_sticks() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        assert!(store.close("a1", Reason::Answered).unwrap());
        assert!(!store.close("a1", Reason::Timeout).unwrap());
        assert_eq!(store.closed("a1").unwrap().reason, Reason::Answered);
        let text = std::fs::read_to_string(tmp.path().join("asks/a1.closed.json")).unwrap();
        assert_eq!(text, r#"{"reason":"answered"}"#);
        assert!(store.open(NOW).is_empty());
    }

    #[test]
    fn withdraw_session_closes_only_that_session() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        question(&store, "a2", "S");
        question(&store, "b1", "T");
        let bash = new_ask("a3", "S", NOW).permission("Bash", &json!({ "command": "ls" }));
        store.create(&bash).unwrap();
        let n = store
            .withdraw_session("S", Some(ASK_TOOL), Reason::AnsweredInSession, NOW)
            .unwrap();
        assert_eq!(n, 2);
        let open: Vec<_> = store.open(NOW).into_iter().map(|a| a.id).collect();
        assert_eq!(open, ["a3", "b1"]);
        store
            .withdraw_session("S", None, Reason::SessionEnded, NOW)
            .unwrap();
        assert_eq!(store.closed("a3").unwrap().reason, Reason::SessionEnded);
        assert_eq!(store.open(NOW).len(), 1);
    }

    #[test]
    fn expired_questions_are_not_open_and_get_swept() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = question(&store, "a1", "S");
        let expires = ask.expires_ms().unwrap();
        assert_eq!(store.open(expires - 1).len(), 1);
        assert!(store.open(expires).is_empty(), "expired");
        assert!(matches!(
            store.find_open("a1", expires),
            Err(AnswerError::Closed)
        ));
        store.sweep(expires + 1000);
        assert_eq!(store.all().len(), 1, "kept for an hour after expiry");
        store.close("a1", Reason::Timeout).unwrap();
        store.sweep(expires + KEEP_MS + 1);
        assert!(store.all().is_empty());
        assert!(!tmp.path().join("asks/a1.closed.json").exists());
    }

    #[test]
    fn find_open_by_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "abc1", "S");
        question(&store, "abd2", "S");
        assert_eq!(store.find_open("abc", NOW).unwrap().id, "abc1");
        assert!(matches!(
            store.find_open("ab", NOW),
            Err(AnswerError::NotFound)
        ));
        assert!(matches!(
            store.find_open("../x", NOW),
            Err(AnswerError::NotFound)
        ));
    }

    #[test]
    fn recent_question_pairs_pre_tool_use_with_permission_request() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        let qs = [pick()];
        assert_eq!(
            store.recent_question("S", "p1", &qs, NOW + 500).unwrap().id,
            "a1"
        );
        assert!(store.recent_question("S", "p2", &qs, NOW).is_none());
        assert!(store.recent_question("T", "p1", &qs, NOW).is_none());
        assert!(store.recent_question("S", "p1", &[many()], NOW).is_none());
        assert!(store
            .recent_question("S", "p1", &qs, NOW + ADOPT_WITHIN_MS + 1)
            .is_none());
    }

    #[test]
    fn taker_heartbeat_counts_for_90_seconds() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let now = SystemTime::now();
        assert!(!store.has_taker(now), "no takers dir");
        store.take("desktop").unwrap();
        assert!(store.has_taker(now));
        assert!(store.has_taker(now + Duration::from_secs(89)));
        assert!(!store.has_taker(now + Duration::from_secs(91)));
        assert!(store.take("Bad/Name").is_err());
        // A file that is not a heartbeat does not count.
        std::fs::remove_file(tmp.path().join("asks/takers/desktop.alive")).unwrap();
        std::fs::write(tmp.path().join("asks/takers/notes.txt"), "").unwrap();
        assert!(!store.has_taker(now));
    }

    #[test]
    fn fills_a_single_choice() {
        let ask = new_ask("a", "S", NOW)
            .question(&json!({ "questions": [pick()] }))
            .unwrap();
        let answers = answers_from(ask.questions.as_ref().unwrap(), &["B".into()], None).unwrap();
        let out = hook_output(
            &ask,
            &Answer {
                answers: Some(answers),
                ..cli(&[])
            },
        )
        .unwrap();
        assert_eq!(
            out,
            json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest",
                "decision": { "behavior": "allow",
                    "updatedInput": { "questions": [pick()], "answers": { "Pick one?": "B" } } } } })
        );
    }

    #[test]
    fn fills_multi_select_joined_with_comma() {
        let qs = [many()];
        let a = answers_from(&qs, &["X".into(), "Z".into()], None).unwrap();
        assert_eq!(a["Which ones?"], "X, Z");
    }

    #[test]
    fn fills_free_text() {
        let a = answers_from(&[pick()], &[], Some("neither, thanks")).unwrap();
        assert_eq!(a["Pick one?"], "neither, thanks");
        assert_eq!(
            answers_from(&[pick(), many()], &[], Some("x")),
            Err(ChoiceError::TextNeedsOneQuestion)
        );
    }

    #[test]
    fn fills_several_questions_in_order() {
        let qs = [many(), pick()];
        let a = answers_from(&qs, &["Y".into(), "Z".into(), "A".into()], None).unwrap();
        assert_eq!(a["Which ones?"], "Y, Z");
        assert_eq!(a["Pick one?"], "A");
        assert_eq!(
            answers_from(&qs, &["Y".into()], None),
            Err(ChoiceError::Unanswered(2))
        );
        assert_eq!(
            answers_from(&[pick()], &["C".into()], None),
            Err(ChoiceError::NotAnOption("C".into()))
        );
        assert_eq!(
            answers_from(&[pick()], &["A".into(), "B".into()], None),
            Err(ChoiceError::OneOnly(1))
        );
    }

    #[test]
    fn an_answer_that_does_not_fit_is_not_handed_over() {
        let ask = new_ask("a", "S", NOW)
            .question(&json!({ "questions": [pick(), many()] }))
            .unwrap();
        assert!(
            hook_output(&ask, &cli(&[("Pick one?", "A")])).is_none(),
            "a question left out"
        );
        let allow = Answer {
            behavior: Some(Behavior::Allow),
            answers: None,
            ..cli(&[])
        };
        assert!(
            hook_output(&ask, &allow).is_none(),
            "allow is not an answer"
        );
    }

    #[test]
    fn fills_permission_allow_and_deny() {
        let ask = new_ask("p", "S", NOW).permission("Bash", &json!({ "command": "ls" }));
        let allow = Answer {
            behavior: Some(Behavior::Allow),
            answers: None,
            ..cli(&[])
        };
        assert_eq!(
            hook_output(&ask, &allow).unwrap()["hookSpecificOutput"]["decision"],
            json!({ "behavior": "allow" })
        );
        let deny = Answer {
            behavior: Some(Behavior::Deny),
            message: Some("not now".into()),
            answers: None,
            ..cli(&[])
        };
        assert_eq!(
            hook_output(&ask, &deny).unwrap()["hookSpecificOutput"]["decision"],
            json!({ "behavior": "deny", "message": "not now" })
        );
        assert!(
            hook_output(&ask, &cli(&[("x", "y")])).is_none(),
            "a permission needs allow or deny"
        );
    }

    #[test]
    fn wait_is_clamped() {
        assert_eq!(clamp_wait(0), 1);
        assert_eq!(clamp_wait(300), 300);
        assert_eq!(clamp_wait(10_000), MAX_WAIT_SECS);
    }

    #[test]
    fn ids_sort_by_time() {
        assert!(new_id(NOW, 99) < new_id(NOW + 1, 1));
        assert!(valid_id(&new_id(NOW, u32::MAX)));
    }
}
