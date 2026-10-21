//! 只使用临时状态、配置和会话历史，不调用真实API。
mod common;

use common::World;
use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;
use std::io::Write;

fn setup() -> (World, std::path::PathBuf) {
    let w = World::new("api-failure");
    std::fs::create_dir_all(w.state()).unwrap();
    let path = w.path("transcript.jsonl");
    std::fs::write(&path, b"").unwrap();
    let mut reg = Registry::open(&w.state().join("registry.db")).unwrap();
    reg.apply(
        &Event::Start {
            source: StartSource::Startup,
            session_id: "api-test".into(),
            cwd: w.path("work").to_str().unwrap().into(),
            title: Some("api-test".into()),
            transcript_path: Some(path.to_str().unwrap().into()),
        },
        &Context {
            pid: None,
            pid_start: None,
            tmux: None,
            launch_args: None,
            now_ms: moss_core::slot::now_ms(),
        },
    )
    .unwrap();
    (w, path)
}
fn append(path: &std::path::Path, line: serde_json::Value) {
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{line}"
    )
    .unwrap();
}
fn listing(w: &World) -> serde_json::Value {
    let out = w
        .cc_moss()
        .env("CC_MOSS_CLAUDE_BIN", "/nonexistent/claude")
        .args(["ls", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn api_failure_cli_detects_without_provider_baseline_and_recovers() {
    let (w, path) = setup();
    append(
        &path,
        serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 400 unknown provider for model SECRET"}}),
    );
    let value = listing(&w);
    assert_eq!(
        value["sessions"][0]["api_failure"]["kind"],
        "model_unavailable"
    );
    assert!(!value.to_string().contains("SECRET"));
    append(
        &path,
        serde_json::json!({"type":"user","message":{"content":"retry"}}),
    );
    assert!(listing(&w)["sessions"][0]["api_failure"].is_object());
    append(
        &path,
        serde_json::json!({"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}}),
    );
    assert!(listing(&w)["sessions"][0]["api_failure"].is_null());
}

#[test]
fn api_failure_cli_replacement_partial_and_cache_loss_are_safe() {
    let (w, path) = setup();
    append(
        &path,
        serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 429 rate limited"}}),
    );
    listing(&w);
    let reg = Registry::open(&w.state().join("registry.db")).unwrap();
    assert_eq!(reg.api_failures().unwrap().len(), 1);
    std::fs::write(w.state().join("request-scan.json"), b"broken cache").unwrap();
    listing(&w);
    assert_eq!(
        reg.events(Some(0), 100)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "api-failed")
            .count(),
        1
    );
    std::fs::write(
        &path,
        r#"{"type":"assistant","message":{"model":"claude-good","stop_reason":"end_turn"}}"#,
    )
    .unwrap();
    listing(&w);
    assert_eq!(reg.api_failures().unwrap().len(), 1);
    writeln!(std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap())
    .unwrap();
    listing(&w);
    assert!(reg.api_failures().unwrap().is_empty());
    // 同尺寸、不同inode的替换必须重新读取。
    let replacement = w.path("replacement.jsonl");
    append(
        &path,
        serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 500 failed"}}),
    );
    listing(&w);
    let size = std::fs::metadata(&path).unwrap().len() as usize;
    let success = "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-good\",\"stop_reason\":\"end_turn\"}}\n";
    std::fs::write(
        &replacement,
        format!("{}{}", " ".repeat(size - success.len()), success),
    )
    .unwrap();
    std::fs::rename(replacement, &path).unwrap();
    listing(&w);
    assert!(reg.api_failures().unwrap().is_empty());
}

#[test]
fn api_failure_daemon_detects_without_listing_or_user_messages() {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (w, path) = setup();
    let _daemon = Child(
        w.cc_moss()
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    append(
        &path,
        serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 400 unknown provider for model"}}),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let reg = Registry::open(&w.state().join("registry.db")).unwrap();
        if !reg.api_failures().unwrap().is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "daemon未自动发现API错误"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn api_failure_unchanged_and_user_append_do_not_touch_registry_stamp() {
    let (w, path) = setup();
    append(
        &path,
        serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 500 failed"}}),
    );
    listing(&w);
    let stamp = w.state().join("registry.changed");
    let before = std::fs::metadata(&stamp).unwrap().modified().unwrap();
    listing(&w);
    assert_eq!(
        std::fs::metadata(&stamp).unwrap().modified().unwrap(),
        before
    );
    append(
        &path,
        serde_json::json!({"type":"user","message":{"content":"retry"}}),
    );
    listing(&w);
    assert_eq!(
        std::fs::metadata(&stamp).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn api_failure_truncate_regrow_beyond_old_size_rechecks_prefix() {
    let (w, path) = setup();
    append(
        &path,
        serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 500 failed"}}),
    );
    listing(&w);
    let old_size = std::fs::metadata(&path).unwrap().len() as usize;
    let success="{\"type\":\"assistant\",\"message\":{\"model\":\"claude-good\",\"stop_reason\":\"end_turn\"}}\n";
    std::fs::write(&path, format!("{success}{}\n", " ".repeat(old_size + 4096))).unwrap();
    listing(&w);
    let reg = Registry::open(&w.state().join("registry.db")).unwrap();
    assert!(reg.api_failures().unwrap().is_empty());
}

#[test]
fn api_failure_cache_save_failure_does_not_break_listing_or_duplicate_events() {
    let (w, path) = setup();
    std::fs::create_dir(w.state().join("request-scan.json.tmp")).unwrap();
    append(
        &path,
        serde_json::json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"API Error: 500 failed"}}),
    );
    listing(&w);
    listing(&w);
    let reg = Registry::open(&w.state().join("registry.db")).unwrap();
    assert_eq!(
        reg.events(Some(0), 100)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "api-failed")
            .count(),
        1
    );
}
