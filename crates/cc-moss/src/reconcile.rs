//! Bringing the registry up to date before showing it. Runs automatically
//! whenever the picker or `ls` opens, so sessions the hooks never saw
//! (started before `cc-moss setup`, or resumed without a name in the
//! payload) still show up correctly.

use anyhow::Result;
use moss_core::agent::claude::{self, LiveSession};
use moss_core::config::Config;
use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;
use moss_core::slot::now_ms;
use moss_core::tmux::{locate_in_panes, Socket, Tmux};
use moss_core::{launch_args, proc, store_dir};

/// Open the registry and repair what hooks could not record.
pub fn run() -> Result<Registry> {
    let mut registry = Registry::open(&store_dir::registry_path())?;
    let claude_bin = std::env::var("CC_MOSS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let live = claude::live_sessions(&claude_bin);
    if !live.is_empty() {
        adopt(&mut registry, &live)?;
    }
    registry.backfill_names()?;
    Ok(registry)
}

/// Register running sessions the registry does not know (or knows under an
/// old process), and refresh whether each is busy.
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
        let slot_id = match tracked {
            Some(k) => k.slot_id.clone(),
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
                match registry.slot_for_event(&s.session_id, &ctx)? {
                    Some(slot) => slot.slot_id,
                    None => continue,
                }
            }
        };
        registry.set_busy(&slot_id, s.busy)?;
    }
    Ok(())
}

fn modified_ms(path: &str) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}
