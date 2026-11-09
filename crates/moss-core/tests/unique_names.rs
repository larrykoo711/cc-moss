//! Names stay one of a kind among open sessions (F61): a session forked in
//! the background is born with its parent's name, written into its own
//! transcript under its new id, and gets `-2`, `-3` like `cc-moss new`.

use std::io::Write;

use moss_core::event::{Context, EndReason, Event, StartSource};
use moss_core::registry::Registry;
use moss_core::slot::Slot;

fn ctx(pid: u32, now_ms: i64) -> Context {
    Context {
        pid: Some(pid),
        pid_start: Some(format!("start-{pid}")),
        tmux: None,
        launch_args: None,
        provider: None,
        now_ms,
    }
}

fn start(id: &str, title: Option<&str>, transcript: Option<&str>) -> Event {
    Event::Start {
        source: StartSource::Startup,
        session_id: id.into(),
        cwd: "/work".into(),
        title: title.map(Into::into),
        transcript_path: transcript.map(Into::into),
    }
}

fn slot(reg: &Registry, session_id: &str) -> Slot {
    reg.slots()
        .unwrap()
        .into_iter()
        .rev()
        .find(|s| s.session_id == session_id)
        .unwrap_or_else(|| panic!("no slot for {session_id}"))
}

fn name(reg: &Registry, session_id: &str) -> Option<String> {
    slot(reg, session_id).name
}

fn titled(title: &str) -> tempfile::NamedTempFile {
    let mut t = tempfile::NamedTempFile::new().unwrap();
    writeln!(t, r#"{{"type":"custom-title","customTitle":"{title}"}}"#).unwrap();
    t
}

#[test]
fn registry_unique_name_on_insert() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start("A", Some("proj-a"), None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(&start("B", Some("proj-a"), None), &ctx(200, 2_000))
        .unwrap();
    reg.apply(&start("C", Some("proj-a"), None), &ctx(300, 3_000))
        .unwrap();
    assert_eq!(name(&reg, "A").as_deref(), Some("proj-a"));
    assert_eq!(name(&reg, "B").as_deref(), Some("proj-a-2"));
    assert_eq!(name(&reg, "C").as_deref(), Some("proj-a-3"));

    // A fork that comes without a name in the payload takes it from its
    // transcript (the hook's fill): numbered the same way.
    let t = titled("proj-a");
    let path = t.path().to_string_lossy().into_owned();
    reg.apply(&start("D", None, Some(&path)), &ctx(400, 4_000))
        .unwrap();
    let d = slot(&reg, "D");
    reg.take_name(&d.slot_id, "proj-a").unwrap();
    assert_eq!(name(&reg, "D").as_deref(), Some("proj-a-4"));
    assert_eq!(name(&reg, "A").as_deref(), Some("proj-a"));
}

#[test]
fn registry_backfill_keeps_unique_name() {
    let mut reg = Registry::open_in_memory().unwrap();
    reg.apply(&start("A", Some("proj-a"), None), &ctx(100, 1_000))
        .unwrap();
    // The fork: Claude Code writes the parent's title into the fork's
    // transcript under the fork's own id, again and again.
    let mut t = titled("proj-a");
    let path = t.path().to_string_lossy().into_owned();
    reg.apply(&start("B", Some("proj-a"), Some(&path)), &ctx(200, 2_000))
        .unwrap();
    assert_eq!(name(&reg, "B").as_deref(), Some("proj-a-2"));
    let mut scanned = std::collections::HashMap::new();
    assert!(reg.backfill_names(&mut scanned).unwrap().is_empty());
    writeln!(t, r#"{{"type":"custom-title","customTitle":"proj-a"}}"#).unwrap();
    assert!(reg.backfill_names(&mut scanned).unwrap().is_empty());
    assert_eq!(name(&reg, "B").as_deref(), Some("proj-a-2"));
    // A prompt carrying the inherited title does not revert it either.
    reg.apply(
        &Event::Prompt {
            session_id: "B".into(),
            title: Some("proj-a".into()),
            prompt: Some("go on".into()),
        },
        &ctx(200, 3_000),
    )
    .unwrap();
    assert_eq!(name(&reg, "B").as_deref(), Some("proj-a-2"));
    assert_eq!(name(&reg, "A").as_deref(), Some("proj-a"));

    // A slot with no name yet that backfills the taken name: numbered too.
    let mut u = tempfile::NamedTempFile::new().unwrap();
    writeln!(u, r#"{{"type":"user","message":"hi"}}"#).unwrap();
    let upath = u.path().to_string_lossy().into_owned();
    reg.apply(&start("C", None, Some(&upath)), &ctx(300, 4_000))
        .unwrap();
    assert_eq!(name(&reg, "C"), None);
    assert!(reg.backfill_names(&mut scanned).unwrap().is_empty());
    writeln!(u, r#"{{"type":"custom-title","customTitle":"proj-a"}}"#).unwrap();
    let renamed = reg.backfill_names(&mut scanned).unwrap();
    assert_eq!(renamed, vec![slot(&reg, "C").slot_id]);
    assert_eq!(name(&reg, "C").as_deref(), Some("proj-a-3"));

    // A real rename to another name still goes through.
    writeln!(t, r#"{{"type":"custom-title","customTitle":"proj-b"}}"#).unwrap();
    assert_eq!(reg.backfill_names(&mut scanned).unwrap().len(), 1);
    assert_eq!(name(&reg, "B").as_deref(), Some("proj-b"));
}

#[test]
fn registry_unique_name_ignores_closed_and_self() {
    let mut reg = Registry::open_in_memory().unwrap();
    // A closed `proj-a` and an archived `proj-b` hold no name.
    reg.apply(&start("A", Some("proj-a"), None), &ctx(100, 1_000))
        .unwrap();
    reg.apply(
        &Event::End {
            session_id: "A".into(),
            reason: EndReason::parse("prompt_input_exit"),
        },
        &ctx(100, 1_500),
    )
    .unwrap();
    reg.apply(&start("B", Some("proj-b"), None), &ctx(200, 2_000))
        .unwrap();
    reg.apply(
        &Event::End {
            session_id: "B".into(),
            reason: EndReason::parse("other"),
        },
        &ctx(200, 2_500),
    )
    .unwrap();
    let b = slot(&reg, "B").slot_id;
    assert_eq!(
        reg.archive(&[b], "you", &|_| false, 2_600).unwrap().len(),
        1
    );
    reg.apply(&start("C", Some("proj-a"), None), &ctx(300, 3_000))
        .unwrap();
    reg.apply(&start("D", Some("proj-b"), None), &ctx(400, 4_000))
        .unwrap();
    assert_eq!(name(&reg, "C").as_deref(), Some("proj-a"));
    assert_eq!(name(&reg, "D").as_deref(), Some("proj-b"));

    // The same id registered again (a resume) keeps its own name.
    reg.apply(
        &Event::Start {
            source: StartSource::Resume,
            session_id: "C".into(),
            cwd: "/work".into(),
            title: Some("proj-a".into()),
            transcript_path: None,
        },
        &ctx(500, 5_000),
    )
    .unwrap();
    assert_eq!(name(&reg, "C").as_deref(), Some("proj-a"));
    // A second live process on the same conversation gets its own slot,
    // and the same conversation's name is not a clash with itself.
    reg.apply(
        &Event::Start {
            source: StartSource::Resume,
            session_id: "C".into(),
            cwd: "/work".into(),
            title: Some("proj-a".into()),
            transcript_path: None,
        },
        &ctx(600, 6_000),
    )
    .unwrap();
    let named_c: Vec<_> = reg
        .slots()
        .unwrap()
        .into_iter()
        .filter(|s| s.session_id == "C")
        .map(|s| s.name)
        .collect();
    assert!(
        named_c.iter().all(|n| n.as_deref() == Some("proj-a")),
        "{named_c:?}"
    );
}
