//! The daemon's side of talking to Moss: it types the inbox into Moss's
//! terminal one message at a time, takes each reply from Moss's transcript
//! into chat.jsonl, and tells Moss what happens to the other sessions.
//! The rules are in `moss_core::moss::{inbox, reply, digest, markers}`; this
//! file talks to tmux, the registry and the files.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use moss_core::moss::chat::{self, Line};
use moss_core::moss::digest::{self, Change, Digest};
use moss_core::moss::inbox::{self, InFlight, Outcome, Pane};
use moss_core::moss::reply::{self, Claim, Timing, Verdict};
use moss_core::moss::slash::{self, Slash};
use moss_core::moss::{self, markers};
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, rfc3339, Status};

use super::moss::{test_millis, tmux};

/// How often the inbox is looked at while nothing is in flight.
const INBOX_EVERY: Duration = Duration::from_millis(250);
/// How often the registry is read for Moss's transcript and session news.
const REGISTRY_EVERY: Duration = Duration::from_secs(2);
/// tmux buffer the daemon pastes from.
const BUFFER: &str = "cc-moss-inbox";

/// Moss's terminal through tmux, found by its role mark.
pub struct TmuxPane<'a> {
    socket: &'a str,
    /// A file only the daemon writes, for `load-buffer`.
    scratch: PathBuf,
    target: Option<String>,
}

impl<'a> TmuxPane<'a> {
    pub fn new(socket: &'a str, state: &Path) -> TmuxPane<'a> {
        TmuxPane {
            socket,
            scratch: moss_core::daemon::Paths::new(state)
                .dir
                .join("moss-paste.txt"),
            target: None,
        }
    }

    fn target(&mut self) -> Result<String, String> {
        if let Some(t) = &self.target {
            return Ok(t.clone());
        }
        let seen = super::moss::look(self.socket);
        let id = seen
            .live
            .first()
            .cloned()
            .ok_or_else(|| "Moss's tmux session is not there".to_string())?;
        self.target = Some(id.clone());
        Ok(id)
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        match tmux(self.socket, args) {
            Ok(out) if out.status.success() => {
                Ok(String::from_utf8_lossy(&out.stdout).into_owned())
            }
            Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            Err(e) => Err(format!("{e:?}")),
        }
    }
}

impl Pane for TmuxPane<'_> {
    fn paste(&mut self, text: &str) -> Result<(), String> {
        let target = self.target()?;
        // From a file: a long message would not fit in one tmux command.
        moss::home::write_if_changed(&self.scratch, text).map_err(|e| e.to_string())?;
        let file = self.scratch.to_string_lossy().into_owned();
        let loaded = self.run(&["load-buffer", "-b", BUFFER, &file]);
        let _ = std::fs::remove_file(&self.scratch);
        loaded?;
        self.run(&["paste-buffer", "-p", "-d", "-b", BUFFER, "-t", &target])
            .map(|_| ())
    }

    fn screen(&mut self) -> Result<String, String> {
        let target = self.target()?;
        self.run(&["capture-pane", "-p", "-J", "-e", "-t", &target])
    }

    fn key(&mut self, key: &str) -> Result<(), String> {
        let target = self.target()?;
        self.run(&["send-keys", "-t", &target, key]).map(|_| ())
    }

    fn wait(&mut self, how_long: Duration) {
        std::thread::sleep(how_long);
    }

    fn attached(&mut self) -> Option<bool> {
        // Moss's server is its own: any client is someone at its terminal.
        self.run(&["list-clients"])
            .ok()
            .map(|out| !out.trim().is_empty())
    }

    fn mark(&mut self) -> Option<i64> {
        let target = self.target().ok()?;
        let at = self
            .run(&[
                "display-message",
                "-p",
                "-t",
                &target,
                "#{history_size} #{cursor_y}",
            ])
            .ok()?;
        let mut it = at.split_whitespace().map(|n| n.parse::<i64>().ok());
        Some(it.next()?? + it.next()??)
    }

    fn screen_since(&mut self, mark: i64) -> Result<String, String> {
        let target = self.target()?;
        let history: i64 = self
            .run(&["display-message", "-p", "-t", &target, "#{history_size}"])?
            .trim()
            .parse()
            .map_err(|_| "tmux did not say its history size".to_string())?;
        // Lines above the visible screen are negative.
        let start = (mark - history).to_string();
        self.run(&[
            "capture-pane",
            "-p",
            "-J",
            "-e",
            "-S",
            &start,
            "-t",
            &target,
        ])
    }

    fn fit(&mut self, columns: u32, rows: u32) {
        let Ok(target) = self.target() else {
            return;
        };
        let Ok(size) = self.run(&[
            "display-message",
            "-p",
            "-t",
            &target,
            "#{window_width} #{window_height}",
        ]) else {
            return;
        };
        let mut it = size
            .split_whitespace()
            .map(|n| n.parse::<u32>().unwrap_or(0));
        let (w, h) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
        if w >= columns && h >= rows {
            return;
        }
        // A Moss started before cc-moss sized it. On its window only (see
        // `Looker::launch`): tmux 3.5 and older crash on a server-wide one.
        let _ = self.run(&["set-option", "-w", "-t", &target, "window-size", "manual"]);
        let _ = self.run(&[
            "resize-window",
            "-t",
            &target,
            "-x",
            &columns.max(w).to_string(),
            "-y",
            &rows.max(h).to_string(),
        ]);
    }
}

/// A `/compact` running in Moss's terminal.
struct Compacting {
    id: String,
    since: Instant,
}

/// The longest a `/compact` is waited for.
const COMPACT_LIMIT: Duration = Duration::from_secs(5 * 60);

/// The conversation with one running Moss.
pub struct Talk {
    home: PathBuf,
    state: PathBuf,
    courier: inbox::Courier,
    claim: Option<(InFlight, Claim)>,
    digest: Digest,
    timing: Timing,
    registry_every: Duration,
    /// Moss's transcript, from its record in the registry.
    transcript: Option<PathBuf>,
    /// The last registry event read; `None` until the first read, which
    /// starts from now (old news is not told).
    cursor: Option<i64>,
    next_inbox: Instant,
    next_registry: Instant,
    waiting_shown: bool,
    compacting: Option<Compacting>,
    /// News lines (ids) that were only finished turns.
    quiet: std::collections::HashSet<String>,
    /// Per news line (id): what it said of each session (slot id → state).
    told: std::collections::HashMap<String, std::collections::BTreeMap<String, String>>,
}

/// Whether a change is still so: a session still waiting on the user, or
/// still dropped. Finished turns stay news. A session that is gone from the
/// registry is not.
fn still_so(c: &Change, slots: &[moss_core::slot::Slot]) -> bool {
    let Some(slot) = slots.iter().find(|s| s.slot_id == c.slot_id) else {
        return false;
    };
    match c.kind {
        digest::Kind::NeedsYou => slot.status == Status::Running && slot.attention.is_some(),
        digest::Kind::Disconnected => {
            slot.effective_status(moss_core::proc::is_alive) != Status::Running
        }
        digest::Kind::Done => true,
    }
}

impl Talk {
    pub fn new(state: &Path) -> Talk {
        let default = Timing::default();
        let timing = Timing {
            settle: test_millis("CC_MOSS_MOSS_SETTLE_MS").unwrap_or(default.settle),
            look_every: test_millis("CC_MOSS_MOSS_LOOK_MS").unwrap_or(default.look_every),
            no_start: test_millis("CC_MOSS_MOSS_NO_START_MS").unwrap_or(default.no_start),
            turn: test_millis("CC_MOSS_MOSS_TURN_MS").unwrap_or(default.turn),
            late: test_millis("CC_MOSS_MOSS_LATE_MS").unwrap_or(default.late),
        };
        let window = test_millis("CC_MOSS_DIGEST_WINDOW_MS").unwrap_or(digest::WINDOW);
        let now = Instant::now();
        Talk {
            home: moss::home_dir(state),
            state: state.to_path_buf(),
            courier: inbox::Courier::default(),
            claim: None,
            digest: Digest::new(window, digest::REPEAT),
            timing,
            registry_every: test_millis("CC_MOSS_MOSS_REGISTRY_MS").unwrap_or(REGISTRY_EVERY),
            transcript: None,
            cursor: None,
            next_inbox: now,
            next_registry: now,
            waiting_shown: true,
            compacting: None,
            quiet: Default::default(),
            told: Default::default(),
        }
    }

    /// A new Moss runs: the one that had the message in flight is gone,
    /// so its reply will not come.
    pub fn moss_restarted(&mut self, log: &dyn Fn(&str)) {
        if let Some((sent, _)) = self.claim.take() {
            self.received(&sent, now_ms(), "Moss restarted before it replied", log);
        }
        self.courier.done();
        self.compacting = None;
        self.transcript = None;
        self.show_waiting(false);
    }

    /// At daemon start: your messages a daemon before this one sent and
    /// never saw answered get their "received".
    pub fn recover(&self, log: &dyn Fn(&str)) {
        for id in chat::unanswered(&self.home) {
            let sent = InFlight {
                id,
                from: chat::FROM_YOU.to_string(),
                tail: String::new(),
                at_ms: 0,
            };
            self.received(
                &sent,
                now_ms(),
                "the daemon restarted before the reply",
                log,
            );
        }
    }

    /// The stand-in reply when Moss's own cannot be had.
    fn received(&self, sent: &InFlight, now_ms: i64, why: &str, log: &dyn Fn(&str)) {
        log(&format!("moss: no reply taken for {}: {why}", sent.id));
        let text = tr!(
            "Moss got it (its reply could not be read back; see its terminal).",
            "Moss 收到了（没能取回回复，去终端看）。"
        );
        let line = Line {
            reply_to: Some(sent.id.clone()),
            fallback: true,
            // Nobody waits on a reply to session news.
            silent: sent.from != chat::FROM_YOU,
            ..Line::message(
                &inbox::new_id(now_ms),
                &rfc3339(now_ms),
                chat::FROM_MOSS,
                text,
            )
        };
        self.record(&line, log);
    }

    /// One go of the daemon's loop, while Moss runs.
    pub fn tick(&mut self, socket: &str, now: Instant, digest_on: bool, log: &dyn Fn(&str)) {
        let now_ms = now_ms();
        if now >= self.next_registry {
            self.next_registry = now + self.registry_every;
            if let Err(e) = self.read_registry(now_ms) {
                log(&format!("moss: error: reading the registry: {e:#}"));
            }
        }
        if self.digest.is_due(now_ms) {
            // Checked against the registry right before it goes: news that
            // is no longer so (handled meanwhile) is not told.
            let slots = self.slots();
            let still = |c: &Change| still_so(c, &slots);
            let zh = crate::i18n::lang() == crate::i18n::Lang::Zh;
            if let Some((line, told)) = self.digest.due_checked(now_ms, zh, &still) {
                if digest_on {
                    self.tell_news(&line, &told, now_ms, log);
                }
            }
        }
        let mut pane = TmuxPane::new(socket, &self.state);
        if self.claim.is_some() {
            self.take_reply(&mut pane, now_ms, log);
            return;
        }
        if self.compacting.is_some() {
            self.check_compact(&mut pane, now, now_ms, log);
            return;
        }
        // Markers left from turns no message of ours started.
        reply::take_stops(&self.home);
        self.show_waiting(false);
        if now < self.next_inbox {
            return;
        }
        self.next_inbox = now + INBOX_EVERY;
        // The inbox is a real folder, never a link elsewhere.
        if moss_core::moss::dir::Dir::open(&self.home, "inbox").is_err()
            || inbox::count(&self.home) == 0
        {
            return;
        }
        let turn = self
            .transcript
            .as_deref()
            .and_then(moss_core::transcript::turn_open);
        // Nothing goes in to a Moss whose transcript isn't known yet, or
        // while its boot turn runs: its reply could not be taken.
        let Some(transcript) = self
            .transcript
            .clone()
            .filter(|t| ready_to_send(Some(t), turn))
        else {
            return;
        };
        let from = std::fs::metadata(&transcript).map_or(0, |m| m.len());
        // A command of yours at the head of the queue.
        if let Some((path, Some(message))) = inbox::waiting(&self.home).into_iter().next() {
            if message.from == chat::FROM_YOU {
                if let Some(command) = slash::classify(&message.text) {
                    self.command(&path, &message, command, &mut pane, turn, now, log);
                    return;
                }
            }
        }
        let outcomes = self.courier.step(&self.home, &mut pane, turn, now_ms);
        self.show_input_busy(self.courier.input_busy());
        for outcome in outcomes {
            match outcome {
                Outcome::Sent(sent) => {
                    if sent.from == chat::FROM_YOU {
                        self.record(&Line::state(&sent.id, &rfc3339(now_ms), "sent", None), log);
                    }
                    let claim = Claim::new(
                        transcript.clone(),
                        from,
                        &sent.tail,
                        sent.at_ms,
                        self.timing,
                    );
                    self.claim = Some((sent, claim));
                }
                Outcome::Cleared => log("moss: cleared stale input"),
                Outcome::Failed { id, from, error } => {
                    log(&format!("moss: error: message {id} did not go in: {error}"));
                    if from == chat::FROM_YOU {
                        let line = Line::state(&id, &rfc3339(now_ms), "failed", Some(&error));
                        self.record(&line, log);
                    }
                }
            }
        }
    }

    /// Run a Claude Code command of yours (AMEND-2): refused ones at once,
    /// the rest once Moss is ready, like a message.
    #[allow(clippy::too_many_arguments)]
    fn command(
        &mut self,
        path: &Path,
        message: &inbox::Message,
        command: Slash,
        pane: &mut dyn Pane,
        turn: Option<bool>,
        now: Instant,
        log: &dyn Fn(&str),
    ) {
        let name = command.name().to_string();
        let answer = |talk: &Talk, text: &str, fallback: bool, refused: bool| {
            let at = now_ms();
            let line = Line {
                reply_to: Some(message.id.clone()),
                command: Some(name.clone()),
                refused,
                fallback,
                ..Line::message(&inbox::new_id(at), &rfc3339(at), chat::FROM_MOSS, text)
            };
            talk.record(&line, log);
        };
        if let Slash::Refused(_) = command {
            let _ = inbox::take(&self.home, path);
            let text = tr!(
                "This one needs the terminal: click Open in Terminal at the top right.",
                "这个命令要在终端里用：点右上角「在终端打开」。"
            );
            answer(self, text, false, true);
            return;
        }
        let ready = turn != Some(true)
            && match self.courier.input(pane, now_ms()) {
                inbox::Input::Ready => true,
                inbox::Input::Cleared => {
                    log("moss: cleared stale input");
                    false
                }
                inbox::Input::Wait => false,
            };
        self.show_input_busy(self.courier.input_busy());
        if !ready {
            return;
        }
        let _ = inbox::take(&self.home, path);
        let at = rfc3339(now_ms());
        self.record(&Line::state(&message.id, &at, "sent", None), log);
        let failed = |talk: &Talk, error: &str| {
            log(&format!("moss: error: {name} did not run: {error}"));
            let line = Line::state(&message.id, &rfc3339(now_ms()), "failed", Some(error));
            talk.record(&line, log);
        };
        let timing = slash::Timing::default();
        match command {
            Slash::Display(_) => {
                pane.fit(inbox::COLUMNS, inbox::ROWS);
                match slash::show(pane, &inbox::printable(&message.text), timing) {
                    Ok(text) if !text.trim().is_empty() => answer(self, &text, false, false),
                    Ok(_) => answer(
                        self,
                        tr!("That command printed nothing.", "这个命令没有输出。"),
                        false,
                        false,
                    ),
                    Err(e) => failed(self, &e),
                }
            }
            Slash::Clear => match slash::run(pane, &inbox::printable(&message.text)) {
                Ok(()) => {
                    let back = slash::Timing {
                        back: Duration::from_secs(10),
                        ..timing
                    };
                    slash::wait_ready(pane, back);
                    answer(
                        self,
                        tr!(
                            "Cleared Moss's conversation (its journal and memory are still there).",
                            "已清空 Moss 这一轮的对话（日志和记忆都还在）。"
                        ),
                        false,
                        false,
                    );
                }
                Err(e) => failed(self, &e),
            },
            Slash::Compact => match slash::run(pane, &inbox::printable(&message.text)) {
                Ok(()) => {
                    self.compacting = Some(Compacting {
                        id: message.id.clone(),
                        since: now,
                    })
                }
                Err(e) => failed(self, &e),
            },
            Slash::Refused(_) => {}
        }
    }

    /// A `/compact` is done once Moss is out of its turn and back at its
    /// prompt (or after [`COMPACT_LIMIT`]).
    fn check_compact(
        &mut self,
        pane: &mut dyn Pane,
        now: Instant,
        now_ms: i64,
        log: &dyn Fn(&str),
    ) {
        let Some(c) = &self.compacting else {
            return;
        };
        if now < self.next_inbox {
            return;
        }
        self.next_inbox = now + INBOX_EVERY;
        let elapsed = now.duration_since(c.since);
        let turn = self
            .transcript
            .as_deref()
            .and_then(moss_core::transcript::turn_open);
        let back = elapsed >= Duration::from_secs(2)
            && turn != Some(true)
            && pane
                .screen()
                .is_ok_and(|s| inbox::input_empty(&s) && !reply::working(&s));
        let late = elapsed >= COMPACT_LIMIT;
        if !back && !late {
            return;
        }
        let id = c.id.clone();
        self.compacting = None;
        let (text, fallback) = if back {
            (
                tr!("Compacted Moss's conversation.", "已压缩 Moss 的对话。"),
                false,
            )
        } else {
            (
                tr!(
                    "Moss is still compacting; see its terminal.",
                    "Moss 还在压缩对话；去它的终端看看。"
                ),
                true,
            )
        };
        let line = Line {
            reply_to: Some(id),
            command: Some("/compact".into()),
            fallback,
            ..Line::message(
                &inbox::new_id(now_ms),
                &rfc3339(now_ms),
                chat::FROM_MOSS,
                text,
            )
        };
        self.record(&line, log);
    }

    fn take_reply(&mut self, pane: &mut dyn Pane, now_ms: i64, log: &dyn Fn(&str)) {
        let Some((sent, claim)) = self.claim.as_mut() else {
            return;
        };
        let stopped = reply::take_stops(&self.home);
        let verdict = claim.tick(now_ms, stopped, pane);
        let waiting = claim.waiting();
        let sent = sent.clone();
        let line = match verdict {
            Verdict::Wait => {
                self.show_waiting(waiting);
                return;
            }
            Verdict::Reply { text, .. } => Some(self.reply_line(&sent, &text, now_ms)),
            Verdict::Received(why) => {
                self.received(&sent, now_ms, &why, log);
                None
            }
        };
        if let Some((_, c)) = &self.claim {
            // A /clear may have moved Moss to a new transcript.
            if !c.transcript().as_os_str().is_empty() {
                self.transcript = Some(c.transcript().to_path_buf());
            }
        }
        self.claim = None;
        self.courier.done();
        self.show_waiting(false);
        if let Some(line) = line {
            self.record(&line, log);
        }
    }

    /// Moss's reply as a chat line, its markers read.
    fn reply_line(&self, sent: &InFlight, text: &str, now_ms: i64) -> Line {
        let parsed = markers::parse(text);
        let mut sessions = self.sessions(&parsed.sessions, now_ms);
        let proposals = self.proposals(&parsed, now_ms);
        // The yes buttons are cc-moss's, after Moss's own (a label of Moss's
        // that reads the same is dropped: there is one button per text).
        let mut yes: Vec<String> = Vec::new();
        for p in &proposals {
            for y in &p.yes {
                if !yes.contains(y) {
                    yes.push(y.clone());
                }
            }
        }
        let mut buttons: Vec<String> = parsed
            .buttons
            .iter()
            .filter(|b| !yes.contains(b))
            .cloned()
            .collect();
        buttons.extend(yes);
        // An answer to news: the cards keep what the news said.
        if sent.from == chat::FROM_EVENTS {
            if let Some(told) = self.told.get(&sent.id) {
                sessions = markers::states_as_told(sessions, told);
            }
        }
        let routine = sent.from == chat::FROM_EVENTS && self.quiet.contains(&sent.id);
        Line {
            reply_to: Some(sent.id.clone()),
            silent: parsed.silent || routine,
            buttons,
            quick: parsed.quick,
            proposals,
            sessions: sessions.iter().map(|(id, _)| id.clone()).collect(),
            session_states: sessions.into_iter().collect(),
            ..Line::message(
                &inbox::new_id(now_ms),
                &rfc3339(now_ms),
                chat::FROM_MOSS,
                &parsed.text,
            )
        }
    }

    /// What Moss asked the user to agree to: each `[[confirm:]]` whose
    /// session is in the registry. Its yes is a button cc-moss words from
    /// the proposal itself, one per action naming every session it covers
    /// («找回 api、db», "End work"), so a tap agrees to exactly what it
    /// says. Moss's own button labels never count.
    fn proposals(&self, parsed: &markers::Parsed, now_ms: i64) -> Vec<chat::Proposal> {
        if parsed.confirms.is_empty() {
            return Vec::new();
        }
        let slots = self.slots();
        let mut found: Vec<(String, String, String)> = Vec::new();
        for c in &parsed.confirms {
            let Some(slot_id) =
                markers::resolve_sessions(std::slice::from_ref(&c.session), &slots, now_ms)
                    .into_iter()
                    .next()
            else {
                continue;
            };
            let Some(slot) = slots.iter().find(|s| s.slot_id == slot_id) else {
                continue;
            };
            let named = slot.name.clone().filter(|n| !n.trim().is_empty());
            // Two open sessions of one name: a button could not say which.
            let shared = named.as_ref().is_some_and(|n| {
                slots
                    .iter()
                    .filter(|s| {
                        s.status != moss_core::slot::Status::Closed
                            && !s.is_moss()
                            && s.name.as_deref().map(str::trim) == Some(n.trim())
                    })
                    .count()
                    > 1
            });
            if shared {
                continue;
            }
            // The folder too, so a session renamed to look like another
            // still shows where it is.
            let name = format!(
                "{} · {}",
                named.unwrap_or_else(|| slot.session_id.chars().take(8).collect()),
                markers::short_folder(
                    &slot.cwd,
                    std::env::var_os("HOME").as_deref().map(Path::new)
                )
            );
            if !found
                .iter()
                .any(|(a, s, _)| *a == c.action && *s == slot_id)
            {
                found.push((c.action.clone(), slot_id, name));
            }
        }
        let chinese = tr!(false, true);
        markers::CONSENT_ACTIONS
            .iter()
            .flat_map(|action| {
                let these: Vec<&(String, String, String)> =
                    found.iter().filter(|(a, _, _)| a == action).collect();
                let names: Vec<String> = these.iter().map(|(_, _, n)| n.clone()).collect();
                let yes = markers::yes_label(action, &names, chinese);
                these
                    .into_iter()
                    .map(|(a, slot_id, _)| chat::Proposal {
                        action: a.clone(),
                        slot_id: slot_id.clone(),
                        yes: vec![yes.clone()],
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Every slot in the registry; none when it can't be read.
    fn slots(&self) -> Vec<moss_core::slot::Slot> {
        let db = self.state.join("registry.db");
        if !db.exists() {
            return Vec::new();
        }
        Registry::open(&db)
            .and_then(|r| r.slots())
            .unwrap_or_default()
    }

    /// The sessions Moss pointed at, checked against the registry, each
    /// with what it is doing now.
    fn sessions(&self, items: &[String], now_ms: i64) -> Vec<(String, String)> {
        if items.is_empty() {
            return Vec::new();
        }
        let db = self.state.join("registry.db");
        let slots = if db.exists() {
            Registry::open(&db)
                .and_then(|r| r.slots())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        markers::resolve_sessions(items, &slots, now_ms)
            .into_iter()
            .filter_map(|id| {
                let slot = slots.iter().find(|s| s.slot_id == id)?;
                let status = slot.effective_status(moss_core::proc::is_alive);
                Some((id, markers::session_state(slot, status).to_string()))
            })
            .collect()
    }

    /// `<home>/input-busy` while text that could not be cleared blocks
    /// Moss's input.
    fn show_input_busy(&self, busy: bool) {
        let path = self.home.join(inbox::INPUT_BUSY);
        if busy {
            if !path.exists() {
                let _ = moss::home::write_if_changed(&path, "");
            }
        } else {
            let _ = moss::remove(&path);
        }
    }

    /// `<home>/waiting` while Moss waits on a confirm prompt.
    fn show_waiting(&mut self, waiting: bool) {
        if waiting == self.waiting_shown {
            return;
        }
        let path = reply::waiting_path(&self.home);
        let done = if waiting {
            moss::home::write_if_changed(&path, "").is_ok()
        } else {
            moss::remove(&path).is_ok()
        };
        if done {
            self.waiting_shown = waiting;
        }
    }

    fn record(&self, line: &Line, log: &dyn Fn(&str)) {
        if let Err(e) = chat::append(&self.home, line) {
            log(&format!("moss: error: chat.jsonl: {e}"));
        }
        if let Err(e) = chat::rotate_if_long(&self.home) {
            log(&format!("moss: error: rotating chat.jsonl: {e}"));
        }
    }

    /// Session news for Moss: into its inbox, and into the chat.
    /// A request of cc-moss's own for Moss (the daily checkup): queued like
    /// session news, so it is not shown as something the user said. True
    /// when it went in.
    pub fn ask(&self, text: &str, now_ms: i64, log: &dyn Fn(&str)) -> bool {
        self.tell(text, now_ms, log).is_some()
    }

    /// Session news for Moss. Its answer to news that is only finished
    /// turns is kept quiet whatever it says: nothing there for the user.
    fn tell_news(&mut self, line: &str, told: &[Change], now_ms: i64, log: &dyn Fn(&str)) {
        if let Some(id) = self.tell(line, now_ms, log) {
            // What the news said of each session, for the cards of Moss's
            // answer to it.
            if self.told.len() > 100 {
                self.told.clear();
            }
            self.told.insert(
                id.clone(),
                told.iter()
                    .map(|c| (c.slot_id.clone(), c.told_state()))
                    .collect(),
            );
            if self.digest.last_finished_only() {
                if self.quiet.len() > 100 {
                    self.quiet.clear();
                }
                self.quiet.insert(id);
            }
        }
    }

    fn tell(&self, text: &str, now_ms: i64, log: &dyn Fn(&str)) -> Option<String> {
        let message = inbox::Message {
            schema: 1,
            id: inbox::new_id(now_ms),
            at: rfc3339(now_ms),
            from: chat::FROM_EVENTS.to_string(),
            text: text.to_string(),
        };
        if let Err(e) = inbox::put(&self.home, &message) {
            log(&format!("moss: error: inbox: {e}"));
            return None;
        }
        self.record(
            &Line::message(&message.id, &message.at, chat::FROM_EVENTS, text),
            log,
        );
        Some(message.id)
    }

    /// Moss's transcript, sessions whose process is gone, and the news since
    /// the last read.
    fn read_registry(&mut self, now_ms: i64) -> anyhow::Result<()> {
        let db = self.state.join("registry.db");
        if !db.exists() {
            return Ok(());
        }
        let registry = Registry::open(&db)?;
        if self.cursor.is_none() {
            self.cursor = Some(registry.last_event_seq()?);
        }
        let slots = registry.slots()?;
        if let Some(path) = slots
            .iter()
            .filter(|s| s.is_moss() && s.status != Status::Closed)
            .max_by_key(|s| s.last_active_at_ms)
            .and_then(|s| s.transcript_path.clone())
        {
            // Not while a claim follows a /clear on its own.
            if self.claim.is_none() {
                self.transcript = Some(PathBuf::from(path));
            }
        }
        for s in &slots {
            // `kill(pid, 0)` first: only a process that seems gone costs a
            // closer look (a `ps` on macOS).
            let gone = s.status == Status::Running
                && !s.is_moss()
                && s.pid.is_some_and(|pid| !process_exists(pid))
                && s.effective_status(moss_core::proc::is_alive) == Status::Interrupted;
            if gone {
                registry.record_lost(&s.slot_id, now_ms)?;
            }
        }
        let since = self.cursor.unwrap_or(0);
        for e in registry.events(Some(since), 500)? {
            self.cursor = Some(e.seq);
            let Some(kind) = digest::Kind::from_event(&e.kind) else {
                continue;
            };
            let Some(slot) = slots.iter().find(|s| s.slot_id == e.slot_id) else {
                continue;
            };
            if slot.is_moss() {
                continue;
            }
            let name = e
                .name
                .clone()
                .or_else(|| slot.name.clone())
                .unwrap_or_else(|| slot.session_id.chars().take(8).collect());
            let why = e
                .detail
                .get("reason")
                .and_then(|r| r.as_str())
                .filter(|_| kind == digest::Kind::NeedsYou)
                .map(String::from);
            self.digest.push(
                Change {
                    slot_id: e.slot_id.clone(),
                    name,
                    kind,
                    why,
                },
                now_ms,
            );
        }
        Ok(())
    }
}

/// Whether messages may go in: Moss's transcript is known (a new Moss's
/// comes from its first hook) and its turn (the boot prompt first) is
/// over.
fn ready_to_send(transcript: Option<&Path>, turn: Option<bool>) -> bool {
    transcript.is_some() && turn != Some(true)
}

/// Whether a process with this pid exists (it may not be the one recorded).
fn process_exists(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks that the process exists.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, from: &str) -> Line {
        Line::message(id, "t", from, "x")
    }

    #[test]
    fn nothing_goes_in_before_moss_is_known_and_done_booting() {
        assert!(
            !ready_to_send(None, None),
            "a new Moss: its transcript not known yet"
        );
        let t = Path::new("/t/x.jsonl");
        assert!(!ready_to_send(Some(t), Some(true)), "its boot turn runs");
        assert!(ready_to_send(Some(t), Some(false)));
        assert!(ready_to_send(Some(t), None));
    }

    #[test]
    fn a_daily_checkup_that_did_not_go_in_is_not_marked_done() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let talk = Talk::new(dir.path());
        std::fs::create_dir_all(&talk.home).unwrap();
        assert!(talk.ask("checkup", 1, &|_| {}), "it went in");
        // An inbox it can't write to.
        let inbox = talk.home.join("inbox");
        std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o500)).unwrap();
        let went = talk.ask("checkup", 2, &|_| {});
        std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!went);
    }

    /// A Talk whose registry holds sessions with these names; their slot ids.
    fn talk_with_sessions(dir: &Path, names: &[&str]) -> (Talk, Vec<String>) {
        use moss_core::event::{Context, Event, StartSource};
        let talk = Talk::new(dir);
        std::fs::create_dir_all(&talk.home).unwrap();
        let mut reg = Registry::open(&dir.join("registry.db")).unwrap();
        for (i, name) in names.iter().enumerate() {
            let start = Event::Start {
                source: StartSource::Startup,
                session_id: format!("a1b2c3d{i}-0000"),
                cwd: format!("/w/{name}"),
                title: Some(name.to_string()),
                transcript_path: None,
            };
            let ctx = Context {
                pid: None,
                pid_start: None,
                tmux: None,
                launch_args: None,
                now_ms: 1,
            };
            reg.apply(&start, &ctx).unwrap();
        }
        let slots = reg.slots().unwrap();
        let ids = names
            .iter()
            .map(|n| {
                slots
                    .iter()
                    .find(|s| s.name.as_deref() == Some(*n))
                    .unwrap()
                    .slot_id
                    .clone()
            })
            .collect();
        (talk, ids)
    }

    fn sent_by_you() -> InFlight {
        InFlight {
            id: "y0".into(),
            from: chat::FROM_YOU.into(),
            tail: String::new(),
            at_ms: 1,
        }
    }

    #[test]
    fn a_confirm_marker_becomes_a_checked_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let (talk, ids) = talk_with_sessions(dir.path(), &["api", "db"]);
        let line = talk.reply_line(
            &sent_by_you(),
            "Bring them back? [[buttons: 不要]][[confirm: restore api | 好的]][[confirm: restore db]][[confirm: end nobody]]",
            2,
        );
        // Only sessions that exist; one yes button per action, worded by
        // cc-moss with every session it covers.
        let chinese = tr!(false, true);
        let yes = markers::yes_label(
            "restore",
            &["api · /w/api".into(), "db · /w/db".into()],
            chinese,
        );
        assert_eq!(
            line.proposals,
            [
                chat::Proposal {
                    action: "restore".into(),
                    slot_id: ids[0].clone(),
                    yes: vec![yes.clone()]
                },
                chat::Proposal {
                    action: "restore".into(),
                    slot_id: ids[1].clone(),
                    yes: vec![yes.clone()]
                },
            ]
        );
        assert_eq!(line.buttons, ["不要".to_string(), yes]);
        assert_eq!(line.text.as_deref(), Some("Bring them back?"));
    }

    #[test]
    fn a_name_two_open_sessions_share_gets_no_yes_button() {
        let dir = tempfile::tempdir().unwrap();
        // Moss renamed work to api: two open sessions now read "api".
        let (talk, _) = talk_with_sessions(dir.path(), &["api", "api", "db"]);
        let line = talk.reply_line(
            &sent_by_you(),
            "End it? [[confirm: end api]][[confirm: end a1b2c3d1]][[confirm: end db]]",
            2,
        );
        let chinese = tr!(false, true);
        let end_db = markers::yes_label("end", &["db · /w/db".into()], chinese);
        assert_eq!(line.proposals.len(), 1, "{:?}", line.proposals);
        assert_eq!(line.proposals[0].yes, std::slice::from_ref(&end_db));
        assert_eq!(line.buttons, [end_db]);
    }

    #[test]
    fn moss_labels_never_count_as_consent() {
        let dir = tempfile::tempdir().unwrap();
        let (talk, ids) = talk_with_sessions(dir.path(), &["api", "work", "db"]);
        // A rename question that hides an end and a forget behind 好的.
        let asked = talk.reply_line(
            &sent_by_you(),
            "给 api 改个名？[[buttons: 好的|不要]][[confirm: end work | 好的]][[confirm: forget db | 好的]]",
            2,
        );
        let chinese = tr!(false, true);
        let end_work = markers::yes_label("end", &["work · /w/work".into()], chinese);
        let forget_db = markers::yes_label("forget", &["db · /w/db".into()], chinese);
        assert_eq!(
            asked.buttons,
            [
                "好的".to_string(),
                "不要".into(),
                forget_db,
                end_work.clone()
            ]
        );
        let now = 1_800_000_000_000;
        let at = rfc3339(now - 5_000);
        let moss = Line {
            at: rfc3339(now - 10_000),
            ..asked
        };
        let check = |reply: &str, action: &str, slot: &str| {
            let you = Line::message("y1", &at, chat::FROM_YOU, reply);
            super::super::act::consent(&[moss.clone(), you], Some("y1"), action, slot, now, &|_| {
                false
            })
        };
        assert!(check("好的", "end", &ids[1]).is_err(), "Moss's label");
        assert!(check("好的", "forget", &ids[2]).is_err(), "Moss's label");
        assert!(check(&end_work, "end", &ids[1]).is_ok(), "cc-moss's button");
        assert!(
            check(&end_work, "forget", &ids[2]).is_err(),
            "one tap agrees to what it says, nothing else"
        );
    }

    #[test]
    fn finished_only_digest_reply_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let mut talk = Talk::new(dir.path());
        std::fs::create_dir_all(&talk.home).unwrap();
        let news = |talk: &mut Talk, changes: &[(&str, digest::Kind)]| {
            for (slot, kind) in changes {
                talk.digest.push(
                    Change {
                        slot_id: slot.to_string(),
                        name: slot.to_string(),
                        kind: *kind,
                        why: None,
                    },
                    0,
                );
            }
            let (line, told) = talk
                .digest
                .due_checked(i64::MAX / 2, true, &|_| true)
                .unwrap();
            talk.tell_news(&line, &told, 1, &|_| {});
            chat::lines(&talk.home).last().unwrap().id.clone()
        };
        let sent = |id: &str| InFlight {
            id: id.into(),
            from: chat::FROM_EVENTS.into(),
            tail: String::new(),
            at_ms: 1,
        };
        // Only "finished": Moss's answer is kept quiet, its pins stay.
        let done = news(
            &mut talk,
            &[("api", digest::Kind::Done), ("web", digest::Kind::Done)],
        );
        let line = talk.reply_line(&sent(&done), "api 又做完一轮了。[[quick: today]]", 2);
        assert!(line.silent, "{line:?}");
        assert_eq!(line.quick, ["today"]);
        // Something the user has to act on: Moss may speak.
        let asks = news(
            &mut talk,
            &[("db", digest::Kind::NeedsYou), ("ui", digest::Kind::Done)],
        );
        let line = talk.reply_line(&sent(&asks), "db 在等你批权限。", 3);
        assert!(!line.silent, "{line:?}");
        // The user's own messages are never forced quiet.
        let mine = InFlight {
            from: chat::FROM_YOU.into(),
            ..sent("y1")
        };
        assert!(!talk.reply_line(&mine, "ok", 4).silent);
    }

    #[test]
    fn a_new_moss_answers_the_message_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let mut talk = Talk::new(dir.path());
        std::fs::create_dir_all(&talk.home).unwrap();
        let sent = InFlight {
            id: "m1".into(),
            from: chat::FROM_YOU.into(),
            tail: "x".into(),
            at_ms: 1,
        };
        let claim = Claim::new(PathBuf::new(), 0, "x", 1, talk.timing);
        talk.claim = Some((sent, claim));
        talk.moss_restarted(&|_| {});
        assert!(talk.claim.is_none());
        assert!(talk.courier.in_flight().is_none());
        let shown = chat::fold(&talk.home, 10);
        assert!(
            shown
                .iter()
                .any(|m| m.reply_to.as_deref() == Some("m1") && m.fallback),
            "{shown:?}"
        );
    }

    #[test]
    fn a_new_daemon_answers_messages_left_sent() {
        let dir = tempfile::tempdir().unwrap();
        let talk = Talk::new(dir.path());
        std::fs::create_dir_all(&talk.home).unwrap();
        for l in [
            line("m1", chat::FROM_YOU),
            Line::state("m1", "t", "sent", None),
            line("m2", chat::FROM_YOU),
            Line::state("m2", "t", "sent", None),
            Line {
                reply_to: Some("m2".into()),
                ..line("r2", chat::FROM_MOSS)
            },
            line("m3", chat::FROM_YOU),
        ] {
            chat::append(&talk.home, &l).unwrap();
        }
        talk.recover(&|_| {});
        let shown = chat::fold(&talk.home, 10);
        let state = |id: &str| shown.iter().find(|m| m.id == id).unwrap().state.clone();
        assert_eq!(state("m1").as_deref(), Some("replied"));
        assert_eq!(state("m2").as_deref(), Some("replied"));
        assert_eq!(state("m3").as_deref(), Some("queued"), "still in the inbox");
        let fallbacks = shown.iter().filter(|m| m.fallback).count();
        assert_eq!(fallbacks, 1, "{shown:?}");
    }
}
