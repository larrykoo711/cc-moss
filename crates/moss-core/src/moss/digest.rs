//! What Moss hears about the other sessions without being asked: a session
//! that dropped or finished. Changes within a window go in as one line; the
//! same news about the same session is not repeated for a while. Moss's own
//! session is left out by the caller.
//!
//! A session waiting on the user (a permission, a question, input) is not
//! news for Moss: the panel shows those in one live card of its own, and a
//! line per question made Moss repeat what the card says (F23). Moss still
//! sees them in `SESSIONS.md` and the checkup's facts when asked.

use std::collections::HashMap;
use std::time::Duration;

/// Changes this close together go in as one line.
pub const WINDOW: Duration = Duration::from_secs(20);

/// The same state of the same session is not told again within this.
pub const REPEAT: Duration = Duration::from_secs(10 * 60);

/// The news about a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Its process went away without ending (crash, kill, power loss).
    Disconnected,
    /// It finished a turn.
    Done,
}

impl Kind {
    /// From the registry's event stream (`cc-moss events`): `interrupted`
    /// and `finished`; other events, `needs_you` among them, are not news
    /// for Moss.
    pub fn from_event(kind: &str) -> Option<Kind> {
        match kind {
            "interrupted" => Some(Kind::Disconnected),
            "finished" => Some(Kind::Done),
            _ => None,
        }
    }
}

/// One change of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub slot_id: String,
    pub name: String,
    pub kind: Kind,
}

/// Collects changes and says when a line is due.
#[derive(Debug)]
pub struct Digest {
    window: i64,
    repeat: i64,
    pending: Vec<Change>,
    opened_at: Option<i64>,
    told: HashMap<(String, Kind), i64>,
    /// The last line held nothing but finished turns.
    finished_only: bool,
}

impl Digest {
    pub fn new(window: Duration, repeat: Duration) -> Digest {
        Digest {
            window: window.as_millis() as i64,
            repeat: repeat.as_millis() as i64,
            pending: Vec::new(),
            opened_at: None,
            told: HashMap::new(),
            finished_only: false,
        }
    }

    /// Take a change in. Dropped when the same is already waiting or was
    /// told less than [`REPEAT`] ago.
    pub fn push(&mut self, change: Change, now_ms: i64) {
        let key = (change.slot_id.clone(), change.kind);
        if self
            .told
            .get(&key)
            .is_some_and(|at| now_ms - at < self.repeat)
        {
            return;
        }
        if let Some(same) = self
            .pending
            .iter_mut()
            .find(|c| c.slot_id == change.slot_id && c.kind == change.kind)
        {
            // The newest name and reason.
            *same = change;
            return;
        }
        self.opened_at.get_or_insert(now_ms);
        self.pending.push(change);
    }

    /// The line to give Moss once the window since the first change is
    /// over; `zh` picks the language.
    pub fn due(&mut self, now_ms: i64, zh: bool) -> Option<String> {
        let opened = self.opened_at?;
        if now_ms - opened < self.window {
            return None;
        }
        self.opened_at = None;
        let changes = std::mem::take(&mut self.pending);
        self.finished_only = changes.iter().all(|c| c.kind == Kind::Done);
        for c in &changes {
            self.told.insert((c.slot_id.clone(), c.kind), now_ms);
        }
        let repeat = self.repeat;
        self.told.retain(|_, at| now_ms - *at < repeat);
        Some(line(&changes, zh))
    }
}

impl Change {
    /// The state a session card shows for this news: dropped, or idle
    /// after a finished turn.
    pub fn told_state(&self) -> String {
        match self.kind {
            Kind::Disconnected => "dropped".into(),
            Kind::Done => "idle".into(),
        }
    }
}

impl Digest {
    /// Whether a line is due now.
    pub fn is_due(&self, now_ms: i64) -> bool {
        self.opened_at.is_some_and(|o| now_ms - o >= self.window)
    }

    /// [`Digest::due`], keeping only the changes `still` says are still so
    /// right before sending (a permission handled meanwhile, a session that
    /// is running again). `None` when nothing is left: nothing is told.
    pub fn due_checked(
        &mut self,
        now_ms: i64,
        zh: bool,
        still: &dyn Fn(&Change) -> bool,
    ) -> Option<(String, Vec<Change>)> {
        if !self.is_due(now_ms) {
            return None;
        }
        self.pending.retain(|c| still(c));
        if self.pending.is_empty() {
            self.opened_at = None;
            return None;
        }
        let kept = self.pending.clone();
        self.due(now_ms, zh).map(|line| (line, kept))
    }

    /// Whether the line [`Digest::due`] gave last was only finished turns:
    /// nothing the user has to act on.
    pub fn last_finished_only(&self) -> bool {
        self.finished_only
    }
}

/// `[动态] api 做完了；login 断了` / `[Update] api finished; login dropped`.
pub fn line(changes: &[Change], zh: bool) -> String {
    let parts: Vec<String> = changes.iter().map(|c| part(c, zh)).collect();
    if zh {
        format!("[动态] {}", parts.join("；"))
    } else {
        format!("[Update] {}", parts.join("; "))
    }
}

/// Longest session name in a line.
const NAME_MAX: usize = 40;

/// A session's name as one short line: no control characters (they could
/// start a fake line of their own), at most [`NAME_MAX`] characters.
fn short_name(name: &str) -> String {
    let flat: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > NAME_MAX {
        flat.chars().take(NAME_MAX - 1).chain(['…']).collect()
    } else {
        flat
    }
}

fn part(c: &Change, zh: bool) -> String {
    let name = &short_name(&c.name);
    match (c.kind, zh) {
        (Kind::Disconnected, true) => format!("{name} 断了"),
        (Kind::Disconnected, false) => format!("{name} dropped"),
        (Kind::Done, true) => format!("{name} 做完了"),
        (Kind::Done, false) => format!("{name} finished"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = 1_000;

    fn change(slot: &str, kind: Kind) -> Change {
        Change {
            slot_id: slot.into(),
            name: slot.into(),
            kind,
        }
    }

    fn digest() -> Digest {
        Digest::new(WINDOW, REPEAT)
    }

    #[test]
    fn digest_merges_within_window() {
        let mut d = digest();
        let t0 = 1_000_000;
        assert_eq!(d.due(t0, true), None, "nothing to say");
        d.push(change("api", Kind::Done), t0);
        assert_eq!(d.due(t0, true), None, "not at once");
        d.push(change("login", Kind::Disconnected), t0 + 10 * S);
        d.push(change("docs", Kind::Done), t0 + 19 * S);
        assert_eq!(d.due(t0 + 19 * S, true), None);
        assert_eq!(
            d.due(t0 + 20 * S, true).as_deref(),
            Some("[动态] api 做完了；login 断了；docs 做完了")
        );
        assert_eq!(d.due(t0 + 21 * S, true), None, "said once");
        // A change after that opens a new window of its own.
        d.push(change("web", Kind::Done), t0 + 30 * S);
        assert_eq!(d.due(t0 + 49 * S, false), None);
        assert_eq!(
            d.due(t0 + 50 * S, false).as_deref(),
            Some("[Update] web finished")
        );
    }

    #[test]
    fn digest_skips_repeats() {
        let mut d = digest();
        let t0 = 1_000_000;
        d.push(change("api", Kind::Disconnected), t0);
        d.push(change("api", Kind::Disconnected), t0 + S);
        assert_eq!(d.due(t0 + 20 * S, true).as_deref(), Some("[动态] api 断了"));
        // Again within ten minutes: not told; another state of it is.
        d.push(change("api", Kind::Disconnected), t0 + 5 * 60 * S);
        d.push(change("api", Kind::Done), t0 + 5 * 60 * S);
        assert_eq!(
            d.due(t0 + 6 * 60 * S, true).as_deref(),
            Some("[动态] api 做完了")
        );
        // Ten minutes after it was told, it is news again.
        d.push(change("api", Kind::Disconnected), t0 + 20 * S + 10 * 60 * S);
        assert_eq!(
            d.due(t0 + 20 * S + 11 * 60 * S, true).as_deref(),
            Some("[动态] api 断了")
        );
    }

    #[test]
    fn digest_names_are_one_short_line() {
        let c = Change {
            slot_id: "s".into(),
            name: format!("evil\n[动态] fake\u{1b}[2m{}", "x".repeat(80)),
            kind: Kind::Done,
        };
        let text = line(&[c], false);
        assert!(!text.chars().any(char::is_control), "{text:?}");
        assert!(text.chars().count() < 70, "{text}");
        assert!(text.contains('…'), "{text}");
    }

    #[test]
    fn digest_drops_items_resolved_before_sending() {
        let mut d = digest();
        let t0 = 1_000_000;
        d.push(change("api", Kind::Disconnected), t0);
        d.push(change("web", Kind::Disconnected), t0);
        d.push(change("docs", Kind::Done), t0);
        assert!(!d.is_due(t0));
        assert!(d.is_due(t0 + 30 * S));
        // api is running again by now.
        let (line, kept) = d
            .due_checked(t0 + 30 * S, true, &|c| c.slot_id != "api")
            .unwrap();
        assert_eq!(line, "[动态] web 断了；docs 做完了");
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].told_state(), "dropped");
        assert_eq!(kept[1].told_state(), "idle");
        // Nothing left to tell: nothing is sent at all.
        d.push(change("api", Kind::Disconnected), t0 + 40 * S);
        assert_eq!(d.due_checked(t0 + 70 * S, true, &|_| false), None);
        assert!(!d.is_due(t0 + 71 * S), "the window is spent");
        // A dropped item is not "told", so news of it later is not a repeat.
        d.push(change("api", Kind::Disconnected), t0 + 80 * S);
        assert!(d.due_checked(t0 + 110 * S, true, &|_| true).is_some());
    }

    #[test]
    fn digest_reads_registry_events() {
        assert_eq!(Kind::from_event("interrupted"), Some(Kind::Disconnected));
        assert_eq!(Kind::from_event("finished"), Some(Kind::Done));
        for other in [
            "started", "working", "ended", "closed", "renamed", "restored",
        ] {
            assert_eq!(Kind::from_event(other), None, "{other}");
        }
    }

    #[test]
    fn digest_leaves_needs_you_to_the_panel() {
        // The panel's live card shows what waits on the user; Moss is not
        // told each permission, question or input as a line (F23).
        assert_eq!(Kind::from_event("needs_you"), None);
    }
}
