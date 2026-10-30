//! Sessions Claude Code runs in its background supervisor (2.1.314): `/resume`
//! and `claude agents` start them there, outside any terminal. Claude Code
//! records each of its processes in `<config dir>/sessions/<pid>.json`.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use common::{bin, dead_pid, FakeProcess};

/// A state dir and a Claude Code config dir.
struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let m = Machine {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(m.config().join("sessions")).unwrap();
        m
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("claude")
    }

    /// What Claude Code writes about process `pid`.
    fn record(&self, pid: u32, entry: serde_json::Value) {
        let mut entry = entry;
        entry["pid"] = pid.into();
        std::fs::write(
            self.config().join("sessions").join(format!("{pid}.json")),
            entry.to_string(),
        )
        .unwrap();
    }

    /// `claude agents --json` answers `agents`.
    fn agents(&self, agents: serde_json::Value) -> PathBuf {
        let stub = self.dir.path().join("claude-bin");
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = agents ] && {{ echo '{agents}'; exit 0; }}\necho \"$@\"\n"
        );
        std::fs::write(&stub, script).unwrap();
        std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        stub
    }

    fn cli(&self, claude: &Path) -> Command {
        let mut cmd = bin();
        cmd.env("CC_MOSS_STATE_DIR", self.state())
            .env("CLAUDE_CONFIG_DIR", self.config())
            .env("CC_MOSS_CLAUDE_BIN", claude);
        cmd
    }

    /// A hook as Claude Code fires it: from process `pid`, attended or not,
    /// inside tmux or not.
    fn hook(&self, event: &str, payload: &str, pid: u32, attended: &str, in_tmux: bool) {
        let mut cmd = self.cli(Path::new("/nonexistent/claude"));
        cmd.args(["hook", event])
            .env("CLAUDE_PID", pid.to_string())
            .env("CLAUDE_CODE_SESSION_ATTENDED", attended)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if in_tmux {
            cmd.env("TMUX", "/nonexistent/tmux-socket,1,0")
                .env("TMUX_PANE", "%59");
        }
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{out:?}");
    }

    fn sessions(&self, claude: &Path) -> Vec<serde_json::Value> {
        let out = self
            .cli(claude)
            .args(["ls", "--json", "--all"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["sessions"].as_array().unwrap().clone()
    }

    fn run(&self, claude: &Path, args: &[&str]) -> Output {
        self.cli(claude).args(args).output().unwrap()
    }
}

fn start(session: &str, source: &str, title: Option<&str>) -> String {
    // A folder that exists, so a restore would go ahead.
    let cwd = std::env::temp_dir();
    serde_json::json!({
        "session_id": session, "cwd": cwd, "hook_event_name": "SessionStart",
        "source": source, "session_title": title,
    })
    .to_string()
}

fn prompt(session: &str, text: &str) -> String {
    serde_json::json!({
        "session_id": session, "hook_event_name": "UserPromptSubmit", "prompt": text,
    })
    .to_string()
}

const NO_CLAUDE: &str = "/nonexistent/claude";

#[test]
fn a_background_session_is_recorded_outside_tmux() {
    let m = Machine::new();
    let bg = FakeProcess::spawn();
    let pid = bg.pid().unwrap();
    m.record(
        pid,
        serde_json::json!({"kind": "bg", "sessionId": "BG-1", "jobId": "99a04088"}),
    );
    // Its supervisor was started from a tmux pane: the variables say tmux.
    m.hook(
        "session-start",
        &start("BG-1", "resume", Some("db-tools")),
        pid,
        "0",
        true,
    );
    m.hook(
        "user-prompt-submit",
        &prompt("BG-1", "pull the members"),
        pid,
        "0",
        true,
    );
    let s = m.sessions(Path::new(NO_CLAUDE));
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0]["status"], "running");
    assert_eq!(s[0]["name"], "db-tools");
    assert_eq!(s[0]["last_prompt"], "pull the members");
    assert!(s[0]["tmux"].is_null(), "not in tmux: {}", s[0]);
    assert_eq!(s[0]["background"], true);
}

#[test]
fn unattended_runs_that_are_not_background_sessions_stay_out() {
    let m = Machine::new();
    let p = FakeProcess::spawn();
    let pid = p.pid().unwrap();
    // `claude -p`: no record at all, or a record of another kind.
    m.hook(
        "session-start",
        &start("P-1", "startup", None),
        pid,
        "0",
        false,
    );
    m.record(
        pid,
        serde_json::json!({"kind": "interactive", "sessionId": "P-2"}),
    );
    m.hook(
        "session-start",
        &start("P-2", "startup", None),
        pid,
        "0",
        false,
    );
    assert!(m.sessions(Path::new(NO_CLAUDE)).is_empty());
}

#[test]
fn a_background_job_takes_over_the_conversation_a_terminal_handed_it() {
    let m = Machine::new();
    let terminal = FakeProcess::spawn();
    let job = FakeProcess::spawn();
    let (t, j) = (terminal.pid().unwrap(), job.pid().unwrap());
    m.hook(
        "session-start",
        &start("S-1", "startup", Some("ofoxai")),
        t,
        "1",
        true,
    );
    m.hook("user-prompt-submit", &prompt("S-1", "hello"), t, "1", true);
    // `/resume` in the terminal parks its conversation in a background job.
    m.record(
        t,
        serde_json::json!({"kind": "interactive", "sessionId": "S-1", "parkedJobId": "585fd743"}),
    );
    m.record(
        j,
        serde_json::json!({"kind": "bg", "sessionId": "S-1", "jobId": "585fd743"}),
    );
    m.hook(
        "session-start",
        &start("S-1", "resume", None),
        j,
        "0",
        false,
    );
    let s = m.sessions(Path::new(NO_CLAUDE));
    assert_eq!(s.len(), 1, "one conversation, one row: {s:?}");
    assert_eq!(s[0]["pid"], j);
    assert_eq!(s[0]["status"], "running");
    assert!(s[0]["tmux"].is_null(), "{}", s[0]);
}

#[test]
fn a_background_session_heard_of_late_drops_its_old_terminal() {
    let m = Machine::new();
    // It once ran in a tmux pane; that process is gone.
    m.hook(
        "session-start",
        &start("S-2", "startup", Some("db-tools")),
        dead_pid(),
        "1",
        true,
    );
    let job = FakeProcess::spawn();
    let j = job.pid().unwrap();
    m.record(
        j,
        serde_json::json!({"kind": "bg", "sessionId": "S-2", "jobId": "99a04088"}),
    );
    // Its SessionStart was skipped (older cc-moss); a prompt comes first.
    m.hook("user-prompt-submit", &prompt("S-2", "go on"), j, "0", false);
    let s = m.sessions(Path::new(NO_CLAUDE));
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0]["status"], "running");
    assert_eq!(s[0]["pid"], j);
    assert!(
        s[0]["tmux"].is_null(),
        "the pane is someone else's now: {}",
        s[0]
    );
}

#[test]
fn background_sessions_claude_agents_lists_are_adopted() {
    let m = Machine::new();
    m.hook(
        "session-start",
        &start("S-3", "startup", Some("db-tools")),
        dead_pid(),
        "1",
        true,
    );
    let job = FakeProcess::spawn();
    let j = job.pid().unwrap();
    let claude = m.agents(serde_json::json!([
        {"pid": j, "id": "99a04088", "cwd": "/work", "kind": "background",
         "sessionId": "S-3", "name": "db-tools", "status": "busy"}
    ]));
    let s = m.sessions(&claude);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0]["status"], "running");
    assert_eq!(s[0]["pid"], j);
    assert_eq!(s[0]["busy"], true);
    assert!(s[0]["tmux"].is_null(), "{}", s[0]);
}

#[test]
fn a_terminal_whose_conversation_left_with_a_job_lets_go_of_it() {
    let m = Machine::new();
    let quiet = FakeProcess::spawn();
    let busy = FakeProcess::spawn();
    let (q, b) = (quiet.pid().unwrap(), busy.pid().unwrap());
    m.hook(
        "session-start",
        &start("Q-1", "startup", Some("ofoxai")),
        q,
        "1",
        true,
    );
    m.hook(
        "session-start",
        &start("W-1", "startup", Some("work")),
        b,
        "1",
        true,
    );
    m.hook("user-prompt-submit", &prompt("W-1", "hello"), b, "1", true);
    // Both terminals parked their conversations; the jobs have since retired.
    m.record(
        q,
        serde_json::json!({"kind": "interactive", "sessionId": "Q-1", "parkedJobId": "585fd743"}),
    );
    m.record(
        b,
        serde_json::json!({"kind": "interactive", "sessionId": "W-1", "parkedJobId": "11112222"}),
    );
    let claude = m.agents(serde_json::json!([]));
    let s = m.sessions(&claude);
    let by = |id: &str| s.iter().find(|x| x["session_id"] == id).unwrap().clone();
    assert_eq!(by("Q-1")["status"], "closed", "never said a word: over");
    assert_eq!(by("W-1")["status"], "interrupted", "it can come back");
    assert!(
        by("W-1")["tmux"].is_null(),
        "the pane shows another session now"
    );
}

#[test]
fn a_terminal_stays_when_claude_code_cannot_be_asked() {
    let m = Machine::new();
    let t = FakeProcess::spawn();
    let pid = t.pid().unwrap();
    m.hook(
        "session-start",
        &start("K-1", "startup", Some("keep")),
        pid,
        "1",
        true,
    );
    m.record(
        pid,
        serde_json::json!({"kind": "interactive", "sessionId": "K-1", "parkedJobId": "585fd743"}),
    );
    let s = m.sessions(Path::new(NO_CLAUDE));
    assert_eq!(s[0]["status"], "running", "nothing known, nothing changed");
}

#[test]
fn a_conversation_running_in_the_background_is_never_restored_twice() {
    let m = Machine::new();
    m.hook(
        "session-start",
        &start("R-1", "startup", Some("db-tools")),
        dead_pid(),
        "1",
        false,
    );
    m.hook(
        "user-prompt-submit",
        &prompt("R-1", "hello"),
        dead_pid(),
        "1",
        false,
    );
    // cc-moss never heard from the job, but Claude Code runs it.
    let job = FakeProcess::spawn();
    m.record(
        job.pid().unwrap(),
        serde_json::json!({"kind": "bg", "sessionId": "R-1", "jobId": "99a04088"}),
    );
    let out = m.run(Path::new(NO_CLAUDE), &["restore", "--dry-run"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{out:?}");
    assert!(!text.contains("would restore 1 "), "{text}");
    assert!(text.contains("open in another session"), "said why: {text}");
}

#[test]
fn a_background_session_opens_with_claude_attach_and_takes_no_typing() {
    let m = Machine::new();
    let job = FakeProcess::spawn();
    let j = job.pid().unwrap();
    m.record(
        j,
        serde_json::json!({"kind": "bg", "sessionId": "A-1", "jobId": "99a04088"}),
    );
    m.hook(
        "session-start",
        &start("A-1", "resume", Some("db-tools")),
        j,
        "0",
        false,
    );
    // The stub prints what it was asked to do.
    let claude = m.agents(serde_json::json!([]));
    let out = m.run(&claude, &["attach", "db-tools"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "attach 99a04088"
    );
    let out = m.run(&claude, &["send", "db-tools", "--", "hello"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("cc-moss attach db-tools"), "{err}");
    let out = m.run(&claude, &["adopt", "db-tools", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["result"], "refused", "{v}");
    assert_eq!(v["reason"], "background", "{v}");
}
