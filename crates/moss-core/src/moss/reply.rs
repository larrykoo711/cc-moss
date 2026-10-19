//! Taking Moss's reply to a message from its transcript (reference study
//! §2.2, design K2).
//!
//! Moss's Stop hook (`cc-moss hook moss-stop`) drops a marker in
//! `<home>/stop/` when a turn ends. Then the reply is looked up in the
//! transcript: the user entry stamped after the message went in and holding
//! its end, then the assistant text after it with the **latest timestamp**
//! (lines are not always written in time order), outside side chains, that
//! ended the turn (`end_turn` / `stop_sequence`). The marker can beat the
//! transcript to disk, so it is looked for again for a few seconds. `/clear`
//! moves Moss to a new transcript in the same folder; a newer one there is
//! followed. When none of that finds the reply (the format changed, the file
//! went), a "received" stands in for it so the queue moves on.
//!
//! A turn longer than [`Timing::turn`]: a Moss still working is left alone
//! and its reply is taken late (up to [`Timing::late`]); a Moss stopped at a
//! confirm prompt is reported as waiting, and never sent Esc.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::inbox::{self, Pane};

/// `<home>/stop/`: one file per ended turn, from Moss's Stop hook.
pub fn stop_dir(home: &Path) -> PathBuf {
    home.join("stop")
}

/// `<home>/waiting`: there while Moss waits on a confirm prompt.
pub fn waiting_path(home: &Path) -> PathBuf {
    home.join("waiting")
}

/// Drop a marker (the Stop hook).
pub fn mark_stop(home: &Path, now_ms: i64) -> std::io::Result<()> {
    let name = format!("{now_ms:013}-{}", std::process::id());
    // Made if missing (the home too); then used only through its descriptor.
    super::home::ensure_real_dir(home, "stop")?;
    super::dir::Dir::open(home, "stop")?.write(&name, "")
}

/// Take every marker there is. True when there was one.
pub fn take_stops(home: &Path) -> bool {
    // Never through a link: a linked folder is replaced, not emptied, and
    // the folder is held open while it is emptied (see [`super::dir`]).
    let linked = std::fs::symlink_metadata(stop_dir(home)).is_ok_and(|m| !m.file_type().is_dir());
    let Ok(stop) = super::dir::Dir::open(home, "stop") else {
        return false;
    };
    if linked {
        return false;
    }
    let mut any = false;
    for name in stop.entries().unwrap_or_default() {
        // A link is removed (the link, not what it points at) and counts.
        if stop.remove(&name).is_ok() {
            any = true;
        }
    }
    any
}

/// How long things may take. Tests shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// After a Stop marker, how long the transcript may lag behind it.
    pub settle: Duration,
    /// How often the transcript is read without a marker (a hook that did
    /// not run) and, past `turn`, the screen.
    pub look_every: Duration,
    /// A message whose user entry never shows up is given up after this.
    pub no_start: Duration,
    /// A turn longer than this is checked on screen.
    pub turn: Duration,
    /// A long turn's reply is taken up to this long after the message.
    pub late: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            settle: Duration::from_secs(3),
            look_every: Duration::from_secs(2),
            no_start: Duration::from_secs(60),
            turn: Duration::from_secs(300),
            late: Duration::from_secs(15 * 60),
        }
    }
}

/// Clocks of different writers and second-rounded stamps: a user entry
/// stamped up to this much before the paste still counts.
const SKEW_MS: i64 = 2_000;

/// What the transcript holds for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// No user entry with the message's end after it went in.
    NoUser,
    /// The message is there, its reply not (yet).
    NoReply,
    Reply {
        text: String,
        at_ms: i64,
    },
}

/// One transcript line, as far as claiming goes.
struct Record {
    kind: String,
    at_ms: i64,
    sidechain: bool,
    meta: bool,
    stop: Option<String>,
    text: String,
    /// A user entry that only carries a tool's result.
    tool_result: bool,
}

impl Record {
    /// Something a person (or cc-moss) said, not a tool result or meta.
    fn spoken(&self) -> bool {
        self.kind == "user" && !self.meta && !self.sidechain && !self.tool_result
    }

    /// Claude Code's own mark that a turn ended.
    fn turn_end(&self) -> bool {
        self.kind == "turn_end"
    }
}

fn record(line: &str) -> Option<Record> {
    let v: Value = serde_json::from_str(line).ok()?;
    let kind = v.get("type")?.as_str()?.to_string();
    let at_ms = parse_ms(v.get("timestamp")?.as_str()?)?;
    let sidechain = v.get("isSidechain").and_then(Value::as_bool) == Some(true);
    if kind == "system" {
        let subtype = v.get("subtype")?.as_str()?;
        if subtype != "turn_duration" && subtype != "stop_hook_summary" {
            return None;
        }
        return Some(Record {
            kind: "turn_end".into(),
            at_ms,
            sidechain,
            meta: false,
            stop: None,
            text: String::new(),
            tool_result: false,
        });
    }
    if kind != "user" && kind != "assistant" {
        return None;
    }
    let message = v.get("message")?;
    let content = message.get("content")?;
    Some(Record {
        kind,
        at_ms,
        sidechain,
        meta: v.get("isMeta").and_then(Value::as_bool) == Some(true),
        stop: message
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(String::from),
        text: text_of(content),
        tool_result: content.as_array().is_some_and(|parts| {
            parts
                .iter()
                .any(|p| p.get("type").and_then(Value::as_str) == Some("tool_result"))
        }),
    })
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| {
                p.get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|t| t == "text")
            })
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// RFC 3339 to Unix milliseconds.
pub fn parse_ms(stamp: &str) -> Option<i64> {
    let t =
        time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(t.unix_timestamp_nanos() / 1_000_000).ok()
}

/// The lines of `path` from byte `from` on (from 0 when the file is
/// shorter: it was replaced).
fn lines_from(path: &Path, from: u64) -> Option<Vec<String>> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let from = if from > size { 0 } else { from };
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(
        String::from_utf8_lossy(&bytes)
            .lines()
            .map(String::from)
            .collect(),
    )
}

/// Look up the reply to the message ending in `tail`, sent at `sent_ms`,
/// in `path` from byte `from` on.
pub fn find(path: &Path, from: u64, tail: &str, sent_ms: i64) -> Found {
    let Some(lines) = lines_from(path, from) else {
        return Found::NoUser;
    };
    let records: Vec<Record> = lines.iter().filter_map(|l| record(l)).collect();
    let asked = records
        .iter()
        .filter(|r| r.spoken() && r.at_ms >= sent_ms - SKEW_MS)
        .filter(|r| inbox::squeeze(&r.text).contains(tail))
        .map(|r| r.at_ms)
        .min();
    let Some(asked) = asked else {
        return Found::NoUser;
    };
    // Up to the next thing said: what follows answers that, not this.
    let until = records
        .iter()
        .filter(|r| r.spoken() && r.at_ms > asked && !r.text.trim().is_empty())
        .map(|r| r.at_ms)
        .min()
        .unwrap_or(i64::MAX);
    let within = |r: &&Record| r.at_ms >= asked && r.at_ms < until && !r.sidechain;
    // Text only: a thinking block can end a turn, too.
    let said: Vec<&Record> = records
        .iter()
        .filter(within)
        .filter(|r| r.kind == "assistant" && !r.text.trim().is_empty())
        .collect();
    let ends_turn = |r: &&&Record| matches!(r.stop.as_deref(), Some("end_turn" | "stop_sequence"));
    let ended = records.iter().filter(within).any(Record::turn_end);
    let mut best: Option<&Record> = None;
    // The text that ended the turn; failing that, once Claude Code marked
    // the turn's end, its last text.
    let pick: Vec<&&Record> = if said.iter().any(|r| ends_turn(&r)) {
        said.iter().filter(ends_turn).collect()
    } else if ended {
        said.iter().collect()
    } else {
        Vec::new()
    };
    for r in pick {
        // By time, not by place in the file; a tie goes to the later line.
        if best.is_none_or(|b| r.at_ms >= b.at_ms) {
            best = Some(r);
        }
    }
    match best {
        Some(r) => Found::Reply {
            text: r.text.trim().to_string(),
            at_ms: r.at_ms,
        },
        None => Found::NoReply,
    }
}

/// After `/clear`: the newest other transcript in the same folder written
/// since `since_ms`.
pub fn newer_transcript(current: &Path, since_ms: i64) -> Option<PathBuf> {
    let dir = current.parent()?;
    let mut best: Option<(i64, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p == current || p.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        let Some(m) = e.metadata().ok().and_then(|m| m.modified().ok()) else {
            continue;
        };
        let ms = millis(m);
        if ms + SKEW_MS < since_ms {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| ms > *b) {
            best = Some((ms, p));
        }
    }
    best.map(|(_, p)| p)
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Claude Code waiting on a choice: its permission or question prompt at
/// the bottom of the screen, where the input box would be. The same words
/// higher up, with the input box below them, are only conversation.
pub fn confirm_prompt(screen: &str) -> bool {
    let lines: Vec<String> = inbox::plain(screen).lines().map(String::from).collect();
    let asks = |l: &String| l.contains("Do you want to") || l.contains("Would you like to");
    match (0..lines.len())
        .rev()
        .find(|i| inbox::prompt_line(&lines[*i]))
    {
        // The last prompt is a dialog's first choice.
        Some(i) => {
            let after = lines[i].split_once('❯').map_or("", |(_, a)| a);
            after.trim_start().starts_with("1.") || lines[i + 1..].iter().any(asks)
        }
        None => lines.iter().any(asks),
    }
}

/// Claude Code working on a turn.
pub fn working(screen: &str) -> bool {
    let text = inbox::plain(screen).to_lowercase();
    text.contains("esc to interrupt") || text.contains("esc to cancel")
}

/// What to do about the message in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Not yet.
    Wait,
    Reply {
        text: String,
        at_ms: i64,
    },
    /// No reply to be had: say it was received.
    Received(String),
}

/// Looking for the reply to one message.
#[derive(Debug, Clone)]
pub struct Claim {
    transcript: PathBuf,
    from: u64,
    tail: String,
    sent_ms: i64,
    marker_at: Option<i64>,
    next_look: i64,
    waiting: bool,
    timing: Timing,
}

impl Claim {
    /// `transcript` as it was when the message went in, and its size then.
    pub fn new(transcript: PathBuf, from: u64, tail: &str, sent_ms: i64, timing: Timing) -> Claim {
        Claim {
            transcript,
            from,
            tail: tail.to_string(),
            sent_ms,
            marker_at: None,
            next_look: sent_ms + ms(timing.look_every),
            waiting: false,
            timing,
        }
    }

    /// Moss is stopped at a confirm prompt.
    pub fn waiting(&self) -> bool {
        self.waiting
    }

    pub fn transcript(&self) -> &Path {
        &self.transcript
    }

    /// The transcript, following `/clear` to a newer one.
    fn look(&mut self) -> Found {
        let found = find(&self.transcript, self.from, &self.tail, self.sent_ms);
        if found != Found::NoUser {
            return found;
        }
        if let Some(newer) = newer_transcript(&self.transcript, self.sent_ms) {
            let there = find(&newer, 0, &self.tail, self.sent_ms);
            if there != Found::NoUser {
                self.transcript = newer;
                self.from = 0;
                return there;
            }
        }
        Found::NoUser
    }

    /// One go. `stopped`: a Stop marker came in since the last go.
    pub fn tick(&mut self, now_ms: i64, stopped: bool, pane: &mut dyn Pane) -> Verdict {
        if stopped && self.marker_at.is_none() {
            self.marker_at = Some(now_ms);
        }
        if let Some(at) = self.marker_at {
            let found = self.look();
            if let Found::Reply { text, at_ms } = found {
                return Verdict::Reply { text, at_ms };
            }
            if now_ms - at < ms(self.timing.settle) {
                return Verdict::Wait;
            }
            self.marker_at = None;
            if found == Found::NoReply
                && crate::transcript::turn_open(&self.transcript) == Some(true)
            {
                // A marker left from an earlier turn; this one goes on.
                return Verdict::Wait;
            }
            return Verdict::Received(match found {
                Found::NoUser => "the message was not found in Moss's transcript".into(),
                _ => "Moss's reply was not found in its transcript".into(),
            });
        }
        if now_ms < self.next_look {
            return Verdict::Wait;
        }
        self.next_look = now_ms + ms(self.timing.look_every);
        let elapsed = now_ms - self.sent_ms;
        let found = self.look();
        // A turn that ended without its hook.
        if let Found::Reply { text, at_ms } = &found {
            if crate::transcript::turn_open(&self.transcript) == Some(false) {
                return Verdict::Reply {
                    text: text.clone(),
                    at_ms: *at_ms,
                };
            }
        }
        if found == Found::NoUser && elapsed >= ms(self.timing.no_start) {
            return Verdict::Received("the message never showed up in Moss's transcript".into());
        }
        if elapsed < ms(self.timing.turn) {
            return Verdict::Wait;
        }
        let screen = pane.screen().unwrap_or_default();
        let in_turn = crate::transcript::turn_open(&self.transcript) == Some(true);
        let late = elapsed >= ms(self.timing.late);
        self.waiting = in_turn && !late && confirm_prompt(&screen);
        if self.waiting {
            // Its choice to make, or yours in its terminal: no Esc.
            return Verdict::Wait;
        }
        if working(&screen) && !late {
            return Verdict::Wait;
        }
        match found {
            Found::Reply { text, at_ms } => Verdict::Reply { text, at_ms },
            _ => Verdict::Received("no reply from Moss in time".into()),
        }
    }
}

fn ms(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moss::inbox::fake::FakePane;

    /// 2026-10-19T08:00:00Z: the fixtures' messages go in at this moment.
    const SENT: i64 = 1_792_396_800_000;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/moss")
            .join(name)
    }

    fn tail() -> String {
        inbox::tail("which sessions need me right now?")
    }

    fn reply(text: &str, at: &str) -> Found {
        Found::Reply {
            text: text.into(),
            at_ms: parse_ms(at).unwrap(),
        }
    }

    #[test]
    fn claim_picks_latest_by_timestamp() {
        // The final answer is written before an earlier partial one.
        assert_eq!(
            find(&fixture("out-of-order.jsonl"), 0, &tail(), SENT),
            reply("Two need you: api and docs.", "2026-10-19T08:00:09.500Z")
        );
    }

    #[test]
    fn claim_skips_sidechain() {
        assert_eq!(
            find(&fixture("sidechain.jsonl"), 0, &tail(), SENT),
            reply("None right now.", "2026-10-19T08:00:05.000Z")
        );
    }

    #[test]
    fn claim_ignores_what_came_before_the_message() {
        // The same question asked an hour earlier, and its answer.
        assert_eq!(
            find(&fixture("asked-before.jsonl"), 0, &tail(), SENT),
            Found::NoUser
        );
        assert_eq!(
            find(&fixture("asked-before.jsonl"), 0, &tail(), SENT - 3_600_000),
            reply("Earlier answer.", "2026-10-19T07:00:05.000Z")
        );
    }

    #[test]
    fn claim_falls_back_to_received() {
        let dir = tempfile::tempdir().unwrap();
        // A transcript in a format cc-moss does not know.
        let path = dir.path().join("t.jsonl");
        std::fs::write(
            &path,
            "{\"kind\":\"turn\",\"said\":\"which sessions need me right now?\"}\n",
        )
        .unwrap();
        let timing = Timing::default();
        let mut c = Claim::new(path, 0, &tail(), SENT, timing);
        let mut pane = FakePane::default();
        assert_eq!(c.tick(SENT + 5_000, true, &mut pane), Verdict::Wait);
        assert_eq!(c.tick(SENT + 6_000, false, &mut pane), Verdict::Wait);
        assert!(matches!(
            c.tick(SENT + 8_000, false, &mut pane),
            Verdict::Received(_)
        ));
        // No marker at all (the hook never ran): given up in time, too.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, "").unwrap();
        let mut c = Claim::new(path, 0, &tail(), SENT, timing);
        let mut verdict = Verdict::Wait;
        let mut now = SENT;
        while verdict == Verdict::Wait && now < SENT + 3_600_000 {
            now += 1_000;
            verdict = c.tick(now, false, &mut pane);
        }
        assert!(matches!(verdict, Verdict::Received(_)), "{verdict:?}");
        assert!(now <= SENT + ms(timing.no_start) + 2_000, "{now}");
    }

    #[test]
    fn claim_waits_for_a_transcript_behind_its_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let text = std::fs::read_to_string(fixture("sidechain.jsonl")).unwrap();
        let (first, rest) = text.split_at(text.find('\n').unwrap() + 1);
        std::fs::write(&path, first).unwrap();
        let mut c = Claim::new(path.clone(), 0, &tail(), SENT, Timing::default());
        let mut pane = FakePane::default();
        assert_eq!(c.tick(SENT + 5_000, true, &mut pane), Verdict::Wait);
        assert_eq!(c.tick(SENT + 7_000, false, &mut pane), Verdict::Wait);
        std::fs::write(&path, format!("{first}{rest}")).unwrap();
        assert_eq!(
            c.tick(SENT + 7_500, false, &mut pane),
            Verdict::Reply {
                text: "None right now.".into(),
                at_ms: parse_ms("2026-10-19T08:00:05.000Z").unwrap()
            }
        );
    }

    #[test]
    fn claim_follows_clear() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.jsonl");
        let new = dir.path().join("new.jsonl");
        std::fs::copy(fixture("asked-before.jsonl"), &old).unwrap();
        std::fs::copy(fixture("sidechain.jsonl"), &new).unwrap();
        // Written after the message went in: /clear started a new transcript.
        let at = |ms: i64| UNIX_EPOCH + Duration::from_millis(ms as u64);
        for (p, t) in [(&old, SENT - 3_600_000), (&new, SENT + 4_000)] {
            File::options()
                .write(true)
                .open(p)
                .unwrap()
                .set_modified(at(t))
                .unwrap();
        }
        let size = std::fs::metadata(&old).unwrap().len();
        let mut c = Claim::new(old, size, &tail(), SENT, Timing::default());
        let verdict = c.tick(SENT + 5_000, true, &mut FakePane::default());
        assert_eq!(
            verdict,
            Verdict::Reply {
                text: "None right now.".into(),
                at_ms: parse_ms("2026-10-19T08:00:05.000Z").unwrap()
            }
        );
        assert_eq!(c.transcript(), new);
        // An older file in the folder is not followed.
        assert_eq!(newer_transcript(&new, SENT), None);
    }

    #[test]
    fn claim_late_and_confirm_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        // Only the user entry: Moss is still on it.
        let text = std::fs::read_to_string(fixture("sidechain.jsonl")).unwrap();
        std::fs::write(&path, text.lines().next().unwrap()).unwrap();
        let t = Timing::default();
        let mut c = Claim::new(path, 0, &tail(), SENT, t);
        let mut pane = FakePane {
            screen_override: Some("✻ Thinking… (esc to interrupt)\n❯ \n".into()),
            ..FakePane::default()
        };
        let past_turn = SENT + ms(t.turn) + 1_000;
        assert_eq!(c.tick(past_turn, false, &mut pane), Verdict::Wait);
        assert!(!c.waiting());
        // A confirm prompt: waiting, and never an Esc.
        pane.screen_override = Some("Do you want to proceed?\n❯ 1. Yes\n  2. No\n".into());
        assert_eq!(c.tick(past_turn + 3_000, false, &mut pane), Verdict::Wait);
        assert!(c.waiting());
        assert_eq!(
            c.tick(SENT + ms(t.late) - 1_000, false, &mut pane),
            Verdict::Wait
        );
        assert!(pane.calls.is_empty(), "never an Esc: {:?}", pane.calls);
        // Waiting has the same limit as a late reply.
        assert!(matches!(
            c.tick(SENT + ms(t.late) + 60_000, false, &mut pane),
            Verdict::Received(_)
        ));
        assert!(!c.waiting());
        // Working on past the late limit: received too.
        let mut c = Claim::new(c.transcript().to_path_buf(), 0, &tail(), SENT, t);
        pane.screen_override = Some("(esc to interrupt)\n".into());
        assert_eq!(c.tick(past_turn, false, &mut pane), Verdict::Wait);
        assert!(matches!(
            c.tick(SENT + ms(t.late) + 70_000, false, &mut pane),
            Verdict::Received(_)
        ));
    }

    #[test]
    fn confirm_prompt_counts_only_at_the_bottom_while_in_a_turn() {
        assert!(confirm_prompt(
            "Do you want to proceed?\n❯ 1. Yes\n  2. No\n"
        ));
        assert!(confirm_prompt(
            "│ Do you want to make this edit?        │\n│ ❯ 1. Yes                │\n"
        ));
        // The same words earlier in the conversation, the input box below.
        assert!(!confirm_prompt(
            "Do you want to proceed?\n❯ 1. Yes\n  2. No\n────\n❯ \n────\n"
        ));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        // The turn is over (and left no text): a dialog on screen is not Moss's.
        let user = std::fs::read_to_string(fixture("sidechain.jsonl")).unwrap();
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                user.lines().next().unwrap(),
                r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-10-19T08:00:09.000Z"}"#
            ),
        )
        .unwrap();
        let t = Timing::default();
        let mut c = Claim::new(path, 0, &tail(), SENT, t);
        let mut pane = FakePane {
            screen_override: Some("Do you want to proceed?\n❯ 1. Yes\n".into()),
            ..FakePane::default()
        };
        assert!(matches!(
            c.tick(SENT + ms(t.turn) + 1_000, false, &mut pane),
            Verdict::Received(_)
        ));
        assert!(!c.waiting());
    }

    #[test]
    fn claim_takes_text_never_thinking() {
        assert_eq!(
            find(&fixture("thinking-last.jsonl"), 0, &tail(), SENT),
            reply("Only api needs you.", "2026-10-19T08:00:05.000Z")
        );
    }

    #[test]
    fn claim_after_a_turn_end_without_stop_reason() {
        assert_eq!(
            find(&fixture("no-stop-reason.jsonl"), 0, &tail(), SENT),
            reply(
                "api is waiting on a permission.",
                "2026-10-19T08:00:06.000Z"
            )
        );
        // Without the turn's end the reply is not taken yet.
        let text = std::fs::read_to_string(fixture("no-stop-reason.jsonl")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let open: Vec<&str> = text
            .lines()
            .filter(|l| !l.contains("turn_duration"))
            .collect();
        std::fs::write(&path, open.join("\n")).unwrap();
        assert_eq!(find(&path, 0, &tail(), SENT), Found::NoReply);
    }

    #[test]
    fn claim_stops_at_the_next_question() {
        assert_eq!(
            find(&fixture("next-question.jsonl"), 0, &tail(), SENT),
            reply("None right now.", "2026-10-19T08:00:05.000Z")
        );
    }

    #[test]
    fn stop_markers_are_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!take_stops(dir.path()));
        mark_stop(dir.path(), 1).unwrap();
        mark_stop(dir.path(), 2).unwrap();
        assert!(take_stops(dir.path()));
        assert!(!take_stops(dir.path()));
    }
}
