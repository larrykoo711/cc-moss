//! Bringing the registry up to date before showing it. Runs automatically
//! whenever the picker or `ls` opens, so sessions the hooks never saw
//! (started before `cc-moss setup`, or resumed without a name in the
//! payload) still show up correctly.

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
    let claude_bin = std::env::var("CC_MOSS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let live = claude::live_sessions(&claude_bin);
    if !live.is_empty() {
        adopt(&mut registry, &live)?;
    }
    for slot_id in registry.backfill_names()? {
        // The panel's rename goes through `/rename`, which fires no hook: the
        // tmux name follows here instead.
        if let Some(slot) = registry.slots()?.into_iter().find(|s| s.slot_id == slot_id) {
            let _ = crate::cmd::hook::sync_tmux_name_of(&registry, &slot);
        }
    }
    record_lost(&registry)?;
    registry.prune_events(now_ms() - EVENT_RETENTION_MS)?;
    Ok(registry)
}

/// Report running slots whose process is gone without a SessionEnd
/// (SIGKILL, power loss) as interrupted, once each.
fn record_lost(registry: &Registry) -> Result<()> {
    let now = now_ms();
    for slot in registry.slots()? {
        if slot.status == Status::Running
            && slot.effective_status(proc::is_alive) == Status::Interrupted
        {
            registry.record_lost(&slot.slot_id, now)?;
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
                let transcript = claude::transcript_path(&s.session_id);
                let ctx = Context {
                    pid: Some(s.pid),
                    pid_start: proc::start_time(s.pid),
                    tmux: locate_in_panes(s.pid, &parents, &panes, &socket_path),
                    launch_args: Some(launch_args::capture(s.pid)),
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
