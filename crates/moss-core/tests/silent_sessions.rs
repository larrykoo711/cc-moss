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
    let quiet = transcript(&[r#"{"type":"system","subtype":"turn_duration"}"#]);
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

/// Each way a quiet session is still kept, and the session id it uses.
fn kept_cases(reg: &mut Registry, dir: &std::path::Path) -> Vec<(&'static str, String)> {
    let quiet = |name: &str| {
        let p = dir.join(format!("{name}.jsonl"));
        std::fs::write(&p, "").unwrap();
        p.to_string_lossy().into_owned()
    };
    let mut cases = Vec::new();
    let mut pid = 100;
    let mut add = |reg: &mut Registry, case: &'static str, event: Event| {
        pid += 1;
        reg.apply(&event, &ctx(pid, 1_000)).unwrap();
        cases.push((case, event.session_id().to_string()));
    };
    add(reg, "moss", start("moss", &quiet("moss")));
    add(reg, "returning", start("returning", &quiet("returning")));
    add(reg, "archived", start("archived", &quiet("archived")));
    let mut named = start("named", &quiet("named"));
    if let Event::Start { title, .. } = &mut named {
        *title = Some("proj-a".into());
    }
    add(reg, "named", named);
    let mut no_path = start("no-path", "");
    if let Event::Start {
        transcript_path, ..
    } = &mut no_path
    {
        *transcript_path = None;
    }
    add(reg, "no transcript on record", no_path);
    // A folder where the transcript should be: it can't be read.
    let unreadable = dir.join("unreadable");
    std::fs::create_dir(&unreadable).unwrap();
    add(
        reg,
        "unreadable transcript",
        start("unreadable", &unreadable.to_string_lossy()),
    );
    let slash = dir.join("slash.jsonl");
    std::fs::write(
        &slash,
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/model</command-name>"}}"#,
    )
    .unwrap();
    add(
        reg,
        "typed a slash command",
        start("slash", &slash.to_string_lossy()),
    );
    let bang = dir.join("bang.jsonl");
    std::fs::write(
        &bang,
        r#"{"type":"user","message":{"role":"user","content":"<bash-input>ls</bash-input>"}}"#,
    )
    .unwrap();
    add(
        reg,
        "typed a shell command",
        start("bang", &bang.to_string_lossy()),
    );
    let id = |sid: &str| slot(reg, sid).slot_id;
    reg.set_role(&id("moss"), moss_core::moss::ROLE).unwrap();
    reg.mark_restore_started(&id("returning"), 1_500).unwrap();
    let archived = id("archived");
    assert_eq!(
        reg.archive(&[archived], "you", &|_| false, 1_500)
            .unwrap()
            .len(),
        1
    );
    cases
}

fn pid_of(reg: &Registry, sid: &str) -> Option<u32> {
    slot(reg, sid).pid
}

#[test]
fn silent_exclusions_on_session_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut reg = Registry::open_in_memory().unwrap();
    for (case, sid) in kept_cases(&mut reg, dir.path()) {
        let pid = pid_of(&reg, &sid).unwrap();
        reg.apply(&end_other(&sid), &ctx(pid, 2_000)).unwrap();
        assert_eq!(slot(&reg, &sid).status, Status::Interrupted, "{case}");
    }
}

#[test]
fn silent_exclusions_on_process_gone() {
    let dir = tempfile::tempdir().unwrap();
    let mut reg = Registry::open_in_memory().unwrap();
    for (case, sid) in kept_cases(&mut reg, dir.path()) {
        let s = slot(&reg, &sid);
        reg.record_lost_of(&s.slot_id, s.pid, 2_000).unwrap();
        assert_ne!(slot(&reg, &sid).status, Status::Closed, "{case}");
    }
    // And a quiet one with none of these is closed by the same path.
    let quiet = dir.path().join("quiet.jsonl");
    std::fs::write(&quiet, "").unwrap();
    reg.apply(&start("quiet", &quiet.to_string_lossy()), &ctx(900, 3_000))
        .unwrap();
    let s = slot(&reg, "quiet");
    reg.record_lost_of(&s.slot_id, s.pid, 4_000).unwrap();
    assert_eq!(slot(&reg, "quiet").status, Status::Closed);
}
