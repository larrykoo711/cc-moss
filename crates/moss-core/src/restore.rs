//! Which interrupted sessions to bring back, and resolving names to slots.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::slot::{Slot, Status};

/// A restore that started this recently and has not been confirmed yet is
/// still in flight. Normally the restored process's pid decides; this window
/// only covers launches whose pid could not be recorded.
pub const PENDING_WINDOW_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Restore,
    /// Killed before the first message: Claude Code wrote no transcript (F8).
    EmptySession,
    MissingCwd,
    RestorePending,
    /// The same conversation is open in another, live process.
    OpenElsewhere,
    /// Another interrupted slot of the same conversation is restored instead.
    Duplicate,
}

#[derive(Debug)]
pub struct Candidate<'a> {
    pub slot: &'a Slot,
    pub verdict: Verdict,
}

/// Interrupted slots (by effective status), optionally limited to the slot
/// ids in `only`, each with a verdict. A conversation is restored at most
/// once: never while it is open anywhere, and from one copy only: the most
/// recently active copy that is itself restorable, among the selected ones.
/// Moss is never a candidate. `exists` is injected so the rules are testable.
pub fn plan<'a>(
    rows: &'a [(Status, Slot)],
    only: Option<&HashSet<String>>,
    now_ms: i64,
    exists: impl Fn(&Path) -> bool,
) -> Vec<Candidate<'a>> {
    let open: HashSet<&str> = rows
        .iter()
        .filter(|(st, _)| *st == Status::Running)
        .map(|(_, s)| s.session_id.as_str())
        .collect();
    let mut out: Vec<Candidate<'a>> = rows
        .iter()
        .filter(|(status, _)| *status == Status::Interrupted)
        .map(|(_, slot)| slot)
        // The daemon brings Moss back itself; restoring it would make two.
        .filter(|slot| !slot.is_moss())
        .filter(|slot| only.is_none_or(|ids| ids.contains(&slot.slot_id)))
        .map(|slot| Candidate {
            slot,
            verdict: if open.contains(slot.session_id.as_str()) {
                Verdict::OpenElsewhere
            } else {
                verdict(slot, now_ms, &exists)
            },
        })
        .collect();
    // Among restorable copies of one conversation, keep the newest.
    let mut newest: HashMap<&str, (&str, i64)> = HashMap::new();
    for c in out.iter().filter(|c| c.verdict == Verdict::Restore) {
        let e = newest
            .entry(c.slot.session_id.as_str())
            .or_insert((c.slot.slot_id.as_str(), c.slot.last_active_at_ms));
        if c.slot.last_active_at_ms > e.1 {
            *e = (c.slot.slot_id.as_str(), c.slot.last_active_at_ms);
        }
    }
    for c in out.iter_mut().filter(|c| c.verdict == Verdict::Restore) {
        if newest[c.slot.session_id.as_str()].0 != c.slot.slot_id {
            c.verdict = Verdict::Duplicate;
        }
    }
    out
}

fn verdict(slot: &Slot, now_ms: i64, exists: &impl Fn(&Path) -> bool) -> Verdict {
    if slot
        .restore_started_at_ms
        .is_some_and(|t| now_ms - t < PENDING_WINDOW_MS)
    {
        return Verdict::RestorePending;
    }
    if !exists(Path::new(&slot.cwd)) {
        return Verdict::MissingCwd;
    }
    // A recorded transcript path that is missing proves the session is empty;
    // no recorded path proves nothing, so we try.
    if let Some(t) = &slot.transcript_path {
        if !exists(Path::new(t)) {
            return Verdict::EmptySession;
        }
    }
    Verdict::Restore
}

/// Shortest session id prefix accepted as a name.
const MIN_ID_PREFIX: usize = 6;

/// Find one slot by Claude session name, else tmux session name, else a
/// session id prefix of at least six characters. Open slots (running or
/// interrupted) win over closed ones with the same name.
pub fn resolve<'a>(slots: &'a [Slot], query: &str) -> std::result::Result<&'a Slot, ResolveError> {
    let tiers: [&dyn Fn(&Slot) -> bool; 3] = [
        &|s| s.name.as_deref() == Some(query),
        &|s| s.tmux.as_ref().and_then(|t| t.session_name.as_deref()) == Some(query),
        &|s| {
            query.len() >= MIN_ID_PREFIX
                && std::iter::once(&s.session_id)
                    .chain(&s.session_ids)
                    .any(|id| id.starts_with(query))
        },
    ];
    for tier in tiers {
        let found: Vec<&Slot> = slots.iter().filter(|s| tier(s)).collect();
        let open: Vec<&Slot> = found
            .iter()
            .copied()
            .filter(|s| s.status != Status::Closed)
            .collect();
        let pick = if open.is_empty() { found } else { open };
        match pick.as_slice() {
            [] => continue,
            [one] => return Ok(one),
            many => {
                return Err(ResolveError::Ambiguous {
                    query: query.to_string(),
                    matches: many
                        .iter()
                        .map(|s| {
                            format!(
                                "{} ({})",
                                s.name.as_deref().unwrap_or("-"),
                                s.session_id.chars().take(8).collect::<String>()
                            )
                        })
                        .collect(),
                })
            }
        }
    }
    Err(ResolveError::NotFound(query.to_string()))
}

/// Why a name did not pick exactly one session. Callers word it for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    NotFound(String),
    /// `matches` are "name (id prefix)".
    Ambiguous {
        query: String,
        matches: Vec<String>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound(q) => {
                write!(f, "no session named `{q}` (see `cc-moss ls --all`)")
            }
            ResolveError::Ambiguous { query, matches } => write!(
                f,
                "`{query}` matches several sessions: {} (use an id prefix)",
                matches.join(", ")
            ),
        }
    }
}

impl std::error::Error for ResolveError {}
