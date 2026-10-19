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

/// Whether a session has to be restarted to use the provider `now` (only a
/// running one can); `None` when it uses it already or nothing is known.
pub fn restart_reason(slot: &Slot, status: Status, now: Option<&Snapshot>) -> Option<&'static str> {
    if status != Status::Running {
        return None;
    }
    let (started, seen) = provider::recorded(
        slot.provider_started.as_deref(),
        slot.provider_seen_keys.as_deref(),
    )?;
    match provider::verdict(&started, &seen, now?) {
        Verdict::Live => None,
        Verdict::NeedsRestart(r) => Some(r.code()),
    }
}

// ---------------------------------------------------------------- the queue

/// `<state>/daemon/restart-queue.json`: slot ids of busy sessions to
/// restart once their turn is over.
pub fn queue_path(state: &Path) -> PathBuf {
    moss_core::daemon::Paths::new(state)
        .dir
        .join("restart-queue.json")
}

pub fn queued(state: &Path) -> Vec<String> {
    std::fs::read_to_string(queue_path(state))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_queue(state: &Path, ids: &[String]) -> std::io::Result<()> {
    let path = queue_path(state);
    if let Some(dir) = path.parent() {
        moss_core::store_dir::ensure_private_dir(dir)?;
    }
    moss_core::moss::home::write_if_changed(&path, &serde_json::to_string(ids)?).map(drop)
}

/// Put a session in the queue; once only.
pub fn enqueue(state: &Path, slot_id: &str) -> std::io::Result<()> {
    let mut ids = queued(state);
    if !ids.iter().any(|i| i == slot_id) {
        ids.push(slot_id.to_string());
    }
    write_queue(state, &ids)
}

pub fn dequeue(state: &Path, slot_id: &str) -> std::io::Result<()> {
    let ids: Vec<String> = queued(state).into_iter().filter(|i| i != slot_id).collect();
    write_queue(state, &ids)
}

/// Whether the session is in a turn: Claude is working, or its
/// transcript says a reply is still open.
pub fn in_turn(slot: &Slot) -> bool {
    slot.busy
        || slot
            .transcript_path
            .as_deref()
            .and_then(|p| moss_core::transcript::turn_open(Path::new(p)))
            == Some(true)
}

// ---------------------------------------------------------------- the daemon

/// The daemon's watch on the settings file (every second) and on the
/// queue of sessions to restart after their turn.
pub struct Watcher {
    state: PathBuf,
    exe: String,
    last: Option<Snapshot>,
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
            last: None,
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
        let Some(snapshot) = current() else {
            return;
        };
        let registry = match Registry::open(&db) {
            Ok(r) => r,
            Err(e) => {
                log(&format!("provider: error: {e:#}"));
                return;
            }
        };
        let changed = self.last.as_ref().is_some_and(|l| *l != snapshot);
        if self.last.is_none() || changed {
            if let Err(e) = self.settings_changed(&registry, &snapshot, changed, log) {
                log(&format!("provider: error: {e:#}"));
                return;
            }
            self.last = Some(snapshot);
        }
        self.drain(&registry, log);
    }

    /// Every running session has now seen the keys the settings name; the
    /// event says which of them have to restart (not on the first look).
    fn settings_changed(
        &self,
        registry: &Registry,
        now: &Snapshot,
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
            if let Some((_, seen)) = provider::recorded(
                slot.provider_started.as_deref(),
                slot.provider_seen_keys.as_deref(),
            ) {
                let all: BTreeSet<String> = provider::seen_with(&seen, now);
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
        let queue = queued(&self.state);
        if queue.is_empty() {
            return;
        }
        let Ok(slots) = registry.slots() else {
            return;
        };
        self.started
            .retain(|_, at| at.elapsed() < Duration::from_secs(60));
        for id in queue {
            let slot = slots.iter().find(|s| s.slot_id == id);
            let running =
                slot.is_some_and(|s| s.effective_status(proc::is_alive) == Status::Running);
            let Some(slot) = slot.filter(|_| running) else {
                let _ = dequeue(&self.state, &id);
                continue;
            };
            if in_turn(slot) || self.started.contains_key(&id) {
                continue;
            }
            self.started.insert(id.clone(), Instant::now());
            let exe = self.exe.clone();
            let session = slot.session_id.clone();
            log(&format!("provider: restarting {id} after its turn"));
            // `restart` takes it off the queue once it is back.
            std::thread::spawn(move || {
                let _ = moss_core::command::run_with_timeout(
                    Command::new(exe).args(["restart", "--json", "--", &session]),
                    Duration::from_secs(60),
                );
            });
        }
    }
}
