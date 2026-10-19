//! The archive (T2.5): sessions nobody used for a while leave the list but
//! stay restorable. Which ones go is decided here; the registry records it.

use crate::slot::{Slot, Status};

/// `[sessions] archive_after_days` when unset.
pub const DEFAULT_AFTER_DAYS: u32 = 30;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The sessions the daily tidy-up archives: not running, not Moss, not
/// archived yet, and last active more than `days` days before `now_ms`.
/// `days` 0 means never. `rows` carries each session's status as shown
/// (a running one is never archived, however old).
pub fn idle(rows: &[(Status, Slot)], now_ms: i64, days: u32) -> Vec<String> {
    if days == 0 {
        return Vec::new();
    }
    let cut = now_ms - i64::from(days) * DAY_MS;
    rows.iter()
        .filter(|(status, _)| *status != Status::Running)
        .map(|(_, slot)| slot)
        .filter(|slot| !slot.is_moss() && !slot.is_archived())
        .filter(|slot| slot.last_active_at_ms < cut)
        .map(|slot| slot.slot_id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(id: &str, last_active_at_ms: i64) -> Slot {
        Slot {
            slot_id: id.into(),
            agent: "claude".into(),
            session_id: format!("{id}-sid"),
            session_ids: vec![],
            name: Some(id.into()),
            cwd: "/w".into(),
            pid: None,
            pid_start: None,
            tmux: None,
            status: Status::Interrupted,
            end_reason: None,
            notes: Default::default(),
            launch_args: vec![],
            transcript_path: None,
            busy: false,
            attention: None,
            last_prompt: None,
            restore_started_at_ms: None,
            role: None,
            created_at_ms: 0,
            last_active_at_ms,
            ended_at_ms: None,
            archived_at_ms: None,
            provider_started: None,
            provider_seen_keys: None,
        }
    }

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn archive_skips_running_sessions() {
        let old = NOW - 400 * DAY_MS;
        let rows = vec![
            (Status::Running, slot("running", old)),
            (Status::Interrupted, slot("dropped", old)),
        ];
        assert_eq!(idle(&rows, NOW, 30), vec!["dropped".to_string()]);
    }

    #[test]
    fn archive_picks_only_sessions_idle_past_the_limit() {
        let mut moss = slot("moss", NOW - 90 * DAY_MS);
        moss.role = Some("moss".into());
        let mut done = slot("done", NOW - 90 * DAY_MS);
        done.archived_at_ms = Some(NOW - DAY_MS);
        let rows = vec![
            (Status::Closed, slot("old", NOW - 31 * DAY_MS)),
            (Status::Interrupted, slot("recent", NOW - 29 * DAY_MS)),
            (Status::Interrupted, moss),
            (Status::Closed, done),
        ];
        assert_eq!(idle(&rows, NOW, 30), vec!["old".to_string()]);
        assert!(idle(&rows, NOW, 0).is_empty(), "0 switches it off");
        assert_eq!(idle(&rows, NOW, 7).len(), 2);
    }
}
