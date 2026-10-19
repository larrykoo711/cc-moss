//! `cc-moss new`: a named Claude Code session in its own tmux session.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use std::collections::HashSet;
use std::path::Path;

use anyhow::bail;
use moss_core::config::{Config, PermissionMode};
use moss_core::launch_args::{sets_permissions, without_permissions};
use moss_core::registry::Registry;
use moss_core::restore::{plan, Verdict};
use moss_core::slot::{now_ms, Slot, Status};
use moss_core::{proc, store_dir};

use super::confirm;
use super::restore::{Launched, Launcher};
use moss_core::naming;
use moss_core::tmux::{Socket, Tmux};

pub struct NewArgs {
    pub name: Option<String>,
    pub dir: Option<PathBuf>,
    pub detach: bool,
    /// Restore an interrupted session of the same name without asking.
    pub yes: bool,
    /// Start this one in Claude Code's normal mode, whatever the config says:
    /// no bypass of ours, and none from `default_args` either.
    pub safe: bool,
    pub claude_args: Vec<String>,
}

pub fn run(args: NewArgs) -> Result<()> {
    let config = Config::load()?;
    super::launch::require_tmux()?;
    let cwd = match args.dir {
        Some(d) => d,
        None => std::env::current_dir()?,
    };
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("directory {}", cwd.display()))?;
    let name = args.name.unwrap_or_else(|| {
        cwd.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "session".into())
    });

    // One open session per name (T1.5): enter or restore it instead.
    if let Some(done) = existing_session(&config, &name, args.detach, args.yes)? {
        return done;
    }

    let socket = Socket::from_config(config.tmux_socket().as_deref());
    let tmux = Tmux::with_socket(socket.clone());
    let prefix = if config.naming.host_prefix {
        naming::host_prefix()
    } else {
        None
    };
    let tmux_name = naming::unique(
        &naming::session_name(prefix.as_deref(), &name),
        &tmux.session_names()?,
        None,
    );

    // A fixed session id lets the hooks match this session from its first event.
    let session_id = uuid::Uuid::new_v4().to_string();
    let claude = super::launch::claude_bin()?;
    // `--name=<n>` keeps a name starting with `-` from reading as a flag.
    let mut argv: Vec<String> = vec![
        claude.clone(),
        format!("--name={name}"),
        "--session-id".into(),
        session_id,
    ];
    if wants_bypass(&config, args.safe, &args.claude_args) {
        argv.push(BYPASS_FLAG.into());
    }
    // `--safe` means normal mode this once, also over bypass in `default_args`.
    if args.safe {
        argv.extend(without_permissions(&config.claude.default_args));
    } else {
        argv.extend(config.claude.default_args.iter().cloned());
    }
    argv.extend(args.claude_args);
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();

    moss_core::tmux::ensure_access(&tmux, &cwd.to_string_lossy())?;
    let pane = tmux
        .new_session(&tmux_name, &cwd.to_string_lossy(), &argv)
        .context("start tmux session")?;
    if super::launch::exited_at_start(&tmux, &pane) {
        bail!(super::launch::exited_message(
            &name,
            &claude,
            &cwd.to_string_lossy()
        ));
    }
    // The terminal tab shows its own name; a title is not worth failing for.
    let _ = tmux.set_display_name(&pane.session_id, &name);
    // The registry learns about the session from its SessionStart hook.
    println!(
        "{}",
        tr!(
            format!("Started {name} in tmux session {tmux_name}."),
            format!("已启动 {name}（tmux 会话 {tmux_name}）。")
        )
    );

    if args.detach {
        return Ok(());
    }
    super::attach::attach_tmux(&socket, &tmux_name)
}

const BYPASS_FLAG: &str = "--dangerously-skip-permissions";

/// Whether `new` adds [`BYPASS_FLAG`]: the config asks for bypass (the
/// default), this run is not `--safe`, no argument already picks a permission
/// mode, and no Claude Code settings turn bypass off.
fn wants_bypass(config: &Config, safe: bool, claude_args: &[String]) -> bool {
    !safe
        && config.claude.permission_mode == PermissionMode::Bypass
        && !sets_permissions(&config.claude.default_args)
        && !sets_permissions(claude_args)
        && super::setup::bypass_blocked_by().is_none()
}

/// If an open session already has `name`, deal with it and return what `new`
/// should return; `None` means the name is free.
fn existing_session(
    config: &Config,
    name: &str,
    detach: bool,
    yes: bool,
) -> Result<Option<Result<()>>> {
    let path = store_dir::registry_path();
    if !path.exists() {
        return Ok(None);
    }
    let registry = Registry::open(&path)?;
    let rows: Vec<(Status, Slot)> = registry
        .slots()?
        .into_iter()
        .filter(|s| s.name.as_deref() == Some(name))
        .map(|s| (s.effective_status(proc::is_alive), s))
        .filter(|(st, _)| *st != Status::Closed)
        .collect();
    let (status, slot) = match rows.as_slice() {
        [] => return Ok(None),
        [one] => (one.0, &one.1),
        many => {
            let ids = many
                .iter()
                .map(|(_, s)| s.session_id.chars().take(8).collect::<String>())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(tr!(
                format!("several open sessions are named `{name}`: {ids}; close the extra ones with `cc-moss close <id>`"),
                format!("有好几个未关闭的会话都叫 `{name}`：{ids}；用 `cc-moss close <id>` 关掉多余的")
            ))
        }
    };

    if status == Status::Running {
        println!(
            "{}",
            tr!(
                format!("{name} is already running; taking you there."),
                format!("{name} 已经在运行了，带你过去。")
            )
        );
        if detach {
            return Ok(Some(Ok(())));
        }
        let Some(loc) = slot.tmux.as_ref() else {
            bail!(tr!(
                format!("{name} is already running outside tmux"),
                format!("{name} 已经在 tmux 之外运行了")
            ));
        };
        let target = loc
            .session_id
            .as_deref()
            .or(loc.session_name.as_deref())
            .unwrap_or(name);
        return Ok(Some(super::attach::attach_tmux(
            &Socket::Path(loc.socket.clone()),
            target,
        )));
    }

    // Interrupted: restore it, unless there is nothing to restore.
    let only: HashSet<String> = [slot.slot_id.clone()].into();
    let verdict = plan(&rows, Some(&only), now_ms(), |p: &Path| p.exists())
        .first()
        .map(|c| c.verdict);
    match verdict {
        Some(Verdict::Restore) => {}
        Some(Verdict::EmptySession) => {
            // It never got a message: nothing to lose, start over under the same name.
            registry.close_by_user(&slot.slot_id, now_ms())?;
            println!(
                "{}",
                tr!(
                    format!("{name} was interrupted before its first message; starting fresh."),
                    format!("{name} 还没说上话就中断了，直接重新开一个。")
                )
            );
            return Ok(None);
        }
        Some(other) => bail!(tr!(
            format!("{name} was interrupted and cannot be restored ({other:?}); free the name with `cc-moss close {name}`"),
            format!("{name} 中断了，但没法恢复（{other:?}）；用 `cc-moss close {name}` 把名字腾出来")
        )),
        None => return Ok(None),
    }
    let question = tr!(
        format!("{name} was interrupted. Bring it back?"),
        format!("{name} 之前中断了，要恢复它吗？")
    );
    let refused = tr!(
        format!("run `cc-moss restore {name}` (or `cc-moss new {name} --yes`)"),
        format!("运行 `cc-moss restore {name}`（或 `cc-moss new {name} --yes`）")
    );
    if !confirm::ask_default_yes(yes, &question, &refused)? {
        bail!(tr!(
            format!(
                "{name} is taken by an interrupted session; restore it or `cc-moss close {name}`"
            ),
            format!("{name} 这个名字被一个中断的会话占着；恢复它，或者 `cc-moss close {name}`")
        ));
    }
    let mut launcher = Launcher::new(config)?;
    let tmux_name = match launcher.launch(&registry, slot)? {
        Launched::Started(t) => t,
        Launched::Changed => bail!(tr!(
            format!("{name} changed meanwhile (resumed elsewhere?); try again"),
            format!("{name} 刚刚有了变化（可能已在别处恢复？），再试一次")
        )),
        Launched::Failed(..) => bail!(tr!(
            format!("could not restore {name}"),
            format!("没能恢复 {name}")
        )),
    };
    if detach {
        return Ok(Some(Ok(())));
    }
    Ok(Some(super::attach::attach_tmux(
        launcher.socket(),
        &tmux_name,
    )))
}
