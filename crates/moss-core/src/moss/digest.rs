//! What Moss hears about the other sessions without being asked: a session
//! that needs you, dropped, or finished. Changes within a window go in as
//! one line; the same news about the same session is not repeated for a
//! while. Moss's own session is left out by the caller.

use std::collections::HashMap;
use std::time::Duration;

/// Changes this close together go in as one line.
pub const WINDOW: Duration = Duration::from_secs(30);

/// The same state of the same session is not told again within this.
pub const REPEAT: Duration = Duration::from_secs(10 * 60);

/// The news about a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// It waits on you: a permission, a question, input.
    NeedsYou,
    /// Its process went away without ending (crash, kill, power loss).
    Disconnected,
    /// It finished a turn.
    Done,
}

impl Kind {
    /// From the registry's event stream (`cc-moss events`): `needs_you`,
    /// `interrupted` and `finished`; other events are not news for Moss.
    pub fn from_event(kind: &str) -> Option<Kind> {
        match kind {
            "needs_you" => Some(Kind::NeedsYou),
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
    /// Why it needs you (`permission`, `question`, `input`).
    pub why: Option<String>,
}

/// Collects changes and says when a line is due.
#[derive(Debug)]
pub struct Digest {
    window: i64,
    repeat: i64,
    pending: Vec<Change>,
    opened_at: Option<i64>,
    told: HashMap<(String, Kind), i64>,
}

impl Digest {
    pub fn new(window: Duration, repeat: Duration) -> Digest {
        Digest {
            window: window.as_millis() as i64,
            repeat: repeat.as_millis() as i64,
            pending: Vec::new(),
            opened_at: None,
            told: HashMap::new(),
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
        for c in &changes {
            self.told.insert((c.slot_id.clone(), c.kind), now_ms);
        }
        let repeat = self.repeat;
        self.told.retain(|_, at| now_ms - *at < repeat);
        Some(line(&changes, zh))
    }
}

/// `[动态] api 需要你（权限）；login 断了` / `[Update] api needs you (permission); login dropped`.
pub fn line(changes: &[Change], zh: bool) -> String {
    let parts: Vec<String> = changes.iter().map(|c| part(c, zh)).collect();
    if zh {
        format!("[动态] {}", parts.join("；"))
    } else {
        format!("[Update] {}", parts.join("; "))
    }
}

fn part(c: &Change, zh: bool) -> String {
    let name = &c.name;
    match (c.kind, zh) {
        (Kind::NeedsYou, true) => match c.why.as_deref().map(why_zh) {
            Some(why) => format!("{name} 需要你（{why}）"),
            None => format!("{name} 需要你"),
        },
        (Kind::NeedsYou, false) => match c.why.as_deref() {
            Some(why) => format!("{name} needs you ({why})"),
            None => format!("{name} needs you"),
        },
        (Kind::Disconnected, true) => format!("{name} 断了"),
        (Kind::Disconnected, false) => format!("{name} dropped"),
        (Kind::Done, true) => format!("{name} 做完了"),
        (Kind::Done, false) => format!("{name} finished"),
    }
}

fn why_zh(why: &str) -> &str {
    match why {
        "permission" => "权限",
        "question" => "提问",
        "input" => "等输入",
        other => other,
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
            why: (kind == Kind::NeedsYou).then(|| "permission".into()),
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
        d.push(change("api", Kind::NeedsYou), t0);
        assert_eq!(d.due(t0, true), None, "not at once");
        d.push(change("login", Kind::Disconnected), t0 + 10 * S);
        d.push(change("docs", Kind::Done), t0 + 29 * S);
        assert_eq!(d.due(t0 + 29 * S, true), None);
        assert_eq!(
            d.due(t0 + 30 * S, true).as_deref(),
            Some("[动态] api 需要你（权限）；login 断了；docs 做完了")
        );
        assert_eq!(d.due(t0 + 31 * S, true), None, "said once");
        // A change after that opens a new window of its own.
        d.push(change("web", Kind::Done), t0 + 40 * S);
        assert_eq!(d.due(t0 + 60 * S, false), None);
        assert_eq!(
            d.due(t0 + 70 * S, false).as_deref(),
            Some("[Update] web finished")
        );
    }

    #[test]
    fn digest_skips_repeats() {
        let mut d = digest();
        let t0 = 1_000_000;
        d.push(change("api", Kind::Disconnected), t0);
        d.push(change("api", Kind::Disconnected), t0 + S);
        assert_eq!(d.due(t0 + 30 * S, true).as_deref(), Some("[动态] api 断了"));
        // Again within ten minutes: not told; another state of it is.
        d.push(change("api", Kind::Disconnected), t0 + 5 * 60 * S);
        d.push(change("api", Kind::Done), t0 + 5 * 60 * S);
        assert_eq!(
            d.due(t0 + 6 * 60 * S, true).as_deref(),
            Some("[动态] api 做完了")
        );
        // Ten minutes after it was told, it is news again.
        d.push(change("api", Kind::Disconnected), t0 + 30 * S + 10 * 60 * S);
        assert_eq!(
            d.due(t0 + 30 * S + 11 * 60 * S, true).as_deref(),
            Some("[动态] api 断了")
        );
    }

    #[test]
    fn digest_reads_registry_events() {
        assert_eq!(Kind::from_event("needs_you"), Some(Kind::NeedsYou));
        assert_eq!(Kind::from_event("interrupted"), Some(Kind::Disconnected));
        assert_eq!(Kind::from_event("finished"), Some(Kind::Done));
        for other in [
            "started", "working", "ended", "closed", "renamed", "restored",
        ] {
            assert_eq!(Kind::from_event(other), None, "{other}");
        }
    }
}
