//! The event stream: order, increments, retention.

use moss_core::event::{AttentionReason, Context, Event, StartSource};
use moss_core::registry::{Registry, EVENT_RETENTION_MS, MESSAGE_MAX};

fn ctx(now_ms: i64) -> Context {
    Context {
        pid: Some(100),
        pid_start: Some("start-100".into()),
        tmux: None,
        launch_args: None,
        now_ms,
    }
}

fn start(id: &str) -> Event {
    Event::Start {
        source: StartSource::Startup,
        session_id: id.into(),
        cwd: "/work".into(),
        title: Some("job-a".into()),
        transcript_path: None,
    }
}

fn prompt(id: &str) -> Event {
    Event::Prompt {
        session_id: id.into(),
        title: None,
        prompt: None,
    }
}

fn stop(id: &str) -> Event {
    Event::Stop {
        session_id: id.into(),
    }
}

fn seqs(reg: &Registry, since: Option<i64>, limit: usize) -> Vec<i64> {
    reg.events(since, limit)
        .unwrap()
        .into_iter()
        .map(|e| e.seq)
        .collect()
}

/// A registry with `turns` prompt/stop pairs whose clock runs backwards,
/// so ordering by time and by sequence disagree.
fn busy_registry(turns: i64) -> Registry {
    let mut reg = Registry::open_in_memory().unwrap();
    let mut now = 1_000_000;
    reg.apply(&start("U1"), &ctx(now)).unwrap();
    for _ in 0..turns {
        now -= 10;
        reg.apply(&prompt("U1"), &ctx(now)).unwrap();
        now -= 10;
        reg.apply(&stop("U1"), &ctx(now)).unwrap();
    }
    reg
}

#[test]
fn since_returns_only_newer_in_order() {
    let reg = busy_registry(5);
    let all = reg.events(Some(0), 500).unwrap();
    assert_eq!(all.len(), 11);
    assert!(all.windows(2).all(|w| w[0].seq < w[1].seq), "{all:?}");
    assert_eq!(all[0].kind, "started");

    let after = all[3].seq;
    let newer = seqs(&reg, Some(after), 500);
    assert_eq!(newer, all[4..].iter().map(|e| e.seq).collect::<Vec<_>>());

    // Paging with the limit walks the same order.
    assert_eq!(seqs(&reg, Some(after), 2), newer[..2]);
    assert_eq!(seqs(&reg, Some(newer[1]), 2), newer[2..4]);
    assert!(seqs(&reg, Some(all[10].seq), 500).is_empty());
    assert_eq!(reg.last_event_seq().unwrap(), all[10].seq);
}

#[test]
fn without_since_returns_the_latest_fifty_oldest_first() {
    let reg = busy_registry(40);
    let all = seqs(&reg, Some(0), 500);
    assert_eq!(all.len(), 81);
    assert_eq!(seqs(&reg, None, 500), all[31..]);
    assert_eq!(seqs(&reg, None, 3), all[78..]);
}

#[test]
fn prune_drops_older_than_seven_days() {
    let mut reg = Registry::open_in_memory().unwrap();
    let now = 100 * 24 * 60 * 60 * 1000;
    let old = now - EVENT_RETENTION_MS - 1;
    reg.apply(&start("U1"), &ctx(old - 1)).unwrap();
    reg.apply(&prompt("U1"), &ctx(old)).unwrap();
    reg.apply(&stop("U1"), &ctx(now - 1_000)).unwrap();
    let before = reg.last_event_seq().unwrap();

    assert_eq!(reg.prune_events(now - EVENT_RETENTION_MS).unwrap(), 2);
    let left = reg.events(Some(0), 500).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].kind, "finished");

    // Sequence numbers are never reused, even after the newest is pruned.
    assert_eq!(reg.prune_events(now + 1).unwrap(), 1);
    reg.apply(&prompt("U1"), &ctx(now)).unwrap();
    assert!(reg.events(Some(0), 500).unwrap()[0].seq > before);
}

#[test]
fn message_is_truncated() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start("U1"), &ctx(1_000)).unwrap();
    let long: String = "需要你确认".repeat(100);
    reg.apply(
        &Event::Attention {
            session_id: "U1".into(),
            reason: AttentionReason::Permission,
            message: Some(long.clone()),
        },
        &ctx(2_000),
    )
    .unwrap();
    let e = reg.events(Some(0), 500).unwrap().pop().unwrap();
    assert_eq!(e.kind, "needs_you");
    let message = e.detail["message"].as_str().unwrap();
    assert_eq!(message.chars().count(), MESSAGE_MAX);
    assert!(long.starts_with(message));
}

#[test]
fn interrupted_is_not_reported_again_after_prune() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start("U1"), &ctx(1_000)).unwrap();
    let id = reg.slots().unwrap()[0].slot_id.clone();
    assert!(reg.record_lost(&id, 2_000).unwrap());

    // Eight days on, the event is pruned; the crash is still the same crash.
    let later = 2_000 + EVENT_RETENTION_MS + 24 * 60 * 60 * 1000;
    reg.prune_events(later - EVENT_RETENTION_MS).unwrap();
    assert!(reg.events(Some(0), 500).unwrap().is_empty());
    assert!(!reg.record_lost(&id, later).unwrap());
    assert!(reg.events(Some(0), 500).unwrap().is_empty());

    // Brought back and lost again: a new interruption.
    reg.apply(&start_resume("U1"), &ctx(later + 1)).unwrap();
    assert!(reg.record_lost(&id, later + 2).unwrap());
}

#[test]
fn last_seq_never_moves_back_after_prune() {
    let reg = busy_registry(3);
    let before = reg.last_event_seq().unwrap();
    assert!(before > 0);
    reg.prune_events(i64::MAX).unwrap();
    assert!(reg.events(Some(0), 500).unwrap().is_empty());
    assert_eq!(reg.last_event_seq().unwrap(), before);
}

fn start_resume(id: &str) -> Event {
    Event::Start {
        source: StartSource::Resume,
        session_id: id.into(),
        cwd: "/work".into(),
        title: None,
        transcript_path: None,
    }
}

#[test]
fn batched_prune_waits_for_an_hours_worth() {
    let mut reg = Registry::open_in_memory().unwrap();
    let hour = 60 * 60 * 1000;
    let now = 100 * 24 * hour;
    let cut = now - EVENT_RETENTION_MS;
    reg.apply(&start("U1"), &ctx(cut - 11)).unwrap();
    reg.apply(&prompt("U1"), &ctx(cut - 10)).unwrap();
    reg.apply(&stop("U1"), &ctx(now - 1_000)).unwrap();
    // Just past the cut: left for the next batch.
    assert_eq!(reg.prune_events_batched(cut, hour).unwrap(), 0);
    // An hour later it goes, with everything else past the cut.
    assert_eq!(reg.prune_events_batched(cut + hour, hour).unwrap(), 2);
    assert_eq!(reg.events(Some(0), 500).unwrap().len(), 1);
}
