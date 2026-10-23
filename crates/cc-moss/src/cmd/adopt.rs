//! `cc-moss adopt <name>`: move a session running outside tmux (a VS Code
//! terminal, a plain window) into tmux. A running process cannot be moved,
//! but Claude Code resumes a session from its transcript: so the session is
//! named if it has no name, its last activity is read back from its
//! transcript, it is asked to quit (SIGTERM; Claude Code 2.1.303 exits from
//! idle in about half a second and reports SessionEnd "other"), and it is
//! resumed in tmux the way `restore` does it: same session id, same launch
//! arguments, its name.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use moss_core::config::Config;
use moss_core::proc::{self, Liveness};
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, rfc3339, Slot, Status};
use moss_core::{naming, store_dir};
use serde_json::json;

use super::restore::{Launched, Launcher};

/// How long the session has to quit.
const QUIT: Duration = Duration::from_secs(10);

/// Why a session is not moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Moss,
    NotRunning,
    InTmux,
    Busy,
    /// Its recorded process is not that Claude Code any more (or cannot be
    /// told).
    NotItsProcess,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Moss => "moss",
            Refusal::NotRunning => "not-running",
            Refusal::InTmux => "in-tmux",
            Refusal::Busy => "busy",
            Refusal::NotItsProcess => "not-its-process",
        }
    }

    pub fn message(self) -> String {
        match self {
            Refusal::Moss => super::moss::protected_hint(),
            Refusal::NotRunning => tr!(
                "It isn't running; bring it back with restore instead.",
                "它没在运行；用「找回」把它恢复回来。"
            )
            .into(),
            Refusal::InTmux => tr!("It is in tmux already.", "它已经在 tmux 里了。").into(),
            Refusal::Busy => tr!(
                "It's working; move it when this turn is done.",
                "它正在干活，等这一轮做完再移。"
            )
            .into(),
            Refusal::NotItsProcess => tr!(
                "cc-moss can't be sure which process is this session, so it leaves it alone.",
                "cc-moss 没法确认哪个进程是这个会话，所以不动它。"
            )
            .into(),
        }
    }
}

/// Whether `slot` may be moved: running, outside tmux, idle, not Moss, and
/// its recorded process still that Claude Code (`process`).
pub fn refusal(
    slot: &Slot,
    status: Status,
    turn_open: Option<bool>,
    process: Liveness,
) -> Option<Refusal> {
    if slot.is_moss() {
        return Some(Refusal::Moss);
    }
    if status != Status::Running {
        return Some(Refusal::NotRunning);
    }
    if slot.tmux.is_some() {
        return Some(Refusal::InTmux);
    }
    if slot.busy || turn_open == Some(true) {
        return Some(Refusal::Busy);
    }
    if slot.pid.is_none() || slot.pid_start.is_none() || process != Liveness::Alive {
        return Some(Refusal::NotItsProcess);
    }
    None
}

/// Shared process identity, including a durable, verified single-hop resume.
pub(super) fn still_it(slot: &Slot) -> bool {
    Registry::open(&store_dir::registry_path())
        .ok()
        .is_some_and(|registry| super::process_identity::verify(&registry, slot).is_ok())
}

/// The name an auto-named session gets: its folder's, unique among the
/// names there are. `None` when it has a name.
pub fn new_name(slot: &Slot, others: &[Slot]) -> Option<String> {
    if !super::checkup::auto_named(slot) {
        return None;
    }
    let taken: Vec<String> = others
        .iter()
        .filter(|s| s.slot_id != slot.slot_id && s.status != Status::Closed)
        .filter_map(|s| s.name.clone())
        .collect();
    Some(naming::unique(&naming::short_name(&slot.cwd), &taken, None))
}

/// `cc-moss adopt <name> [--json] [--dry-run]`. False when it was refused
/// or did not work (exit 1).
pub fn run(name: &str, json_out: bool, dry_run: bool) -> Result<bool> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let slot = match super::resolve_item(&slots, name) {
        Ok(s) => s.clone(),
        Err(u) if json_out => {
            println!(
                "{}",
                json!({"schema": 1, "result": "refused", "reason": u.code(false),
                    "message": u.message, "candidates": u.candidates})
            );
            return Ok(false);
        }
        Err(u) => anyhow::bail!(u.message),
    };
    let say = |v: serde_json::Value, plain: String| {
        if json_out {
            println!("{v}");
        } else {
            println!("{plain}");
        }
    };
    let turn = slot
        .transcript_path
        .as_deref()
        .and_then(|p| moss_core::transcript::turn_open(Path::new(p)));
    let process = slot.pid.map_or(Liveness::Unknown, |pid| {
        proc::liveness(pid, slot.pid_start.as_deref())
    });
    let status = slot.effective_status(proc::is_alive);
    if let Some(r) = refusal(&slot, status, turn, process) {
        say(
            json!({"schema": 1, "result": "refused", "reason": r.code(), "message": r.message()}),
            r.message(),
        );
        return Ok(false);
    }
    let identity = match super::process_identity::verify(&registry, &slot) {
        Ok(identity) => identity,
        Err(error) => {
            say(
                json!({"schema":1,"result":"refused","reason":"not-its-process","message":error.to_string()}),
                error.to_string(),
            );
            return Ok(false);
        }
    };
    let named = new_name(&slot, &slots);
    let mut preview = slot.clone();
    if let Some(name) = &named {
        preview.name = Some(name.clone());
    }
    if let Err(error) = super::restart::resume_argv("claude", &preview, false) {
        say(
            json!({"schema":1,"result":"refused","reason":"config-source","message":error.to_string()}),
            error.to_string(),
        );
        return Ok(false);
    }
    let working = match super::working_directory::for_slot(&slot) {
        Ok(working) => working,
        Err(error) => {
            say(
                json!({"schema":1,"result":"refused","reason":super::working_directory::code(&error),"message":error.to_string()}),
                error.to_string(),
            );
            return Ok(false);
        }
    };
    let tail = slot
        .transcript_path
        .as_deref()
        .and_then(|p| moss_core::transcript::last_prompt(Path::new(p)));
    let refreshed = tail
        .as_ref()
        .is_some_and(|(_, at)| *at > slot.last_active_at_ms);
    if dry_run {
        say(
            json!({"schema": 1, "result": "would-move", "slot_id": slot.slot_id,
                "session_id": slot.session_id, "named": named, "refreshed": refreshed}),
            tr!(
                format!(
                    "Would move {} into tmux.",
                    super::restore::display_name(&slot)
                ),
                format!("会把 {} 移进 tmux。", super::restore::display_name(&slot))
            ),
        );
        return Ok(true);
    }
    let config = Config::load()?;
    let mut launcher = Launcher::new(&config)?.quiet();
    if let Err(error) = moss_core::working_directory::recheck(&working) {
        say(
            json!({"schema":1,"result":"refused","reason":error.code(),"message":error.to_string()}),
            error.to_string(),
        );
        return Ok(false);
    }
    // Tidy first: a name, and what it last did.
    if let Some(n) = &named {
        registry.set_name(&slot.slot_id, n)?;
    }
    if let Some((prompt, at)) = &tail {
        registry.refresh_activity(&slot.slot_id, prompt, *at)?;
    }
    let pid = slot.pid.unwrap_or_default();
    let fail = |message: String| {
        say(
            json!({"schema": 1, "result": "failed", "message": message}),
            message.clone(),
        );
        audit(&slot, "failed", &message);
        Ok(false)
    };
    let current = registry
        .slots()?
        .into_iter()
        .find(|s| s.slot_id == slot.slot_id)
        .ok_or_else(|| anyhow::anyhow!("the session disappeared"))?;
    let claimed_at = now_ms();
    if !registry.claim_restore(&current, claimed_at)? {
        return fail("another operation owns this session".into());
    }
    if let Err(error) = identity.recheck(&registry, &slot) {
        registry.clear_restore_claim(&current, claimed_at)?;
        return fail(error.to_string());
    }
    if let Err(error) = moss_core::working_directory::recheck(&working) {
        registry.clear_restore_claim(&current, claimed_at)?;
        return fail(error.to_string());
    }
    // Ask it to quit, and wait for it to go.
    // SAFETY: a signal to a pid we just checked is that Claude Code.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let until = Instant::now() + QUIT;
    while proc::liveness(pid, slot.pid_start.as_deref()) == Liveness::Alive {
        if Instant::now() >= until {
            registry.clear_restore_claim(&current, claimed_at)?;
            return fail(tr!(
                "It didn't quit within 10 seconds, so it stays where it is. Quit it in its own terminal and try again.",
                "它 10 秒内没有退出，所以留在原处。在它自己的终端里退出后再试。"
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !identity.after_exit(&registry, &slot, claimed_at) {
        registry.clear_restore_claim(&current, claimed_at)?;
        return fail("the process owner changed while exiting; no replacement was started".into());
    }
    registry.clear_restore_claim(&current, claimed_at)?;
    // Its SessionEnd hook may still be writing: a moment for it (the
    // process is already gone). A SessionEnd that comes later from this old
    // process is ignored by the registry, so it can't end the new one.
    let reread = || -> Result<Slot> {
        Ok(registry
            .slots()?
            .into_iter()
            .find(|s| s.slot_id == slot.slot_id)
            .unwrap_or_else(|| slot.clone()))
    };
    let started = Instant::now();
    let mut fresh = reread()?;
    while fresh.status == Status::Running && started.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(100));
        fresh = reread()?;
    }
    if fresh.cwd != slot.cwd {
        return fail(moss_core::working_directory::Error::Changed.to_string());
    }
    if let Err(error) = moss_core::working_directory::recheck(&working) {
        return fail(error.to_string());
    }
    let mut launched = launcher.launch(&registry, &fresh)?;
    // A hook wrote meanwhile: once more, from what is recorded now.
    if matches!(launched, Launched::Changed) {
        std::thread::sleep(Duration::from_millis(500));
        let retry = reread()?;
        if retry.cwd != slot.cwd {
            return fail(moss_core::working_directory::Error::Changed.to_string());
        }
        if let Err(error) = moss_core::working_directory::recheck(&working) {
            return fail(error.to_string());
        }
        launched = launcher.launch(&registry, &retry)?;
    }
    let tmux = match launched {
        Launched::Started(name) => name,
        Launched::Changed => {
            return fail(
                tr!(
                    "It changed while it was being moved; look at it in the list.",
                    "移动时它有了变化；去列表里看看。"
                )
                .into(),
            )
        }
        Launched::Failed(_, message) => return fail(message),
    };
    // Up and recorded in tmux.
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        let up = registry
            .slots()?
            .into_iter()
            .any(|s| s.slot_id == slot.slot_id && s.tmux.is_some() && s.status == Status::Running);
        if up {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    audit(&slot, "moved", &tmux);
    say(
        json!({"schema": 1, "result": "moved", "slot_id": slot.slot_id,
            "session_id": slot.session_id, "named": named, "refreshed": refreshed,
            "tmux_session": tmux}),
        tr!(
            format!("Moved it into tmux session {tmux}."),
            format!("已移进 tmux 会话 {tmux}。")
        ),
    );
    Ok(true)
}

/// One line in `<state>/audit.jsonl`.
fn audit(slot: &Slot, outcome: &str, message: &str) {
    let line = json!({
        "schema": 1, "at": rfc3339(now_ms()), "by": "you", "action": "adopt",
        "args": [slot.slot_id], "outcome": outcome, "message": message,
    });
    let _ = moss_core::moss::act::append_audit(
        &moss_core::moss::act::audit_path(&store_dir::state_dir()),
        &line.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::event::TmuxLoc;

    fn slot() -> Slot {
        Slot {
            slot_id: "s1".into(),
            agent: "claude".into(),
            session_id: "0837a145-0000".into(),
            session_ids: vec![],
            name: None,
            cwd: "/Users/me/photo-lab".into(),
            pid: Some(42),
            pid_start: Some("start".into()),
            tmux: None,
            status: Status::Running,
            end_reason: None,
            notes: Default::default(),
            launch_args: vec![],
            transcript_path: None,
            busy: false,
            attention: None,
            last_prompt: None,
            restore_started_at_ms: None,
            role: None,
            created_at_ms: 0,
            last_active_at_ms: 0,
            ended_at_ms: None,
            archived_at_ms: None,
            archived_by: None,
            provider_started: None,
            provider_seen_keys: None,
        }
    }

    #[test]
    fn adopt_refuses_what_it_should_not_move() {
        let ok = slot();
        assert_eq!(
            refusal(&ok, Status::Running, Some(false), Liveness::Alive),
            None
        );
        let mut moss = slot();
        moss.role = Some("moss".into());
        assert_eq!(
            refusal(&moss, Status::Running, None, Liveness::Alive),
            Some(Refusal::Moss)
        );
        assert_eq!(
            refusal(&ok, Status::Interrupted, None, Liveness::Alive),
            Some(Refusal::NotRunning)
        );
        let mut tmuxed = slot();
        tmuxed.tmux = Some(TmuxLoc {
            socket: "/s".into(),
            session_id: Some("$1".into()),
            session_name: None,
            pane_id: "%1".into(),
        });
        assert_eq!(
            refusal(&tmuxed, Status::Running, None, Liveness::Alive),
            Some(Refusal::InTmux)
        );
        let mut busy = slot();
        busy.busy = true;
        assert_eq!(
            refusal(&busy, Status::Running, None, Liveness::Alive),
            Some(Refusal::Busy)
        );
        assert_eq!(
            refusal(&ok, Status::Running, Some(true), Liveness::Alive),
            Some(Refusal::Busy),
            "mid-turn by its transcript"
        );
        // A pid that is someone else now, or that can't be checked.
        for p in [Liveness::Dead, Liveness::Unknown] {
            assert_eq!(
                refusal(&ok, Status::Running, None, p),
                Some(Refusal::NotItsProcess)
            );
        }
        assert!(Refusal::Busy.message().contains("this turn is done"));
    }

    #[test]
    fn adopt_refuses_missing_process_start() {
        let mut s = slot();
        s.pid_start = None;
        assert_eq!(
            refusal(&s, Status::Running, None, Liveness::Alive),
            Some(Refusal::NotItsProcess)
        );
    }

    #[test]
    fn adopt_names_a_nameless_session_after_its_folder() {
        let mut taken = slot();
        taken.slot_id = "s2".into();
        taken.name = Some("photo-lab".into());
        let s = slot();
        assert_eq!(
            new_name(&s, &[s.clone(), taken]).as_deref(),
            Some("photo-lab-2")
        );
        let mut named = slot();
        named.name = Some("api".into());
        assert_eq!(new_name(&named, &[]), None);
    }
}
