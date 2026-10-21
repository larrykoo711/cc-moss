use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;
use moss_core::transcript::{request_file_stamp, request_result_since};
use std::io::Write;

fn setup() -> (Registry, tempfile::NamedTempFile, String) {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut registry = Registry::open_in_memory().unwrap();
    registry
        .apply(
            &Event::Start {
                source: StartSource::Startup,
                session_id: "s1".into(),
                cwd: "/work".into(),
                title: None,
                transcript_path: Some(file.path().to_str().unwrap().into()),
            },
            &Context {
                pid: None,
                pid_start: None,
                tmux: None,
                launch_args: None,
                now_ms: 100,
            },
        )
        .unwrap();
    let id = registry.slots().unwrap()[0].slot_id.clone();
    (registry, file, id)
}
fn refresh(reg: &Registry, file: &tempfile::NamedTempFile, id: &str) {
    if let Some(result) = request_result_since(file.path(), 0).unwrap().0 {
        reg.apply_api_result(
            id,
            "s1",
            file.path().to_str().unwrap(),
            "generation",
            &request_file_stamp(file.path()).unwrap(),
            &result,
        )
        .unwrap();
    }
}
fn failure(file: &mut tempfile::NamedTempFile, at: &str) {
    writeln!(file, "{}", serde_json::json!({"type":"assistant","isApiErrorMessage":true,"timestamp":at,"message":{"content":"API Error: 400 unknown provider for model secret"}})).unwrap();
}
#[test]
fn api_failure_survives_retry_until_success() {
    let (reg, mut file, id) = setup();
    failure(&mut file, "2026-10-21T00:00:00Z");
    refresh(&reg, &file, &id);
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"user","message":{"content":"retry"}})
    )
    .unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"system","subtype":"turn_duration"})
    )
    .unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"assistant","isSynthetic":true,"message":{"model":"claude-good","stop_reason":"end_turn"}})
    )
    .unwrap();
    refresh(&reg, &file, &id);
    assert!(reg.api_failures().unwrap().contains_key(&id));
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}})
    )
    .unwrap();
    refresh(&reg, &file, &id);
    assert!(reg.api_failures().unwrap().is_empty());
}
#[test]
fn api_failure_recovers_and_fails_again() {
    let (reg, mut file, id) = setup();
    failure(&mut file, "2026-10-21T00:00:00Z");
    refresh(&reg, &file, &id);
    failure(&mut file, "2026-10-21T00:01:00Z");
    refresh(&reg, &file, &id);
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}})
    )
    .unwrap();
    refresh(&reg, &file, &id);
    failure(&mut file, "2026-10-21T00:02:00Z");
    refresh(&reg, &file, &id);
    let kinds: Vec<_> = reg
        .events(Some(0), 100)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind.starts_with("api-"))
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, ["api-failed", "api-recovered", "api-failed"]);
}
#[test]
fn api_failure_old_observation_cannot_overwrite_success() {
    let (reg, mut file, id) = setup();
    failure(&mut file, "2026-10-21T00:00:00Z");
    let old = request_result_since(file.path(), 0).unwrap().0.unwrap();
    let stamp = request_file_stamp(file.path()).unwrap();
    refresh(&reg, &file, &id);
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}})
    )
    .unwrap();
    refresh(&reg, &file, &id);
    assert!(!reg
        .apply_api_result(
            &id,
            "s1",
            file.path().to_str().unwrap(),
            "generation",
            &stamp,
            &old
        )
        .unwrap());
    assert!(reg.api_failures().unwrap().is_empty());
}

#[test]
fn api_failure_new_session_and_clear_do_not_inherit_failure() {
    let (mut reg, mut file, id) = setup();
    failure(&mut file, "2026-10-21T00:00:00Z");
    refresh(&reg, &file, &id);
    let context = Context {
        pid: None,
        pid_start: None,
        tmux: None,
        launch_args: None,
        now_ms: 200,
    };
    reg.apply(
        &Event::End {
            session_id: "s1".into(),
            reason: moss_core::event::EndReason::Clear,
        },
        &context,
    )
    .unwrap();
    refresh(&reg, &file, &id);
    assert!(reg.api_failures().unwrap().is_empty());
    reg.apply(
        &Event::Start {
            source: StartSource::Clear,
            session_id: "s2".into(),
            cwd: "/work".into(),
            title: None,
            transcript_path: Some(file.path().to_str().unwrap().into()),
        },
        &context,
    )
    .unwrap();
    assert!(reg.api_failures().unwrap().is_empty());
    let result = request_result_since(file.path(), 0).unwrap().0.unwrap();
    reg.apply_api_result(
        &id,
        "s1",
        file.path().to_str().unwrap(),
        "old",
        &request_file_stamp(file.path()).unwrap(),
        &result,
    )
    .unwrap();
    assert!(reg.api_failures().unwrap().is_empty());
}

#[test]
fn api_failure_same_snapshot_reverse_positions_are_monotonic() {
    let (reg, mut file, id) = setup();
    failure(&mut file, "2026-10-21T00:00:00Z");
    let old = request_result_since(file.path(), 0).unwrap().0.unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}})
    )
    .unwrap();
    let new = request_result_since(file.path(), 0).unwrap().0.unwrap();
    let stamp = request_file_stamp(file.path()).unwrap();
    reg.apply_api_result(
        &id,
        "s1",
        file.path().to_str().unwrap(),
        "generation",
        &stamp,
        &new,
    )
    .unwrap();
    reg.apply_api_result(
        &id,
        "s1",
        file.path().to_str().unwrap(),
        "generation",
        &stamp,
        &old,
    )
    .unwrap();
    assert!(reg.api_failures().unwrap().is_empty());
    assert!(!reg
        .events(Some(0), 100)
        .unwrap()
        .iter()
        .any(|e| e.kind == "api-failed"));
}

#[test]
fn api_failure_concurrent_connections_cannot_commit_stale_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transcript.jsonl");
    let database = dir.path().join("registry.db");
    let mut reg = Registry::open(&database).unwrap();
    reg.apply(
        &Event::Start {
            source: StartSource::Startup,
            session_id: "s1".into(),
            cwd: "/work".into(),
            title: None,
            transcript_path: Some(path.to_str().unwrap().into()),
        },
        &Context {
            pid: None,
            pid_start: None,
            tmux: None,
            launch_args: None,
            now_ms: 100,
        },
    )
    .unwrap();
    let id = reg.slots().unwrap()[0].slot_id.clone();
    std::fs::write(&path,"{\"type\":\"assistant\",\"isApiErrorMessage\":true,\"message\":{\"content\":\"API Error: 500 failed\"}}\n").unwrap();
    let old = request_result_since(&path, 0).unwrap().0.unwrap();
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap(),
        "{}",
        serde_json::json!({"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}})
    )
    .unwrap();
    let new = request_result_since(&path, 0).unwrap().0.unwrap();
    let stamp = request_file_stamp(&path).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let worker_id = id.clone();
    let worker_path = path.clone();
    let worker_stamp = stamp.clone();
    let worker = std::thread::spawn(move || {
        let other = Registry::open(&database).unwrap();
        receive.recv().unwrap();
        other
            .apply_api_result(
                &worker_id,
                "s1",
                worker_path.to_str().unwrap(),
                "generation",
                &worker_stamp,
                &old,
            )
            .unwrap();
    });
    reg.apply_api_result(
        &id,
        "s1",
        path.to_str().unwrap(),
        "generation",
        &stamp,
        &new,
    )
    .unwrap();
    send.send(()).unwrap();
    worker.join().unwrap();
    assert!(reg.api_failures().unwrap().is_empty());
}

#[test]
fn api_failure_clear_before_first_scan_blocks_other_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transcript.jsonl");
    let database = dir.path().join("registry.db");
    let mut reg = Registry::open(&database).unwrap();
    let context = Context {
        pid: None,
        pid_start: None,
        tmux: None,
        launch_args: None,
        now_ms: 100,
    };
    reg.apply(
        &Event::Start {
            source: StartSource::Startup,
            session_id: "s1".into(),
            cwd: "/work".into(),
            title: None,
            transcript_path: Some(path.to_str().unwrap().into()),
        },
        &context,
    )
    .unwrap();
    let id = reg.slots().unwrap()[0].slot_id.clone();
    std::fs::write(&path,"{\"type\":\"assistant\",\"isApiErrorMessage\":true,\"message\":{\"content\":\"API Error: 500 failed\"}}\n").unwrap();
    let late = request_result_since(&path, 0).unwrap().0.unwrap();
    let other = Registry::open(&database).unwrap();
    reg.apply(
        &Event::End {
            session_id: "s1".into(),
            reason: moss_core::event::EndReason::Clear,
        },
        &context,
    )
    .unwrap();
    other
        .apply_api_result(
            &id,
            "s1",
            path.to_str().unwrap(),
            "old",
            &request_file_stamp(&path).unwrap(),
            &late,
        )
        .unwrap();
    assert!(reg.api_failures().unwrap().is_empty());
    assert!(!reg
        .events(Some(0), 100)
        .unwrap()
        .iter()
        .any(|e| e.kind == "api-failed"));
}

#[test]
fn model_evidence_migrates_existing_candidate_table_without_losing_failures() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("registry.db");
    drop(Registry::open(&db).unwrap());
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("INSERT INTO api_results(slot_id,session_id,path,generation,stamp,position,failure) VALUES('old-slot','old-session','/unused','generation','stamp',0,'safe-failure'); ALTER TABLE api_results DROP COLUMN model_evidence").unwrap();
    drop(conn);
    let registry = Registry::open(&db).unwrap();
    assert!(registry.model_evidence().unwrap().is_empty());
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert!(conn
        .prepare("SELECT 1 FROM pragma_table_info('api_results') WHERE name='model_evidence'")
        .unwrap()
        .exists([])
        .unwrap());
    assert_eq!(
        conn.query_row(
            "SELECT failure FROM api_results WHERE slot_id='old-slot'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "safe-failure"
    );
}

#[test]
fn model_evidence_clear_new_session_and_failure_revoke_success() {
    let (mut reg, mut file, id) = setup();
    writeln!(file,"{}",serde_json::json!({"type":"assistant","timestamp":"2026-10-21T00:00:00Z","message":{"model":"claude-good","stop_reason":"end_turn"}})).unwrap();
    let mut result = request_result_since(file.path(), 0).unwrap().0.unwrap();
    let at_ms = result.success.as_ref().unwrap().at_ms.unwrap();
    result.evidence = Some(moss_core::transcript::ModelEvidence {
        session_id: "s1".into(),
        path: file.path().to_str().unwrap().into(),
        generation: "generation".into(),
        epoch: "epoch".into(),
        fence_ms: at_ms - 1,
        position: result.position,
        model: "claude-good".into(),
        at_ms,
    });
    reg.apply_api_result(
        &id,
        "s1",
        file.path().to_str().unwrap(),
        "generation",
        &request_file_stamp(file.path()).unwrap(),
        &result,
    )
    .unwrap();
    assert_eq!(reg.model_evidence().unwrap().len(), 1);
    failure(&mut file, "2026-10-21T00:01:00Z");
    refresh(&reg, &file, &id);
    assert!(reg.model_evidence().unwrap().is_empty());
    reg.apply(
        &Event::End {
            session_id: "s1".into(),
            reason: moss_core::event::EndReason::Clear,
        },
        &Context {
            pid: None,
            pid_start: None,
            tmux: None,
            launch_args: None,
            now_ms: 200,
        },
    )
    .unwrap();
    assert!(reg.model_evidence().unwrap().is_empty());
}
