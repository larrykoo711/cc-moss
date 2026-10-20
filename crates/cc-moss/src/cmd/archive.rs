//! The archive (T2.5): `cc-moss archive|unarchive <name>…`, the daemon's
//! daily tidy-up that archives sessions nobody used for a while, and purge,
//! which forgets archived sessions for good (only through Moss, with your
//! yes: see [`super::act`]) and puts their transcripts in the Trash.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use moss_core::config::Config;
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, Slot, Status};
use moss_core::trash::Trash;
use moss_core::{proc, store_dir};
use serde_json::{json, Value};

use super::ls::JSON_SCHEMA;

/// One session's result, for text and `--json`.
struct Outcome {
    name: String,
    session_id: Option<String>,
    result: &'static str,
    reason: Option<&'static str>,
    message: String,
    /// For `ambiguous`: the id prefixes of the sessions the name matches.
    candidates: Vec<String>,
}

impl Outcome {
    fn to_json(&self) -> Value {
        let mut v = json!({
            "name": self.name,
            "session_id": self.session_id,
            "result": self.result,
        });
        if let Some(reason) = self.reason {
            v["reason"] = reason.into();
            v["message"] = self.message.clone().into();
        }
        if !self.candidates.is_empty() {
            v["candidates"] = self.candidates.clone().into();
        }
        v
    }
}

fn print(outcomes: &[Outcome], as_json: bool) -> Result<bool> {
    if as_json {
        let results: Vec<Value> = outcomes.iter().map(Outcome::to_json).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "schema": JSON_SCHEMA, "results": results }))?
        );
    } else {
        for o in outcomes {
            println!("{}", o.message);
        }
    }
    Ok(outcomes.iter().all(|o| o.result != "refused"))
}

fn refused(name: &str, slot: Option<&Slot>, reason: &'static str, message: String) -> Outcome {
    Outcome {
        name: name.to_string(),
        session_id: slot.map(|s| s.session_id.clone()),
        result: "refused",
        reason: Some(reason),
        message,
        candidates: Vec::new(),
    }
}

/// The refusal for a name that picks no one session.
fn unresolved(name: &str, u: super::Unresolved) -> Outcome {
    Outcome {
        reason: Some(u.code(false)),
        candidates: u.candidates,
        ..refused(name, None, "not-found", u.message)
    }
}

/// `cc-moss archive <name>… [--end] [--json]`. False when one was refused.
/// With `end`, a running session that is idle is ended first (asked to
/// quit, recorded as closed) and then archived.
pub fn archive(names: &[String], end: bool, as_json: bool) -> Result<bool> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let mut outcomes = Vec::new();
    for name in names {
        let slot = match super::resolve_item(&slots, name) {
            Ok(s) => s,
            Err(u) => {
                outcomes.push(unresolved(name, u));
                continue;
            }
        };
        if slot.is_moss() {
            outcomes.push(refused(
                name,
                Some(slot),
                "moss",
                super::moss::protected_hint(),
            ));
            continue;
        }
        let running = slot.effective_status(proc::is_alive) == Status::Running;
        if running && end {
            if let Some(no) = end_idle(&registry, name, slot) {
                outcomes.push(no);
                continue;
            }
        } else if running {
            outcomes.push(refused(
                name,
                Some(slot),
                "running",
                tr!(
                    format!("{name} is still running; end it first, then archive it."),
                    format!("{name} 还在运行；先结束它，再归档。")
                ),
            ));
            continue;
        }
        registry.archive(
            std::slice::from_ref(&slot.slot_id),
            "you",
            // Ended just now on purpose: its process is gone.
            &|s| !(end && running) && s.effective_status(proc::is_alive) == Status::Running,
            now_ms(),
        )?;
        outcomes.push(Outcome {
            name: name.clone(),
            session_id: Some(slot.session_id.clone()),
            result: "archived",
            reason: None,
            candidates: Vec::new(),
            message: tr!(
                format!("Archived {name}. `cc-moss unarchive {name}` brings it back."),
                format!("已归档 {name}。`cc-moss unarchive {name}` 可以拿回来。")
            ),
        });
    }
    print(&outcomes, as_json)
}

/// End a running session that is idle, the way `adopt` asks one to quit,
/// and record it as closed by the user (no "dropped" news). Its tmux
/// session goes with it when Claude Code was the pane's program; a shell
/// holding it stays. The refusal, if it can't.
fn end_idle(registry: &Registry, name: &str, slot: &Slot) -> Option<Outcome> {
    if is_busy(slot) {
        return Some(refused(
            name,
            Some(slot),
            "busy",
            tr!(
                format!("{name} is working or waiting for you; archive it when it's idle."),
                format!("{name} 正在干活或在等你；等它闲下来再归档。")
            ),
        ));
    }
    let failed = |why: String| {
        Some(refused(
            name,
            Some(slot),
            "failed",
            tr!(
                format!("Couldn't end {name}: {why}"),
                format!("没能结束 {name}：{why}")
            ),
        ))
    };
    let Some(pid) = slot.pid else {
        return failed("cc-moss doesn't know its process".into());
    };
    if !super::adopt::still_it(slot) {
        return failed("its process isn't that Claude Code any more".into());
    }
    // Its exit is on purpose: no interrupted news while it goes.
    if let Err(e) = registry.mark_restore_started(&slot.slot_id, now_ms()) {
        return failed(format!("{e:#}"));
    }
    // SAFETY: a signal to a pid just checked to be this Claude Code.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let until = Instant::now() + Duration::from_secs(10);
    while proc::liveness(pid, slot.pid_start.as_deref()) == proc::Liveness::Alive {
        if Instant::now() >= until {
            let _ = registry.clear_restore_started(&slot.slot_id);
            return failed(tr!("it didn't quit within 10 seconds", "它 10 秒内没有退出").into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let closed = registry
        .close_by_user(&slot.slot_id, now_ms())
        .and_then(|()| registry.clear_restore_started(&slot.slot_id));
    match closed {
        Ok(()) => None,
        Err(e) => failed(format!("{e:#}")),
    }
}

/// In a turn, waiting for the user, or with something typed in its input.
fn is_busy(slot: &Slot) -> bool {
    if super::provider::in_turn(slot) || slot.attention.is_some() {
        return true;
    }
    let Some(loc) = slot.tmux.as_ref() else {
        return false;
    };
    let screen = moss_core::command::run_with_timeout(
        std::process::Command::new("tmux")
            .args(moss_core::tmux::Socket::Path(loc.socket.clone()).args())
            .args(["capture-pane", "-p", "-J", "-e", "-t", &loc.pane_id]),
        Duration::from_secs(3),
    );
    match screen {
        Ok(out) if out.status.success() => {
            moss_core::moss::inbox::input_has_text(&String::from_utf8_lossy(&out.stdout))
        }
        // Can't look: not sure it's idle.
        _ => true,
    }
}

/// `cc-moss unarchive <name>… [--json]`. False when one was refused.
pub fn unarchive(names: &[String], as_json: bool) -> Result<bool> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let mut outcomes = Vec::new();
    for name in names {
        // An archived session is the one meant when a name is shared.
        let archived: Vec<Slot> = slots.iter().filter(|s| s.is_archived()).cloned().collect();
        let slot = match super::resolve(&archived, name)
            .map_err(|_| ())
            .or_else(|_| super::resolve_item(&slots, name))
        {
            Ok(s) => s,
            Err(u) => {
                outcomes.push(unresolved(name, u));
                continue;
            }
        };
        registry.unarchive(std::slice::from_ref(&slot.slot_id), "you", now_ms())?;
        outcomes.push(Outcome {
            name: name.clone(),
            session_id: Some(slot.session_id.clone()),
            result: "unarchived",
            reason: None,
            candidates: Vec::new(),
            message: tr!(
                format!("{name} is back in the list."),
                format!("{name} 回到列表里了。")
            ),
        });
    }
    print(&outcomes, as_json)
}

// ---------------------------------------------------------------- auto

/// Archive what was idle longer than `days` (0: nothing). The slot ids
/// archived; one `archived {count, by: "auto"}` event when there were any.
pub fn archive_idle(registry: &Registry, days: u32, now_ms: i64) -> Result<Vec<String>> {
    let rows: Vec<(Status, Slot)> = registry
        .slots()?
        .into_iter()
        .map(|s| (s.effective_status(proc::is_alive), s))
        .collect();
    let idle = moss_core::archive::idle(&rows, now_ms, days);
    if idle.is_empty() {
        return Ok(idle);
    }
    registry.archive(
        &idle,
        "auto",
        &|s| s.effective_status(proc::is_alive) == Status::Running,
        now_ms,
    )
}

/// The daemon's daily tidy-up: once a local day (and right after the
/// daemon starts), archive sessions idle past `[sessions]
/// archive_after_days`.
pub struct Tidy {
    state: PathBuf,
    next: Instant,
}

/// How often the daemon looks whether today's tidy-up is done.
const TIDY_EVERY: Duration = Duration::from_secs(30);

impl Tidy {
    pub fn new(state: &Path) -> Tidy {
        Tidy {
            state: state.to_path_buf(),
            next: Instant::now(),
        }
    }

    /// `<state>/daemon/archive-day.txt`: the local date of the last tidy-up.
    fn stamp(&self) -> PathBuf {
        moss_core::daemon::Paths::new(&self.state)
            .dir
            .join("archive-day.txt")
    }

    pub fn tick(&mut self, now: Instant, log: &dyn Fn(&str)) {
        if now < self.next {
            return;
        }
        self.next = now + TIDY_EVERY;
        let today = super::checkup::today();
        let last = std::fs::read_to_string(self.stamp()).unwrap_or_default();
        if last.trim() == today {
            return;
        }
        let db = self.state.join("registry.db");
        if !db.exists() {
            return;
        }
        let days = match Config::load() {
            Ok(c) => c.sessions.archive_after_days,
            Err(e) => {
                log(&format!(
                    "archive: cannot read config.toml ({e:#}); trying later"
                ));
                return;
            }
        };
        match Registry::open(&db).and_then(|r| archive_idle(&r, days, now_ms())) {
            Ok(done) => {
                if !done.is_empty() {
                    log(&format!(
                        "archive: archived {} session(s) idle over {days} days",
                        done.len()
                    ));
                }
                if let Err(e) =
                    moss_core::moss::home::write_if_changed(&self.stamp(), &format!("{today}\n"))
                {
                    log(&format!("archive: error: noting today's tidy-up: {e}"));
                }
            }
            Err(e) => log(&format!("archive: error: {e:#}")),
        }
    }
}

// ---------------------------------------------------------------- purge

/// What a purge did to one session.
#[derive(Debug, Default)]
pub struct Purged {
    /// Slot ids removed from the registry.
    pub removed: Vec<String>,
    /// Transcripts now in the Trash.
    pub trashed: Vec<PathBuf>,
    /// Slot ids left alone, with why.
    pub kept: Vec<(String, String)>,
}

/// Forget the archived sessions `slot_ids` for good: each one's own
/// transcripts go to `trash` first (moved, never deleted), then its
/// records. A session whose transcript could not be moved keeps its
/// records, so nothing is lost without a trace. Sessions that are not
/// archived, or run again, are left alone.
pub fn purge(registry: &Registry, slot_ids: &[String], trash: &Trash) -> Result<Purged> {
    let slots = registry.slots()?;
    let mut out = Purged::default();
    let mut ready: Vec<String> = Vec::new();
    for id in slot_ids {
        let Some(slot) = slots.iter().find(|s| &s.slot_id == id) else {
            out.kept
                .push((id.clone(), "it is not in the records".into()));
            continue;
        };
        if !slot.is_archived() || slot.is_moss() {
            out.kept.push((id.clone(), "it is not archived".into()));
            continue;
        }
        if slot.effective_status(proc::is_alive) == Status::Running {
            out.kept.push((id.clone(), "it is running again".into()));
            continue;
        }
        let mut moved_all = true;
        for path in registry.own_transcripts(id, slot_ids)? {
            let path = Path::new(&path);
            match trash.put(path) {
                Ok(to) => out.trashed.push(to),
                // Already gone: nothing to keep.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    moved_all = false;
                    out.kept.push((
                        id.clone(),
                        format!("its transcript could not go to the Trash: {e}"),
                    ));
                    break;
                }
            }
        }
        if moved_all {
            ready.push(id.clone());
        }
    }
    out.removed = registry.purge(&ready)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::event::{Context, Event, StartSource};

    fn start(r: &mut Registry, sid: &str, transcript: &Path, at: i64) -> String {
        let ctx = Context {
            pid: None,
            pid_start: None,
            tmux: None,
            launch_args: None,
            now_ms: at,
        };
        r.apply(
            &Event::Start {
                source: StartSource::Startup,
                session_id: sid.into(),
                cwd: "/work".into(),
                title: Some(sid.into()),
                transcript_path: Some(transcript.to_string_lossy().into_owned()),
            },
            &ctx,
        )
        .unwrap();
        r.slots()
            .unwrap()
            .into_iter()
            .find(|s| s.session_id == sid)
            .unwrap()
            .slot_id
    }

    #[test]
    fn purge_keeps_a_session_whose_transcript_cannot_be_trashed() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = Registry::open(&dir.path().join("state/registry.db")).unwrap();
        // A folder where the transcript should be: not a file, not moved.
        let odd = dir.path().join("odd.jsonl");
        std::fs::create_dir_all(&odd).unwrap();
        let id = start(&mut r, "odd", &odd, 1_000);
        r.archive(std::slice::from_ref(&id), "you", &|_| false, 2_000)
            .unwrap();
        let trash = Trash::Folder(dir.path().join("Trash"));
        let done = purge(&r, std::slice::from_ref(&id), &trash).unwrap();
        assert!(done.removed.is_empty());
        assert_eq!(done.kept.len(), 1);
        assert!(odd.exists());
        assert_eq!(r.slots().unwrap().len(), 1, "its records stay");
    }
}
