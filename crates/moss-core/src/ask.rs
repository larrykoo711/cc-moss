//! Questions a session is waiting on, kept as files so anyone can answer.
//!
//! `<state>/asks/` holds, per question `<id>`:
//! - `<id>.json`: the question ([`Ask`]) while it stands, written whole with a
//!   rename. Once it is closed it moves to `done/<id>.json`, so the hooks that
//!   run on every event only ever see the questions still standing;
//! - `<id>.answer.json`: the answer ([`Answer`]), written to a temp file and
//!   hard-linked into place: it appears whole, and the link fails when an
//!   answer is already there, so the first one to answer wins;
//! - `<id>.closed.json`: why the question no longer stands ([`Closed`]),
//!   linked into place the same way, so the first reason sticks.
//!
//! `takers/<name>.alive` are heartbeats: the hook only waits for an answer
//! when one was touched in the last [`TAKER_FRESH_SECS`] seconds.
//!
//! These names and shapes are a protocol literal shared with the menubar
//! app, the daemon and Slack (spec T1.3 § contract).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
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
/// An answer that still does not parse after this long is not being written
/// any more: whoever wrote it did not follow the protocol.
const UNREADABLE_GRACE: Duration = Duration::from_secs(2);
/// A temp file or unreadable question older than this was left by a crash.
const LEFTOVER: Duration = Duration::from_secs(60);
const DONE: &str = "done";

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
    /// [`input_hash`] of the tool's input, so the tool finishing withdraws
    /// this request and not another call of the same tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_hash: Option<String>,
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
    /// The answer on file does not fit the question (or does not parse):
    /// the hook let the session ask as usual.
    InvalidAnswer,
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
    /// The answer does not fit the question (wrong kind, a question left
    /// out, a label that is not an option).
    Invalid,
    Io(std::io::Error),
}

impl std::fmt::Display for AnswerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnswerError::NotFound => write!(f, "no such question"),
            AnswerError::Closed => write!(f, "the question is no longer open"),
            AnswerError::AlreadyAnswered => write!(f, "someone already answered"),
            AnswerError::Invalid => write!(f, "the answer does not fit the question"),
            AnswerError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AnswerError {}

/// What a waiting hook found when it looked.
#[derive(Debug, Clone, PartialEq)]
pub enum Poll {
    /// A fitting answer, now marked answered: print this.
    Deliver(Value),
    /// The question closed (withdrawn, timed out, an answer that does not
    /// fit, or someone else marked it): print nothing.
    Gone,
    /// Nothing yet.
    Wait,
}

/// After a tool ran, which of the session's questions it settles.
#[derive(Debug, Clone, Copy)]
pub enum Moved<'a> {
    /// All of them: the turn moved on or the session ended.
    All,
    /// The question or permission request for this call (`tool_input` as
    /// PostToolUse reports it).
    Tool { name: &'a str, input: &'a Value },
}

impl Moved<'_> {
    fn settles(&self, ask: &Ask) -> bool {
        match *self {
            Moved::All => true,
            Moved::Tool { name, input } if name == ASK_TOOL => {
                ask.kind == Kind::Question
                    && ask.questions.as_deref()
                        == input
                            .get("questions")
                            .and_then(Value::as_array)
                            .map(Vec::as_slice)
            }
            Moved::Tool { name, input } => {
                let Some(tool) = ask.tool.as_ref().filter(|_| ask.kind == Kind::Permission) else {
                    return false;
                };
                // Questions written before the hash existed match by name.
                tool.name == name
                    && tool
                        .input_hash
                        .as_deref()
                        .is_none_or(|h| h == input_hash(input))
            }
        }
    }
}

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

    fn expired(&self, now_ms: i64) -> bool {
        self.expires_ms().is_none_or(|e| e <= now_ms)
    }

    /// The question's tool: `AskUserQuestion` or the tool asking permission.
    pub fn tool_name(&self) -> &str {
        match self.kind {
            Kind::Question => ASK_TOOL,
            Kind::Permission => self.tool.as_ref().map_or("", |t| t.name.as_str()),
        }
    }
}

/// Read a file without following a symlink in its place.
fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Create a new file only the owner can read; never through a symlink.
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

fn older_than(path: &Path, age: Duration) -> bool {
    std::fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|d| d > age))
}

fn ignore_missing(r: std::io::Result<()>) -> std::io::Result<()> {
    match r {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The ids of the `<id>.json` files in `dir`, sorted (oldest first).
fn ids_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
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
    ids
}

fn read_ask(path: &Path) -> Option<Ask> {
    serde_json::from_slice(&read_file(path).ok()?).ok()
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

    fn done(&self) -> PathBuf {
        self.dir.join(DONE)
    }

    fn done_path(&self, id: &str) -> PathBuf {
        self.done().join(format!("{id}.json"))
    }

    fn takers(&self) -> PathBuf {
        self.dir.join("takers")
    }

    fn tmp_path(&self, id: &str, what: &str) -> PathBuf {
        // Unique within the process too: the clock alone repeats for
        // threads writing in the same microsecond.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        self.dir.join(format!(
            ".{id}.{what}.{}.{nanos}.{seq}.tmp",
            std::process::id()
        ))
    }

    /// Whether `asks/` is a real directory of this user's that nobody else
    /// can get into. The hooks only use it then: otherwise another user
    /// could plant answers.
    pub fn is_private(&self) -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let euid = unsafe { libc::geteuid() };
        std::fs::symlink_metadata(&self.dir)
            .is_ok_and(|m| m.is_dir() && m.uid() == euid && m.mode() & 0o077 == 0)
    }

    /// Write a new question, or rewrite one still standing. The file
    /// appears whole or not at all.
    pub fn create(&self, ask: &Ask) -> std::io::Result<()> {
        crate::store_dir::ensure_private_dir(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(ask).map_err(std::io::Error::other)?;
        let tmp = self.tmp_path(&ask.id, "ask");
        let result = (|| {
            let mut f = create_private(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            std::fs::rename(&tmp, self.path(&ask.id, ".json"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    /// Put `bytes` at `target` only if nothing is there yet, whole: a temp
    /// file hard-linked into place. `false` when `target` already exists.
    fn link_new(&self, id: &str, target: &Path, bytes: &[u8]) -> std::io::Result<bool> {
        let tmp = self.tmp_path(id, "new");
        let result = (|| {
            let mut f = create_private(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            match std::fs::hard_link(&tmp, target) {
                Ok(()) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
                Err(e) => Err(e),
            }
        })();
        let _ = std::fs::remove_file(&tmp);
        result
    }

    /// The question `id`, standing or closed.
    pub fn get(&self, id: &str) -> Option<Ask> {
        if !valid_id(id) {
            return None;
        }
        read_ask(&self.path(id, ".json")).or_else(|| read_ask(&self.done_path(id)))
    }

    /// The questions not yet moved to `done/`, oldest first: open ones,
    /// answered ones the hook has not picked up yet, and closed ones a
    /// crash left behind.
    pub fn standing(&self) -> Vec<Ask> {
        ids_in(&self.dir)
            .iter()
            .filter_map(|id| read_ask(&self.path(id, ".json")))
            .collect()
    }

    /// Every question on file, standing or done, oldest first.
    pub fn all(&self) -> Vec<Ask> {
        let mut asks = self.standing();
        asks.extend(
            ids_in(&self.done())
                .iter()
                .filter_map(|id| read_ask(&self.done_path(id))),
        );
        asks.sort_by(|a, b| a.id.cmp(&b.id));
        asks.dedup_by(|a, b| a.id == b.id);
        asks
    }

    pub fn is_closed(&self, id: &str) -> bool {
        self.path(id, ".closed.json").exists()
    }

    pub fn closed(&self, id: &str) -> Option<Closed> {
        serde_json::from_slice(&read_file(&self.path(id, ".closed.json")).ok()?).ok()
    }

    pub fn is_answered(&self, id: &str) -> bool {
        self.path(id, ".answer.json").exists()
    }

    /// Whether `ask` still waits for an answer at `now_ms`: not withdrawn,
    /// not answered, not expired.
    pub fn is_open(&self, ask: &Ask, now_ms: i64) -> bool {
        !self.is_closed(&ask.id) && !self.is_answered(&ask.id) && !ask.expired(now_ms)
    }

    /// The questions waiting for an answer, oldest first.
    pub fn open(&self, now_ms: i64) -> Vec<Ask> {
        self.standing()
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
        match self.closed_error(&ask.id) {
            Some(e) => Err(e),
            None if self.is_answered(&ask.id) => Err(AnswerError::AlreadyAnswered),
            None if ask.expired(now_ms) => Err(AnswerError::Closed),
            None => Ok(ask),
        }
    }

    fn closed_error(&self, id: &str) -> Option<AnswerError> {
        if !self.is_closed(id) {
            return None;
        }
        Some(match self.closed(id) {
            Some(Closed {
                reason: Reason::Answered,
            }) => AnswerError::AlreadyAnswered,
            _ => AnswerError::Closed,
        })
    }

    /// Record `answer` for question `id`. It must fit the question, and only
    /// the first answer is kept: the file is linked into place, which fails
    /// when one is already there, so two answers at once cannot both succeed.
    pub fn answer(&self, id: &str, answer: &Answer) -> Result<(), AnswerError> {
        let Some(ask) = self.get(id) else {
            return Err(AnswerError::NotFound);
        };
        if let Some(e) = self.closed_error(id) {
            return Err(e);
        }
        if hook_output(&ask, answer).is_none() {
            return Err(AnswerError::Invalid);
        }
        let bytes = serde_json::to_vec(answer).map_err(|e| AnswerError::Io(e.into()))?;
        match self.link_new(id, &self.path(id, ".answer.json"), &bytes) {
            Ok(true) => Ok(()),
            Ok(false) => Err(AnswerError::AlreadyAnswered),
            Err(e) => Err(AnswerError::Io(e)),
        }
    }

    /// The answer to `id`, once it is there and complete.
    pub fn read_answer(&self, id: &str) -> Option<Answer> {
        serde_json::from_slice(&read_file(&self.path(id, ".answer.json")).ok()?).ok()
    }

    /// Close question `id` and move it to `done/`. `false` when it was
    /// already closed (the first reason stays).
    pub fn close(&self, id: &str, reason: Reason) -> std::io::Result<bool> {
        let bytes = serde_json::to_vec(&Closed { reason }).map_err(std::io::Error::other)?;
        let first = self.link_new(id, &self.path(id, ".closed.json"), &bytes)?;
        // Best effort: whoever next finds it standing and closed moves it.
        let _ = self.retire(id);
        Ok(first)
    }

    /// Move closed question `id` out of the way, into `done/`, without
    /// replacing one already there.
    fn retire(&self, id: &str) -> std::io::Result<()> {
        let from = self.path(id, ".json");
        crate::store_dir::ensure_private_dir(&self.done())?;
        match std::fs::hard_link(&from, self.done_path(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        }
        ignore_missing(std::fs::remove_file(from))
    }

    /// What a hook waiting on `ask` should do now. An answer that fits is
    /// handed over only by whoever marks the question answered first; one
    /// that does not fit closes it as `invalid-answer`.
    pub fn poll(&self, ask: &Ask, now_ms: i64) -> std::io::Result<Poll> {
        let id = &ask.id;
        let answer_path = self.path(id, ".answer.json");
        match read_file(&answer_path) {
            Ok(bytes) => match serde_json::from_slice::<Answer>(&bytes) {
                Ok(answer) => {
                    return Ok(match hook_output(ask, &answer) {
                        Some(out) if self.close(id, Reason::Answered)? => Poll::Deliver(out),
                        Some(_) => Poll::Gone,
                        None => {
                            self.close(id, Reason::InvalidAnswer)?;
                            Poll::Gone
                        }
                    });
                }
                Err(_) if older_than(&answer_path, UNREADABLE_GRACE) => {
                    self.close(id, Reason::InvalidAnswer)?;
                    return Ok(Poll::Gone);
                }
                // Someone is still writing it in place: look again.
                Err(_) => {}
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if self.is_closed(id) {
            return Ok(Poll::Gone);
        }
        // The wall clock, not only the hook's own timer: that one stops
        // while the Mac sleeps.
        if ask.expired(now_ms) {
            self.close(id, Reason::Timeout)?;
            return Ok(Poll::Gone);
        }
        Ok(Poll::Wait)
    }

    /// Withdraw the standing questions of `session_id` that `moved`
    /// settles; on the way, close expired ones and move closed ones out.
    /// Returns how many were withdrawn.
    pub fn withdraw_session(
        &self,
        session_id: &str,
        moved: Moved<'_>,
        reason: Reason,
        now_ms: i64,
    ) -> std::io::Result<usize> {
        let mut n = 0;
        for ask in self.standing() {
            if self.is_closed(&ask.id) {
                let _ = self.retire(&ask.id);
            } else if ask.expired(now_ms) {
                self.close(&ask.id, Reason::Timeout)?;
            } else if ask.session_id == session_id
                && moved.settles(&ask)
                && self.close(&ask.id, reason)?
            {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Whether any question is standing; cheap, for the hooks that run on
    /// every event and usually have nothing to withdraw. Closed questions
    /// are in `done/` and do not count.
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
    /// `questions` moments ago, not withdrawn and not expired, perhaps
    /// already answered: PermissionRequest takes it over.
    pub fn recent_question(
        &self,
        session_id: &str,
        prompt_id: &str,
        questions: &[Value],
        now_ms: i64,
    ) -> Option<Ask> {
        self.standing().into_iter().rev().find(|a| {
            a.kind == Kind::Question
                && a.session_id == session_id
                && a.prompt_id == prompt_id
                && a.questions.as_deref() == Some(questions)
                && a.created_ms()
                    .is_some_and(|c| now_ms - c <= ADOPT_WITHIN_MS)
                && !a.expired(now_ms)
                && !self.is_closed(&a.id)
        })
    }

    /// Tidy up, best effort: close expired questions, move closed ones to
    /// `done/`, and delete what expired more than an hour ago, along with
    /// answers, withdrawals and temp files nothing refers to any more.
    pub fn sweep(&self, now_ms: i64) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        for name in &names {
            let path = self.dir.join(name);
            if name.starts_with('.') && name.ends_with(".tmp") {
                if older_than(&path, LEFTOVER) {
                    let _ = std::fs::remove_file(&path);
                }
            } else if let Some(id) = name.strip_suffix(".json").filter(|id| valid_id(id)) {
                match read_ask(&path) {
                    Some(_) if self.is_closed(id) => {
                        let _ = self.retire(id);
                    }
                    Some(ask) if ask.expired(now_ms) => {
                        let _ = self.close(id, Reason::Timeout);
                    }
                    Some(_) => {}
                    None if older_than(&path, LEFTOVER) => {
                        let _ = self.retire(id);
                    }
                    None => {}
                }
            } else if let Some(id) = name
                .strip_suffix(".answer.json")
                .or_else(|| name.strip_suffix(".closed.json"))
                .filter(|id| valid_id(id))
            {
                // Kept while its question is on file; an orphan goes once old.
                let orphan = !self.path(id, ".json").exists() && !self.done_path(id).exists();
                if orphan && older_than(&path, Duration::from_millis(KEEP_MS as u64)) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        for id in ids_in(&self.done()) {
            let path = self.done_path(&id);
            let stale = match read_ask(&path) {
                Some(ask) => ask.expires_ms().is_none_or(|e| e + KEEP_MS < now_ms),
                None => older_than(&path, Duration::from_millis(KEEP_MS as u64)),
            };
            if stale {
                for p in [
                    self.path(&id, ".answer.json"),
                    self.path(&id, ".closed.json"),
                    path,
                ] {
                    let _ = std::fs::remove_file(p);
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
            .custom_flags(libc::O_NOFOLLOW)
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
            input_hash: Some(input_hash(tool_input)),
        };
        self.build(Kind::Permission, None, Some(tool))
    }
}

/// A short fingerprint of a tool's input (FNV-1a over the JSON with sorted
/// keys), the same whatever order the keys came in.
pub fn input_hash(input: &Value) -> String {
    fn feed(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= u64::from(*b);
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn walk(h: &mut u64, v: &Value) {
        match v {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                feed(h, b"{");
                for k in keys {
                    feed(h, Value::String(k.clone()).to_string().as_bytes());
                    feed(h, b":");
                    walk(h, &map[k]);
                    feed(h, b",");
                }
                feed(h, b"}");
            }
            Value::Array(items) => {
                feed(h, b"[");
                for item in items {
                    walk(h, item);
                    feed(h, b",");
                }
                feed(h, b"]");
            }
            other => feed(h, other.to_string().as_bytes()),
        }
    }
    let mut h = 0xcbf2_9ce4_8422_2325;
    walk(&mut h, input);
    format!("{h:016x}")
}

/// What a tool is about to do, in at most [`PREVIEW_MAX`] characters: the
/// command, file or address when there is one, else the input as JSON.
/// Things that look like keys and tokens are masked.
pub fn preview(tool_input: &Value) -> String {
    let text = ["command", "file_path", "url", "path", "pattern"]
        .iter()
        .find_map(|k| tool_input.get(*k).and_then(Value::as_str))
        .map(String::from)
        .unwrap_or_else(|| tool_input.to_string());
    truncate(&redact(&text), PREVIEW_MAX)
}

/// Well-known key and token shapes: a prefix and how many key characters
/// must follow it.
const SECRETS: &[(&str, usize)] = &[
    ("sk-", 16),
    ("xoxa-", 8),
    ("xoxb-", 8),
    ("xoxe-", 8),
    ("xoxp-", 8),
    ("xoxr-", 8),
    ("xoxs-", 8),
    ("ghp_", 20),
    ("gho_", 20),
    ("ghs_", 20),
    ("ghu_", 20),
    ("ghr_", 20),
    ("github_pat_", 20),
    ("glpat-", 20),
    ("AKIA", 16),
    ("AIza", 30),
    ("Bearer ", 8),
    ("bearer ", 8),
];
const MASK: &str = "***";

/// `text` with whatever looks like a key or token after a known prefix
/// replaced by `***` (the prefix stays, so people can tell what it was).
pub fn redact(text: &str) -> String {
    let key_char =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+' | '=');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        let at_boundary = prev.is_none_or(|p| !(p.is_ascii_alphanumeric() || p == '_'));
        let hit = at_boundary
            .then(|| {
                SECRETS.iter().find_map(|(prefix, min)| {
                    let tail = rest.strip_prefix(prefix)?;
                    let len: usize = tail
                        .chars()
                        .take_while(|c| key_char(*c))
                        .map(char::len_utf8)
                        .sum();
                    (tail[..len].chars().count() >= *min).then_some((prefix.len(), len))
                })
            })
            .flatten();
        match hit {
            Some((prefix, len)) => {
                out.push_str(&rest[..prefix]);
                out.push_str(MASK);
                prev = Some('*');
                rest = &rest[prefix + len..];
            }
            None => {
                out.push(c);
                prev = Some(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
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

/// `answers` for `questions` from labels, or from free text when there is
/// one question. Labels go in question order (several in a row for a
/// multi-select question), or each as `N:label` for question `N` when every
/// choice is written that way. Multi-select labels are joined with `", "`.
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
    let numbered = numbered(questions.len(), choices);
    let mut rest = choices;
    for (i, q) in questions.iter().enumerate() {
        let key = question_text(q).ok_or(ChoiceError::Unanswered(i + 1))?;
        let options = labels(q);
        let mine: Vec<&str> = match &numbered {
            Some(pairs) => pairs
                .iter()
                .filter(|(n, _)| *n == i)
                .map(|(_, l)| *l)
                .collect(),
            None => {
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
                mine.iter().map(String::as_str).collect()
            }
        };
        if mine.is_empty() {
            return Err(ChoiceError::Unanswered(i + 1));
        }
        if !multi(q) && mine.len() > 1 {
            return Err(ChoiceError::OneOnly(i + 1));
        }
        if let Some(bad) = mine.iter().find(|c| !options.contains(c)) {
            return Err(ChoiceError::NotAnOption((*bad).to_string()));
        }
        out.insert(key.to_string(), mine.join(", "));
    }
    Ok(out)
}

/// `(question index, label)` for choices all written `N:label` with `N`
/// between 1 and `count`; `None` otherwise (the choices go in order).
fn numbered(count: usize, choices: &[String]) -> Option<Vec<(usize, &str)>> {
    if choices.is_empty() {
        return None;
    }
    choices
        .iter()
        .map(|c| {
            let (n, label) = c.split_once(':')?;
            let n: usize = n.trim().parse().ok()?;
            (1..=count).contains(&n).then_some((n - 1, label))
        })
        .collect()
}

/// What the PermissionRequest hook prints to hand `answer` to the session.
/// `None` when the answer does not fit the question (wrong kind, or a
/// question left unanswered).
pub fn hook_output(ask: &Ask, answer: &Answer) -> Option<Value> {
    let decision = match ask.kind {
        Kind::Question => {
            let questions = ask.questions.as_ref()?;
            let given = answer.answers.as_ref()?;
            if answer.behavior.is_some() {
                return None;
            }
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
        Kind::Permission => {
            if answer.answers.is_some() {
                return None;
            }
            match answer.behavior? {
                Behavior::Allow => json!({ "behavior": "allow" }),
                Behavior::Deny => match answer.message.as_deref().filter(|m| !m.is_empty()) {
                    Some(m) => json!({ "behavior": "deny", "message": m }),
                    None => json!({ "behavior": "deny" }),
                },
            }
        }
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
        let asked = json!({ "questions": [pick()], "answers": { "Pick one?": "A" } });
        let moved = Moved::Tool {
            name: ASK_TOOL,
            input: &asked,
        };
        let n = store
            .withdraw_session("S", moved, Reason::AnsweredInSession, NOW)
            .unwrap();
        assert_eq!(n, 2);
        let open: Vec<_> = store.open(NOW).into_iter().map(|a| a.id).collect();
        assert_eq!(open, ["a3", "b1"]);
        store
            .withdraw_session("S", Moved::All, Reason::SessionEnded, NOW)
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
    }

    #[test]
    fn ids_cannot_reach_outside_the_asks_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        // A question-shaped file right outside asks/: `../x` would find it.
        let planted = new_ask("x", "S", NOW)
            .question(&json!({ "questions": [pick()] }))
            .unwrap();
        std::fs::write(
            tmp.path().join("x.json"),
            serde_json::to_vec(&planted).unwrap(),
        )
        .unwrap();
        assert!(store.get("../x").is_none());
        assert!(matches!(
            store.find_open("../x", NOW),
            Err(AnswerError::NotFound)
        ));
        assert!(matches!(
            store.answer("../x", &cli(&[("Pick one?", "A")])),
            Err(AnswerError::NotFound)
        ));
        assert!(!tmp.path().join("x.answer.json").exists());
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

    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn set_age(path: &Path, age: Duration) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - age)
            .unwrap();
    }

    #[test]
    fn closed_questions_move_out_of_the_way() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        for i in 0..20 {
            question(&store, &format!("a{i}"), "S");
        }
        assert!(store.any());
        store
            .withdraw_session("S", Moved::All, Reason::AnsweredInSession, NOW)
            .unwrap();
        assert!(!store.any(), "only closed questions left: nothing standing");
        assert!(store.standing().is_empty());
        assert!(tmp.path().join("asks/done/a7.json").exists());
        assert!(!tmp.path().join("asks/a7.json").exists());
        // Still there to look up, with why it closed.
        assert_eq!(store.get("a7").unwrap().id, "a7");
        assert_eq!(
            store.closed("a7").unwrap().reason,
            Reason::AnsweredInSession
        );
        assert_eq!(store.all().len(), 20);
    }

    #[test]
    fn a_closed_question_left_standing_by_a_crash_is_moved_later() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        // The closed marker made it, the move did not.
        std::fs::write(
            tmp.path().join("asks/a1.closed.json"),
            r#"{"reason":"timeout"}"#,
        )
        .unwrap();
        assert!(store.any());
        store
            .withdraw_session("T", Moved::All, Reason::AnsweredInSession, NOW)
            .unwrap();
        assert!(!store.any());
        assert_eq!(store.closed("a1").unwrap().reason, Reason::Timeout);
    }

    #[test]
    fn an_answered_question_can_still_be_taken_over() {
        // Answered between PreToolUse and PermissionRequest: the answer
        // must reach the PermissionRequest that comes next.
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = question(&store, "a1", "S");
        store.answer("a1", &cli(&[("Pick one?", "B")])).unwrap();
        let seen = store
            .recent_question("S", "p1", &[pick()], NOW + 100)
            .unwrap();
        assert_eq!(seen.id, "a1");
        assert!(matches!(
            store.poll(&ask, NOW + 100).unwrap(),
            Poll::Deliver(_)
        ));
        // Withdrawn ones are not taken over.
        question(&store, "a2", "S");
        store.close("a2", Reason::AnsweredInSession).unwrap();
        assert!(store.recent_question("S", "p1", &[pick()], NOW).is_none());
    }

    #[test]
    fn poll_hands_a_fitting_answer_over_once() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = question(&store, "a1", "S");
        assert_eq!(store.poll(&ask, NOW).unwrap(), Poll::Wait);
        store.answer("a1", &cli(&[("Pick one?", "A")])).unwrap();
        let Poll::Deliver(out) = store.poll(&ask, NOW).unwrap() else {
            panic!("no delivery");
        };
        assert_eq!(
            out["hookSpecificOutput"]["decision"]["updatedInput"]["answers"]["Pick one?"],
            "A"
        );
        assert_eq!(store.closed("a1").unwrap().reason, Reason::Answered);
        assert_eq!(store.poll(&ask, NOW).unwrap(), Poll::Gone);
    }

    #[test]
    fn poll_does_not_hand_over_an_answer_once_withdrawn() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = question(&store, "a1", "S");
        store.answer("a1", &cli(&[("Pick one?", "A")])).unwrap();
        // The session answered in its terminal before the hook looked.
        store.close("a1", Reason::AnsweredInSession).unwrap();
        assert_eq!(store.poll(&ask, NOW).unwrap(), Poll::Gone);
        assert_eq!(
            store.closed("a1").unwrap().reason,
            Reason::AnsweredInSession
        );
    }

    #[test]
    fn poll_closes_on_an_answer_that_does_not_fit() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = new_ask("a1", "S", NOW)
            .question(&json!({ "questions": [pick(), many()] }))
            .unwrap();
        store.create(&ask).unwrap();
        // Written straight to the file by a client that got it wrong.
        std::fs::write(
            tmp.path().join("asks/a1.answer.json"),
            r#"{"schema":1,"by":"desktop","answers":{"Pick one?":"A"}}"#,
        )
        .unwrap();
        assert_eq!(store.poll(&ask, NOW).unwrap(), Poll::Gone);
        assert_eq!(store.closed("a1").unwrap().reason, Reason::InvalidAnswer);
        let closed = std::fs::read_to_string(tmp.path().join("asks/a1.closed.json")).unwrap();
        assert_eq!(closed, r#"{"reason":"invalid-answer"}"#);
    }

    #[test]
    fn an_unreadable_answer_closes_the_question_after_a_moment() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = question(&store, "a1", "S");
        let answer = tmp.path().join("asks/a1.answer.json");
        std::fs::write(&answer, "").unwrap();
        assert_eq!(
            store.poll(&ask, NOW).unwrap(),
            Poll::Wait,
            "maybe mid-write"
        );
        set_age(&answer, Duration::from_secs(5));
        assert_eq!(store.poll(&ask, NOW).unwrap(), Poll::Gone);
        assert_eq!(store.closed("a1").unwrap().reason, Reason::InvalidAnswer);
    }

    #[test]
    fn poll_times_out_by_the_wall_clock() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ask = question(&store, "a1", "S");
        let expires = ask.expires_ms().unwrap();
        assert_eq!(store.poll(&ask, expires - 1).unwrap(), Poll::Wait);
        assert_eq!(store.poll(&ask, expires).unwrap(), Poll::Gone);
        assert_eq!(store.closed("a1").unwrap().reason, Reason::Timeout);
        assert!(!store.any());
    }

    #[test]
    fn answers_that_do_not_fit_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        let allow = Answer {
            behavior: Some(Behavior::Allow),
            answers: None,
            ..cli(&[])
        };
        assert!(matches!(
            store.answer("a1", &allow),
            Err(AnswerError::Invalid)
        ));
        assert!(matches!(
            store.answer("a1", &cli(&[("Another question?", "A")])),
            Err(AnswerError::Invalid)
        ));
        assert!(!store.is_answered("a1"), "nothing written");
        store.answer("a1", &cli(&[("Pick one?", "A")])).unwrap();
    }

    #[test]
    fn an_answer_is_never_seen_half_written() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let long = "x".repeat(1 << 20);
        let torn = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for i in 0..30 {
            let id = format!("a{i}");
            question(&store, &id, "S");
            let path = tmp.path().join(format!("asks/{id}.answer.json"));
            let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let reader = {
                let (done, torn) = (done.clone(), torn.clone());
                std::thread::spawn(move || {
                    while !done.load(std::sync::atomic::Ordering::SeqCst) {
                        if let Ok(bytes) = std::fs::read(&path) {
                            if serde_json::from_slice::<Answer>(&bytes).is_err() {
                                torn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            }
                        }
                    }
                })
            };
            store.answer(&id, &cli(&[("Pick one?", &long)])).unwrap();
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            reader.join().unwrap();
        }
        assert_eq!(torn.load(std::sync::atomic::Ordering::SeqCst), 0);
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path().join("asks"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_tool_finishing_withdraws_only_its_own_request() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let ls = json!({ "command": "ls", "description": "List" });
        let rm = json!({ "command": "rm -rf build", "description": "Clean" });
        store
            .create(&new_ask("p1", "S", NOW).permission("Bash", &ls))
            .unwrap();
        store
            .create(&new_ask("p2", "S", NOW).permission("Bash", &rm))
            .unwrap();
        // The same input with its keys in another order is the same call.
        let ls_again = json!({ "description": "List", "command": "ls" });
        let n = store
            .withdraw_session(
                "S",
                Moved::Tool {
                    name: "Bash",
                    input: &ls_again,
                },
                Reason::AnsweredInSession,
                NOW,
            )
            .unwrap();
        assert_eq!(n, 1);
        let open: Vec<_> = store.open(NOW).into_iter().map(|a| a.id).collect();
        assert_eq!(open, ["p2"], "the other Bash call still waits");

        // Questions: only the one with the same questions.
        question(&store, "q1", "S");
        let other = new_ask("q2", "S", NOW)
            .question(&json!({ "questions": [many()] }))
            .unwrap();
        store.create(&other).unwrap();
        let answered = json!({ "questions": [many()], "answers": { "Which ones?": "X" } });
        store
            .withdraw_session(
                "S",
                Moved::Tool {
                    name: ASK_TOOL,
                    input: &answered,
                },
                Reason::AnsweredInSession,
                NOW,
            )
            .unwrap();
        let open: Vec<_> = store.open(NOW).into_iter().map(|a| a.id).collect();
        assert_eq!(open, ["p2", "q1"]);
    }

    #[test]
    fn input_hash_ignores_key_order_only() {
        let a = json!({ "command": "ls", "description": "d", "nested": { "x": 1, "y": [1, 2] } });
        let b = json!({ "nested": { "y": [1, 2], "x": 1 }, "description": "d", "command": "ls" });
        assert_eq!(input_hash(&a), input_hash(&b));
        assert_ne!(input_hash(&a), input_hash(&json!({ "command": "ls" })));
        assert_ne!(
            input_hash(&json!({ "y": [1, 2] })),
            input_hash(&json!({ "y": [2, 1] }))
        );
        assert_eq!(input_hash(&a).len(), 16);
    }

    #[test]
    fn files_are_for_the_owner_only() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        assert_eq!(mode(&tmp.path().join("asks/a1.json")), 0o600);
        store.answer("a1", &cli(&[("Pick one?", "A")])).unwrap();
        assert_eq!(mode(&tmp.path().join("asks/a1.answer.json")), 0o600);
        store.close("a1", Reason::Answered).unwrap();
        assert_eq!(mode(&tmp.path().join("asks/a1.closed.json")), 0o600);
        assert_eq!(mode(&tmp.path().join("asks/done/a1.json")), 0o600);
        assert_eq!(mode(&tmp.path().join("asks/done")), 0o700);
        store.take("me").unwrap();
        assert_eq!(mode(&tmp.path().join("asks/takers/me.alive")), 0o600);
    }

    #[test]
    fn only_a_private_asks_dir_is_trusted() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        assert!(!store.is_private(), "missing");
        store.take("me").unwrap();
        assert!(store.is_private());
        let asks = tmp.path().join("asks");
        std::fs::set_permissions(&asks, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(!store.is_private(), "group can get in");
        std::fs::set_permissions(&asks, std::fs::Permissions::from_mode(0o700)).unwrap();
        // A symlink to a private dir is not the dir.
        let other = tempfile::tempdir().unwrap();
        let elsewhere = Store::new(other.path());
        std::os::unix::fs::symlink(&asks, other.path().join("asks")).unwrap();
        assert!(!elsewhere.is_private());
    }

    #[test]
    fn a_symlinked_answer_is_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        question(&store, "a1", "S");
        let real = tmp.path().join("elsewhere.json");
        std::fs::write(
            &real,
            r#"{"schema":1,"by":"cli","answers":{"Pick one?":"A"}}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&real, tmp.path().join("asks/a1.answer.json")).unwrap();
        assert!(store.read_answer("a1").is_none());
    }

    #[test]
    fn sweep_clears_leftovers_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        let asks = tmp.path().join("asks");
        question(&store, "a1", "S");
        // A withdrawal whose question is gone (deleted while its hook slept).
        let orphan = asks.join("0abc.closed.json");
        std::fs::write(&orphan, r#"{"reason":"timeout"}"#).unwrap();
        let fresh_orphan = asks.join("0abd.answer.json");
        std::fs::write(&fresh_orphan, "{}").unwrap();
        let tmp_file = asks.join(".a9.new.1.2.tmp");
        std::fs::write(&tmp_file, "").unwrap();
        set_age(&orphan, Duration::from_secs(2 * 3600));
        set_age(&tmp_file, Duration::from_secs(120));
        store.sweep(NOW);
        assert!(!orphan.exists());
        assert!(fresh_orphan.exists(), "may still get its question");
        assert!(!tmp_file.exists());
        assert!(asks.join("a1.json").exists(), "open: untouched");
    }

    #[test]
    fn redacts_keys_and_tokens_in_previews() {
        let cases = [
            (
                "curl -H 'Authorization: Bearer abcdef0123456789' x",
                "curl -H 'Authorization: Bearer ***' x",
            ),
            (
                "export OPENAI_API_KEY=sk-proj-abcdefghijklmnop1234",
                "export OPENAI_API_KEY=sk-***",
            ),
            (
                "git push https://ghp_abcdefghijklmnopqrstuvwxyz0123@github.com/x",
                "git push https://ghp_***@github.com/x",
            ),
            ("SLACK=xoxb-1234-5678-abcdefgh", "SLACK=xoxb-***"),
            ("aws AKIAABCDEFGHIJKLMNOP s3 ls", "aws AKIA*** s3 ls"),
            ("ls -la", "ls -la"),
            ("risk-free task", "risk-free task"),
            ("sk-short", "sk-short"),
            ("中文 sk-abcdefghijklmnopqrst 完", "中文 sk-*** 完"),
        ];
        for (input, want) in cases {
            assert_eq!(redact(input), want, "{input}");
        }
        let ask = new_ask("p", "S", NOW).permission(
            "Bash",
            &json!({ "command": "TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123 ./deploy" }),
        );
        assert_eq!(ask.tool.unwrap().preview, "TOKEN=ghp_*** ./deploy");
        assert_eq!(
            preview(&json!({ "key": "sk-abcdefghijklmnopqrstuv" })),
            r#"{"key":"sk-***"}"#
        );
    }

    #[test]
    fn numbered_choices_go_to_their_question() {
        let yes_no = |q: &str, several: bool| {
            json!({ "question": q, "header": q,
                "options": [{"label": "yes", "description": ""}, {"label": "no", "description": ""}],
                "multiSelect": several })
        };
        let qs = [yes_no("First?", true), yes_no("Second?", false)];
        // In order, the multi-select question takes both labels.
        assert_eq!(
            answers_from(&qs, &["yes".into(), "no".into()], None),
            Err(ChoiceError::Unanswered(2))
        );
        let a = answers_from(&qs, &["1:yes".into(), "2:no".into()], None).unwrap();
        assert_eq!(a["First?"], "yes");
        assert_eq!(a["Second?"], "no");
        let a = answers_from(&qs, &["2:yes".into(), "1:no".into(), "1:yes".into()], None).unwrap();
        assert_eq!(a["First?"], "no, yes");
        assert_eq!(
            answers_from(&qs, &["1:yes".into()], None),
            Err(ChoiceError::Unanswered(2))
        );
        assert_eq!(
            answers_from(&qs, &["1:maybe".into(), "2:no".into()], None),
            Err(ChoiceError::NotAnOption("maybe".into()))
        );
    }
}
