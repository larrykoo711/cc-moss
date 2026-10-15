//! `cc-moss new`: a named Claude Code session in its own tmux session.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use std::collections::HashSet;
use std::path::Path;

use anyhow::bail;
use moss_core::config::Config;
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
    pub claude_args: Vec<String>,
}

pub fn run(args: NewArgs) -> Result<()> {
    let config = Config::load()?;
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
    let claude = std::env::var("CC_MOSS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    // `--name=<n>` keeps a name starting with `-` from reading as a flag.
    let mut argv: Vec<String> = vec![
        claude,
        format!("--name={name}"),
        "--session-id".into(),
        session_id,
    ];
    argv.extend(config.claude.default_args.iter().cloned());
    argv.extend(args.claude_args);
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();

    tmux.new_session(&tmux_name, &cwd.to_string_lossy(), &argv)
        .context("start tmux session")?;
    // The registry learns about the session from its SessionStart hook.
    println!("Started {name} in tmux session {tmux_name}");

    if args.detach {
        return Ok(());
    }
    super::attach::attach_tmux(&socket, &tmux_name)
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
        many => bail!(
            "several open sessions are named `{name}`: {}; close the extra ones with `cc-moss close <id>`",
            many.iter()
                .map(|(_, s)| s.session_id.chars().take(8).collect::<String>())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };

    if status == Status::Running {
        println!("{name} is already running; attaching.");
        if detach {
            return Ok(Some(Ok(())));
        }
        let Some(loc) = slot.tmux.as_ref() else {
            bail!("{name} is already running outside tmux");
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
            println!("{name} was interrupted before its first message; starting a new session.");
            return Ok(None);
        }
        Some(other) => bail!(
            "{name} was interrupted and cannot be restored ({other:?}); free the name with `cc-moss close {name}`"
        ),
        None => return Ok(None),
    }
    let question = format!("{name} was interrupted. Restore it?");
    let refused = format!("run `cc-moss restore {name}` (or `cc-moss new {name} --yes`)");
    if !confirm::ask_default_yes(yes, &question, &refused)? {
        bail!("{name} is taken by an interrupted session; restore it or `cc-moss close {name}`");
    }
    let mut launcher = Launcher::new(config)?;
    let tmux_name = match launcher.launch(&registry, slot)? {
        Launched::Started(t) => t,
        Launched::Changed => bail!("{name} changed meanwhile (resumed elsewhere?); try again"),
        Launched::Failed => bail!("could not restore {name}"),
    };
    if detach {
        return Ok(Some(Ok(())));
    }
    Ok(Some(super::attach::attach_tmux(
        launcher.socket(),
        &tmux_name,
    )))
}
