//! `cc-moss adopt`: a session started outside tmux (a plain child process,
//! like one in a VS Code terminal) is named, quits, and comes back in the
//! test's tmux with the same session id. Isolated tmux socket, state and
//! HOME; the fake claude.

mod common;

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{ls_json, World};

struct Outside(Child);

impl Drop for Outside {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_until<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn session(w: &World, sid: &str) -> Option<serde_json::Value> {
    ls_json(&w.state(), true)["sessions"]
        .as_array()?
        .iter()
        .find(|s| s["session_id"] == sid)
        .cloned()
}

/// RFC 3339 for `ms` after the epoch (UTC).
fn rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60,
        ms.rem_euclid(1000)
    )
}

#[test]
fn adopt_moves_a_session_into_tmux() {
    let w = World::new("adopt");
    let sid = "0837a145-0000-4000-8000-000000000001";
    // Started in a terminal of its own, not in tmux, with no name.
    let mut cmd = Command::new(&w.claude);
    for var in ["TMUX", "TMUX_PANE", "CLAUDE_PID", "CLAUDE_CODE_SESSION_ID"] {
        cmd.env_remove(var);
    }
    let child = cmd
        .args(["--session-id", sid])
        .current_dir(w.path("work"))
        .env("HOME", "/nonexistent/cc-moss-test-home")
        .env("CC_MOSS_BIN", env!("CARGO_BIN_EXE_cc-moss"))
        .env("CC_MOSS_STATE_DIR", w.state())
        .env("CC_MOSS_FAKE_DIR", w.path("fifos"))
        .env("CC_MOSS_CONFIG", w.path("config.toml"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut outside = Outside(child);
    let before = wait_until("the session to be recorded", 10, || session(&w, sid));
    assert!(before["tmux"].is_null(), "{before}");
    assert!(before["name"].is_null(), "{before}");
    // Its transcript says more than the registry: a prompt from just now.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let said = serde_json::json!({"type": "user", "timestamp": rfc3339(now + 1_000),
        "message": {"role": "user", "content": "tidy up the about page"}});
    let answered = serde_json::json!({"type": "assistant", "timestamp": rfc3339(now + 2_000),
        "message": {"role": "assistant", "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "Done."}]}});
    let line = format!("{said}\n{answered}");
    std::fs::write(
        w.path("fifos").join(format!("{sid}.jsonl")),
        format!("{line}\n"),
    )
    .unwrap();

    // What it would do, first.
    let out = w
        .cc_moss()
        .args(["adopt", &sid[..8], "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let would: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(would["result"], "would-move", "{would}");
    assert_eq!(would["named"], "work");

    let out = w
        .cc_moss()
        .args(["adopt", &sid[..8], "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let moved: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(moved["result"], "moved", "{moved}");
    assert_eq!(moved["named"], "work");
    assert_eq!(moved["refreshed"], true);
    assert_eq!(moved["tmux_session"], "host_work");
    // The process outside quit; the session runs in tmux, same id.
    let quit = wait_until("the outside process to quit", 10, || {
        outside.0.try_wait().ok().flatten()
    });
    let _ = quit;
    let after = wait_until("the session in tmux", 10, || {
        let s = session(&w, sid)?;
        (s["status"] == "running" && !s["tmux"].is_null()).then_some(s)
    });
    assert_eq!(after["name"], "work");
    assert_eq!(after["tmux"]["session_name"], "host_work");
    assert_eq!(after["last_prompt"], "tidy up the about page");
    assert!(w.session_names().contains(&"host_work".to_string()));
    // Claude Code itself is started with the name, not only the registry.
    let cmd = w.tmux(&[
        "list-panes",
        "-t",
        "=host_work",
        "-F",
        "#{pane_start_command}",
    ]);
    assert!(
        cmd.contains(&format!("--resume {sid} --name=work")),
        "{cmd}"
    );
    // Its terminal tab shows its name.
    let shown = w.tmux(&[
        "display-message",
        "-p",
        "-t",
        "=host_work:",
        "#{@cc-moss-name}",
    ]);
    assert_eq!(shown.trim(), "work");
    let audit = std::fs::read_to_string(w.state().join("audit.jsonl")).unwrap();
    assert!(
        audit.contains("\"action\":\"adopt\"") && audit.contains("\"moved\""),
        "{audit}"
    );
    // In tmux now: nothing more to move.
    let out = w
        .cc_moss()
        .args(["adopt", "work", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let again: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(again["reason"], "in-tmux", "{again}");
}
