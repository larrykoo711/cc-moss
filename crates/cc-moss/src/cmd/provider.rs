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

/// Resolve the target using the same user settings the resumed process will read.
/// Project and local provider overrides are refused until their source is recorded.
pub fn model_plan(slot: &Slot) -> anyhow::Result<(provider::ModelPlan, PathBuf)> {
    if !Path::new(&slot.cwd).is_absolute() || slot.cwd.chars().any(char::is_control) {
        anyhow::bail!("the session's working directory is not safe to replay");
    }
    let path = settings_of(slot)
        .or_else(settings_path)
        .ok_or_else(|| anyhow::anyhow!("the session's settings folder is unknown"))?;
    if !path.is_absolute() || path.to_string_lossy().chars().any(char::is_control) {
        anyhow::bail!("the settings path is not safe to replay");
    }
    let args = slot
        .pid
        .and_then(moss_core::launch_args::argv)
        .unwrap_or_default();
    if args.iter().chain(slot.launch_args.iter()).any(|a| {
        matches!(
            a.split('=').next(),
            Some("--settings" | "--setting-sources")
        )
    }) {
        anyhow::bail!("custom settings arguments cannot be replayed safely; select the model in the session instead");
    }
    for dir in Path::new(&slot.cwd).ancestors() {
        for name in ["settings.json", "settings.local.json"] {
            let project = dir.join(".claude").join(name);
            if project == path {
                continue;
            }
            let snapshot = Snapshot::read(&project).ok_or_else(|| {
                anyhow::anyhow!("project or local settings are unreadable or invalid")
            })?;
            if !snapshot.keys.is_empty() {
                anyhow::bail!("project or local settings override the provider; select the model in the session instead");
            }
        }
    }
    let snapshot = Snapshot::read(&path)
        .ok_or_else(|| anyhow::anyhow!("the session's settings are unreadable or invalid"))?;
    let uncertain = slot
        .provider_started
        .as_deref()
        .and_then(Snapshot::from_json)
        .is_none();
    Ok((snapshot.model_plan(uncertain), path))
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

/// 配置文件元数据版本；包括缺失source，创建/删除也会改变epoch。
/// 不读取或散列认证值，模型响应不能据此证明endpoint或凭据已生效。
pub fn evidence_epoch(slot: &Slot) -> Option<String> {
    let user = settings_of(slot).or_else(settings_path)?;
    if !user.is_absolute() || !Path::new(&slot.cwd).is_absolute() {
        return None;
    }
    let mut sources = BTreeSet::from([user]);
    for dir in Path::new(&slot.cwd).ancestors() {
        for name in ["settings.json", "settings.local.json"] {
            sources.insert(dir.join(".claude").join(name));
        }
    }
    let managed = std::env::var_os("CC_MOSS_CLAUDE_MANAGED_SETTINGS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(if cfg!(target_os = "macos") {
                "/Library/Application Support/ClaudeCode/managed-settings.json"
            } else {
                "/etc/claude-code/managed-settings.json"
            })
        });
    sources.insert(managed);
    let mut epochs = Vec::new();
    for path in sources {
        let stamp = match moss_core::transcript::request_file_stamp(&path) {
            Ok(stamp) => stamp,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing".to_owned(),
            Err(_) => return None,
        };
        epochs.push((path.to_str()?.to_owned(), stamp));
    }
    serde_json::to_string(&epochs).ok()
}

/// 一次加载证据与库外fence；缓存丢失时绝不复用主库中的旧成功。
pub fn current_model_evidence(
    registry: &Registry,
    state: &Path,
) -> anyhow::Result<HashMap<String, moss_core::transcript::ModelEvidence>> {
    let cache: HashMap<String, serde_json::Value> = std::fs::read(state.join("request-scan.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut evidence = registry.model_evidence()?;
    evidence.retain(|id, e| {
        cache.get(id).is_some_and(|c| {
            c["session"] == e.session_id
                && c["path"] == e.path
                && c["generation"] == e.generation
                && c["config_epoch"] == e.epoch
                && c["fence_ms"].as_i64() == Some(e.fence_ms)
                && c["watermark"]
                    .as_u64()
                    .is_some_and(|watermark| e.position >= watermark)
                && moss_core::transcript::request_file_stamp(Path::new(&e.path))
                    .ok()
                    .as_deref()
                    == c["stamp"].as_str()
        })
    });
    Ok(evidence)
}

/// 所有自动待应用入口共享这一判定；明确手动请求不经过它。
pub fn effective_reason(
    slot: &Slot,
    status: Status,
    now: Option<&Snapshot>,
    evidence: Option<&moss_core::transcript::ModelEvidence>,
) -> Option<&'static str> {
    let reason = restart_reason(slot, status, now)?;
    if reason != "model" {
        return Some(reason);
    }
    let valid = evidence.is_some_and(|e| {
        e.session_id == slot.session_id
            && slot.transcript_path.as_deref() == Some(e.path.as_str())
            && e.at_ms > e.fence_ms
            && e.at_ms <= now_ms()
            && evidence_epoch(slot).as_deref() == Some(e.epoch.as_str())
            && model_plan(slot)
                .ok()
                .and_then(|(plan, _)| plan.model)
                .as_deref()
                == Some(e.model.as_str())
            && evidence_epoch(slot).as_deref() == Some(e.epoch.as_str())
    });
    (!valid).then_some(reason)
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
    pub provider_default: bool,
    /// Claimed under the queue lock before spawning, so cancellation cannot race it.
    pub executing: bool,
    pub execution_until_ms: i64,
}

impl Queued {
    fn from_json(v: &serde_json::Value) -> Option<Queued> {
        Some(Queued {
            slot_id: v["slot_id"].as_str()?.to_string(),
            recheck: v["recheck"].as_bool().unwrap_or(false),
            attempts: v["attempts"].as_u64().unwrap_or(0) as u32,
            provider_default: v["provider_default"].as_bool().unwrap_or(false),
            executing: v["executing"].as_bool().unwrap_or(false),
            execution_until_ms: v["execution_until_ms"].as_i64().unwrap_or(0),
        })
    }

    fn executing(&self) -> bool {
        self.executing && self.execution_until_ms > now_ms()
    }

    fn to_json(&self) -> serde_json::Value {
        json!({ "slot_id": self.slot_id, "recheck": self.recheck, "attempts": self.attempts, "provider_default": self.provider_default, "executing": self.executing, "execution_until_ms": self.execution_until_ms })
    }
}

/// The daemon gives up on a queued session after this many failures.
pub const MAX_ATTEMPTS: u32 = 3;
/// Twice the worker's hard timeout, so an abandoned daemon claim can expire
/// without allowing cancellation while a bounded worker is still starting.
const QUEUE_WORKER_WAIT: Duration = Duration::from_secs(60);
const QUEUE_EXECUTION_MS: i64 = 120_000;

fn read_queue(state: &Path) -> Vec<Queued> {
    std::fs::read_to_string(queue_path(state))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<serde_json::Value>>(&t).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(Queued::from_json)
        .collect()
}

/// Only pending slot ids; execution ownership is not a queued operation.
pub fn queued(state: &Path) -> Vec<String> {
    read_queue(state)
        .into_iter()
        .filter(|q| !q.executing())
        .map(|q| q.slot_id)
        .collect()
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

/// Put a session in the queue; the latest explicit request picks its strategy.
#[cfg(test)]
pub fn enqueue(state: &Path, slot_id: &str, recheck: bool) -> std::io::Result<()> {
    enqueue_policy(state, slot_id, recheck, false)
}

pub fn enqueue_policy(
    state: &Path,
    slot_id: &str,
    recheck: bool,
    provider_default: bool,
) -> std::io::Result<()> {
    with_queue(state, |all| {
        if let Some(q) = all.iter_mut().find(|q| q.slot_id == slot_id) {
            if q.executing() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "restart is already in progress",
                ));
            }
            q.recheck = recheck;
            q.provider_default = provider_default;
            q.attempts = 0;
        } else {
            all.push(Queued {
                slot_id: slot_id.to_string(),
                recheck,
                attempts: 0,
                provider_default,
                executing: false,
                execution_until_ms: 0,
            });
        }
        Ok(())
    })?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    Removed,
    NotQueued,
    InProgress,
}

/// Cancel only pending work, under the same lock used to claim execution.
pub fn cancel(state: &Path, slot_id: &str) -> std::io::Result<CancelOutcome> {
    with_queue(state, |all| {
        if all.iter().any(|q| q.slot_id == slot_id && q.executing()) {
            return CancelOutcome::InProgress;
        }
        let before = all.len();
        all.retain(|q| q.slot_id != slot_id);
        if all.len() == before {
            CancelOutcome::NotQueued
        } else {
            CancelOutcome::Removed
        }
    })
}

fn claim_queue(state: &Path, slot_id: &str) -> std::io::Result<Option<Queued>> {
    with_queue(state, |all| {
        let q = all.iter_mut().find(|q| q.slot_id == slot_id)?;
        if q.executing() {
            return None;
        }
        q.executing = true;
        q.execution_until_ms = now_ms() + QUEUE_EXECUTION_MS;
        Some(q.clone())
    })
}

/// Direct and daemon restarts share the same atomic execution ownership.
/// Drop removes only this claim, never a later queued request.
pub struct DirectExecution {
    state: PathBuf,
    claim: Queued,
}

impl Drop for DirectExecution {
    fn drop(&mut self) {
        if let Err(error) = with_queue(&self.state, |all| all.retain(|q| q != &self.claim)) {
            eprintln!("cc-moss: could not release restart claim: {error}");
        }
    }
}

pub fn claim_direct(
    state: &Path,
    slot_id: &str,
    provider_default: bool,
) -> std::io::Result<Option<DirectExecution>> {
    let claim = with_queue(state, |all| {
        if all.iter().any(|q| q.slot_id == slot_id && q.executing()) {
            return None;
        }
        let claim = Queued {
            slot_id: slot_id.to_string(),
            recheck: false,
            attempts: 0,
            provider_default,
            executing: true,
            execution_until_ms: now_ms() + QUEUE_EXECUTION_MS,
        };
        all.retain(|q| q.slot_id != slot_id);
        all.push(claim.clone());
        Some(claim)
    })?;
    // Construct the guard only after persistence succeeded and the lock was
    // released. A write error must never drop a guard while holding that lock.
    Ok(claim.map(|claim| DirectExecution {
        state: state.to_path_buf(),
        claim,
    }))
}

pub fn queue_claimed(state: &Path, slot_id: &str) -> bool {
    read_queue(state)
        .iter()
        .any(|q| q.slot_id == slot_id && q.executing())
}

pub fn dequeue(state: &Path, slot_id: &str) -> std::io::Result<()> {
    with_queue(state, |all| all.retain(|q| q.slot_id != slot_id))
}

/// Terminal exit failures must not turn into repeated signals. Compare the
/// entire claim so a late worker cannot remove a later explicit request.
fn finish_terminal_failure(state: &Path, claim: &Queued) -> std::io::Result<bool> {
    with_queue(state, |all| {
        let before = all.len();
        all.retain(|q| q != claim);
        all.len() != before
    })
}

fn terminal_failure(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some(
            "quit-timeout"
                | "signal-failed"
                | "process-unknown"
                | "cwd_forbidden"
                | "cwd_missing"
                | "cwd_not_directory"
                | "cwd_unreadable"
                | "cwd_unavailable"
                | "cwd_invalid"
                | "cwd_changed"
        )
    )
}

/// A queued restart didn't work; true when that was the last try (it left
/// the queue).
pub fn failed_attempt(state: &Path, slot_id: &str) -> std::io::Result<bool> {
    with_queue(state, |all| {
        let Some(q) = all.iter_mut().find(|q| q.slot_id == slot_id) else {
            return false;
        };
        q.executing = false;
        q.execution_until_ms = 0;
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
        let evidence = current_model_evidence(registry, &self.state)?;
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
            if !slot.is_moss()
                && effective_reason(&slot, status, Some(now), evidence.get(&slot.slot_id)).is_some()
            {
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
        let evidence = current_model_evidence(registry, &self.state).unwrap_or_default();
        self.started
            .retain(|_, at| at.elapsed() < Duration::from_secs(60));
        for q in queue {
            if q.executing() {
                continue;
            }
            let id = q.slot_id.clone();
            let slot = slots.iter().find(|s| s.slot_id == id);
            let running =
                slot.is_some_and(|s| s.effective_status(proc::is_alive) == Status::Running);
            let Some(slot) = slot.filter(|_| running) else {
                let _ = with_queue(&self.state, |all| all.retain(|current| current != &q));
                continue;
            };
            // The switch reached it meanwhile (another restart, a switch
            // back): nothing left to do.
            if q.recheck
                && effective_reason(
                    slot,
                    Status::Running,
                    settings.as_ref(),
                    evidence.get(&slot.slot_id),
                )
                .is_none()
            {
                let _ = with_queue(&self.state, |all| all.retain(|current| current != &q));
                continue;
            }
            if in_turn(slot) || self.started.contains_key(&id) {
                continue;
            }
            if let Err(error) = super::working_directory::for_slot(slot) {
                log(&format!(
                    "provider: refusing queued restart: {}",
                    super::working_directory::code(&error)
                ));
                if let Err(error) =
                    with_queue(&self.state, |all| all.retain(|current| current != &q))
                {
                    log(&format!(
                        "provider: could not remove refused restart: {error}"
                    ));
                }
                continue;
            }
            let Ok(Some(claimed)) = claim_queue(&self.state, &id) else {
                continue;
            };
            self.started.insert(id.clone(), Instant::now());
            let exe = self.exe.clone();
            let session = slot.session_id.clone();
            let state = self.state.clone();
            log(&format!("provider: restarting {id} after its turn"));
            // `restart` takes it off the queue once it is back; a refusal
            // counts, and after the last one the user is told.
            std::thread::spawn(move || {
                let mut command = Command::new(exe);
                command.args(["restart", "--json", "--from-queue"]);
                if claimed.provider_default {
                    command.arg("--provider-default");
                }
                command.args(["--", &session]);
                let out = moss_core::command::run_with_timeout(&mut command, QUEUE_WORKER_WAIT);
                let outcome = out
                    .ok()
                    .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
                    .and_then(|v| v["results"].as_array()?.first().cloned());
                let result = outcome.as_ref().and_then(|v| v["result"].as_str());
                let reason = outcome.as_ref().and_then(|v| v["reason"].as_str());
                if matches!(result, Some("restarted" | "starting")) {
                    return;
                }
                if result == Some("refused") && terminal_failure(reason) {
                    if let Err(error) = finish_terminal_failure(&state, &claimed) {
                        eprintln!("cc-moss: could not finish failed restart queue entry: {error}");
                    }
                    return;
                }
                if result == Some("queued") {
                    let _ = with_queue(&state, |all| {
                        if let Some(q) = all.iter_mut().find(|q| **q == claimed) {
                            q.executing = false;
                            q.execution_until_ms = 0;
                        }
                    });
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
    fn restart_terminal_failure_queue_snapshot_and_cancel_have_three_states() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(cancel(dir.path(), "a").unwrap(), CancelOutcome::NotQueued);
        enqueue_policy(dir.path(), "a", false, true).unwrap();
        assert_eq!(queued(dir.path()), ["a"]);
        let claim = claim_queue(dir.path(), "a").unwrap().unwrap();
        assert!(queued(dir.path()).is_empty(), "executing is not pending");
        assert_eq!(cancel(dir.path(), "a").unwrap(), CancelOutcome::InProgress);
        assert!(finish_terminal_failure(dir.path(), &claim).unwrap());
        enqueue_policy(dir.path(), "a", false, true).unwrap();
        assert_eq!(cancel(dir.path(), "a").unwrap(), CancelOutcome::Removed);
        assert_eq!(cancel(dir.path(), "a").unwrap(), CancelOutcome::NotQueued);
    }

    #[test]
    fn restart_terminal_failure_removes_only_the_same_execution() {
        let dir = tempfile::tempdir().unwrap();
        enqueue_policy(dir.path(), "a", false, true).unwrap();
        let claim = claim_queue(dir.path(), "a").unwrap().unwrap();
        assert!(finish_terminal_failure(dir.path(), &claim).unwrap());
        enqueue_policy(dir.path(), "a", false, false).unwrap();
        assert!(!finish_terminal_failure(dir.path(), &claim).unwrap());
        assert_eq!(queued(dir.path()), ["a"]);
    }

    #[test]
    fn restart_terminal_failure_codes_are_not_retryable() {
        for reason in ["quit-timeout", "signal-failed", "process-unknown"] {
            assert!(terminal_failure(Some(reason)));
        }
        assert!(!terminal_failure(Some("settings")));
    }

    #[test]
    fn provider_model_queue_cancel_and_policy_are_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path();
        enqueue_policy(state, "a", false, true).unwrap();
        enqueue_policy(state, "a", false, false).unwrap();
        assert!(!read_queue(state)[0].provider_default);
        enqueue_policy(state, "a", false, true).unwrap();
        assert!(claim_queue(state, "a").unwrap().unwrap().provider_default);
        assert_eq!(cancel(state, "a").unwrap(), CancelOutcome::InProgress);
        assert!(!failed_attempt(state, "a").unwrap());
        assert_eq!(cancel(state, "a").unwrap(), CancelOutcome::Removed);
        assert!(
            claim_queue(state, "a").unwrap().is_none(),
            "a cancelled entry cannot execute"
        );
    }

    #[test]
    fn provider_model_abandoned_queue_claim_can_be_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        enqueue_policy(dir.path(), "a", false, true).unwrap();
        with_queue(dir.path(), |all| {
            all[0].executing = true;
        })
        .unwrap();
        // Old queue files have no valid live execution lease.
        assert_eq!(cancel(dir.path(), "a").unwrap(), CancelOutcome::Removed);
    }

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
