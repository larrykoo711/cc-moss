//! A session that never heard a word from you is over when it exits (F62):
//! closed, not left behind as interrupted for `restore` to fail on.

use std::io::Write;

use moss_core::event::{Context, EndReason, Event, StartSource};
use moss_core::registry::Registry;
use moss_core::slot::{Slot, Status};

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

fn start(id: &str, transcript: &str) -> Event {
    Event::Start {
        source: StartSource::Startup,
        session_id: id.into(),
        cwd: "/work".into(),
        title: None,
        transcript_path: Some(transcript.into()),
    }
}

fn end_other(id: &str) -> Event {
    Event::End {
        session_id: id.into(),
        reason: EndReason::parse("other"),
    }
}

fn slot(reg: &Registry, session_id: &str) -> Slot {
    reg.slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == session_id)
        .unwrap()
}

fn transcript(lines: &[&str]) -> tempfile::NamedTempFile {
    let mut t = tempfile::NamedTempFile::new().unwrap();
    for l in lines {
        writeln!(t, "{l}").unwrap();
    }
    t
}

const SAID: &str = r#"{"type":"user","timestamp":"2026-11-09T10:00:00Z","message":{"role":"user","content":"fix the login page"}}"#;

#[test]
fn silent_session_end_closes() {
    let mut reg = Registry::open_in_memory().unwrap();
    let quiet = transcript(&[r#"{"type":"custom-title","customTitle":"proj-a"}"#]);
    reg.apply(
        &start("A", &quiet.path().to_string_lossy()),
        &ctx(100, 1_000),
    )
    .unwrap();
    reg.apply(&end_other("A"), &ctx(100, 2_000)).unwrap();
    assert_eq!(slot(&reg, "A").status, Status::Closed);

    // Said something (the Prompt hook saw it): stopped, can come back.
    let said = transcript(&[SAID]);
    reg.apply(
        &start("B", &said.path().to_string_lossy()),
        &ctx(200, 3_000),
    )
    .unwrap();
    reg.apply(
        &Event::Prompt {
            session_id: "B".into(),
            title: None,
            prompt: Some("fix the login page".into()),
        },
        &ctx(200, 3_500),
    )
    .unwrap();
    reg.apply(&end_other("B"), &ctx(200, 4_000)).unwrap();
    assert_eq!(slot(&reg, "B").status, Status::Interrupted);

    // No prompt hook seen, but the transcript has your words (adopted): kept.
    let adopted = transcript(&[SAID]);
    reg.apply(
        &start("C", &adopted.path().to_string_lossy()),
        &ctx(300, 5_000),
    )
    .unwrap();
    reg.apply(&end_other("C"), &ctx(300, 6_000)).unwrap();
    assert_eq!(slot(&reg, "C").status, Status::Interrupted);

    // A missing transcript counts as nothing said.
    reg.apply(&start("D", "/nonexistent/D.jsonl"), &ctx(400, 7_000))
        .unwrap();
    reg.apply(&end_other("D"), &ctx(400, 8_000)).unwrap();
    assert_eq!(slot(&reg, "D").status, Status::Closed);

    // No "interrupted" news for the silent ones.
    let interrupted: Vec<String> = reg
        .events(Some(0), 1_000)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "interrupted")
        .map(|e| e.slot_id)
        .collect();
    assert_eq!(
        interrupted,
        vec![slot(&reg, "B").slot_id, slot(&reg, "C").slot_id]
    );
}
