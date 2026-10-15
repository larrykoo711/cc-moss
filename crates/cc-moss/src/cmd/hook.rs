//! `cc-moss hook <event>`: called by Claude Code on every session event.
//!
//! Contract with the agent: never fail, stay fast, and print nothing to stdout
//! except one `systemMessage` JSON on SessionStart outside tmux. Any error is
//! appended to `hook.log` and swallowed.

use std::io::{Read, Write};

use anyhow::{anyhow, Result};
use moss_core::agent::claude::ClaudeAgent;
use moss_core::agent::{Agent, HookKind};
use moss_core::config::Config;
use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, rfc3339};
use moss_core::tmux::{Socket, Tmux};
use moss_core::{launch_args, naming, proc, store_dir, tmux};

const LOG_MAX_BYTES: u64 = 1 << 20;

pub fn run(event: &str) {
    let outcome = std::panic::catch_unwind(|| handle(event));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log(event, &format!("error: {e:#}")),
        Err(_) => log(event, "error: panic"),
    }
}

fn handle(event: &str) -> Result<()> {
    let kind = HookKind::from_cli_name(event).ok_or_else(|| anyhow!("unknown hook event"))?;
    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    if !is_interactive() {
        // `claude -p` and SDK sessions fire the same hooks (F10) but are not
        // something a person would want restored.
        log(kind.cli_name(), "skipped: non-interactive session");
        return Ok(());
    }
    let event = ClaudeAgent.parse_hook(kind, &payload)?;

    let pid = env("CLAUDE_PID").and_then(|p| p.parse::<u32>().ok());
    if pid.is_none() {
        log(
            kind.cli_name(),
            "warning: CLAUDE_PID missing; liveness checks are disabled for this session",
        );
    }
    let is_start = kind == HookKind::SessionStart;
    let ctx = Context {
        pid,
        // Every event carries the start time so a lost SessionStart can be repaired.
        pid_start: pid.and_then(proc::start_time),
        tmux: if is_start {
            tmux::locate(env("TMUX").as_deref(), env("TMUX_PANE").as_deref())
        } else {
            None
        },
        launch_args: if is_start {
            pid.map(launch_args::capture)
        } else {
            None
        },
        now_ms: now_ms(),
    };
    let mut registry = Registry::open(&store_dir::registry_path())?;
    registry.apply_with(&event, &ctx, &proc::is_alive)?;

    // Side effects after the registry is committed; failures are only logged.
    match &event {
        Event::Start {
            source, session_id, ..
        } => {
            if let Err(e) = sync_tmux_name(&registry, session_id, &ctx) {
                log(kind.cli_name(), &format!("tmux rename failed: {e:#}"));
            }
            let fresh = matches!(source, StartSource::Startup | StartSource::Resume);
            if fresh && ctx.tmux.is_none() {
                warn_outside_tmux();
            }
        }
        Event::Prompt {
            session_id, title, ..
        } => {
            if let Err(e) = sync_tmux_name(&registry, session_id, &ctx) {
                log(kind.cli_name(), &format!("tmux rename failed: {e:#}"));
            }
            if let Some(name) = title.as_deref().filter(|t| !t.is_empty()) {
                warn_if_name_taken(&registry, session_id, &ctx, name)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Keep the tmux session name equal to `<prefix>_<Claude session name>`
/// (R7: the Claude name is the single source of truth). Only the primary
/// slot of a tmux session renames it, and only by session id.
fn sync_tmux_name(registry: &Registry, session_id: &str, ctx: &Context) -> Result<()> {
    let Some(slot) = registry.slot_for_event(session_id, ctx)? else {
        return Ok(());
    };
    let (Some(loc), Some(name)) = (slot.tmux.as_ref(), slot.name.as_deref()) else {
        return Ok(());
    };
    let Some(tmux_id) = loc.session_id.as_deref() else {
        return Ok(());
    };
    if !registry.is_tmux_primary(&slot)? {
        return Ok(());
    }
    let config = Config::load().unwrap_or_else(|e| {
        log("config", &format!("error: {e:#}; using defaults"));
        Config::default()
    });
    let prefix = if config.naming.host_prefix {
        naming::cached_host_prefix(&store_dir::state_dir())
    } else {
        None
    };
    let tmux = Tmux::with_socket(Socket::Path(loc.socket.clone()));
    // One tmux call gives both our current name and every name in use.
    let sessions = tmux.sessions();
    let Some(current) = sessions
        .iter()
        .find(|(id, _)| id == tmux_id)
        .map(|(_, name)| name.clone())
    else {
        return Ok(());
    };
    let taken: Vec<String> = sessions.into_iter().map(|(_, name)| name).collect();
    let desired = naming::session_name(prefix.as_deref(), name);
    let target = naming::unique(&desired, &taken, Some(&current));
    if target != current {
        tmux.rename_session(tmux_id, &target)?;
    }
    registry.set_tmux_session_name(&slot.slot_id, &target)
}

/// Names are unique (T1.5), but `/rename` cannot be stopped: say so instead.
fn warn_if_name_taken(
    registry: &Registry,
    session_id: &str,
    ctx: &Context,
    name: &str,
) -> Result<()> {
    let Some(slot) = registry.slot_for_event(session_id, ctx)? else {
        return Ok(());
    };
    if registry.name_taken_by_other(&slot.slot_id, name)? {
        let msg = serde_json::json!({
            "systemMessage": format!(
                "cc-moss: another session is already named `{name}`. Names must be unique: /rename this one."
            )
        });
        println!("{msg}");
    }
    Ok(())
}

/// A JSON `systemMessage` is the only thing a hook prints; Claude Code shows it to the user.
fn warn_outside_tmux() {
    let msg = serde_json::json!({
        "systemMessage": "cc-moss: this session is not running in tmux, so closing the terminal ends it. \
    It is still recorded, and `cc-moss restore` can bring it back. Start sessions with `cc-moss new` to keep them alive."
    });
    println!("{msg}");
}

/// Interactive sessions have `CLAUDE_CODE_SESSION_ATTENDED=1`; `claude -p`
/// and SDK sessions have `0` (F10). Claude Code recomputes this variable for
/// every process, whereas `CLAUDE_CODE_ENTRYPOINT` is inherited when already
/// set (e.g. from a tmux server first started by an agent), so the entry point
/// is only consulted when ATTENDED is missing. Unknown counts as interactive
/// so a real session is never dropped.
fn is_interactive() -> bool {
    match env("CLAUDE_CODE_SESSION_ATTENDED") {
        Some(attended) => attended != "0",
        None => env("CLAUDE_CODE_ENTRYPOINT").map_or(true, |e| e == "cli"),
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn log(event: &str, message: &str) {
    let path = store_dir::hook_log_path();
    // One write per line so concurrent hooks do not interleave.
    let line = format!("{} {event} {message}\n", rfc3339(now_ms()));
    let _ = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::metadata(&path)
            .map(|m| m.len() > LOG_MAX_BYTES)
            .unwrap_or(false)
        {
            std::fs::remove_file(&path)?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?
            .write_all(line.as_bytes())
    })();
}
