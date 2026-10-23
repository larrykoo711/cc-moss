//! `cc-moss hook <event>`: called by Claude Code on every session event.
//!
//! Contract with the agent: never fail, stay fast, and print nothing to stdout
//! except one `systemMessage` JSON on SessionStart outside tmux. Any error is
//! appended to `hook.log` and swallowed.
//!
//! `pre-tool-use` and `permission-request` are the exception to "stay fast":
//! they belong to answering questions from outside ([`super::ask_hook`]) and
//! only print a decision for an answer that arrived.

use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{anyhow, Result};
use moss_core::agent::claude::ClaudeAgent;
use moss_core::agent::{Agent, HookKind};
use moss_core::config::Config;
use moss_core::event::{Context, Event, StartSource};
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, rfc3339};
use moss_core::tmux::{Socket, Tmux};
use moss_core::{launch_args, moss, naming, proc, store_dir, tmux};

const LOG_MAX_BYTES: u64 = 1 << 20;

pub fn run(event: &str) {
    // Hooks run for Moss too and record it: they write the registry.
    moss_core::registry::as_hook();
    // The default panic hook prints to stderr, which Claude Code may show.
    // The panic itself is logged below.
    std::panic::set_hook(Box::new(|_| {}));
    // Do not build with `panic = "abort"`: the hook relies on catch_unwind
    // to exit 0 whatever happens.
    let outcome = std::panic::catch_unwind(|| handle(event));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log(event, &format!("error: {e:#}")),
        Err(_) => log(event, "error: panic"),
    }
}

fn handle(event: &str) -> Result<()> {
    // Lets the tests prove that a panic stays silent; debug builds only.
    #[cfg(debug_assertions)]
    if env("CC_MOSS_HOOK_PANIC").is_some() {
        panic!("CC_MOSS_HOOK_PANIC is set");
    }
    let kind = HookKind::from_cli_name(event).ok_or_else(|| anyhow!("unknown hook event"))?;
    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    if !is_interactive() {
        // `claude -p` and SDK sessions fire the same hooks (F10) but are not
        // something a person would want restored. PostToolUse runs after
        // every tool call: logging those would flood the log.
        if !matches!(
            kind,
            HookKind::PostToolUse
                | HookKind::Notification
                | HookKind::PreToolUse
                | HookKind::PermissionRequest
        ) {
            log(kind.cli_name(), "skipped: non-interactive session");
        }
        return Ok(());
    }
    if matches!(kind, HookKind::PreToolUse | HookKind::PermissionRequest) {
        return super::ask_hook::handle(kind, &payload);
    }
    if let Err(e) = super::ask_hook::withdraw(kind, &payload) {
        log(
            kind.cli_name(),
            &format!("withdrawing questions failed: {e:#}"),
        );
    }
    let Some(event) = ClaudeAgent.parse_hook(kind, &payload)? else {
        // A notification cc-moss does not act on (idle reminder, completion).
        return Ok(());
    };

    let pid = env("CLAUDE_PID").and_then(|p| p.parse::<u32>().ok());
    if pid.is_none() && kind != HookKind::PostToolUse {
        log(
            kind.cli_name(),
            "warning: CLAUDE_PID missing; liveness checks are disabled for this session",
        );
    }
    let is_start = kind == HookKind::SessionStart;
    // PostToolUse runs after every tool call and only clears a mark by
    // session id: it skips the process lookup (a `ps` on macOS).
    let fast = kind == HookKind::PostToolUse;
    let ctx = Context {
        pid,
        // Every other event carries the start time so a lost SessionStart can be repaired.
        pid_start: if fast {
            None
        } else {
            pid.and_then(proc::start_time)
        },
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
    if let Some(found) = registry.newer_schema() {
        // After a downgrade: leave the newer registry alone, and say so once
        // rather than on every prompt.
        log_once(
            "newer-schema",
            kind.cli_name(),
            &format!(
                "skipped: the registry was written by a newer cc-moss (schema {found}); upgrade cc-moss"
            ),
        );
        return Ok(());
    }
    let origin = if matches!(event, Event::Start { .. } | Event::End { .. }) {
        ctx.pid
            .zip(ctx.pid_start.as_deref())
            .and_then(|(pid, start)| super::process_identity::hook_origin(pid, start))
    } else {
        None
    };
    if let Some(origin) = origin {
        registry.apply_verified(&event, &ctx, &proc::is_alive, &origin)?;
    } else {
        registry.apply_with(&event, &ctx, &proc::is_alive)?;
    }
    // Moss (started by the daemon) is recorded as such: only the session
    // the daemon started, by its id. Anything else that inherited the role
    // (a window opened in Moss's tmux, a tmux server Moss's shell started,
    // `claude --resume` of Moss's id elsewhere) stays an ordinary session.
    if let Event::Start { session_id, .. } = &event {
        if is_moss_start(
            env(moss::ROLE_ENV).as_deref(),
            env(moss::SESSION_ENV).as_deref(),
            session_id,
        ) {
            if let Some(slot) = registry.slot_for_event(session_id, &ctx)? {
                registry.set_role(&slot.slot_id, moss::ROLE)?;
            }
        }
    }

    // Side effects after the registry is committed; failures are only logged.
    match &event {
        Event::Start {
            source,
            session_id,
            title,
            transcript_path,
            ..
        } => {
            if title.is_none() {
                fill_name_from_transcript(&registry, session_id, &ctx, transcript_path.as_deref());
            }
            if let Err(e) = sync_tmux_name(&registry, session_id, &ctx) {
                log(kind.cli_name(), &format!("tmux rename failed: {e:#}"));
            }
            let fresh = matches!(source, StartSource::Startup | StartSource::Resume);
            if fresh {
                if let Err(e) = record_provider(&registry, session_id, &ctx) {
                    log(kind.cli_name(), &format!("provider: {e:#}"));
                }
            }
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

/// The API provider this process starts with, from the settings Claude Code
/// itself reads (`CLAUDE_CONFIG_DIR`, else `~/.claude`): key names, host and
/// models only (T2.6).
fn record_provider(registry: &Registry, session_id: &str, ctx: &Context) -> Result<()> {
    // 先采集来源；坏 settings 和缺失 HOME 都不能抹去启动证据。
    let launch_source =
        moss_core::provider::LaunchSource::capture(std::env::var_os("CLAUDE_CONFIG_DIR"));
    let argv = ctx.pid.and_then(super::process_identity::entry_arguments);
    let unreplayable_args = moss_core::provider::unreplayable_args(argv.as_deref());
    let home = env("HOME").map(std::path::PathBuf::from);
    let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(std::path::PathBuf::from);
    let path = moss_core::provider::settings_path(None, config_dir.as_deref(), home.as_deref());
    let read = path
        .as_deref()
        .and_then(moss_core::provider::Snapshot::read);
    let valid = read.is_some();
    let mut now = read.unwrap_or_default();
    now.settings_valid = Some(valid);
    now.source = path.map(|p| p.to_string_lossy().into_owned());
    now.launch_source = launch_source;
    now.unreplayable_args = unreplayable_args;
    let Some(slot) = registry.slot_for_event(session_id, ctx)? else {
        return Ok(());
    };
    registry.set_provider(
        &slot.slot_id,
        &now.to_json(),
        &moss_core::provider::keys_json(&now.keys),
    )
}

/// A SessionStart from the Moss the daemon started: the role it was given,
/// and the session id it was started with is this one.
fn is_moss_start(role: Option<&str>, moss_session: Option<&str>, session_id: &str) -> bool {
    role == Some(moss::ROLE) && moss_session.is_some_and(|id| !id.is_empty() && id == session_id)
}

/// After `--resume` Claude Code omits the name from the payload; the
/// transcript still records the last `/rename`.
fn fill_name_from_transcript(
    registry: &Registry,
    session_id: &str,
    ctx: &Context,
    transcript: Option<&str>,
) {
    let Some(path) = transcript else { return };
    let Ok(Some(slot)) = registry.slot_for_event(session_id, ctx) else {
        return;
    };
    if slot.name.is_some() {
        return;
    }
    if let Some(name) = moss_core::transcript::last_custom_title(std::path::Path::new(path)) {
        if let Err(e) = registry.set_name(&slot.slot_id, &name) {
            log("session-start", &format!("error: {e:#}"));
        }
    }
}

/// Keep the tmux session name equal to `<prefix>_<Claude session name>`
/// (R7: the Claude name is the single source of truth). Only the primary
/// slot of a tmux session renames it, and only by session id.
fn sync_tmux_name(registry: &Registry, session_id: &str, ctx: &Context) -> Result<()> {
    let Some(slot) = registry.slot_for_event(session_id, ctx)? else {
        return Ok(());
    };
    sync_tmux_name_of(registry, &slot)
}

/// [`sync_tmux_name`] for a slot in hand (also after `reconcile` picks up a
/// `/rename` from the transcript).
pub(crate) fn sync_tmux_name_of(registry: &Registry, slot: &moss_core::slot::Slot) -> Result<()> {
    // Moss keeps the tmux name it was started with.
    if slot.is_moss() {
        return Ok(());
    }
    let (Some(loc), Some(name)) = (slot.tmux.as_ref(), slot.name.as_deref()) else {
        return Ok(());
    };
    let Some(tmux_id) = loc.session_id.as_deref() else {
        return Ok(());
    };
    if !registry.is_tmux_primary(slot)? {
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
    // Its own name for terminal tabs, as written (spaces, Chinese and all).
    tmux.set_display_name(tmux_id, name)?;
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
            "systemMessage": tr!(
                format!("cc-moss: another session is already named `{name}`. Names are one of a kind: /rename this one."),
                format!("cc-moss：已经有另一个会话叫 `{name}` 了。名字不能重复，给这个会话 /rename 一下吧。")
            )
        });
        println!("{msg}");
    }
    Ok(())
}

/// A JSON `systemMessage` is the only thing a hook prints; Claude Code shows it to the user.
fn warn_outside_tmux() {
    let msg = serde_json::json!({
        "systemMessage": tr!(
            "cc-moss: this session is not in tmux, so closing the terminal ends it. It is still recorded, \
             and `cc-moss restore` can bring it back. Start sessions with `cc-moss new` to keep them alive.",
            "cc-moss：这个会话不在 tmux 里，关掉终端它就结束了。不过它已经登记在案，`cc-moss restore` 能把它找回来。\
             用 `cc-moss new` 开会话，就不怕终端被关。"
        )
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
        None => env("CLAUDE_CODE_ENTRYPOINT").is_none_or(|e| e == "cli"),
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Log `message` only the first time `key` comes up; a marker file in the
/// state dir remembers it.
fn log_once(key: &str, event: &str, message: &str) {
    let marker = store_dir::state_dir().join(format!(".logged-{key}"));
    if marker.exists() || !marker.is_absolute() {
        return;
    }
    let _ = std::fs::write(&marker, b"");
    log(event, message);
}

fn log(event: &str, message: &str) {
    let path = store_dir::hook_log_path();
    // One write per line so concurrent hooks do not interleave.
    let line = format!("{} {event} {message}\n", rfc3339(now_ms()));
    let _ = (|| -> std::io::Result<()> {
        // Without HOME the path is relative: there is nowhere sensible to
        // log, and the hook must stay silent.
        if let Some(dir) = path.parent() {
            store_dir::ensure_private_dir(dir)?;
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
            .mode(0o600)
            .open(&path)?
            .write_all(line.as_bytes())
    })();
}
