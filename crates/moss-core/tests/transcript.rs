use std::io::Write;

use moss_core::transcript::{custom_title_since, last_custom_title, last_prompt, turn_open};

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

fn said(text: &str, at: &str) -> String {
    serde_json::json!({"type": "user", "timestamp": at, "message": {"role": "user", "content": text}})
        .to_string()
}

#[test]
fn last_prompt_is_the_last_thing_typed() {
    let f = file_with(&[
        said("first ask", "2026-10-19T08:00:00.000Z"),
        said("fix the login page\nand the footer", "2026-10-19T09:00:00.000Z"),
        // Not prompts: a tool's result, meta, a local command, an interrupt.
        serde_json::json!({"type": "user", "timestamp": "2026-10-19T09:01:00.000Z",
            "message": {"content": [{"type": "tool_result", "content": "ok"}]}})
        .to_string(),
        serde_json::json!({"type": "user", "isMeta": true, "timestamp": "2026-10-19T09:02:00.000Z",
            "message": {"content": "meta"}})
        .to_string(),
        said("<command-name>/rename</command-name>", "2026-10-19T09:03:00.000Z"),
        said("[Request interrupted by user]", "2026-10-19T09:04:00.000Z"),
        serde_json::json!({"type": "user", "isCompactSummary": true, "timestamp": "2026-10-19T09:02:30.000Z",
            "message": {"content": "This session is being continued from a previous conversation"}})
        .to_string(),
        // A background task finished: Claude Code's notice, not your words.
        said(
            "<task-notification>\n<task-id>bv9y8wkxk</task-id>\n</task-notification>",
            "2026-10-19T09:04:30.000Z",
        ),
        r#"{"type":"assistant","timestamp":"2026-10-19T09:05:00.000Z","message":{"content":"done"}}"#
            .to_string(),
    ]);
    let (text, at) = last_prompt(f.path()).unwrap();
    assert_eq!(text, "fix the login page\nand the footer");
    assert_eq!(at, 1_792_400_400_000);
    assert_eq!(
        last_prompt(std::path::Path::new("/nonexistent/x.jsonl")),
        None
    );
}

#[test]
fn finds_the_last_rename() {
    let mut lines = vec![title("first")];
    lines.extend(filler(10));
    lines.push(title("job-final"));
    lines.extend(filler(10));
    let f = file_with(&lines);
    assert_eq!(last_custom_title(f.path()).as_deref(), Some("job-final"));
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

/// One JSON line of about `bytes` bytes, like a pasted image or a large
/// tool result.
fn huge_line(bytes: usize) -> String {
    serde_json::json!({"type": "user", "message": "y".repeat(bytes)}).to_string()
}

#[test]
fn a_huge_line_is_read_in_linear_time() {
    // 12 MB in one line: above the per-line cap, so it is skipped without
    // being collected, and the title behind it is still found.
    let mut lines = vec![title("before-the-paste")];
    lines.extend(filler(3));
    lines.push(huge_line(12 << 20));
    lines.extend(filler(3));
    let f = file_with(&lines);
    let t = std::time::Instant::now();
    assert_eq!(
        last_custom_title(f.path()).as_deref(),
        Some("before-the-paste")
    );
    assert!(
        t.elapsed() < std::time::Duration::from_secs(3),
        "{:?}",
        t.elapsed()
    );
}

#[test]
fn a_line_under_the_cap_spanning_many_chunks_is_still_read() {
    // 3 MB: many read chunks, kept whole and parsed.
    let big = serde_json::json!({
        "type": "custom-title",
        "customTitle": "big",
        "pad": "z".repeat(3 << 20),
    })
    .to_string();
    let mut lines = filler(3);
    lines.push(big);
    lines.extend(filler(3));
    let f = file_with(&lines);
    assert_eq!(last_custom_title(f.path()).as_deref(), Some("big"));
}

#[test]
fn scanning_stops_after_the_budget() {
    // A turn record behind 40 MB of records that say nothing is out of reach: give up
    // rather than read a whole huge file on every picker refresh.
    let mut lines = vec![assistant("end_turn")];
    lines.extend(std::iter::repeat_with(|| huge_line(1 << 20)).take(40));
    let f = file_with(&lines);
    let t = std::time::Instant::now();
    assert_eq!(turn_open(f.path()), None);
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        t.elapsed()
    );
}

#[test]
fn api_failure_last_assistant_model_ignores_untrusted_records() {
    let f = file_with(&[
        r#"{"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}}"#.into(),
        r#"{"type":"assistant","isApiErrorMessage":true,"message":{"model":"claude-error"}}"#
            .into(),
        r#"{"type":"assistant","isSidechain":true,"message":{"model":"claude-side"}}"#.into(),
        r#"{"type":"assistant","isSynthetic":true,"message":{"model":"claude-fake"}}"#.into(),
        r#"{"type":"assistant","message":{"model":"<synthetic>"}}"#.into(),
        r#"{"type":"assistant","message":{"model":"bad\nmodel"}}"#.into(),
    ]);
    assert_eq!(
        moss_core::transcript::last_assistant_model(f.path()).as_deref(),
        Some("claude-good")
    );
}

#[test]
fn api_failure_parser_requires_explicit_error_and_complete_line() {
    use moss_core::transcript::{request_result_since, ApiFailureKind};
    let mut f = file_with(&[r#"{"type":"assistant","isApiErrorMessage":true,"timestamp":"2026-10-21T00:00:00Z","message":{"content":[{"type":"text","text":"API Error: 400 unknown provider for model secret-token"}]}}"#.into()]);
    let (result, end) = request_result_since(f.path(), 0).unwrap();
    let failure = result.unwrap().failure.unwrap();
    assert_eq!(failure.kind, ApiFailureKind::ModelUnavailable);
    assert_eq!(failure.status, Some(400));
    assert!(!serde_json::to_string(&failure)
        .unwrap()
        .contains("secret-token"));
    f.write_all(
        br#"{"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}}"#,
    )
    .unwrap();
    assert_eq!(request_result_since(f.path(), end).unwrap().1, end);
    writeln!(f).unwrap();
    assert!(request_result_since(f.path(), end)
        .unwrap()
        .0
        .unwrap()
        .failure
        .is_none());
}

#[test]
fn api_failure_ignores_quotes_and_non_main_success() {
    use moss_core::transcript::{request_result_since, ApiFailureKind};
    let f = file_with(&[
        r#"{"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 400 bad input"}}"#.into(),
        r#"{"type":"user","message":{"content":"API Error: 400 unknown provider for model"}}"#.into(),
        r#"{"type":"assistant","isSidechain":true,"message":{"model":"claude-good","stop_reason":"end_turn"}}"#.into(),
        r#"{"type":"assistant","message":{"model":"<synthetic>"}}"#.into(),
        r#"{"type":"assistant","message":{"content":"API Error: 500 quoted text"}}"#.into(),
        "not json".into(),
    ]);
    assert_eq!(
        request_result_since(f.path(), 0)
            .unwrap()
            .0
            .unwrap()
            .failure
            .unwrap()
            .kind,
        ApiFailureKind::GenericApi
    );
}

#[test]
fn api_failure_huge_line_is_skipped_and_result_position_is_exact() {
    use moss_core::transcript::request_result_since;
    let good = r#"{"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}}"#
        .to_string();
    let huge=serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"x".repeat(9<<20)}}).to_string();
    let f = file_with(&[good, huge]);
    let result = request_result_since(f.path(), 0).unwrap().0.unwrap();
    assert!(result.failure.is_none());
    assert_eq!(result.position, 0);
}

#[test]
fn api_failure_does_not_read_other_session_results() {
    let f=file_with(&[r#"{"type":"assistant","sessionId":"old","isApiErrorMessage":true,"message":{"content":"API Error: 500 failed"}}"#.into()]);
    assert!(
        moss_core::transcript::request_result_for_session(f.path(), 0, Some("new"))
            .unwrap()
            .0
            .is_none()
    );
}

#[test]
fn api_failure_streaming_model_metadata_does_not_recover() {
    let mut f=file_with(&[
        r#"{"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 500 failed"}}"#.into(),
        r#"{"type":"assistant","message":{"model":"claude-good","stop_reason":null}}"#.into(),
    ]);
    assert!(moss_core::transcript::request_result_since(f.path(), 0)
        .unwrap()
        .0
        .unwrap()
        .failure
        .is_some());
    writeln!(f,"{}",serde_json::json!({"type":"assistant","message":{"model":"claude-good","stop_reason":"tool_use"}})).unwrap();
    assert!(moss_core::transcript::request_result_since(f.path(), 0)
        .unwrap()
        .0
        .unwrap()
        .failure
        .is_none());
}

#[test]
fn model_evidence_completed_response_carries_only_safe_model_and_time() {
    let f=file_with(&[r#"{"type":"assistant","timestamp":"2026-10-21T00:00:00Z","message":{"model":"model+valid","stop_reason":"tool_use","content":"SECRET private text"}}"#.into()]);
    let result = moss_core::transcript::request_result_since(f.path(), 0)
        .unwrap()
        .0
        .unwrap();
    let success = result.success.unwrap();
    assert_eq!(success.model, "model+valid");
    assert!(success.at_ms.is_some());
    assert!(!serde_json::to_string(&success).unwrap().contains("SECRET"));
}
