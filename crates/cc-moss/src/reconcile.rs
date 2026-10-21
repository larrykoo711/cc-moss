//! Bringing the registry up to date before showing it. Runs automatically
//! whenever the picker or `ls` opens, so sessions the hooks never saw
//! (started before `cc-moss setup`, or resumed without a name in the
//! payload) still show up correctly.

use std::collections::HashMap;

use anyhow::Result;
use moss_core::agent::claude::{self, LiveSession};
use moss_core::config::Config;
use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::{Registry, EVENT_RETENTION_MS};
use moss_core::slot::{now_ms, Status};
use moss_core::tmux::{locate_in_panes, Socket, Tmux};
use moss_core::{launch_args, proc, store_dir};

/// Open the registry and repair what hooks could not record.
pub fn run() -> Result<Registry> {
    let mut registry = Registry::open(&store_dir::registry_path())?;
    // Moss reads only: what it sees is what was recorded.
    if registry.read_only() {
        return Ok(registry);
    }
    if registry.newer_schema().is_some() {
        eprintln!(
            "{}",
            tr!(
                "cc-moss: this registry was written by a newer cc-moss; showing it read-only. Upgrade cc-moss.",
                "cc-moss：这份会话记录是更新版本的 cc-moss 写的，先只读展示。请升级 cc-moss。"
            )
        );
        return Ok(registry);
    }
    // Asking Claude Code which sessions run starts a process (a few hundred
    // ms); the app refreshes far more often than sessions appear unseen.
    if adoption_due() {
        let claude_bin = std::env::var("CC_MOSS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
        let live = claude::live_sessions(&claude_bin);
        if !live.is_empty() {
            adopt(&mut registry, &live)?;
        }
    }
    crate::cmd::api_failure::refresh(&registry, &store_dir::state_dir())?;
    let mut scanned = load_scanned();
    let before = scanned.clone();
    let renamed = registry.backfill_names(&mut scanned)?;
    // Only sessions still open are worth remembering.
    if let Ok(slots) = registry.slots() {
        let open: std::collections::HashSet<&str> = slots
            .iter()
            .filter(|s| s.status != Status::Closed)
            .map(|s| s.slot_id.as_str())
            .collect();
        scanned.retain(|id, _| open.contains(id.as_str()));
    }
    if scanned != before {
        save_scanned(&scanned);
    }
    for slot_id in renamed {
        // The panel's rename goes through `/rename`, which fires no hook: the
        // tmux name follows here instead.
        if let Some(slot) = registry.slots()?.into_iter().find(|s| s.slot_id == slot_id) {
            let _ = crate::cmd::hook::sync_tmux_name_of(&registry, &slot);
        }
    }
    label_tmux(&registry);
    registry.close_superseded_duplicates(&|s| s.effective_status(proc::is_alive), now_ms())?;
    record_lost(&registry)?;
    registry.prune_events_batched(now_ms() - EVENT_RETENTION_MS, PRUNE_SLACK_MS)?;
    Ok(registry)
}

/// Give every running session's tmux session its own name for terminal
/// titles where it is missing or out of date (sessions from before titles,
/// and idle ones that fire no hook). One tmux call per server, then only the
/// ones that differ; by session id, the slot that owns the tmux session
/// only. Moss's server is the keeper's. Nothing goes to the registry.
fn label_tmux(registry: &Registry) {
    let Ok(slots) = registry.slots() else {
        return;
    };
    let moss_sockets: Vec<&str> = slots
        .iter()
        .filter(|s| s.is_moss())
        .filter_map(|s| s.tmux.as_ref().map(|t| t.socket.as_str()))
        .collect();
    let mut servers: std::collections::HashMap<
        String,
        Option<std::collections::HashMap<String, String>>,
    > = std::collections::HashMap::new();
    for slot in &slots {
        if slot.is_moss() || slot.status != Status::Running {
            continue;
        }
        let (Some(loc), Some(name)) = (slot.tmux.as_ref(), slot.name.as_deref()) else {
            continue;
        };
        let Some(id) = loc.session_id.as_deref() else {
            continue;
        };
        if moss_sockets.contains(&loc.socket.as_str())
            || !registry.is_tmux_primary(slot).unwrap_or(false)
        {
            continue;
        }
        let tmux = Tmux::with_socket(Socket::Path(loc.socket.clone()));
        let shown = servers
            .entry(loc.socket.clone())
            .or_insert_with(|| tmux.display_names());
        let Some(shown) = shown else {
            continue;
        };
        // Only a session tmux has, and only when it says something else.
        if shown.get(id).is_some_and(|now| now != name) {
            let _ = tmux.set_display_name(id, name);
        }
    }
}

/// How often to ask Claude Code for running sessions the hooks never saw.
const ADOPT_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// True at most once per [`ADOPT_EVERY`], marked by a stamp file's time.
/// `CC_MOSS_ADOPT_ALWAYS` (tests) asks every time.
fn adoption_due() -> bool {
    if std::env::var_os("CC_MOSS_ADOPT_ALWAYS").is_some() {
        return true;
    }
    let stamp = store_dir::state_dir().join("adopt.stamp");
    let fresh = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < ADOPT_EVERY);
    if fresh {
        return false;
    }
    let _ = std::fs::write(&stamp, b"");
    true
}

/// Old events go an hour's worth at a time, not on every look.
const PRUNE_SLACK_MS: i64 = 60 * 60 * 1000;

/// How far each transcript was read for names, kept beside the registry
/// rather than in it (see `Registry::backfill_names`).
fn scanned_path() -> std::path::PathBuf {
    store_dir::state_dir().join("name-scan.json")
}

fn load_scanned() -> HashMap<String, u64> {
    std::fs::read(scanned_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Best effort: losing it only means reading some transcript again.
fn save_scanned(scanned: &HashMap<String, u64>) {
    let path = scanned_path();
    // Its own temporary file: two `ls` at once don't trip on each other.
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    if let Ok(bytes) = serde_json::to_vec(scanned) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Report running slots whose process is gone without a SessionEnd
/// (SIGKILL, power loss) as interrupted, once each.
fn record_lost(registry: &Registry) -> Result<()> {
    let now = now_ms();
    for slot in registry.slots()? {
        if slot.status == Status::Running
            && slot.effective_status(proc::is_alive) == Status::Interrupted
        {
            registry.record_lost_of(&slot.slot_id, slot.pid, now)?;
        }
    }
    Ok(())
}

/// Register running sessions the registry does not know (or knows under an
/// old process), and clear "working" marks that went stale.
fn adopt(registry: &mut Registry, live: &[LiveSession]) -> Result<()> {
    let known = registry.slots()?;
    let socket = Socket::from_config(Config::load().ok().and_then(|c| c.tmux_socket()).as_deref());
    let tmux = Tmux::with_socket(socket);
    let panes = tmux.panes();
    let socket_path = tmux.socket_path().unwrap_or_default();
    let parents = proc::parents();
    for s in live {
        let tracked = known
            .iter()
            .find(|k| k.session_id == s.session_id && k.pid == Some(s.pid));
        match tracked {
            // The hooks know this one: only correct a busy mark that no Stop
            // hook cleared (Claude Code sends none when the user hits Esc).
            Some(k) => {
                if k.busy && !still_working(s, k.transcript_path.as_deref(), k.last_active_at_ms) {
                    registry.set_busy(&k.slot_id, false)?;
                }
            }
            None => {
                let argv = launch_args::argv(s.pid).unwrap_or_default();
                let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
                // A plugin's or script's `claude -p` (summarizers run one
                // every few minutes): the hooks skip those, so must this.
                if launch_args::is_headless(&argv) {
                    continue;
                }
                let transcript = claude::transcript_path(&s.session_id);
                let ctx = Context {
                    pid: Some(s.pid),
                    pid_start: proc::start_time(s.pid),
                    tmux: locate_in_panes(s.pid, &parents, &panes, &socket_path),
                    launch_args: Some(launch_args::whitelist(&argv)),
                    // Last activity is when Claude last wrote the
                    // transcript, not when cc-moss noticed the session.
                    now_ms: transcript
                        .as_deref()
                        .and_then(modified_ms)
                        .unwrap_or_else(now_ms),
                };
                let event = Event::Start {
                    source: StartSource::Resume,
                    session_id: s.session_id.clone(),
                    cwd: s.cwd.clone(),
                    title: s.name.clone(),
                    transcript_path: transcript.clone(),
                };
                registry.apply_with(&event, &ctx, &proc::is_alive)?;
                if let Some(slot) = registry.slot_for_event(&s.session_id, &ctx)? {
                    let busy = still_working(s, transcript.as_deref(), 0);
                    registry.set_busy(&slot.slot_id, busy)?;
                }
            }
        }
    }
    Ok(())
}

/// Whether a session is working on a turn. `claude agents` says busy also
/// while background shells run after the turn ended, so a busy report only
/// counts when the transcript, written after `since_ms`, agrees.
fn still_working(live: &LiveSession, transcript: Option<&str>, since_ms: i64) -> bool {
    if !live.busy {
        return false;
    }
    let Some(path) = transcript else {
        return true;
    };
    // Written before the mark was set: the prompt may not be recorded yet.
    if !modified_ms(path).is_some_and(|m| m >= since_ms) {
        return true;
    }
    moss_core::transcript::turn_open(std::path::Path::new(path)) != Some(false)
}

fn modified_ms(path: &str) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}
