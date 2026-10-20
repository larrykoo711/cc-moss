use std::collections::HashSet;
use std::path::Path;

use moss_core::event::TmuxLoc;
use moss_core::restore::{plan, resolve, Verdict, PENDING_WINDOW_MS};
use moss_core::slot::{Notes, Slot, Status};

fn slot(name: &str, status: Status) -> Slot {
    Slot {
        slot_id: format!("slot-{name}"),
        agent: "claude".into(),
        session_id: format!("{name}-0000-1111"),
        session_ids: vec![format!("{name}-0000-1111")],
        name: Some(name.into()),
        cwd: "/work".into(),
        pid: Some(1),
        pid_start: None,
        tmux: Some(TmuxLoc {
            socket: "/tmp/s".into(),
            session_id: Some("$1".into()),
            session_name: Some(format!("host_{name}")),
            pane_id: "%0".into(),
        }),
        status,
        end_reason: None,
        notes: Notes::default(),
        launch_args: vec![],
        transcript_path: Some(format!("/t/{name}.jsonl")),
        restore_started_at_ms: None,
        role: None,
        busy: false,
        last_prompt: None,
        attention: None,
        created_at_ms: 0,
        last_active_at_ms: 0,
        ended_at_ms: None,
        archived_at_ms: None,
        archived_by: None,
        provider_started: None,
        provider_seen_keys: None,
    }
}

fn all_exist(_: &Path) -> bool {
    true
}

fn verdicts(
    rows: &[(Status, Slot)],
    names: &[&str],
    exists: impl Fn(&Path) -> bool,
) -> Vec<(String, Verdict)> {
    let ids: HashSet<String> = names.iter().map(|n| format!("slot-{n}")).collect();
    let only = (!names.is_empty()).then_some(&ids);
    plan(rows, only, 100_000, exists)
        .into_iter()
        .map(|c| (c.slot.name.clone().unwrap(), c.verdict))
        .collect()
}

#[test]
fn candidates_only_interrupted() {
    let rows = vec![
        (Status::Interrupted, slot("a", Status::Interrupted)),
        (Status::Running, slot("b", Status::Running)),
        (Status::Closed, slot("c", Status::Closed)),
    ];
    assert_eq!(
        verdicts(&rows, &[], all_exist),
        vec![("a".into(), Verdict::Restore)]
    );
}

#[test]
fn name_filter() {
    let rows = vec![
        (Status::Interrupted, slot("a", Status::Interrupted)),
        (Status::Interrupted, slot("b", Status::Interrupted)),
    ];
    assert_eq!(
        verdicts(&rows, &["b"], all_exist),
        vec![("b".into(), Verdict::Restore)]
    );
}

#[test]
fn empty_session_skipped() {
    // F8: killed before the first message, so no transcript was ever written.
    let rows = vec![(Status::Interrupted, slot("a", Status::Interrupted))];
    let v = verdicts(&rows, &[], |p| !p.to_string_lossy().ends_with(".jsonl"));
    assert_eq!(v, vec![("a".into(), Verdict::EmptySession)]);

    // No recorded path (older agents): not provably empty, so restore.
    let mut s = slot("b", Status::Interrupted);
    s.transcript_path = None;
    let v = verdicts(&[(Status::Interrupted, s)], &[], |p| {
        !p.to_string_lossy().ends_with(".jsonl")
    });
    assert_eq!(v, vec![("b".into(), Verdict::Restore)]);
}

#[test]
fn missing_cwd_skipped() {
    let rows = vec![(Status::Interrupted, slot("a", Status::Interrupted))];
    let v = verdicts(&rows, &[], |p| p != Path::new("/work"));
    assert_eq!(v, vec![("a".into(), Verdict::MissingCwd)]);
}

#[test]
fn pending_restore_skipped() {
    let mut s = slot("a", Status::Interrupted);
    s.restore_started_at_ms = Some(100_000 - PENDING_WINDOW_MS + 1);
    let v = verdicts(&[(Status::Interrupted, s.clone())], &[], all_exist);
    assert_eq!(v, vec![("a".into(), Verdict::RestorePending)]);

    // After the window the previous attempt is considered failed.
    s.restore_started_at_ms = Some(100_000 - PENDING_WINDOW_MS - 1);
    let v = verdicts(&[(Status::Interrupted, s)], &[], all_exist);
    assert_eq!(v, vec![("a".into(), Verdict::Restore)]);
}

#[test]
fn resolve_cases() {
    let mut x = slot("job-a", Status::Running);
    x.session_id = "abcdef12-3456".into();
    let mut y = slot("other", Status::Running);
    y.tmux.as_mut().unwrap().session_name = Some("host_job-a-tmux".into());
    y.session_id = "abcdef99-0000".into();
    let slots = vec![x, y];

    assert_eq!(resolve(&slots, "job-a").unwrap().slot_id, "slot-job-a");
    assert_eq!(
        resolve(&slots, "host_job-a-tmux").unwrap().slot_id,
        "slot-other"
    );
    assert_eq!(resolve(&slots, "abcdef99").unwrap().slot_id, "slot-other");
    // Too short to be an id prefix.
    assert!(resolve(&slots, "abc").is_err());
    // Ambiguous id prefix lists both.
    let err = resolve(&slots, "abcdef").unwrap_err().to_string();
    assert!(err.contains("job-a") && err.contains("other"), "{err}");
    assert!(resolve(&slots, "nope").is_err());
}

#[test]
fn duplicates_pick_the_newest_restorable_copy() {
    // Reviewer (d2): the newest copy's directory is gone; the older one is restored.
    let mut old = slot("api", Status::Interrupted);
    old.slot_id = "old".into();
    old.last_active_at_ms = 1;
    let mut new = slot("api", Status::Interrupted);
    new.slot_id = "new".into();
    new.last_active_at_ms = 2;
    new.cwd = "/gone".into();
    let rows = vec![
        (Status::Interrupted, old.clone()),
        (Status::Interrupted, new.clone()),
    ];
    let v: Vec<_> = plan(&rows, None, 100_000, |p: &Path| p != Path::new("/gone"))
        .into_iter()
        .map(|c| (c.slot.slot_id.clone(), c.verdict))
        .collect();
    assert_eq!(
        v,
        vec![
            ("old".into(), Verdict::Restore),
            ("new".into(), Verdict::MissingCwd)
        ]
    );

    // Reviewer (d1): naming the older copy restores it.
    let mut new_ok = new;
    new_ok.cwd = "/work".into();
    let rows = vec![(Status::Interrupted, old), (Status::Interrupted, new_ok)];
    let only: HashSet<String> = ["old".to_string()].into();
    let v: Vec<_> = plan(&rows, Some(&only), 100_000, all_exist)
        .into_iter()
        .map(|c| c.verdict)
        .collect();
    assert_eq!(v, vec![Verdict::Restore]);
}

#[test]
fn moss_is_never_restored() {
    let mut moss = slot("moss", Status::Interrupted);
    moss.role = Some("moss".into());
    let rows = vec![
        (Status::Interrupted, moss),
        (Status::Interrupted, slot("a", Status::Interrupted)),
    ];
    let names: Vec<String> = verdicts(&rows, &[], all_exist)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(names, ["a"]);
}
