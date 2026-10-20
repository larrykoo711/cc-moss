//! Answering a session's question from outside (T1.3): the hooks against a
//! throwaway state dir, and end to end with a fake `claude` on an isolated
//! tmux server.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::{bin, start_json, wait_for, World};
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

/// A hook that doesn't wait. Well under [`SHORT_WAIT`]'s 3 s, so one that
/// waits is still caught, with room for a busy CI machine: a hook there
/// once took 811 ms to start and exit, another 1014 ms, and 130 ms is
/// common in a Linux container under load.
const FAST: Duration = Duration::from_millis(2000);
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
    assert_quiet(&out, t.elapsed(), Some(FAST));
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
    assert_eq!(ask["tool"]["name"], "Bash");
    assert_eq!(ask["tool"]["preview"], "touch probe.txt");
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

fn pre_tool_use(session: &str) -> String {
    json!({ "session_id": session, "cwd": "/work", "prompt_id": "p1",
        "hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion",
        "tool_input": tool_input(), "tool_use_id": "toolu_1" })
    .to_string()
}

fn bash_request(session: &str, command: &str) -> String {
    json!({ "session_id": session, "cwd": "/work", "prompt_id": "p1",
        "hook_event_name": "PermissionRequest", "tool_name": "Bash",
        "tool_input": { "command": command, "description": "Run it" } })
    .to_string()
}

fn bash_done(session: &str, command: &str) -> String {
    json!({ "session_id": session, "hook_event_name": "PostToolUse", "tool_name": "Bash",
        "tool_input": { "description": "Run it", "command": command },
        "tool_response": { "stdout": "" } })
    .to_string()
}

fn standing_files(env: &Env) -> Vec<String> {
    std::fs::read_dir(env.state().join("asks"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .filter(|n| {
            n.strip_suffix(".json")
                .is_some_and(|id| id.bytes().all(|b| b.is_ascii_alphanumeric()))
        })
        .collect()
}

#[test]
fn answer_before_permission_request_reaches_session() {
    // A fast taker answers what PreToolUse noted before PermissionRequest
    // has started: the answer must still reach the session.
    let env = Env::new(SHORT_WAIT);
    env.take();
    let (out, _) = env.hook("pre-tool-use", &pre_tool_use("S"));
    assert!(out.status.success());
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap().to_string();
    let answering = env
        .cmd()
        .args(["ask", "answer", &id, "--choice", "B"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for(|| exists(&env.state().join(format!("asks/{id}.answer.json"))));
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    assert!(took < Duration::from_secs(1), "waited {took:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["hookSpecificOutput"]["decision"]["updatedInput"]["answers"][QUESTION],
        "B"
    );
    let cli = answering.wait_with_output().unwrap();
    assert!(cli.status.success(), "{cli:?}");
    assert!(
        String::from_utf8_lossy(&cli.stdout).contains("Answered."),
        "{cli:?}"
    );
}

#[test]
fn answer_tells_when_no_session_is_waiting() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    env.hook("pre-tool-use", &pre_tool_use("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let out = env.cc_moss(&[
        "ask",
        "answer",
        ask["id"].as_str().unwrap(),
        "--choice",
        "A",
    ]);
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("isn't waiting on it right now"),
        "{out:?}"
    );
}

#[test]
fn a_late_answer_is_reported_as_too_late() {
    // Open long enough to be seen by a busy runner's polling.
    let env = Env::new(SHORT_WAIT);
    env.take();
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap().to_string();
    // The hook times out between the answer and its next look.
    let out = child.wait_with_output().unwrap();
    assert!(out.stdout.is_empty());
    let late = env.cc_moss(&["ask", "answer", &id, "--choice", "A"]);
    assert!(!late.status.success());
    assert!(
        String::from_utf8_lossy(&late.stderr).contains("no longer needs an answer"),
        "{late:?}"
    );
}

#[test]
fn a_tool_finishing_withdraws_only_its_own_request() {
    // Two Bash calls of one session wait at once (parallel tool calls or a
    // subagent); one finishing must not withdraw the other.
    let env = Env::new("[ask]\nwait_secs = 30\n");
    env.take();
    let ls = env.spawn_hook("permission-request", &bash_request("S", "ls"));
    let rm = env.spawn_hook("permission-request", &bash_request("S", "rm -rf build"));
    wait_for(|| (env.asks().len() == 2).then_some(()));
    let t = Instant::now();
    env.hook("post-tool-use", &bash_done("S", "ls"));
    let out = ls.wait_with_output().unwrap();
    assert_quiet(&out, t.elapsed(), Some(FAST));
    let left = env.asks();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["tool"]["preview"], "rm -rf build");
    env.hook("stop", &json!({ "session_id": "S" }).to_string());
    let out = rm.wait_with_output().unwrap();
    assert!(out.stdout.is_empty());
}

#[test]
fn an_answer_that_does_not_fit_lets_go_at_once() {
    let env = Env::new("[ask]\nwait_secs = 30\n");
    env.take();
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap();
    // An app writing the file itself, with allow for a question.
    std::fs::write(
        env.state().join(format!("asks/{id}.answer.json")),
        r#"{"schema":1,"by":"desktop","behavior":"allow"}"#,
    )
    .unwrap();
    let t = Instant::now();
    let out = child.wait_with_output().unwrap();
    assert_quiet(&out, t.elapsed(), Some(FAST));
    assert_eq!(env.closed(id).unwrap()["reason"], "invalid-answer");
}

#[test]
fn closed_questions_leave_nothing_standing() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    for i in 0..20 {
        let session = format!("W{i}");
        env.hook("pre-tool-use", &pre_tool_use(&session));
        env.hook("stop", &json!({ "session_id": session }).to_string());
    }
    assert!(
        standing_files(&env).is_empty(),
        "{:?}",
        standing_files(&env)
    );
    assert_eq!(
        std::fs::read_dir(env.state().join("asks/done"))
            .unwrap()
            .count(),
        20
    );
}

#[test]
fn wait_is_capped_below_the_hook_timeout() {
    let env = Env::new("[ask]\nwait_secs = 10000\n");
    env.take();
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let ms = |k: &str| moss_core::ask::parse_time(ask[k].as_str().unwrap()).unwrap();
    assert_eq!(ms("expires_at") - ms("created_at"), 590_000, "{ask}");
    env.hook("stop", &json!({ "session_id": "S" }).to_string());
    child.wait_with_output().unwrap();
}

#[test]
fn permission_request_sweeps_old_questions() {
    let env = Env::new("[ask]\nwait_secs = 1\n");
    env.take();
    let done = env.state().join("asks/done");
    std::fs::create_dir_all(&done).unwrap();
    let old = json!({ "schema": 1, "id": "00000000001", "kind": "question", "session_id": "OLD",
        "created_at": "2020-01-01T00:00:00Z", "expires_at": "2020-01-01T00:05:00Z",
        "questions": tool_input()["questions"] });
    std::fs::write(done.join("00000000001.json"), old.to_string()).unwrap();
    std::fs::write(
        env.state().join("asks/00000000001.closed.json"),
        r#"{"reason":"timeout"}"#,
    )
    .unwrap();
    env.hook("permission-request", &permission_request("S"));
    assert!(!done.join("00000000001.json").exists());
    assert!(!env.state().join("asks/00000000001.closed.json").exists());
}

#[test]
fn non_interactive_sessions_never_wait() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    let t = Instant::now();
    let mut child = env
        .cmd()
        .env("CLAUDE_CODE_SESSION_ATTENDED", "0")
        .args(["hook", "permission-request"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(permission_request("S").as_bytes());
    let out = child.wait_with_output().unwrap();
    assert_quiet(&out, t.elapsed(), Some(FAST));
    assert!(env.asks().is_empty(), "nothing written for `claude -p`");
}

#[test]
fn an_asks_dir_others_can_reach_is_not_used() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new(SHORT_WAIT);
    env.take();
    let asks = env.state().join("asks");
    std::fs::set_permissions(&asks, std::fs::Permissions::from_mode(0o770)).unwrap();
    let (out, took) = env.hook("permission-request", &permission_request("S"));
    std::fs::set_permissions(&asks, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_quiet(&out, took, Some(FAST));
    assert!(standing_files(&env).is_empty());
}

#[test]
fn a_permission_needs_a_longer_id_and_is_said_back() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    let child = env.spawn_hook("permission-request", &bash_request("S", "touch probe.txt"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap();
    let short = env.cc_moss(&["ask", "answer", &id[..6], "--allow"]);
    assert!(!short.status.success());
    assert!(
        String::from_utf8_lossy(&short.stderr).contains("at least 12 characters"),
        "{short:?}"
    );
    let out = env.cc_moss(&["ask", "answer", &id[..12], "--allow"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "Allowed. S runs Bash: touch probe.txt"
    );
    let out = child.wait_with_output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "allow");
}

#[test]
fn pre_tool_use_stays_off_the_registry() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    let mut start = env.cmd();
    start
        .env("CLAUDE_PID", std::process::id().to_string())
        .args(["hook", "session-start"])
        .stdin(Stdio::piped());
    let mut child = start.spawn().unwrap();
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(start_json("S", "startup", Some("job")).as_bytes());
    assert!(child.wait().unwrap().success());
    env.hook("pre-tool-use", &pre_tool_use("S"));
    let noted = wait_for(|| env.asks().into_iter().next());
    assert!(noted.get("session_name").is_none(), "{noted}");
    // PermissionRequest takes it over and fills in the name.
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let named = wait_for(|| {
        env.asks()
            .into_iter()
            .find(|a| a["session_name"].is_string())
    });
    assert_eq!(named["id"], noted["id"]);
    assert_eq!(named["session_name"], "job");
    env.hook("stop", &json!({ "session_id": "S" }).to_string());
    child.wait_with_output().unwrap();
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
    // PreToolUse notes the question; PermissionRequest adds the name.
    let ask = wait_for(|| {
        world_asks(&w)
            .into_iter()
            .find(|a| a["session_name"].is_string())
    });
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

/// The menu bar app answering (T1.4): it says it's here with `ask take
/// --name desktop`, reads `ask list --json`, and answers with exactly the
/// arguments MossKit builds (`AskReply.arguments`: the full id, `--by
/// desktop`, `--json`). The session gets the answer and the answer says who gave it.
#[test]
fn answer_from_desktop_client() {
    let w = World::new("ask-desktop");
    let take = w
        .cc_moss()
        .args(["ask", "take", "--name", "desktop"])
        .output()
        .unwrap();
    assert!(take.status.success(), "{take:?}");
    assert!(w.state().join("asks/takers/desktop.alive").exists());
    w.new_session("job-d");
    let sid = w.session_id("job-d");
    w.send(&sid, "ask");
    let ask = wait_for(|| {
        world_asks(&w)
            .into_iter()
            .find(|a| a["session_name"].is_string())
    });
    // What a card shows: the session, the whole question, the options as written.
    assert_eq!(ask["session_name"], "job-d");
    assert_eq!(ask["questions"][0]["question"], QUESTION);
    assert_eq!(ask["questions"][0]["options"][1]["label"], "B");

    let id = ask["id"].as_str().unwrap();
    let out = w
        .cc_moss()
        .args([
            "ask", "answer", id, "--choice", "B", "--by", "desktop", "--json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    // What MossKit reads.
    assert_eq!(
        json_result(&out),
        json!({ "schema": 1, "result": "answered", "session": "job-d" })
    );
    let (out, done) = ask_files(&w, &sid);
    wait_for(|| exists(&done));
    let v: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(
        v["hookSpecificOutput"]["decision"]["updatedInput"]["answers"],
        json!({ QUESTION: "B" })
    );
    let answer: Value = serde_json::from_slice(
        &std::fs::read(w.state().join(format!("asks/{id}.answer.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(answer["by"], "desktop");
    // The card goes: the question is no longer listed.
    assert!(world_asks(&w).is_empty());

    // Answering again, as a second click would: "too late", not an error.
    let again = w
        .cc_moss()
        .args([
            "ask", "answer", id, "--choice", "A", "--by", "desktop", "--json",
        ])
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert_eq!(json_result(&again)["result"], "too-late");
}

/// `ask answer --json`: the one line the app reads instead of the words.
fn json_result(out: &Output) -> Value {
    let line = String::from_utf8_lossy(&out.stdout);
    assert_eq!(line.lines().count(), 1, "{out:?}");
    serde_json::from_str(line.trim()).unwrap()
}

#[test]
fn answer_json_answered() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap();
    let out = env.cc_moss(&["ask", "answer", id, "--choice", "B", "--json"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        json_result(&out),
        json!({ "schema": 1, "result": "answered", "session": "S" })
    );
    child.wait_with_output().unwrap();
}

#[test]
fn answer_json_not_waiting() {
    let env = Env::new(SHORT_WAIT);
    env.take();
    env.hook("pre-tool-use", &pre_tool_use("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap();
    let out = env.cc_moss(&["ask", "answer", id, "--choice", "A", "--json"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        json_result(&out),
        json!({ "schema": 1, "result": "not-waiting", "session": "S" })
    );
}

#[test]
fn answer_json_too_late() {
    // Open long enough to be seen by a busy runner's polling.
    let env = Env::new(SHORT_WAIT);
    env.take();
    let child = env.spawn_hook("permission-request", &permission_request("S"));
    let ask = wait_for(|| env.asks().into_iter().next());
    let id = ask["id"].as_str().unwrap().to_string();
    child.wait_with_output().unwrap(); // timed out
    let late = env.cc_moss(&["ask", "answer", &id, "--choice", "A", "--json"]);
    assert!(!late.status.success(), "too late keeps its exit status");
    assert_eq!(
        json_result(&late),
        json!({ "schema": 1, "result": "too-late", "session": "S" })
    );
    // Without --json the words are unchanged.
    let plain = env.cc_moss(&["ask", "answer", &id, "--choice", "A"]);
    assert!(plain.stdout.is_empty());
    assert!(String::from_utf8_lossy(&plain.stderr).contains("no longer needs an answer"));
}
