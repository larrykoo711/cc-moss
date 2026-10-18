//! Answering a session's question from outside (T1.3): the hooks against a
//! throwaway state dir, and end to end with a fake `claude` on an isolated
//! tmux server.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::{bin, wait_for, World};
use serde_json::{json, Value};

const QUESTION: &str = "Pick one?";

fn tool_input() -> Value {
    json!({ "questions": [{ "question": QUESTION, "header": "Pick",
        "options": [{ "label": "A", "description": "first" }, { "label": "B", "description": "second" }],
        "multiSelect": false }] })
}

fn permission_request(session: &str) -> String {
    json!({ "session_id": session, "cwd": "/work", "prompt_id": "p1",
        "hook_event_name": "PermissionRequest", "permission_mode": "bypassPermissions",
        "tool_name": "AskUserQuestion", "tool_input": tool_input() })
    .to_string()
}

/// A state dir and a config file of its own.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new(config: &str) -> Self {
        warm_up();
        let env = Env {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::write(env.config(), config).unwrap();
        env
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    fn cmd(&self) -> Command {
        let mut cmd = bin();
        cmd.env("CC_MOSS_STATE_DIR", self.state())
            .env("CC_MOSS_CONFIG", self.config());
        cmd
    }

    fn cc_moss(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().unwrap()
    }

    fn take(&self) {
        let out = self.cc_moss(&["ask", "take", "--name", "test"]);
        assert!(out.status.success(), "{out:?}");
    }

    fn spawn_hook(&self, event: &str, payload: &str) -> Child {
        let mut child = self
            .cmd()
            .args(["hook", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
        child
    }

    /// Run a hook to the end: its output and how long it took.
    fn hook(&self, event: &str, payload: &str) -> (Output, Duration) {
        let t = Instant::now();
        let out = self.spawn_hook(event, payload).wait_with_output().unwrap();
        (out, t.elapsed())
    }

    fn asks(&self) -> Vec<Value> {
        asks_of(self.cc_moss(&["ask", "list", "--json"]))
    }

    fn closed(&self, id: &str) -> Option<Value> {
        let text = std::fs::read(self.state().join(format!("asks/{id}.closed.json"))).ok()?;
        serde_json::from_slice(&text).ok()
    }
}

fn asks_of(out: Output) -> Vec<Value> {
    assert!(out.status.success(), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    v["asks"].as_array().unwrap().clone()
}

/// Silent, successful, and (with `under`) quick.
fn assert_quiet(out: &Output, took: Duration, under: Option<Duration>) {
    assert!(out.status.success(), "{out:?}");
    assert!(
        out.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    if let Some(limit) = under {
        assert!(took < limit, "took {took:?}, over {limit:?}");
    }
}

/// macOS checks a freshly built binary on its first launch, which can take
/// most of a second; the timings are about the hook, so launch it once first.
fn warm_up() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = bin().arg("--version").output();
    });
}

const FAST: Duration = Duration::from_millis(100);
/// Long enough that a hook which waits when it should not is caught by
/// [`FAST`], short enough not to stall the suite if it does.
const SHORT_WAIT: &str = "[ask]\nwait_secs = 3\n";

#[test]
fn disabled_no_wait() {
    let env = Env::new("[ask]\nenabled = false\nwait_secs = 3\n");
    env.take();
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    assert_quiet(&out, took, Some(FAST));
    assert!(env.asks().is_empty(), "nothing written");
}

#[test]
fn no_taker_no_wait() {
    let env = Env::new(SHORT_WAIT);
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    assert_quiet(&out, took, Some(FAST));
    assert!(env.asks().is_empty());
    // A heartbeat older than 90 seconds does not count.
    env.take();
    let alive = env.state().join("asks/takers/test.alive");
    let old = std::time::SystemTime::now() - Duration::from_secs(120);
    std::fs::File::options()
        .write(true)
        .open(&alive)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    assert_quiet(&out, took, Some(FAST));
}

#[test]
fn broken_state_fails_open() {
    // Broken JSON, from a session with a taker.
    let env = Env::new(SHORT_WAIT);
    env.take();
    let (out, took) = env.hook("permission-request", "{not json");
    assert_quiet(&out, took, Some(FAST));

    // Questions cannot be written.
    use std::os::unix::fs::PermissionsExt;
    let asks = env.state().join("asks");
    std::fs::set_permissions(&asks, std::fs::Permissions::from_mode(0o500)).unwrap();
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    std::fs::set_permissions(&asks, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_quiet(&out, took, Some(FAST));

    // The config does not parse.
    std::fs::write(env.config(), "[ask\n").unwrap();
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    assert_quiet(&out, took, Some(FAST));

    // The state dir is a file.
    let file = Env::new(SHORT_WAIT);
    std::fs::write(file.state(), "").unwrap();
    let (out, took) = file.hook("permission-request", &permission_request("S"));
    assert_quiet(&out, took, Some(FAST));
    let (out, took) = file.hook("pre-tool-use", &permission_request("S"));
    assert_quiet(&out, took, Some(FAST));

    // A panic inside the hook.
    let mut cmd = env.cmd();
    cmd.env("CC_MOSS_HOOK_PANIC", "1");
    let out = cmd
        .args(["hook", "permission-request"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_quiet(&out, Duration::ZERO, None);
}

#[test]
fn times_out_quietly() {
    let env = Env::new("[ask]\nwait_secs = 1\n");
    env.take();
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    assert_quiet(&out, took, None);
    assert!(took >= Duration::from_secs(1), "waited {took:?}");
    assert!(took < Duration::from_secs(3), "waited {took:?}");
    let all: Vec<_> = std::fs::read_dir(env.state().join("asks"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".closed.json"))
        .collect();
    assert_eq!(all.len(), 1, "{all:?}");
    let id = all[0].trim_end_matches(".closed.json");
    assert_eq!(env.closed(id).unwrap()["reason"], "timeout");
    assert!(env.asks().is_empty());
}

#[test]
fn withdrawn_quietly() {
    let env = Env::new("[ask]\nwait_secs = 30\n");
    env.take();
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    // Another session moving on leaves this question alone.
    let (out, _) = env.hook("stop", &json!({ "session_id": "T" }).to_string());
    assert!(out.status.success());
    assert_eq!(env.asks().len(), 1);

    let t = Instant::now();
    let (out, _) = env.hook("stop", &json!({ "session_id": "S" }).to_string());
    assert!(out.status.success());
    let out = child.wait_with_output().unwrap();
    assert_quiet(&out, t.elapsed(), Some(Duration::from_secs(1)));
    let id = ask["id"].as_str().unwrap();
    assert_eq!(env.closed(id).unwrap()["reason"], "answered-in-session");
}

#[test]
fn first_answer_is_kept() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap();
    let first = env.cc_moss(&["ask", "answer", id, "--choice", "A"]);
    assert!(first.status.success(), "{first:?}");
    let second = env.cc_moss(&["ask", "answer", id, "--choice", "B"]);
    assert!(!second.status.success());
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(err.contains("already answered"), "{err}");
    let out = child.wait_with_output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["hookSpecificOutput"]["decision"]["updatedInput"]["answers"][QUESTION],
        "A"
    );
}

#[test]
fn permission_is_answered_with_allow_or_deny() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    let bash = json!({ "session_id": "S", "cwd": "/work", "prompt_id": "p1",
        "hook_event_name": "PermissionRequest", "tool_name": "Bash",
        "tool_input": { "command": "touch probe.txt", "description": "Create a file" } })
    .to_string();
    let child = env.spawn_hook("permission-request", &bash);
    let ask = wait_for(|| env.asks().into_iter().next());
    assert_eq!(ask["kind"], "permission");
    assert_eq!(
        ask["tool"],
        json!({ "name": "Bash", "preview": "touch probe.txt" })
    );
    let id = ask["id"].as_str().unwrap();
    let wrong = env.cc_moss(&["ask", "answer", id, "--choice", "A"]);
    assert!(!wrong.status.success());
    let out = env.cc_moss(&["ask", "answer", id, "--deny", "--message", "not today"]);
    assert!(out.status.success(), "{out:?}");
    let out = child.wait_with_output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v,
        json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest",
            "decision": { "behavior": "deny", "message": "not today" } } })
    );
    // The same tool finishing in the terminal withdraws a later request.
    let child = env.spawn_hook("permission-request", &bash);
    wait_for(|| env.asks().into_iter().next());
    let done = json!({ "session_id": "S", "hook_event_name": "PostToolUse", "tool_name": "Bash",
        "tool_input": { "command": "touch probe.txt" } })
    .to_string();
    env.hook("post-tool-use", &done);
    let out = child.wait_with_output().unwrap();
    assert!(out.stdout.is_empty());
}

#[test]
fn pre_tool_use_only_takes_note() {
    let env = Env::new(SHORT_WAIT);
    let pre = json!({ "session_id": "S", "cwd": "/work", "prompt_id": "p1",
        "hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion",
        "tool_input": tool_input(), "tool_use_id": "toolu_1" })
    .to_string();
    let (out, took) = env.hook("pre-tool-use", &pre);
    assert_quiet(&out, took, Some(FAST));
    assert!(env.asks().is_empty(), "no taker, nothing written");
    env.take();
    let (out, took) = env.hook("pre-tool-use", &pre);
    assert_quiet(&out, took, Some(FAST));
    let asks = env.asks();
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0]["tool_use_id"], "toolu_1");
    assert_eq!(asks[0]["questions"], tool_input()["questions"]);
}

fn world_with_taker(test: &str) -> World {
    let w = World::new(test);
    let out = w
        .cc_moss()
        .args(["ask", "take", "--name", "test"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    w
}

fn world_asks(w: &World) -> Vec<Value> {
    asks_of(
        w.cc_moss()
            .args(["ask", "list", "--json"])
            .output()
            .unwrap(),
    )
}

fn ask_files(w: &World, sid: &str) -> (PathBuf, PathBuf) {
    let base = w.path("fifos");
    (
        base.join(format!("{sid}.ask.out")),
        base.join(format!("{sid}.ask.done")),
    )
}

fn exists(p: &Path) -> Option<()> {
    p.exists().then_some(())
}

#[test]
fn answer_reaches_session() {
    let w = world_with_taker("ask-answer");
    w.new_session("job-a");
    let sid = w.session_id("job-a");
    w.send(&sid, "ask");
    let ask = wait_for(|| world_asks(&w).into_iter().next());
    assert_eq!(ask["kind"], "question");
    assert_eq!(ask["session_id"], sid.as_str());
    assert_eq!(ask["session_name"], "job-a");
    // PermissionRequest took over the question PreToolUse wrote.
    assert_eq!(ask["tool_use_id"], "toolu_fake");
    assert_eq!(world_asks(&w).len(), 1);

    let text = w.cc_moss().args(["ask", "list"]).output().unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(
        text.contains("job-a") && text.contains(QUESTION) && text.contains("A / B"),
        "{text}"
    );

    let id = ask["id"].as_str().unwrap();
    let out = w
        .cc_moss()
        .args(["ask", "answer", &id[..6], "--choice", "B"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let (out, done) = ask_files(&w, &sid);
    wait_for(|| exists(&done));
    let v: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(
        v,
        json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest",
            "decision": { "behavior": "allow", "updatedInput": {
                "questions": tool_input()["questions"], "answers": { QUESTION: "B" } } } } })
    );
    assert!(world_asks(&w).is_empty());
    let closed = std::fs::read_to_string(w.state().join(format!("asks/{id}.closed.json"))).unwrap();
    assert_eq!(closed, r#"{"reason":"answered"}"#);
}

#[test]
fn answered_in_session_closes_ask() {
    let w = world_with_taker("ask-terminal");
    w.new_session("job-b");
    let sid = w.session_id("job-b");
    w.send(&sid, "ask");
    let ask = wait_for(|| world_asks(&w).into_iter().next());
    let (out, done) = ask_files(&w, &sid);
    assert!(!done.exists(), "the hook is waiting");

    let t = Instant::now();
    w.send(&sid, "answered");
    wait_for(|| exists(&done));
    let took = t.elapsed();
    assert!(
        took < Duration::from_secs(1),
        "the hook took {took:?} to let go"
    );
    assert!(
        std::fs::read(out).unwrap().is_empty(),
        "no answer handed over"
    );
    assert!(world_asks(&w).is_empty());
    let id = ask["id"].as_str().unwrap();
    let late = w
        .cc_moss()
        .args(["ask", "answer", id, "--choice", "A"])
        .output()
        .unwrap();
    assert!(!late.status.success());
    assert!(String::from_utf8_lossy(&late.stderr).contains("no longer needs an answer"));
}
