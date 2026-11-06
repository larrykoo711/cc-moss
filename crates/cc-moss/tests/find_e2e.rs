//! `cc-moss find` (F44) against a fake Claude Code in an isolated tmux and
//! state: what was typed or replied is found, tool output isn't, keys are
//! redacted, and a session that went takes its words with it.

mod common;

use std::io::Write;

use common::World;
use serde_json::Value;

fn find(w: &World, words: &[&str]) -> Value {
    let out = w
        .cc_moss()
        .arg("find")
        .arg("--json")
        .args(words)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

fn say(w: &World, sid: &str, line: Value) {
    let path = w.path("fifos").join(format!("{sid}.jsonl"));
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(f, "{line}").unwrap();
}

fn typed(text: &str) -> Value {
    serde_json::json!({"type":"user","timestamp":"2026-11-05T08:00:00Z",
        "message":{"role":"user","content":text}})
}

#[test]
fn finds_sessions_by_what_was_said() {
    let w = World::new("find-said");
    w.new_session("refunds");
    let sid = w.session_id("refunds");
    w.send(&sid, "prompt refunds");
    say(
        &w,
        &sid,
        typed("把退款逻辑改成按订单分摊 with OPENAI_API_KEY=sk-proj-abcdefghijklmnop1234"),
    );
    say(
        &w,
        &sid,
        serde_json::json!({"type":"assistant","timestamp":"2026-11-05T08:01:00Z",
        "message":{"role":"assistant","content":[{"type":"text","text":"Refunds now split per order."}]}}),
    );
    say(
        &w,
        &sid,
        serde_json::json!({"type":"user","timestamp":"2026-11-05T08:02:00Z",
        "message":{"role":"user","content":[{"type":"tool_result","content":"tool-output-only-words"}]}}),
    );

    for q in [
        &["退款"][..],
        &["split", "per", "order"],
        &["REFUNDS", "NOW"],
    ] {
        let v = find(&w, q);
        let m = v["matches"].as_array().unwrap();
        assert_eq!(m.len(), 1, "{q:?}: {v}");
        assert_eq!(m[0]["name"], "refunds", "{v}");
        assert_eq!(m[0]["session_id"], sid.as_str(), "{v}");
        assert_eq!(v["partial"], false, "{v}");
    }
    let v = find(&w, &["退款逻辑"]);
    let snippet = v["matches"][0]["snippet"].as_str().unwrap();
    assert!(
        snippet.contains("sk-***") && !snippet.contains("abcdefghijklmnop"),
        "{snippet}"
    );
    assert!(find(&w, &["tool-output-only"])["matches"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(find(&w, &["abcdefghijklmnop1234"])["matches"]
        .as_array()
        .unwrap()
        .is_empty());

    // The index is the owner's only.
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(w.state().join("search.db"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");

    // A session that went (purged) takes its words with it.
    w.send(&sid, "exit");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let out = w
            .cc_moss()
            .args(["archive", "refunds", "--json"])
            .output()
            .unwrap();
        if out.status.success() {
            break;
        }
        assert!(std::time::Instant::now() < until, "not archived: {out:?}");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let reg = moss_core::registry::Registry::open(&w.state().join("registry.db")).unwrap();
    let slot = reg
        .slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == sid)
        .unwrap();
    // Archived, still found; purged, gone.
    assert_eq!(find(&w, &["退款"])["matches"][0]["archived"], true);
    assert_eq!(reg.purge(&[slot.slot_id]).unwrap().len(), 1);
    assert!(find(&w, &["退款"])["matches"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn plain_output_says_when_nothing_matches() {
    let w = World::new("find-none");
    let out = w
        .cc_moss()
        .args(["find", "nothing-like-this"])
        .env("CC_MOSS_LANG", "en")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "No session mentions that."
    );
}
