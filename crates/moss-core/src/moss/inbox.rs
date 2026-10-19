//! Messages for Moss wait in `<state>/moss/inbox/`, one file each, until the
//! daemon types them into Moss's terminal. Only the daemon touches Moss's
//! tmux (design A9): `moss say` and the app only drop a file here.
//!
//! Typing a message in (reference study §2.2): paste the whole text as one
//! bracketed paste, wait, read the input line back and look for the end of
//! the message there; only then press Enter. A paste that does not show is
//! pasted again, then once more after Esc, and otherwise given up (the
//! message is marked failed and the next one goes on). The text never goes
//! through `send-keys`, which would type it key by key and submit at its
//! first line break.
//!
//! The tmux side is behind [`Pane`], so tests type into a fake.

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// `<home>/inbox/`.
pub fn dir(home: &Path) -> PathBuf {
    home.join("inbox")
}

/// How much of the message's end is looked for on screen.
pub const TAIL_CHARS: usize = 12;

/// How long a paste has to show up.
pub const SETTLE: Duration = Duration::from_millis(300);

/// One message waiting for Moss: `<inbox>/<id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub schema: u32,
    pub id: String,
    pub at: String,
    /// `you`, `events`; `slack` later.
    pub from: String,
    pub text: String,
}

/// `<13-digit unix millis>-<6 hex>`: sorts by time, and two messages in the
/// same millisecond still differ.
pub fn new_id(now_ms: i64) -> String {
    let random = ulid::Ulid::generate().random() as u32 & 0x00ff_ffff;
    format!("{:013}-{random:06x}", now_ms.max(0))
}

/// Drop `message` in the inbox. Never replaces a file (`O_EXCL`).
pub fn put(home: &Path, message: &Message) -> io::Result<PathBuf> {
    let dir = dir(home);
    crate::store_dir::ensure_private_dir(&dir)?;
    let path = dir.join(format!("{}.json", message.id));
    let mut text = serde_json::to_string(message).map_err(io::Error::other)?;
    text.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}

/// What waits, oldest first. A file that does not read as a message comes
/// with `None`, so it can be put aside rather than block the rest.
pub fn waiting(home: &Path) -> Vec<(PathBuf, Option<Message>)> {
    let Ok(entries) = std::fs::read_dir(dir(home)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let m = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| serde_json::from_str::<Message>(&t).ok());
            (p, m)
        })
        .collect()
}

/// How many messages wait.
pub fn count(home: &Path) -> usize {
    waiting(home).len()
}

// ---------------------------------------------------------------- the terminal

/// Moss's terminal, as far as typing a message in goes.
pub trait Pane {
    /// Paste `text` as one bracketed paste (`load-buffer` + `paste-buffer -p -d`).
    fn paste(&mut self, text: &str) -> Result<(), String>;
    /// What the pane shows, wrapped lines joined, with its colours
    /// (`capture-pane -p -J -e`).
    fn screen(&mut self) -> Result<String, String>;
    /// Press one key (`send-keys Enter`, `send-keys Escape`).
    fn key(&mut self, key: &str) -> Result<(), String>;
    fn wait(&mut self, how_long: Duration);
}

/// Claude Code's prompt mark, in front of the input line.
const PROMPT: char = '❯';

/// Claude Code shows a long paste as this instead of its text.
const PASTED: &str = "[Pastedtext";

/// Whether Moss's input line is empty: the last line with the prompt mark
/// has nothing typed after it. Placeholder hints (dim) and the cursor
/// (inverse) are not typing. No prompt on screen (starting up, a dialog)
/// is not empty: nothing is pasted then.
pub fn input_empty(screen: &str) -> bool {
    let Some(line) = screen.lines().rev().find(|l| l.contains(PROMPT)) else {
        return false;
    };
    let after = &line[line.rfind(PROMPT).unwrap_or(0) + PROMPT.len_utf8()..];
    !typed(after)
}

/// Whether `styled` shows a character in plain (not dim, not inverse) style.
fn typed(styled: &str) -> bool {
    let (mut dim, mut inverse) = (false, false);
    let mut chars = styled.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                let mut params = String::new();
                for p in chars.by_ref() {
                    if p.is_ascii_alphabetic() || p == '~' {
                        if p == 'm' {
                            apply_sgr(&params, &mut dim, &mut inverse);
                        }
                        break;
                    }
                    params.push(p);
                }
            } else {
                chars.next();
            }
            continue;
        }
        // Box borders (│ ╭ ─) frame the input; they are not typing.
        let border = ('\u{2500}'..='\u{257f}').contains(&c);
        if !c.is_whitespace() && !c.is_control() && !border && !dim && !inverse {
            return true;
        }
    }
    false
}

fn apply_sgr(params: &str, dim: &mut bool, inverse: &mut bool) {
    let codes: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            "" | "0" => {
                *dim = false;
                *inverse = false;
            }
            "2" | "90" => *dim = true,
            "22" | "39" => *dim = false,
            "7" => *inverse = true,
            "27" => *inverse = false,
            // A grey picked from the 256 or RGB palette reads as a hint too.
            "38" if codes.get(i + 1) == Some(&"5") => {
                let n: u8 = codes.get(i + 2).and_then(|n| n.parse().ok()).unwrap_or(0);
                *dim = (240..=250).contains(&n) || n == 8;
                i += 2;
            }
            "38" if codes.get(i + 1) == Some(&"2") => {
                let rgb: Vec<u8> = (2..5)
                    .filter_map(|k| codes.get(i + k).and_then(|n| n.parse().ok()))
                    .collect();
                *dim = rgb.len() == 3
                    && rgb.iter().max().unwrap() - rgb.iter().min().unwrap() < 16
                    && (90..=170).contains(&rgb[0]);
                i += 4;
            }
            c if c.starts_with('3') && c.len() == 2 => *dim = false,
            _ => {}
        }
        i += 1;
    }
}

/// `styled` without its escape sequences.
pub fn plain(styled: &str) -> String {
    let mut out = String::new();
    let mut chars = styled.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for p in chars.by_ref() {
                    if p.is_ascii_alphabetic() || p == '~' {
                        break;
                    }
                }
            } else {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The last [`TAIL_CHARS`] characters of `text` that are not blanks: what
/// is looked for on screen after a paste, and in the transcript after.
pub fn tail(text: &str) -> String {
    let squeezed: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed[squeezed.len().saturating_sub(TAIL_CHARS)..]
        .iter()
        .collect()
}

/// Whether the screen shows `tail` in the input (after the last prompt
/// mark), or Claude Code's stand-in for a long paste.
pub fn shows(screen: &str, tail: &str) -> bool {
    let text = plain(screen);
    let input = match text.rfind(PROMPT) {
        Some(at) => &text[at..],
        None => &text[..],
    };
    let squeezed: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed.contains(tail) || squeezed.contains(PASTED)
}

/// How a message went in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Pasted, seen and submitted; `pastes` pastes it took.
    Sent {
        pastes: u32,
    },
    Failed(String),
}

/// Paste `text` and submit it: paste, look; paste again, look; Esc, paste,
/// look; then give up. Enter only once the text was seen.
pub fn type_in(pane: &mut dyn Pane, text: &str) -> Typed {
    let tail = tail(text);
    if tail.is_empty() {
        return Typed::Failed("the message is empty".into());
    }
    for attempt in 1..=3u32 {
        if attempt == 3 {
            if let Err(e) = pane.key("Escape") {
                return Typed::Failed(format!("tmux: {e}"));
            }
        }
        if let Err(e) = pane.paste(text) {
            return Typed::Failed(format!("tmux: {e}"));
        }
        pane.wait(SETTLE);
        let seen = pane.screen().is_ok_and(|s| shows(&s, &tail));
        if seen {
            return match pane.key("Enter") {
                Ok(()) => Typed::Sent { pastes: attempt },
                Err(e) => Typed::Failed(format!("tmux: {e}")),
            };
        }
    }
    Typed::Failed("the message did not show up in Moss's input after three pastes".into())
}

// ---------------------------------------------------------------- the queue

/// A message typed in whose reply is not in yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub id: String,
    pub from: String,
    /// [`tail`] of its text.
    pub tail: String,
    /// When it was pasted, Unix milliseconds.
    pub at_ms: i64,
}

/// What became of one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Sent(InFlight),
    Failed {
        id: String,
        from: String,
        error: String,
    },
}

/// Hands the inbox to Moss one message at a time: the next only after the
/// reply to the last was taken ([`Courier::done`]), and only while Moss is
/// between turns with nothing typed in its input.
#[derive(Debug, Default)]
pub struct Courier {
    in_flight: Option<InFlight>,
}

impl Courier {
    pub fn in_flight(&self) -> Option<&InFlight> {
        self.in_flight.as_ref()
    }

    /// The reply to the message in flight is in (or will not come).
    pub fn done(&mut self) {
        self.in_flight = None;
    }

    /// Type in the oldest message if Moss is ready for it. `turn_open` is
    /// what Moss's transcript says. A message that cannot be typed in is
    /// failed and the next one tried.
    pub fn step(
        &mut self,
        home: &Path,
        pane: &mut dyn Pane,
        turn_open: Option<bool>,
        now_ms: i64,
    ) -> Vec<Outcome> {
        let mut out = Vec::new();
        if self.in_flight.is_some() || turn_open == Some(true) {
            return out;
        }
        for (path, message) in waiting(home) {
            let Some(message) = message else {
                let id = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let _ = crate::moss::remove(&path);
                out.push(Outcome::Failed {
                    id,
                    from: String::new(),
                    error: "the message file could not be read".into(),
                });
                continue;
            };
            match pane.screen() {
                Ok(screen) if input_empty(&screen) => {}
                // Someone is typing in Moss's terminal, or it is not at
                // its prompt: the next tick looks again.
                _ => break,
            }
            let typed = type_in(pane, &message.text);
            let _ = crate::moss::remove(&path);
            match typed {
                Typed::Sent { .. } => {
                    let sent = InFlight {
                        id: message.id,
                        from: message.from,
                        tail: tail(&message.text),
                        at_ms: now_ms,
                    };
                    self.in_flight = Some(sent.clone());
                    out.push(Outcome::Sent(sent));
                    break;
                }
                Typed::Failed(error) => out.push(Outcome::Failed {
                    id: message.id,
                    from: message.from,
                    error,
                }),
            }
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    /// A terminal that shows what was pasted after the prompt, unless told
    /// to swallow the first `swallow` pastes.
    #[derive(Debug, Default)]
    pub struct FakePane {
        pub input: String,
        pub swallow: u32,
        pub calls: Vec<String>,
        pub submitted: Vec<String>,
        /// What shows instead of the prompt (a dialog), when set.
        pub screen_override: Option<String>,
    }

    impl Pane for FakePane {
        fn paste(&mut self, text: &str) -> Result<(), String> {
            self.calls.push(format!("paste {text}"));
            if self.swallow > 0 {
                self.swallow -= 1;
            } else {
                self.input.push_str(text);
            }
            Ok(())
        }
        fn screen(&mut self) -> Result<String, String> {
            if let Some(s) = &self.screen_override {
                return Ok(s.clone());
            }
            Ok(format!(
                "Moss\n\u{1b}[2m────\u{1b}[0m\n❯ {}\u{1b}[7m \u{1b}[27m\n  ? for shortcuts\n",
                self.input
            ))
        }
        fn key(&mut self, key: &str) -> Result<(), String> {
            self.calls.push(format!("key {key}"));
            match key {
                "Enter" => self.submitted.push(std::mem::take(&mut self.input)),
                "Escape" => self.input.clear(),
                _ => {}
            }
            Ok(())
        }
        fn wait(&mut self, _: Duration) {}
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakePane;
    use super::*;

    fn say(home: &Path, id: &str, text: &str) {
        put(
            home,
            &Message {
                schema: 1,
                id: id.into(),
                at: "t".into(),
                from: "you".into(),
                text: text.into(),
            },
        )
        .unwrap();
    }

    #[test]
    fn inbox_keeps_order_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000002-aaaaaa", "second");
        say(h, "0000000000001-bbbbbb", "first");
        let m = Message {
            schema: 1,
            id: "0000000000001-bbbbbb".into(),
            at: "t".into(),
            from: "you".into(),
            text: "again".into(),
        };
        assert_eq!(put(h, &m).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        let texts: Vec<String> = waiting(h)
            .into_iter()
            .map(|(_, m)| m.unwrap().text)
            .collect();
        assert_eq!(texts, ["first", "second"]);
        assert_eq!(count(h), 2);
        let a = new_id(1_700_000_000_000);
        assert_eq!(a.len(), 20);
        assert!(a.starts_with("1700000000000-"));
        assert!(new_id(5) < new_id(6));
    }

    #[test]
    fn inbox_sends_when_seen_and_presses_enter_once() {
        let mut pane = FakePane::default();
        assert_eq!(
            type_in(&mut pane, "two lines\nof text"),
            Typed::Sent { pastes: 1 }
        );
        assert_eq!(pane.calls, ["paste two lines\nof text", "key Enter"]);
        assert_eq!(pane.submitted, ["two lines\nof text"]);
    }

    #[test]
    fn inbox_retries_when_readback_misses() {
        // Missed once: pasted again, no Esc.
        let mut pane = FakePane {
            swallow: 1,
            ..FakePane::default()
        };
        assert_eq!(type_in(&mut pane, "hello moss"), Typed::Sent { pastes: 2 });
        assert_eq!(
            pane.calls,
            ["paste hello moss", "paste hello moss", "key Enter"]
        );
        // Missed twice: Esc, then the third paste.
        let mut pane = FakePane {
            swallow: 2,
            ..FakePane::default()
        };
        assert_eq!(type_in(&mut pane, "hello moss"), Typed::Sent { pastes: 3 });
        assert_eq!(
            pane.calls,
            [
                "paste hello moss",
                "paste hello moss",
                "key Escape",
                "paste hello moss",
                "key Enter"
            ]
        );
        // Never seen: failed, and Enter is never pressed.
        let mut pane = FakePane {
            swallow: 3,
            ..FakePane::default()
        };
        assert!(matches!(type_in(&mut pane, "hello moss"), Typed::Failed(_)));
        assert!(
            !pane.calls.iter().any(|c| c == "key Enter"),
            "{:?}",
            pane.calls
        );
        assert_eq!(
            pane.calls.iter().filter(|c| c.starts_with("paste")).count(),
            3
        );
    }

    #[test]
    fn inbox_failure_does_not_block_queue() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000001-aaaaaa", "lost one");
        say(h, "0000000000002-aaaaaa", "kept one");
        std::fs::write(super::dir(h).join("0000000000000-zzzzzz.json"), "{broken").unwrap();
        let mut pane = FakePane {
            swallow: 3,
            ..FakePane::default()
        };
        let mut c = Courier::default();
        let out = c.step(h, &mut pane, Some(false), 10);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(matches!(&out[0], Outcome::Failed { id, .. } if id == "0000000000000-zzzzzz"));
        assert!(matches!(&out[1], Outcome::Failed { id, .. } if id == "0000000000001-aaaaaa"));
        assert!(matches!(&out[2], Outcome::Sent(f) if f.id == "0000000000002-aaaaaa"));
        assert_eq!(pane.submitted, ["kept one"]);
        assert_eq!(count(h), 0, "the failed one is not tried for ever");
    }

    #[test]
    fn inbox_waits_for_turn_end() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000001-aaaaaa", "first");
        say(h, "0000000000002-aaaaaa", "second");
        let mut pane = FakePane::default();
        let mut c = Courier::default();
        // Moss is in a turn: nothing goes in.
        assert!(c.step(h, &mut pane, Some(true), 1).is_empty());
        assert!(pane.calls.is_empty());
        // Between turns: the oldest goes in, alone.
        let out = c.step(h, &mut pane, Some(false), 2);
        assert!(
            matches!(&out[..], [Outcome::Sent(f)] if f.id == "0000000000001-aaaaaa" && f.at_ms == 2)
        );
        // Its reply is not in yet: the second waits, whatever the transcript says.
        assert!(c.step(h, &mut pane, Some(false), 3).is_empty());
        assert!(c.step(h, &mut pane, Some(true), 4).is_empty());
        c.done();
        assert!(c.step(h, &mut pane, Some(true), 5).is_empty());
        let out = c.step(h, &mut pane, Some(false), 6);
        assert!(matches!(&out[..], [Outcome::Sent(f)] if f.id == "0000000000002-aaaaaa"));
        assert_eq!(pane.submitted, ["first", "second"]);
    }

    #[test]
    fn inbox_waits_while_someone_types_or_a_dialog_shows() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000001-aaaaaa", "hi");
        let mut pane = FakePane {
            input: "half a thought".into(),
            ..FakePane::default()
        };
        let mut c = Courier::default();
        assert!(c.step(h, &mut pane, Some(false), 1).is_empty());
        pane.input.clear();
        pane.screen_override = Some("Do you want to proceed?\n❯ 1. Yes\n  2. No\n".into());
        assert!(c.step(h, &mut pane, Some(false), 2).is_empty());
        assert_eq!(count(h), 1);
    }

    #[test]
    fn input_line_reads_hints_and_cursor_as_empty() {
        assert!(input_empty("❯ \n"));
        assert!(input_empty(
            "> old\n│ ❯ \u{1b}[7mT\u{1b}[27m\u{1b}[2mry \"fix lint errors\"\u{1b}[22m │\n"
        ));
        assert!(!input_empty("❯ hello\u{1b}[7m \u{1b}[27m\n"));
        assert!(!input_empty("no prompt here\n"));
        assert!(!input_empty("❯ 1. Yes\n"));
    }

    #[test]
    fn readback_looks_in_the_input_only() {
        let t = tail("please look at the api session");
        assert_eq!(t, "heapisession");
        assert!(shows(
            "old: the api session\n❯ please look at the api\nsession\n",
            &t
        ));
        assert!(!shows("❯ please look at the api session\n❯ \n", &t));
        assert!(shows("❯ [Pasted text #1 +40 lines]\n", &t));
    }
}
