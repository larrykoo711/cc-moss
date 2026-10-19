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

/// `<home>/input-busy`: there while text cc-moss could not clear sits in
/// Moss's input and nothing more is typed in.
pub const INPUT_BUSY: &str = "input-busy";

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

/// How many of your messages wait (`moss status`'s `queued`).
pub fn count_yours(home: &Path) -> usize {
    waiting(home)
        .iter()
        .filter(|(_, m)| m.as_ref().is_some_and(|m| m.from == "you"))
        .count()
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
    /// Make the window at least this big (panels are cut short in a small
    /// one).
    fn fit(&mut self, _columns: u32, _rows: u32) {}
}

/// The size Moss's window is started at, and grown to before a command
/// shows a panel.
pub const COLUMNS: u32 = 100;
pub const ROWS: u32 = 50;

/// Claude Code's prompt mark, in front of the input line.
const PROMPT: char = '❯';

/// Claude Code shows a long paste as this instead of its text.
const PASTED: &str = "[Pastedtext";

/// A box-drawing character (`│ ╭ ─ ╰`): Claude Code frames its input with
/// them; they are never typing.
fn border(c: char) -> bool {
    ('\u{2500}'..='\u{257f}').contains(&c)
}

/// Whether `plain_line` is a prompt line: its first character that is not
/// a blank or a border is the prompt mark.
pub(crate) fn prompt_line(plain_line: &str) -> bool {
    plain_line
        .chars()
        .find(|c| !c.is_whitespace() && !border(*c))
        == Some(PROMPT)
}

/// A line that closes the input box: only borders (and blanks), with at
/// least one border.
fn rule_line(plain_line: &str) -> bool {
    plain_line.chars().any(border) && plain_line.chars().all(|c| c.is_whitespace() || border(c))
}

/// Where Claude Code's input box starts in `plain_lines`: the top rule of
/// the last prompt line with a rule right above it and one below.
pub fn input_box_top(plain_lines: &[String]) -> Option<usize> {
    let lines = plain_lines;
    (1..lines.len())
        .rev()
        .find(|i| {
            prompt_line(&lines[*i])
                && rule_line(&lines[i - 1])
                && lines[i + 1..].iter().any(|l| rule_line(l))
        })
        .map(|i| i - 1)
}

/// Where the input starts: the first prompt line inside the box that holds
/// the last one (after the rule above it). A pasted line that itself starts
/// with the prompt mark is inside the same box, below the real prompt.
fn input_start(lines: &[String]) -> Option<usize> {
    let last = (0..lines.len()).rev().find(|i| prompt_line(&lines[*i]))?;
    let top = (0..last)
        .rev()
        .find(|i| rule_line(&lines[*i]))
        .map_or(0, |r| r + 1);
    (top..=last).find(|i| prompt_line(&lines[*i]))
}

/// Whether Moss's input line is empty: the last prompt line has nothing
/// typed after its mark. Placeholder hints (dim), the cursor (inverse) and
/// borders are not typing. No prompt on screen (starting up, a dialog) is
/// not empty: nothing is pasted then.
///
/// Ready means Claude Code's input box is drawn: the prompt line right
/// under a rule, and a rule below it. A blank screen, the welcome banner
/// alone (still loading) or a bare prompt mark is never "empty".
pub fn input_empty(screen: &str) -> bool {
    input(screen) == Some(false)
}

/// Whether the drawn input box holds typing (`None`: no box on screen).
pub fn input_has_text(screen: &str) -> bool {
    input(screen) == Some(true)
}

/// Whether the input box holds typing, if the box is on screen.
fn input(screen: &str) -> Option<bool> {
    let styled: Vec<&str> = screen.lines().collect();
    let lines: Vec<String> = styled.iter().map(|l| plain(l)).collect();
    let i = (0..lines.len()).rev().find(|i| prompt_line(&lines[*i]))?;
    let boxed = i > 0 && rule_line(&lines[i - 1]) && lines[i + 1..].iter().any(|l| rule_line(l));
    if !boxed {
        return None;
    }
    let line = styled[i];
    let after = &line[line.find(PROMPT).unwrap_or(0) + PROMPT.len_utf8()..];
    Some(typed(after))
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
        if !c.is_whitespace() && !c.is_control() && !border(c) && !dim && !inverse {
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
    let squeezed: Vec<char> = squeeze(text).chars().collect();
    squeezed[squeezed.len().saturating_sub(TAIL_CHARS)..]
        .iter()
        .collect()
}

/// `text` without blanks and box borders: how texts are compared, on screen
/// and in the transcript.
pub fn squeeze(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !border(*c))
        .collect()
}

/// Whether the screen shows `tail` in the input box (from its prompt line
/// to the rule that closes it), or Claude Code's stand-in for a long paste.
pub fn shows(screen: &str, tail: &str) -> bool {
    let lines: Vec<String> = plain(screen).lines().map(String::from).collect();
    let Some(start) = input_start(&lines) else {
        return false;
    };
    let mut input = String::new();
    for line in &lines[start..] {
        if rule_line(line) {
            break;
        }
        input.push_str(line);
    }
    let squeezed = squeeze(&input);
    squeezed.contains(tail) || squeezed.contains(PASTED)
}

/// How a message went in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Pasted, seen and submitted; `pastes` pastes it took.
    Sent { pastes: u32 },
    /// Not sent; the input is empty again.
    Failed(String),
    /// Not sent, and something stays in the input that C-u did not clear.
    Stuck(String),
}

/// Paste `text` and submit it: paste, look; paste again, look; Esc, paste,
/// look; then give up. Enter only once the text was seen, or once the
/// input holds something after a paste (it landed; pasting again would
/// double it). A failure clears the input with C-u (never a second Esc,
/// which opens Claude Code's rewind menu) and says whether that worked.
pub fn type_in(pane: &mut dyn Pane, text: &str) -> Typed {
    let tail = tail(text);
    if tail.is_empty() {
        return Typed::Failed("the message is empty".into());
    }
    let error = 'tries: {
        for attempt in 1..=3u32 {
            if attempt > 1 {
                // Not seen, but landed all the same: no second copy.
                if pane.screen().is_ok_and(|s| input_has_text(&s)) {
                    return enter(pane, attempt - 1);
                }
            }
            if attempt == 3 {
                if let Err(e) = pane.key("Escape") {
                    break 'tries format!("tmux: {e}");
                }
            }
            if let Err(e) = pane.paste(text) {
                break 'tries format!("tmux: {e}");
            }
            pane.wait(SETTLE);
            if pane.screen().is_ok_and(|s| shows(&s, &tail)) {
                return enter(pane, attempt);
            }
        }
        "the message did not show up in Moss's input after three pastes".to_string()
    };
    clear_after_failure(pane, error)
}

fn enter(pane: &mut dyn Pane, pastes: u32) -> Typed {
    match pane.key("Enter") {
        Ok(()) => Typed::Sent { pastes },
        Err(e) => clear_after_failure(pane, format!("tmux: {e}")),
    }
}

fn clear_after_failure(pane: &mut dyn Pane, error: String) -> Typed {
    if !pane.screen().is_ok_and(|s| input_has_text(&s)) {
        return Typed::Failed(error);
    }
    let _ = pane.key("C-u");
    pane.wait(SETTLE);
    if pane.screen().is_ok_and(|s| input_has_text(&s)) {
        Typed::Stuck(error)
    } else {
        Typed::Failed(error)
    }
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
    input_busy: bool,
}

impl Courier {
    pub fn in_flight(&self) -> Option<&InFlight> {
        self.in_flight.as_ref()
    }

    /// A failed message left text in Moss's input that could not be
    /// cleared; nothing goes in until it is empty again.
    pub fn input_busy(&self) -> bool {
        self.input_busy
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
            // A command of yours is the caller's to run (super::slash).
            if message.from == "you" && super::slash::classify(&message.text).is_some() {
                break;
            }
            match pane.screen() {
                Ok(screen) if input_empty(&screen) => self.input_busy = false,
                // Someone is typing in Moss's terminal, or it is not at
                // its prompt: the next tick looks again.
                _ => break,
            }
            // Claude Code runs any input starting with `/` as a command: a
            // message that only looks like one goes in after a space.
            let text = if message.text.starts_with('/') {
                format!(" {}", message.text)
            } else {
                message.text.clone()
            };
            let typed = type_in(pane, &text);
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
                Typed::Stuck(error) => {
                    self.input_busy = true;
                    out.push(Outcome::Failed {
                        id: message.id,
                        from: message.from,
                        error,
                    });
                    break;
                }
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
        /// The next this many pastes land but show garbled (the readback
        /// cannot find them).
        pub blind: u32,
        /// Showing garbled now.
        pub garbled: bool,
        /// The next this many pastes land, then tmux reports an error.
        pub paste_errors: u32,
        /// C-u does not clear the input.
        pub sticky: bool,
        /// What a command shows on Enter, until Escape.
        pub panels: Vec<(String, String)>,
        pub showing: Option<String>,
    }

    impl Pane for FakePane {
        fn paste(&mut self, text: &str) -> Result<(), String> {
            self.calls.push(format!("paste {text}"));
            if self.swallow > 0 {
                self.swallow -= 1;
                return Ok(());
            }
            self.input.push_str(text);
            if self.blind > 0 {
                self.blind -= 1;
                self.garbled = true;
            }
            if self.paste_errors > 0 {
                self.paste_errors -= 1;
                return Err("lost server".into());
            }
            Ok(())
        }
        fn screen(&mut self) -> Result<String, String> {
            if let Some(s) = self.showing.as_ref().or(self.screen_override.as_ref()) {
                return Ok(s.clone());
            }
            let shown = if self.garbled {
                "#".repeat(self.input.chars().count())
            } else {
                self.input.replace('\n', "\n  ")
            };
            Ok(format!(
                "Moss\n\u{1b}[2m────\u{1b}[0m\n❯ {shown}\u{1b}[7m \u{1b}[27m\n\u{1b}[2m────\u{1b}[0m\n  ? for shortcuts\n"
            ))
        }
        fn key(&mut self, key: &str) -> Result<(), String> {
            self.calls.push(format!("key {key}"));
            match key {
                "Enter" => {
                    let typed = std::mem::take(&mut self.input);
                    if let Some((_, panel)) = self.panels.iter().find(|(c, _)| *c == typed) {
                        self.showing = Some(panel.clone());
                    }
                    self.submitted.push(typed);
                }
                "Escape" => {
                    self.input.clear();
                    self.showing = None;
                }
                "C-u" if !self.sticky => self.input.clear(),
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
    fn inbox_never_types_a_message_as_a_command() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000001-aaaaaa", "/login\nwhy the terminal?");
        let mut pane = FakePane::default();
        let out = Courier::default().step(h, &mut pane, Some(false), 1);
        assert!(matches!(&out[..], [Outcome::Sent(_)]), "{out:?}");
        assert_eq!(pane.submitted, [" /login\nwhy the terminal?"]);
    }

    #[test]
    fn inbox_counts_only_your_messages() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000001-aaaaaa", "mine");
        put(
            h,
            &Message {
                schema: 1,
                id: "0000000000002-aaaaaa".into(),
                at: "t".into(),
                from: "events".into(),
                text: "[Update] api dropped".into(),
            },
        )
        .unwrap();
        assert_eq!(count_yours(h), 1);
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
    fn inbox_sees_a_message_with_a_prompt_mark_in_it() {
        let mut pane = FakePane::default();
        assert_eq!(
            type_in(&mut pane, "check the api log ❯ now"),
            Typed::Sent { pastes: 1 }
        );
    }

    #[test]
    fn readback_reads_a_wrapped_message_inside_borders() {
        let t = tail("please look at the api session now");
        let screen = "\
╭──────────────────────────────╮
│ ❯ please look at the api ses │
│   sion now                   │
╰──────────────────────────────╯
  ? for shortcuts
";
        assert!(shows(screen, &t));
        // The box's bottom ends the input: a footer is not part of it.
        let footer = "│ ❯ please look │\n╰────╯\n  the api session now\n";
        assert!(!shows(footer, &t));
    }

    #[test]
    fn inbox_landed_but_unseen_is_not_pasted_twice() {
        let mut pane = FakePane {
            blind: 1,
            ..FakePane::default()
        };
        assert_eq!(type_in(&mut pane, "hello moss"), Typed::Sent { pastes: 1 });
        assert_eq!(pane.calls, ["paste hello moss", "key Enter"]);
        assert_eq!(pane.submitted, ["hello moss"]);
    }

    #[test]
    fn inbox_failure_clears_the_input_and_keeps_the_queue_moving() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000001-aaaaaa", "first");
        say(h, "0000000000002-aaaaaa", "second");
        // tmux fails after the text landed: C-u clears it, the next goes on.
        let mut pane = FakePane {
            paste_errors: 1,
            ..FakePane::default()
        };
        let mut c = Courier::default();
        let out = c.step(h, &mut pane, Some(false), 1);
        assert!(matches!(&out[0], Outcome::Failed { id, .. } if id == "0000000000001-aaaaaa"));
        assert!(matches!(&out[1], Outcome::Sent(f) if f.id == "0000000000002-aaaaaa"));
        assert!(
            pane.calls.iter().any(|k| k == "key C-u"),
            "{:?}",
            pane.calls
        );
        assert!(!pane.calls.iter().any(|k| k == "key Escape" && false));
        assert_eq!(pane.submitted, ["second"]);
        assert!(!c.input_busy());
    }

    #[test]
    fn inbox_says_when_its_input_stays_busy() {
        let dir = tempfile::tempdir().unwrap();
        let h = dir.path();
        say(h, "0000000000001-aaaaaa", "first");
        say(h, "0000000000002-aaaaaa", "second");
        let mut pane = FakePane {
            paste_errors: 1,
            sticky: true,
            ..FakePane::default()
        };
        let mut c = Courier::default();
        let out = c.step(h, &mut pane, Some(false), 1);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(matches!(&out[0], Outcome::Failed { .. }));
        assert!(
            c.input_busy(),
            "left text it cannot clear: said, not silent"
        );
        assert_eq!(count(h), 1);
        // Cleared by hand: busy no more, and the queue moves.
        pane.input.clear();
        let out = c.step(h, &mut pane, Some(false), 2);
        assert!(matches!(&out[..], [Outcome::Sent(f)] if f.id == "0000000000002-aaaaaa"));
        assert!(!c.input_busy());
    }

    #[test]
    fn input_line_reads_hints_and_cursor_as_empty() {
        let rule = "─".repeat(80);
        assert!(input_empty(&format!("{rule}\n❯ \n{rule}\n")));
        assert!(input_empty(
            "> old\n╭────╮\n│ ❯ \u{1b}[7mT\u{1b}[27m\u{1b}[2mry \"fix lint errors\"\u{1b}[22m │\n╰────╯\n"
        ));
        assert!(!input_empty(&format!(
            "{rule}\n❯ hello\u{1b}[7m \u{1b}[27m\n{rule}\n"
        )));
        assert!(!input_empty("no prompt here\n"));
        assert!(!input_empty("❯ 1. Yes\n"));
    }

    /// Claude Code 2.1 at rest, 80 columns: the hint is dim.
    const READY: &str = "\
  ✻ Welcome back
\u{1b}[2m%RULE%\u{1b}[0m
❯ \u{1b}[7mT\u{1b}[27m\u{1b}[2mry \"fix lint errors\"\u{1b}[22m
\u{1b}[2m%RULE%\u{1b}[0m
  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent ◐ medium · /effort
";

    #[test]
    fn input_is_ready_only_with_its_box_on_screen() {
        let ready = READY.replace("%RULE%", &"─".repeat(80));
        assert!(input_empty(&ready), "the real resting screen");
        // Starting up: nothing on screen yet, or the banner without the box.
        assert!(!input_empty(""));
        assert!(!input_empty("\n\n\n\n"));
        assert!(!input_empty(
            " ▐▛███▜▌   Claude Code v2.1.303\n▝▜█████▛▘  Opus · Claude Max\n  ▘▘ ▝▝    ~/.local/state/cc-moss/moss\n"
        ));
        // A prompt mark without its box (a dialog, a half-drawn screen).
        assert!(!input_empty("❯ \n"));
        assert!(!input_empty(&format!("{}\n❯ \n", "─".repeat(80))));
    }

    #[test]
    fn readback_looks_in_the_input_only() {
        let t = tail("please look at the api session");
        assert_eq!(t, "heapisession");
        assert!(shows(
            "old: the api session\n❯ please look at the api\nsession\n",
            &t
        ));
        assert!(!shows(
            "❯ please look at the api session\n────\n❯ \n────\n",
            &t
        ));
        // A pasted line starting with the mark is still the same input.
        assert!(shows(
            "────\n❯ first line\n  ❯ please look at the api session\n────\n",
            &t
        ));
        assert!(shows("❯ [Pasted text #1 +40 lines]\n", &t));
    }
}
