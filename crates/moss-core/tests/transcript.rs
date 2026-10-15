use std::io::Write;

use moss_core::transcript::{custom_title_since, last_custom_title, turn_open};

fn file_with(lines: &[String]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
    f
}

fn title(t: &str) -> String {
    serde_json::json!({"type": "custom-title", "customTitle": t, "sessionId": "x"}).to_string()
}

fn filler(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            serde_json::json!({"type": "user", "message": format!("line {i} {}", "x".repeat(200))})
                .to_string()
        })
        .collect()
}

#[test]
fn finds_the_last_rename() {
    let mut lines = vec![title("first")];
    lines.extend(filler(10));
    lines.push(title("em-final"));
    lines.extend(filler(10));
    let f = file_with(&lines);
    assert_eq!(last_custom_title(f.path()).as_deref(), Some("em-final"));
}

#[test]
fn finds_a_title_far_from_the_end() {
    // Bigger than one read chunk on both sides of the title.
    let mut lines = filler(2_000);
    lines.push(title("deep"));
    lines.extend(filler(2_000));
    let f = file_with(&lines);
    assert_eq!(last_custom_title(f.path()).as_deref(), Some("deep"));
}

#[test]
fn none_without_a_rename_or_a_file() {
    let f = file_with(&filler(5));
    assert_eq!(last_custom_title(f.path()), None);
    assert_eq!(
        last_custom_title(std::path::Path::new("/nonexistent/x.jsonl")),
        None
    );
}

#[test]
fn rescans_only_what_was_appended() {
    let mut f = file_with(&filler(50));
    let (none, scanned) = custom_title_since(f.path(), 0);
    assert_eq!(none, None);
    assert_eq!(scanned, f.as_file().metadata().unwrap().len());
    writeln!(f, "{}", title("renamed")).unwrap();
    let (found, _) = custom_title_since(f.path(), scanned);
    assert_eq!(found.as_deref(), Some("renamed"));
}

#[test]
fn a_title_before_the_scanned_mark_is_not_read_again() {
    let f = file_with(&[title("old")]);
    let size = f.as_file().metadata().unwrap().len();
    assert_eq!(custom_title_since(f.path(), size), (None, size));
}

#[test]
fn a_title_just_before_the_mark_in_the_same_chunk_is_skipped() {
    let mut f = file_with(&[filler(1)[0].clone(), title("old")]);
    let mark = f.as_file().metadata().unwrap().len();
    writeln!(f, "{}", filler(1)[0]).unwrap();
    assert_eq!(custom_title_since(f.path(), mark).0, None);
}

fn rec(v: serde_json::Value) -> String {
    v.to_string()
}

fn assistant(stop: &str) -> String {
    rec(
        serde_json::json!({"type": "assistant", "message": {"stop_reason": stop, "content": [{"type": "text", "text": "ok"}]}}),
    )
}

#[test]
fn a_finished_turn_is_not_open() {
    let f = file_with(&[
        assistant("end_turn"),
        rec(serde_json::json!({"type": "system", "subtype": "stop_hook_summary"})),
        rec(serde_json::json!({"type": "system", "subtype": "turn_duration"})),
        rec(serde_json::json!({"type": "ai-title", "aiTitle": "x"})),
    ]);
    assert_eq!(turn_open(f.path()), Some(false));
}

#[test]
fn a_turn_waiting_on_a_tool_is_open() {
    let f = file_with(&[
        rec(serde_json::json!({"type": "system", "subtype": "turn_duration"})),
        rec(serde_json::json!({"type": "user", "message": {"content": "deploy it"}})),
        assistant("tool_use"),
        rec(serde_json::json!({"type": "attachment"})),
    ]);
    assert_eq!(turn_open(f.path()), Some(true));
}

#[test]
fn an_interrupted_turn_is_not_open() {
    let f = file_with(&[
        assistant("tool_use"),
        rec(
            serde_json::json!({"type": "user", "message": {"content": [{"type": "text", "text": "[Request interrupted by user for tool use]"}]}}),
        ),
    ]);
    assert_eq!(turn_open(f.path()), Some(false));
}

#[test]
fn an_empty_or_missing_transcript_says_nothing() {
    let f = file_with(&[rec(serde_json::json!({"type": "mode"}))]);
    assert_eq!(turn_open(f.path()), None);
    assert_eq!(
        turn_open(std::path::Path::new("/nonexistent/x.jsonl")),
        None
    );
}

#[test]
fn local_commands_after_a_turn_do_not_reopen_it() {
    let f = file_with(&[
        rec(serde_json::json!({"type": "system", "subtype": "turn_duration"})),
        rec(
            serde_json::json!({"type": "user", "isMeta": true, "message": {"content": "<local-command-caveat>x"}}),
        ),
        rec(
            serde_json::json!({"type": "user", "message": {"content": "<command-name>/rename</command-name>"}}),
        ),
        rec(
            serde_json::json!({"type": "user", "message": {"content": "<local-command-stdout>ok</local-command-stdout>"}}),
        ),
    ]);
    assert_eq!(turn_open(f.path()), Some(false));
}

#[test]
fn a_background_task_notification_opens_a_turn() {
    let f = file_with(&[
        rec(serde_json::json!({"type": "system", "subtype": "turn_duration"})),
        rec(
            serde_json::json!({"type": "user", "message": {"content": "<task-notification>done</task-notification>"}}),
        ),
    ]);
    assert_eq!(turn_open(f.path()), Some(true));
}

#[test]
fn a_final_answer_ends_the_turn_before_the_summary_lands() {
    let f = file_with(&[assistant("tool_use"), assistant("end_turn")]);
    assert_eq!(turn_open(f.path()), Some(false));
}

#[test]
fn meta_records_are_skipped() {
    let f = file_with(&[
        rec(serde_json::json!({"type": "system", "subtype": "turn_duration"})),
        rec(
            serde_json::json!({"type": "user", "isMeta": true, "message": {"content": "reminder"}}),
        ),
    ]);
    assert_eq!(turn_open(f.path()), Some(false));
}

#[test]
fn a_request_to_the_model_opens_a_turn() {
    let f = file_with(&[
        rec(serde_json::json!({"type": "system", "subtype": "turn_duration"})),
        rec(serde_json::json!({"type": "api-request"})),
    ]);
    assert_eq!(turn_open(f.path()), Some(true));
}
