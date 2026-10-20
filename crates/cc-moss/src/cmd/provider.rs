//! The API provider (T2.6): where Claude Code's user settings are, which
//! running sessions need a restart to use what they say now, the queue of
//! busy sessions to restart after their turn, and the daemon's watch on
//! the settings file.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use moss_core::proc;
use moss_core::provider::{self, Snapshot, Verdict};
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, Slot, Status};
use serde_json::json;

/// Claude Code's user settings, as the daemon and the app see them:
/// `[claude] config_dir`, else `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn settings_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    provider::settings_path(super::moss::config_dir().as_deref(), None, home.as_deref())
}

/// What the settings name now; `None` when they can't be read.
pub fn current() -> Option<Snapshot> {
    Snapshot::read(&settings_path()?)
}

/// The settings file a session's Claude Code reads: the one its hook read
/// at the start (its own `CLAUDE_CONFIG_DIR`), else the daemon's.
pub fn settings_of(slot: &Slot) -> Option<PathBuf> {
    let started = slot
        .provider_started
        .as_deref()
        .and_then(Snapshot::from_json)?;
    started.source.map(PathBuf::from).or_else(settings_path)
}

/// Whether a session has to be restarted to use the provider its settings
/// name (only a running one can); `None` when it uses it already or
/// nothing is known. `now` is the daemon's settings; a session reading
/// another file is compared with that one.
pub fn restart_reason(slot: &Slot, status: Status, now: Option<&Snapshot>) -> Option<&'static str> {
    if status != Status::Running {
        return None;
    }
    let (started, seen) = provider::recorded(
        slot.provider_started.as_deref(),
        slot.provider_seen_keys.as_deref(),
    )?;
    let own;
    let now = match started.source.as_deref().map(PathBuf::from) {
        Some(path) if Some(&path) != settings_path().as_ref() => {
            own = Snapshot::read(&path);
            own.as_ref()
        }
        _ => now,
    };
    match provider::verdict(&started, &seen, now?) {
        Verdict::Live => None,
        Verdict::NeedsRestart(r) => Some(r.code()),
    }
}

// ---------------------------------------------------------------- the queue

/// `<state>/daemon/restart-queue.json`: busy sessions to restart once
/// their turn is over.
pub fn queue_path(state: &Path) -> PathBuf {
    moss_core::daemon::Paths::new(state)
        .dir
        .join("restart-queue.json")
}

/// One session in the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub slot_id: String,
    /// Restart only if the provider switch still hasn't reached it (it was
    /// queued by `--stale`, or because another restart had it).
    pub recheck: bool,
    /// Restarts that didn't work so far.
    pub attempts: u32,
}

impl Queued {
    fn from_json(v: &serde_json::Value) -> Option<Queued> {
        Some(Queued {
            slot_id: v["slot_id"].as_str()?.to_string(),
            recheck: v["recheck"].as_bool().unwrap_or(false),
            attempts: v["attempts"].as_u64().unwrap_or(0) as u32,
        })
    }

    fn to_json(&self) -> serde_json::Value {
        json!({ "slot_id": self.slot_id, "recheck": self.recheck, "attempts": self.attempts })
    }
}

/// The daemon gives up on a queued session after this many failures.
pub const MAX_ATTEMPTS: u32 = 3;

fn read_queue(state: &Path) -> Vec<Queued> {
    std::fs::read_to_string(queue_path(state))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<serde_json::Value>>(&t).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(Queued::from_json)
        .collect()
}

/// The slot ids in the queue.
pub fn queued(state: &Path) -> Vec<String> {
    read_queue(state).into_iter().map(|q| q.slot_id).collect()
}

/// Change the queue under its lock (`flock` on a file beside it), so the
/// daemon and a `restart` never lose each other's change.
fn with_queue<T>(state: &Path, change: impl FnOnce(&mut Vec<Queued>) -> T) -> std::io::Result<T> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let path = queue_path(state);
    if let Some(dir) = path.parent() {
        moss_core::store_dir::ensure_private_dir(dir)?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.with_extension("lock"))?;
    // SAFETY: flock on a descriptor this function owns; released on close.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut all = read_queue(state);
    let out = change(&mut all);
    let text = serde_json::to_string(&all.iter().map(Queued::to_json).collect::<Vec<_>>())?;
    moss_core::moss::home::write_if_changed(&path, &text)?;
    drop(lock);
    Ok(out)
}

/// Put a session in the queue; once only (a recheck it had stays).
pub fn enqueue(state: &Path, slot_id: &str, recheck: bool) -> std::io::Result<()> {
    with_queue(state, |all| {
        match all.iter_mut().find(|q| q.slot_id == slot_id) {
            Some(q) => q.recheck &= recheck,
            None => all.push(Queued {
                slot_id: slot_id.to_string(),
                recheck,
                attempts: 0,
            }),
        }
    })
}

pub fn dequeue(state: &Path, slot_id: &str) -> std::io::Result<()> {
    with_queue(state, |all| all.retain(|q| q.slot_id != slot_id))
}

/// A queued restart didn't work; true when that was the last try (it left
/// the queue).
pub fn failed_attempt(state: &Path, slot_id: &str) -> std::io::Result<bool> {
    with_queue(state, |all| {
        let Some(q) = all.iter_mut().find(|q| q.slot_id == slot_id) else {
            return false;
        };
        q.attempts += 1;
        let last = q.attempts >= MAX_ATTEMPTS;
        if last {
            all.retain(|q| q.slot_id != slot_id);
        }
        last
    })
}

/// Whether the session is in a turn: Claude is working, or its
/// transcript says a reply is still open. A transcript that says the turn
/// is over wins over the busy mark (Esc ends a turn without a Stop hook).
pub fn in_turn(slot: &Slot) -> bool {
    let open = slot
        .transcript_path
        .as_deref()
        .and_then(|p| moss_core::transcript::turn_open(Path::new(p)));
    match open {
        Some(open) => open,
        None => slot.busy,
    }
}

// ---------------------------------------------------------------- the daemon

/// The daemon's watch on the settings file (every second) and on the
/// queue of sessions to restart after their turn.
pub struct Watcher {
    state: PathBuf,
    exe: String,
    /// What each settings file said last: the daemon's and every file a
    /// running session reads.
    last: HashMap<PathBuf, Snapshot>,
    next: Instant,
    /// Restarts started for a queued session, so one is not started twice.
    started: HashMap<String, Instant>,
}

const WATCH_EVERY: Duration = Duration::from_secs(1);

impl Watcher {
    pub fn new(state: &Path) -> Watcher {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "cc-moss".into());
        Watcher {
            state: state.to_path_buf(),
            exe,
            last: HashMap::new(),
            next: Instant::now(),
            started: HashMap::new(),
        }
    }

    pub fn tick(&mut self, now: Instant, log: &dyn Fn(&str)) {
        if now < self.next {
            return;
        }
        self.next = now + WATCH_EVERY;
        let db = self.state.join("registry.db");
        if !db.exists() {
            return;
        }
        let Some(own) = settings_path() else {
            return;
        };
        let registry = match Registry::open(&db) {
            Ok(r) => r,
            Err(e) => {
                log(&format!("provider: error: {e:#}"));
                return;
            }
        };
        let slots = registry.slots().unwrap_or_default();
        let mut paths: Vec<PathBuf> = vec![own.clone()];
        for slot in &slots {
            if let Some(path) = settings_of(slot) {
                if !paths.contains(&path)
                    && slot.effective_status(proc::is_alive) == Status::Running
                {
                    paths.push(path);
                }
            }
        }
        let mut seen_now: HashMap<PathBuf, Snapshot> = HashMap::new();
        for path in paths {
            if let Some(s) = Snapshot::read(&path) {
                seen_now.insert(path, s);
            }
        }
        let Some(snapshot) = seen_now.get(&own).cloned() else {
            return;
        };
        let first = self.last.is_empty();
        let changed = !first
            && seen_now
                .iter()
                .any(|(p, s)| self.last.get(p).is_some_and(|l| l != s));
        if first || changed || seen_now.keys().any(|p| !self.last.contains_key(p)) {
            if let Err(e) = self.settings_changed(&registry, &snapshot, &seen_now, changed, log) {
                log(&format!("provider: error: {e:#}"));
                return;
            }
            self.last = seen_now;
        }
        self.drain(&registry, log);
    }

    /// Every running session has now seen the keys the settings name; the
    /// event says which of them have to restart (not on the first look).
    fn settings_changed(
        &self,
        registry: &Registry,
        now: &Snapshot,
        files: &HashMap<PathBuf, Snapshot>,
        announce: bool,
        log: &dyn Fn(&str),
    ) -> anyhow::Result<()> {
        let mut restart_needed = Vec::new();
        for slot in registry.slots()? {
            let status = slot.effective_status(proc::is_alive);
            if status != Status::Running {
                continue;
            }
            let mut slot = slot;
            // What the file this session reads says now.
            let theirs = settings_of(&slot)
                .and_then(|p| files.get(&p))
                .unwrap_or(now);
            if let Some((_, seen)) = provider::recorded(
                slot.provider_started.as_deref(),
                slot.provider_seen_keys.as_deref(),
            ) {
                let all: BTreeSet<String> = provider::seen_with(&seen, theirs);
                let json = provider::keys_json(&all);
                if all != seen {
                    registry.set_provider_seen(&slot.slot_id, &json)?;
                }
                slot.provider_seen_keys = Some(json);
            }
            if !slot.is_moss() && restart_reason(&slot, status, Some(now)).is_some() {
                restart_needed.push(slot.slot_id.clone());
            }
        }
        if announce {
            let detail = json!({
                "host": now.host,
                "live": restart_needed.is_empty(),
                "restart_needed": restart_needed,
            });
            registry.record_global("provider-changed", &detail, now_ms())?;
            log(&format!(
                "provider: settings changed; {} session(s) need a restart",
                restart_needed.len()
            ));
        }
        Ok(())
    }

    /// Restart the queued sessions whose turn is over; drop the ones that
    /// stopped running.
    fn drain(&mut self, registry: &Registry, log: &dyn Fn(&str)) {
        let queue = read_queue(&self.state);
        if queue.is_empty() {
            return;
        }
        let Ok(slots) = registry.slots() else {
            return;
        };
        let settings = current();
        self.started
            .retain(|_, at| at.elapsed() < Duration::from_secs(60));
        for q in queue {
            let id = q.slot_id;
            let slot = slots.iter().find(|s| s.slot_id == id);
            let running =
                slot.is_some_and(|s| s.effective_status(proc::is_alive) == Status::Running);
            let Some(slot) = slot.filter(|_| running) else {
                let _ = dequeue(&self.state, &id);
                continue;
            };
            // The switch reached it meanwhile (another restart, a switch
            // back): nothing left to do.
            if q.recheck && restart_reason(slot, Status::Running, settings.as_ref()).is_none() {
                let _ = dequeue(&self.state, &id);
                continue;
            }
            if in_turn(slot) || self.started.contains_key(&id) {
                continue;
            }
            self.started.insert(id.clone(), Instant::now());
            let exe = self.exe.clone();
            let session = slot.session_id.clone();
            let state = self.state.clone();
            log(&format!("provider: restarting {id} after its turn"));
            // `restart` takes it off the queue once it is back; a refusal
            // counts, and after the last one the user is told.
            std::thread::spawn(move || {
                let out = moss_core::command::run_with_timeout(
                    Command::new(exe).args(["restart", "--json", "--", &session]),
                    Duration::from_secs(60),
                );
                let result = out
                    .ok()
                    .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
                    .and_then(|v| v["results"][0]["result"].as_str().map(String::from));
                if matches!(result.as_deref(), Some("restarted" | "queued")) {
                    return;
                }
                if failed_attempt(&state, &id).unwrap_or(false) {
                    if let Ok(r) = Registry::open(&state.join("registry.db")) {
                        let _ = r.record(
                            &id,
                            "restart-failed",
                            &json!({ "attempts": MAX_ATTEMPTS }),
                            now_ms(),
                        );
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::event::{Context, Event, StartSource};

    #[test]
    fn restart_queue_gives_up_after_three_failures() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path();
        enqueue(state, "a", false).unwrap();
        enqueue(state, "a", false).unwrap();
        enqueue(state, "b", true).unwrap();
        assert_eq!(queued(state), ["a", "b"], "once each");
        assert!(!failed_attempt(state, "a").unwrap());
        assert!(!failed_attempt(state, "a").unwrap());
        assert!(failed_attempt(state, "a").unwrap(), "the third is the last");
        assert_eq!(queued(state), ["b"]);
        // A name asked for keeps it from being dropped on a recheck.
        enqueue(state, "b", false).unwrap();
        assert!(!read_queue(state)[0].recheck);
    }

    #[test]
    fn restart_queue_drops_what_no_longer_needs_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().to_path_buf();
        let mut r = Registry::open(&state.join("registry.db")).unwrap();
        // Two Claude Codes: two live processes.
        let mut kids: Vec<std::process::Child> = (0..2)
            .map(|_| {
                std::process::Command::new("sleep")
                    .arg("30")
                    .spawn()
                    .unwrap()
            })
            .collect();
        for (sid, kid) in ["stale", "asked"].iter().zip(&kids) {
            let pid = kid.id();
            let ctx = Context {
                pid: Some(pid),
                pid_start: proc::start_time(pid),
                tmux: None,
                launch_args: None,
                now_ms: now_ms(),
            };
            r.apply(
                &Event::Start {
                    source: StartSource::Startup,
                    session_id: sid.to_string(),
                    cwd: "/w".into(),
                    title: Some(sid.to_string()),
                    transcript_path: None,
                },
                &ctx,
            )
            .unwrap();
            // Busy: nothing is started from a unit test.
            r.apply(
                &Event::Prompt {
                    session_id: sid.to_string(),
                    title: None,
                    prompt: Some("go".into()),
                },
                &ctx,
            )
            .unwrap();
        }
        let id = |sid: &str| {
            r.slots()
                .unwrap()
                .into_iter()
                .find(|s| s.session_id == sid)
                .unwrap()
                .slot_id
        };
        enqueue(&state, &id("stale"), true).unwrap();
        enqueue(&state, &id("asked"), false).unwrap();
        let mut w = Watcher::new(&state);
        w.drain(&r, &|_| {});
        // No provider recorded: no switch to wait for, so the rechecked one
        // goes; the one asked for by name waits for its turn to end.
        assert_eq!(queued(&state), [id("asked")]);
        for kid in &mut kids {
            let _ = kid.kill();
            let _ = kid.wait();
        }
    }

    #[test]
    fn a_closed_turn_beats_a_stale_busy_mark() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("t.jsonl");
        std::fs::write(
            &transcript,
            "{\"type\":\"system\",\"subtype\":\"turn_duration\"}\n",
        )
        .unwrap();
        let mut r = Registry::open(&dir.path().join("registry.db")).unwrap();
        let ctx = Context {
            pid: None,
            pid_start: None,
            tmux: None,
            launch_args: None,
            now_ms: 1,
        };
        r.apply(
            &Event::Start {
                source: StartSource::Startup,
                session_id: "s".into(),
                cwd: "/w".into(),
                title: None,
                transcript_path: Some(transcript.to_string_lossy().into_owned()),
            },
            &ctx,
        )
        .unwrap();
        let mut slot = r.slots().unwrap().remove(0);
        // Esc ended the turn: no Stop hook, so the mark stayed.
        slot.busy = true;
        assert!(!in_turn(&slot));
        // No transcript to tell: the mark decides.
        slot.transcript_path = None;
        assert!(in_turn(&slot));
    }
}
