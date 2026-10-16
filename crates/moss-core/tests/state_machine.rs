//! Slot state machine, driven by the event sequences recorded in
//! `.sdd/001-cli/notes/hook-probe.md` (Claude Code 2.1.300).

use moss_core::agent::claude::ClaudeAgent;
use moss_core::agent::{Agent, HookKind};
use moss_core::event::{AttentionReason, Context, EndReason, Event, StartSource, TmuxLoc};
use moss_core::registry::Registry;
use moss_core::slot::{Slot, Status};
use serde_json::json;

fn ctx(pid: u32, now_ms: i64) -> Context {
    Context {
        pid: Some(pid),
        pid_start: Some(format!("start-{pid}")),
        tmux: Some(TmuxLoc {
            socket: "/tmp/tmux-501/test".into(),
            session_id: Some("$1".into()),
            session_name: Some("host_job-a".into()),
            pane_id: "%0".into(),
        }),
        launch_args: None,
        now_ms,
    }
}

fn start(source: StartSource, id: &str, title: Option<&str>) -> Event {
    Event::Start {
        source,
        session_id: id.into(),
        cwd: "/work".into(),
        title: title.map(Into::into),
        transcript_path: Some(format!("/t/{id}.jsonl")),
    }
}

fn end(id: &str, reason: &str) -> Event {
    Event::End {
        session_id: id.into(),
        reason: EndReason::parse(reason),
    }
}

fn only_slot(reg: &Registry) -> Slot {
    let slots = reg.slots().unwrap();
    assert_eq!(slots.len(), 1, "expected exactly one slot, got {slots:?}");
    slots.into_iter().next().unwrap()
}

#[test]
fn startup_creates_running_slot() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-a")),
        &ctx(100, 1_000),
    )
    .unwrap();

    let s = only_slot(&reg);
    assert_eq!(s.status, Status::Running);
    assert_eq!(s.session_id, "U1");
    assert_eq!(s.name.as_deref(), Some("job-a"));
    assert_eq!(s.cwd, "/work");
    assert_eq!(s.pid, Some(100));
    assert_eq!(s.tmux.as_ref().unwrap().pane_id, "%0");
    assert_eq!(s.created_at_ms, 1_000);
    assert_eq!(s.last_active_at_ms, 1_000);
}

#[test]
fn startup_outside_tmux_has_null_tmux() {
    let mut reg = Registry::open_in_memory().unwrap();
    let mut c = ctx(100, 1_000);
    c.tmux = None;
    reg.apply(&start(StartSource::Startup, "U1", None), &c)
        .unwrap();
    assert!(only_slot(&reg).tmux.is_none());
}

#[test]
fn clear_rotates_id_in_same_slot() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-b")),
        &ctx(100, 1_000),
    )
    .unwrap();
    // F4: SessionEnd(clear) for the old id, then SessionStart(clear) for the new id, same pid.
    reg.apply(&end("U1", "clear"), &ctx(100, 2_000)).unwrap();
    reg.apply(
        &start(StartSource::Clear, "U2", Some("job-b")),
        &ctx(100, 2_001),
    )
    .unwrap();

    let s = only_slot(&reg);
    assert_eq!(s.status, Status::Running);
    assert_eq!(s.session_id, "U2");
    assert_eq!(s.session_ids, vec!["U1".to_string(), "U2".to_string()]);
    assert_eq!(s.name.as_deref(), Some("job-b"));
}

#[test]
fn compact_rotates_like_clear() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&start(StartSource::Compact, "U2", None), &ctx(100, 2_000))
        .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.session_ids, vec!["U1".to_string(), "U2".to_string()]);
}

#[test]
fn end_reason_matrix() {
    let cases = [
        ("prompt_input_exit", Status::Closed),
        ("logout", Status::Closed),
        ("other", Status::Interrupted),
        ("something_new", Status::Interrupted),
        ("clear", Status::Running),
    ];
    for (reason, want) in cases {
        let mut reg = Registry::open_in_memory().unwrap();
        reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
            .unwrap();
        reg.apply(&end("U1", reason), &ctx(100, 5_000)).unwrap();
        let s = only_slot(&reg);
        assert_eq!(s.status, want, "reason={reason}");
        if want == Status::Running {
            assert_eq!(s.end_reason, None, "reason={reason}");
            assert_eq!(s.ended_at_ms, None, "reason={reason}");
        } else {
            assert_eq!(s.end_reason.as_deref(), Some(reason), "reason={reason}");
            assert_eq!(s.ended_at_ms, Some(5_000), "reason={reason}");
        }
    }
}

#[test]
fn end_other_marks_interrupted() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&end("U1", "other"), &ctx(100, 2_000)).unwrap();
    assert_eq!(only_slot(&reg).status, Status::Interrupted);
}

#[test]
fn resume_reuses_slot() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(
        &start(StartSource::Clear, "U2", Some("job-a")),
        &ctx(100, 1_500),
    )
    .unwrap();
    reg.apply(&end("U2", "other"), &ctx(100, 2_000)).unwrap();
    // After a reboot: resumed with the latest id under a new pid (F9).
    reg.apply(
        &start(StartSource::Resume, "U2", Some("job-a")),
        &ctx(200, 9_000),
    )
    .unwrap();

    let s = only_slot(&reg);
    assert_eq!(s.status, Status::Running);
    assert_eq!(s.pid, Some(200));
    assert_eq!(s.session_id, "U2");
    assert_eq!(s.end_reason, None);
    assert_eq!(s.ended_at_ms, None);
    assert_eq!(s.last_active_at_ms, 9_000);
}

#[test]
fn resume_of_older_id_still_finds_slot() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&start(StartSource::Clear, "U2", None), &ctx(100, 1_500))
        .unwrap();
    reg.apply(&end("U2", "other"), &ctx(100, 2_000)).unwrap();
    reg.apply(&start(StartSource::Resume, "U1", None), &ctx(300, 3_000))
        .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.session_id, "U1");
    assert_eq!(s.status, Status::Running);
}

#[test]
fn prompt_updates_name_and_activity() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    // F2/F3: /rename fires nothing; the next prompt carries the new title.
    reg.apply(
        &Event::Prompt {
            session_id: "U1".into(),
            title: Some("job-b".into()),
            prompt: None,
        },
        &ctx(100, 4_000),
    )
    .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.name.as_deref(), Some("job-b"));
    assert_eq!(s.last_active_at_ms, 4_000);
}

#[test]
fn prompt_without_title_keeps_name() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(
        &Event::Prompt {
            session_id: "U1".into(),
            title: None,
            prompt: None,
        },
        &ctx(100, 4_000),
    )
    .unwrap();
    assert_eq!(only_slot(&reg).name.as_deref(), Some("job-a"));
}

#[test]
fn stop_updates_activity_only() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(
        &Event::Stop {
            session_id: "U1".into(),
        },
        &ctx(100, 7_000),
    )
    .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.last_active_at_ms, 7_000);
    assert_eq!(s.status, Status::Running);
    assert_eq!(s.name.as_deref(), Some("job-a"));
}

#[test]
fn end_for_unknown_session_is_ignored() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&end("nope", "other"), &ctx(100, 1_000)).unwrap();
    assert!(reg.slots().unwrap().is_empty());
}

#[test]
fn end_for_superseded_id_is_ignored() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&start(StartSource::Clear, "U2", None), &ctx(100, 1_500))
        .unwrap();
    // A late End for the pre-clear id must not close the live slot.
    reg.apply(&end("U1", "prompt_input_exit"), &ctx(100, 2_000))
        .unwrap();
    assert_eq!(only_slot(&reg).status, Status::Running);
}

#[test]
fn separate_processes_get_separate_slots() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&start(StartSource::Startup, "V1", None), &ctx(101, 1_000))
        .unwrap();
    assert_eq!(reg.slots().unwrap().len(), 2);
}

#[test]
fn effective_status_rules() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.effective_status_since(|_, _| true, None), Status::Running);
    assert_eq!(
        s.effective_status_since(|_, _| false, None),
        Status::Interrupted
    );

    let mut no_pid = s.clone();
    no_pid.pid = None;
    assert_eq!(
        no_pid.effective_status_since(|_, _| false, None),
        Status::Running
    );

    let mut closed = s;
    closed.status = Status::Closed;
    assert_eq!(
        closed.effective_status_since(|_, _| false, None),
        Status::Closed
    );
}

// ── Review round 1 (2026-10-15): cases found by the independent reviewer and probes F10–F13 ──

fn alive(_: u32, _: Option<&str>) -> bool {
    true
}

#[test]
fn in_session_resume_closes_the_session_left_behind() {
    // F11: `/resume <first>` inside the second session's process.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "FIRST", Some("first")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(&end("FIRST", "prompt_input_exit"), &ctx(100, 2_000))
        .unwrap();
    reg.apply(
        &start(StartSource::Startup, "SECOND", Some("second")),
        &ctx(200, 3_000),
    )
    .unwrap();
    reg.apply(&end("SECOND", "resume"), &ctx(200, 4_000))
        .unwrap();
    reg.apply_with(
        &start(StartSource::Resume, "FIRST", Some("first")),
        &ctx(200, 4_001),
        &alive,
    )
    .unwrap();

    let slots = reg.slots().unwrap();
    let by_name = |n: &str| slots.iter().find(|s| s.name.as_deref() == Some(n)).unwrap();
    assert_eq!(by_name("second").status, Status::Closed);
    assert_eq!(by_name("first").status, Status::Running);
    assert_eq!(by_name("first").pid, Some(200));
}

#[test]
fn start_closes_other_running_slot_of_same_process() {
    // Defence in depth for F11 when the End(resume) was lost.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "A", Some("a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(
        &start(StartSource::Startup, "B", Some("b")),
        &ctx(200, 1_000),
    )
    .unwrap();
    reg.apply_with(
        &start(StartSource::Resume, "A", Some("a")),
        &ctx(200, 2_000),
        &alive,
    )
    .unwrap();
    let slots = reg.slots().unwrap();
    let b = slots
        .iter()
        .find(|s| s.name.as_deref() == Some("b"))
        .unwrap();
    assert_eq!(b.status, Status::Closed);
    assert_eq!(b.end_reason.as_deref(), Some("switched"));
}

#[test]
fn resume_of_id_owned_by_live_slot_creates_new_slot() {
    // Reviewer I3: another process resumes an id from a slot whose process is alive.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "A", Some("x")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(&start(StartSource::Clear, "B", Some("x")), &ctx(100, 1_500))
        .unwrap();
    reg.apply_with(
        &start(StartSource::Resume, "A", Some("x")),
        &ctx(300, 2_000),
        &alive,
    )
    .unwrap();

    let slots = reg.slots().unwrap();
    assert_eq!(slots.len(), 2);
    let live = slots.iter().find(|s| s.pid == Some(100)).unwrap();
    assert_eq!(live.session_id, "B");
    assert_eq!(live.status, Status::Running);
    let fork = slots.iter().find(|s| s.pid == Some(300)).unwrap();
    assert_eq!(fork.session_id, "A");

    // The original process keeps receiving its own events.
    reg.apply(&end("B", "other"), &ctx(100, 3_000)).unwrap();
    let slots = reg.slots().unwrap();
    assert_eq!(
        slots.iter().find(|s| s.pid == Some(100)).unwrap().status,
        Status::Interrupted
    );
}

#[test]
fn clear_does_not_attach_to_dead_slot_with_reused_pid() {
    // Reviewer I2: pid 100 was reused after a crash; pid_start differs.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "OLD", Some("old")),
        &ctx(100, 1_000),
    )
    .unwrap();
    let mut reused = ctx(100, 9_000);
    reused.pid_start = Some("another-process".into());
    reg.apply(&start(StartSource::Clear, "NEW", Some("new")), &reused)
        .unwrap();

    let slots = reg.slots().unwrap();
    assert_eq!(slots.len(), 2, "{slots:?}");
    let old = slots
        .iter()
        .find(|s| s.name.as_deref() == Some("old"))
        .unwrap();
    assert_eq!(old.session_id, "OLD");
}

#[test]
fn prompt_repairs_a_lost_resume() {
    // Reviewer I1: SessionStart(resume) was dropped; the next prompt comes from the new pid.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "A", Some("a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(&end("A", "other"), &ctx(100, 2_000)).unwrap();
    reg.apply(
        &Event::Prompt {
            session_id: "A".into(),
            title: None,
            prompt: None,
        },
        &ctx(400, 5_000),
    )
    .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.status, Status::Running);
    assert_eq!(s.pid, Some(400));
    assert_eq!(s.pid_start.as_deref(), Some("start-400"));
    assert_eq!(s.end_reason, None);
}

#[test]
fn missing_pid_does_not_erase_known_pid() {
    // Reviewer I4.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "A", None), &ctx(100, 1_000))
        .unwrap();
    let mut no_pid = ctx(0, 2_000);
    no_pid.pid = None;
    no_pid.pid_start = None;
    reg.apply(&start(StartSource::Startup, "A", None), &no_pid)
        .unwrap();
    assert_eq!(only_slot(&reg).pid, Some(100));
}

#[test]
fn clear_without_pid_matches_by_tmux_pane() {
    // Reviewer I4: fall back to the pane when CLAUDE_PID is missing.
    let mut reg = Registry::open_in_memory().unwrap();
    let mut c = ctx(0, 1_000);
    c.pid = None;
    c.pid_start = None;
    reg.apply(&start(StartSource::Startup, "A", None), &c)
        .unwrap();
    reg.apply(&start(StartSource::Clear, "B", None), &c)
        .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.session_ids, vec!["A".to_string(), "B".to_string()]);
}

// ── Review round 2 (2026-10-15): regressions found while verifying round 1 ──

fn proc_ctx(pid: u32, pid_start: Option<&str>, now_ms: i64) -> Context {
    Context {
        pid: Some(pid),
        pid_start: pid_start.map(String::from),
        tmux: None,
        launch_args: None,
        now_ms,
    }
}

#[test]
fn unknown_start_time_never_closes_another_slot() {
    // Reviewer R2: crashed slot A (pid 500); after reboot a new process reuses
    // pid 500 and its `ps` lookup timed out, so its start time is unknown.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "A", None),
        &proc_ctx(500, Some("t0"), 1),
    )
    .unwrap();
    reg.apply(
        &start(StartSource::Startup, "B", None),
        &proc_ctx(500, None, 2),
    )
    .unwrap();
    let slots = reg.slots().unwrap();
    let a = slots.iter().find(|s| s.session_id == "A").unwrap();
    assert_ne!(a.status, Status::Closed, "crashed A was silently closed");
}

#[test]
fn two_live_processes_on_one_conversation_stay_separate() {
    // Reviewer R1: P runs A in slot X; Q resumes A and gets slot Y.
    let mut reg = Registry::open_in_memory().unwrap();
    let p = proc_ctx(100, Some("p"), 1);
    let q = proc_ctx(200, Some("q"), 2);
    reg.apply_with(&start(StartSource::Startup, "A", None), &p, &alive)
        .unwrap();
    reg.apply_with(&start(StartSource::Resume, "A", None), &q, &alive)
        .unwrap();
    // P keeps typing, then exits.
    let p3 = proc_ctx(100, Some("p"), 3);
    reg.apply_with(
        &Event::Prompt {
            session_id: "A".into(),
            title: None,
            prompt: None,
        },
        &p3,
        &alive,
    )
    .unwrap();
    let p4 = proc_ctx(100, Some("p"), 4);
    reg.apply_with(&end("A", "prompt_input_exit"), &p4, &alive)
        .unwrap();

    let slots = reg.slots().unwrap();
    let running: Vec<_> = slots
        .iter()
        .filter(|s| s.status == Status::Running)
        .map(|s| s.pid)
        .collect();
    assert_eq!(
        running,
        vec![Some(200)],
        "only Q should be running: {slots:?}"
    );
    let q_slot = slots.iter().find(|s| s.pid == Some(200)).unwrap();
    assert_eq!(q_slot.session_ids, vec!["A".to_string()]);
}

#[test]
fn same_process_does_not_revive_a_closed_slot() {
    // Reviewer M1: a stray event from the same process after it closed.
    let mut reg = Registry::open_in_memory().unwrap();
    let p = proc_ctx(100, Some("p"), 1);
    reg.apply(&start(StartSource::Startup, "A", None), &p)
        .unwrap();
    reg.apply(&end("A", "prompt_input_exit"), &proc_ctx(100, Some("p"), 2))
        .unwrap();
    reg.apply(
        &Event::Stop {
            session_id: "A".into(),
        },
        &proc_ctx(100, Some("p"), 3),
    )
    .unwrap();
    assert_eq!(only_slot(&reg).status, Status::Closed);
}

#[test]
fn unprovable_caller_does_not_steal_a_live_slot() {
    // P's `ps` lookup timed out, so its prompt cannot be matched to its own
    // slot and lands on Q's (most recently active). It must not take it over.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply_with(
        &start(StartSource::Startup, "A", None),
        &proc_ctx(100, Some("p"), 1),
        &alive,
    )
    .unwrap();
    reg.apply_with(
        &start(StartSource::Resume, "A", None),
        &proc_ctx(200, Some("q"), 2),
        &alive,
    )
    .unwrap();
    reg.apply_with(
        &Event::Prompt {
            session_id: "A".into(),
            title: None,
            prompt: None,
        },
        &proc_ctx(100, None, 3),
        &alive,
    )
    .unwrap();
    let slots = reg.slots().unwrap();
    let mut pids: Vec<_> = slots.iter().map(|s| s.pid).collect();
    pids.sort();
    assert_eq!(pids, vec![Some(100), Some(200)], "{slots:?}");
}

// ── T1.4 review round 2: restore marks and process identity ──

fn restored_loc() -> TmuxLoc {
    TmuxLoc {
        socket: "/s".into(),
        session_id: Some("$0".into()),
        session_name: Some("x".into()),
        pane_id: "%0".into(),
    }
}

#[test]
fn restore_mark_does_not_let_a_later_process_steal_the_slot() {
    // Reviewer (b): the restored process's SessionStart was lost and the mark
    // stayed; hours later another process resumes the same conversation.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply_with(
        &start(StartSource::Startup, "X", None),
        &proc_ctx(100, Some("s100"), 1_000),
        &alive,
    )
    .unwrap();
    let snap = reg.slots().unwrap().remove(0);
    assert!(reg.claim_restore(&snap, 10_000_000).unwrap());
    reg.set_restored_process(
        &snap.slot_id,
        200,
        Some("s200"),
        &restored_loc(),
        10_000_000,
    )
    .unwrap();
    reg.apply_with(
        &start(StartSource::Resume, "X", None),
        &proc_ctx(300, Some("s300"), 50_000_000),
        &alive,
    )
    .unwrap();
    let slots = reg.slots().unwrap();
    assert_eq!(
        slots.len(),
        2,
        "the later process gets its own slot: {slots:?}"
    );
    assert!(
        slots.iter().any(|s| s.pid == Some(200)),
        "restored process still tracked"
    );
}

#[test]
fn restore_mark_adopts_only_on_resume_start_within_window() {
    // A wrapper's child reports in right after the restore: it adopts the slot.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply_with(
        &start(StartSource::Startup, "X", None),
        &proc_ctx(100, Some("s100"), 1_000),
        &alive,
    )
    .unwrap();
    let snap = reg.slots().unwrap().remove(0);
    assert!(reg.claim_restore(&snap, 10_000_000).unwrap());
    reg.set_restored_process(
        &snap.slot_id,
        200,
        Some("s200"),
        &restored_loc(),
        10_000_000,
    )
    .unwrap();
    // A stray prompt from another process does not take it over...
    reg.apply_with(
        &Event::Prompt {
            session_id: "X".into(),
            title: None,
            prompt: None,
        },
        &proc_ctx(400, Some("s400"), 10_000_500),
        &alive,
    )
    .unwrap();
    assert_eq!(only_slot(&reg).pid, Some(200));
    // ...but the resumed child's SessionStart does.
    reg.apply_with(
        &start(StartSource::Resume, "X", None),
        &proc_ctx(201, Some("s201"), 10_001_000),
        &alive,
    )
    .unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.pid, Some(201));
    assert_eq!(s.restore_started_at_ms, None);
}

#[test]
fn claim_fails_if_a_hook_touched_the_slot_after_planning() {
    // Reviewer (c): the user resumed the conversation by hand while restore waited at [y/N].
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "X", None),
        &proc_ctx(100, Some("s100"), 1_000),
    )
    .unwrap();
    let snap = reg.slots().unwrap().remove(0);
    reg.apply(
        &start(StartSource::Resume, "X", None),
        &proc_ctx(300, Some("s300"), 6_000),
    )
    .unwrap();
    assert!(!reg.claim_restore(&snap, 70_000).unwrap());
}

#[test]
fn pid_recorded_before_boot_is_dead() {
    // Reviewer (a): start time unknown and the pid reused after a reboot.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "X", None),
        &proc_ctx(512, None, 1_000),
    )
    .unwrap();
    reg.apply(&end("X", "other"), &proc_ctx(512, None, 2_000))
        .unwrap();
    let s = only_slot(&reg);
    // pid 512 now belongs to some daemon, but the machine booted at t=5_000.
    assert_eq!(
        s.effective_status_since(|_, _| true, Some(5_000)),
        Status::Interrupted
    );
    assert_eq!(
        s.effective_status_since(|_, _| true, Some(500)),
        Status::Running
    );
}

#[test]
fn end_after_user_close_keeps_closed() {
    // T2.1: the picker kills a session (Ctrl-X) and closes its slot; claude
    // then gets SIGHUP and reports SessionEnd(other) a moment later.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "A", Some("a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    let id = only_slot(&reg).slot_id;
    reg.close_by_user(&id, 2_000).unwrap();
    reg.apply(&end("A", "other"), &ctx(100, 2_001)).unwrap();
    let s = only_slot(&reg);
    assert_eq!(s.status, Status::Closed);
    assert_eq!(s.end_reason.as_deref(), Some("closed-by-user"));
}

#[test]
fn prompt_and_stop_track_busy() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "A", Some("a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    assert!(!only_slot(&reg).busy);
    let long = format!("  fix the {} bug\nsecond line ignored", "x".repeat(300));
    reg.apply(
        &Event::Prompt {
            session_id: "A".into(),
            title: None,
            prompt: Some(long),
        },
        &ctx(100, 2_000),
    )
    .unwrap();
    let s = only_slot(&reg);
    assert!(s.busy);
    let preview = s.last_prompt.unwrap();
    assert!(preview.starts_with("fix the xxx"), "{preview}");
    assert_eq!(preview.chars().count(), 200);
    reg.apply(
        &Event::Stop {
            session_id: "A".into(),
        },
        &ctx(100, 3_000),
    )
    .unwrap();
    assert!(!only_slot(&reg).busy);
    // A prompt without text keeps the previous preview.
    reg.apply(
        &Event::Prompt {
            session_id: "A".into(),
            title: None,
            prompt: None,
        },
        &ctx(100, 4_000),
    )
    .unwrap();
    assert!(only_slot(&reg).busy);
    assert!(only_slot(&reg).last_prompt.is_some());
    reg.apply(&end("A", "other"), &ctx(100, 5_000)).unwrap();
    assert!(!only_slot(&reg).busy);
}

#[test]
fn set_name_fills_a_missing_name() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Resume, "A", None), &ctx(100, 1_000))
        .unwrap();
    let id = only_slot(&reg).slot_id;
    reg.set_name(&id, "job-final").unwrap();
    assert_eq!(only_slot(&reg).name.as_deref(), Some("job-final"));
}

#[test]
fn backfill_picks_up_a_rename_made_after_an_earlier_look() {
    use std::io::Write;
    let mut t = tempfile::NamedTempFile::new().unwrap();
    writeln!(t, r#"{{"type":"user","message":"hi"}}"#).unwrap();
    let mut reg = Registry::open_in_memory().unwrap();
    let event = Event::Start {
        source: StartSource::Resume,
        session_id: "A".into(),
        cwd: "/work".into(),
        title: None,
        transcript_path: Some(t.path().to_string_lossy().into_owned()),
    };
    reg.apply(&event, &ctx(100, 1_000)).unwrap();
    assert_eq!(reg.backfill_names().unwrap(), 0);
    writeln!(t, r#"{{"type":"custom-title","customTitle":"job-late"}}"#).unwrap();
    assert_eq!(reg.backfill_names().unwrap(), 1);
    assert_eq!(only_slot(&reg).name.as_deref(), Some("job-late"));
}

// ---- Event stream and attention (T2.1) ----

fn prompt(id: &str, title: Option<&str>) -> Event {
    Event::Prompt {
        session_id: id.into(),
        title: title.map(Into::into),
        prompt: Some("do the thing".into()),
    }
}

fn stop(id: &str) -> Event {
    Event::Stop {
        session_id: id.into(),
    }
}

fn attention(id: &str, reason: AttentionReason) -> Event {
    Event::Attention {
        session_id: id.into(),
        reason,
        message: Some("Claude needs your permission".into()),
    }
}

fn tool_done(id: &str) -> Event {
    Event::ToolDone {
        session_id: id.into(),
    }
}

/// `(kind, detail)` of every recorded event, in order.
fn events(reg: &Registry) -> Vec<(String, serde_json::Value)> {
    reg.events(Some(0), 10_000)
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.detail))
        .collect()
}

fn kinds(reg: &Registry) -> Vec<String> {
    events(reg).into_iter().map(|(k, _)| k).collect()
}

#[test]
fn events_started_working_finished() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(&prompt("U1", Some("job-a")), &ctx(100, 2_000))
        .unwrap();
    // Still busy: a second prompt is not a second `working`.
    reg.apply(&prompt("U1", None), &ctx(100, 2_500)).unwrap();
    reg.apply(&stop("U1"), &ctx(100, 3_000)).unwrap();
    // Already idle: a second Stop is not a second `finished`.
    reg.apply(&stop("U1"), &ctx(100, 3_500)).unwrap();

    assert_eq!(kinds(&reg), ["started", "working", "finished"]);
    assert_eq!(events(&reg)[0].1, json!({ "source": "startup" }));
    assert_eq!(events(&reg)[1].1, json!({}), "no prompt text in events");
    let e = &reg.events(None, 500).unwrap()[0];
    assert_eq!(e.name.as_deref(), Some("job-a"));
    assert_eq!(e.at_ms, 1_000);
}

#[test]
fn events_started_on_resume_but_not_on_clear() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&end("U1", "clear"), &ctx(100, 1_500)).unwrap();
    reg.apply(&start(StartSource::Clear, "U2", None), &ctx(100, 1_501))
        .unwrap();
    reg.apply(&start(StartSource::Compact, "U3", None), &ctx(100, 1_600))
        .unwrap();
    reg.apply(&end("U3", "other"), &ctx(100, 2_000)).unwrap();
    reg.apply(&start(StartSource::Resume, "U3", None), &ctx(200, 3_000))
        .unwrap();
    assert_eq!(kinds(&reg), ["started", "interrupted", "started"]);
    assert_eq!(events(&reg)[2].1, json!({ "source": "resume" }));
}

#[test]
fn events_rename() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &start(StartSource::Startup, "U1", Some("job-a")),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(&prompt("U1", Some("job-a")), &ctx(100, 2_000))
        .unwrap();
    reg.apply(&prompt("U1", Some("job-b")), &ctx(100, 3_000))
        .unwrap();
    let slot = only_slot(&reg);
    reg.set_name(&slot.slot_id, "job-c").unwrap();
    reg.set_name(&slot.slot_id, "job-c").unwrap();

    let renames: Vec<_> = events(&reg)
        .into_iter()
        .filter(|(k, _)| k == "renamed")
        .map(|(_, d)| d)
        .collect();
    assert_eq!(
        renames,
        [
            json!({ "from": "job-a", "to": "job-b" }),
            json!({ "from": "job-b", "to": "job-c" }),
        ]
    );
}

#[test]
fn events_end_closed_vs_interrupted() {
    for (reason, want) in [
        ("prompt_input_exit", Some("ended")),
        ("logout", Some("ended")),
        ("other", Some("interrupted")),
        ("clear", None),
    ] {
        let mut reg = Registry::open_in_memory().unwrap();
        reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
            .unwrap();
        reg.apply(&end("U1", reason), &ctx(100, 2_000)).unwrap();
        let got = events(&reg);
        match want {
            Some(kind) => {
                assert_eq!(got.len(), 2, "{reason}: {got:?}");
                assert_eq!(got[1], (kind.to_string(), json!({ "reason": reason })));
            }
            None => assert_eq!(got.len(), 1, "{reason}: {got:?}"),
        }
    }

    // Closed by the user, then the killed process reports SessionEnd(other):
    // one `closed`, no `interrupted`.
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    let id = only_slot(&reg).slot_id;
    reg.close_by_user(&id, 1_500).unwrap();
    reg.apply(&end("U1", "other"), &ctx(100, 2_000)).unwrap();
    assert_eq!(kinds(&reg), ["started", "closed"]);
}

#[test]
fn events_interrupted_once_per_interruption() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&end("U1", "other"), &ctx(100, 2_000)).unwrap();
    let id = only_slot(&reg).slot_id;
    // Reconcile later sees the process gone too: already reported.
    assert!(!reg.record_lost(&id, 3_000).unwrap());
    reg.apply(&start(StartSource::Resume, "U1", None), &ctx(200, 4_000))
        .unwrap();
    // A second crash is a second interruption.
    assert!(reg.record_lost(&id, 5_000).unwrap());
    assert!(!reg.record_lost(&id, 6_000).unwrap());
    assert_eq!(
        kinds(&reg),
        ["started", "interrupted", "started", "interrupted"]
    );
}

#[test]
fn attention_set_by_permission_prompt() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&prompt("U1", None), &ctx(100, 2_000)).unwrap();
    let parsed = ClaudeAgent
        .parse_hook(
            HookKind::Notification,
            r#"{"session_id":"U1","hook_event_name":"Notification",
                "message":"Claude needs your permission","notification_type":"permission_prompt"}"#,
        )
        .unwrap()
        .expect("a permission prompt is acted on");
    reg.apply(&parsed, &ctx(100, 3_000)).unwrap();

    assert_eq!(only_slot(&reg).attention.as_deref(), Some("permission"));
    assert_eq!(
        events(&reg).last().unwrap(),
        &(
            "needs_you".to_string(),
            json!({ "reason": "permission", "message": "Claude needs your permission" })
        )
    );
}

#[test]
fn attention_reasons_from_notification_types() {
    for (kind, want) in [
        ("permission_prompt", "permission"),
        ("elicitation_dialog", "question"),
        ("elicitation_url_dialog", "question"),
        ("agent_needs_input", "input"),
    ] {
        let payload = json!({ "session_id": "U1", "notification_type": kind, "message": "m" });
        let event = ClaudeAgent
            .parse_hook(HookKind::Notification, &payload.to_string())
            .unwrap()
            .unwrap();
        let mut reg = Registry::open_in_memory().unwrap();
        reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
            .unwrap();
        reg.apply(&event, &ctx(100, 2_000)).unwrap();
        assert_eq!(only_slot(&reg).attention.as_deref(), Some(want), "{kind}");
    }
}

#[test]
fn attention_ignores_idle_prompt() {
    for kind in ["idle_prompt", "agent_completed"] {
        let payload = json!({ "session_id": "U1", "notification_type": kind, "message": "m" });
        let parsed = ClaudeAgent
            .parse_hook(HookKind::Notification, &payload.to_string())
            .unwrap();
        assert_eq!(parsed, None, "{kind}");
    }
    // A type nobody knows yet is not guessed at; the hook logs it.
    let payload = json!({ "session_id": "U1", "notification_type": "brand_new" });
    let err = ClaudeAgent
        .parse_hook(HookKind::Notification, &payload.to_string())
        .unwrap_err();
    assert!(err.to_string().contains("brand_new"), "{err}");
}

#[test]
fn post_tool_use_clears_attention() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(
        &attention("U1", AttentionReason::Permission),
        &ctx(100, 2_000),
    )
    .unwrap();
    let before = kinds(&reg);
    let parsed = ClaudeAgent
        .parse_hook(
            HookKind::PostToolUse,
            r#"{"session_id":"U1","tool_name":"Bash","tool_input":{"command":"ls"}}"#,
        )
        .unwrap()
        .unwrap();
    assert_eq!(parsed, tool_done("U1"));
    reg.apply(&parsed, &ctx(100, 3_000)).unwrap();
    assert_eq!(only_slot(&reg).attention, None);
    assert_eq!(kinds(&reg), before, "clearing is not an event");
}

#[test]
fn attention_cleared_by_stop() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&prompt("U1", None), &ctx(100, 1_500)).unwrap();
    reg.apply(
        &attention("U1", AttentionReason::Question),
        &ctx(100, 2_000),
    )
    .unwrap();
    assert_eq!(only_slot(&reg).attention.as_deref(), Some("question"));
    // Denied: no tool ran, the turn just ended.
    reg.apply(&stop("U1"), &ctx(100, 3_000)).unwrap();
    assert_eq!(only_slot(&reg).attention, None);
}

#[test]
fn attention_cleared_by_prompt_end_and_start() {
    let cases: [(&str, Event); 3] = [
        ("prompt", prompt("U1", None)),
        ("end", end("U1", "prompt_input_exit")),
        ("start", start(StartSource::Resume, "U1", None)),
    ];
    for (what, event) in cases {
        let mut reg = Registry::open_in_memory().unwrap();
        reg.apply(&start(StartSource::Startup, "U1", None), &ctx(100, 1_000))
            .unwrap();
        reg.apply(&attention("U1", AttentionReason::Input), &ctx(100, 2_000))
            .unwrap();
        reg.apply(&event, &ctx(100, 3_000)).unwrap();
        assert_eq!(only_slot(&reg).attention, None, "{what}");
    }
}

#[test]
fn attention_for_unknown_session_is_ignored() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(
        &attention("nobody", AttentionReason::Permission),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(&tool_done("nobody"), &ctx(100, 1_000)).unwrap();
    assert!(reg.slots().unwrap().is_empty());
    assert!(kinds(&reg).is_empty());
}
