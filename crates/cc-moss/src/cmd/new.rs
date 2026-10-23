//! `cc-moss new`: a fresh named Claude Code session in its own tmux session.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use moss_core::config::{Config, PermissionMode};
use moss_core::launch_args::{sets_permissions, without_permissions};
use moss_core::registry::Registry;
use moss_core::slot::Status;
use moss_core::tmux::{NewSessionFailure, Socket, Tmux};
use moss_core::{naming, proc, store_dir};
use serde_json::json;

pub struct NewArgs {
    pub name: Option<String>,
    pub dir: Option<PathBuf>,
    pub detach: bool,
    /// 兼容旧命令；新建不再把此参数解释为恢复已有会话。
    pub yes: bool,
    pub safe: bool,
    pub claude_args: Vec<String>,
}

pub fn run(args: NewArgs) -> Result<()> {
    create(args, false)
}

pub fn run_json(args: NewArgs) -> Result<()> {
    if !args.detach {
        bail!("new --json requires --detach");
    }
    create(args, true)
}

fn occupied_names() -> Result<HashSet<String>> {
    let path = store_dir::registry_path();
    if !path.exists() {
        return Ok(HashSet::new());
    }
    Ok(Registry::open(&path)?
        .slots()?
        .into_iter()
        .filter(|slot| {
            !slot.is_archived() && slot.effective_status(proc::is_alive) != Status::Closed
        })
        .filter_map(|slot| slot.name)
        .map(|name| naming::sanitize(&name))
        .collect())
}

fn create(args: NewArgs, json_out: bool) -> Result<()> {
    let config = Config::load()?;
    if !moss_core::launch_args::fresh_arguments(&args.claude_args)
        || !moss_core::launch_args::fresh_arguments(&config.claude.default_args)
    {
        bail!("new requires fresh-session arguments; use native claude for explicit resume/continue and pass the new name as a positional argument");
    }
    let current = std::env::current_dir()?;
    let requested = args.dir.clone().unwrap_or_else(|| current.clone());
    let requested = if requested.is_absolute() {
        requested
    } else {
        current.join(requested)
    };
    let validated = moss_core::working_directory::check(
        &requested,
        &super::working_directory::current_context()?,
    )?;
    let cwd = validated.canonical().to_path_buf();
    let launch_source =
        moss_core::provider::LaunchSource::capture(std::env::var_os("CLAUDE_CONFIG_DIR"));
    let mut launch_env = launch_source.env_argv().map_err(anyhow::Error::msg)?;
    launch_env.push(format!("HOME={}", validated.target_home().display()));
    super::launch::require_tmux()?;
    let seed = args.name.clone().unwrap_or_else(|| {
        cwd.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "session".into())
    });
    let _ = args.yes;
    let session_id = uuid::Uuid::new_v4().to_string();
    let socket = Socket::from_config(config.tmux_socket().as_deref());
    let mut timeout = Duration::from_secs(10);
    if cfg!(debug_assertions) {
        if let Some(ms) = std::env::var("CC_MOSS_NEW_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        {
            timeout = Duration::from_millis(ms.max(1));
        }
    }
    let tmux = Tmux::with_timeout(socket.clone(), timeout);
    let prefix = config
        .naming
        .host_prefix
        .then(naming::host_prefix)
        .flatten();
    let claude = super::launch::claude_bin()?;
    let bypass = wants_bypass(&config, args.safe, &args.claude_args);
    let mut occupied = occupied_names()?;
    let mut tmux_taken: HashSet<String> = tmux.session_names()?.into_iter().collect();
    let mut ordinal = 1usize;
    for _ in 0..32 {
        let (name, tmux_name) = loop {
            let name = naming::fresh_name(&seed, ordinal);
            ordinal += 1;
            let tmux_name = naming::session_name(prefix.as_deref(), &name);
            if !occupied.contains(&naming::sanitize(&name)) && !tmux_taken.contains(&tmux_name) {
                break (name, tmux_name);
            }
        };
        let mut argv = launch_env.clone();
        argv.extend([
            claude.clone(),
            format!("--name={name}"),
            "--session-id".into(),
            session_id.clone(),
        ]);
        if bypass {
            argv.push(BYPASS_FLAG.into());
        }
        if args.safe {
            argv.extend(without_permissions(&config.claude.default_args));
        } else {
            argv.extend(config.claude.default_args.iter().cloned());
        }
        argv.extend(args.claude_args.iter().cloned());
        moss_core::working_directory::recheck(&validated)?;
        moss_core::tmux::ensure_access(&tmux, &cwd.to_string_lossy())?;
        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
        moss_core::working_directory::recheck(&validated)?;
        let pane = match tmux.new_session(&tmux_name, &cwd.to_string_lossy(), &words) {
            Ok(pane) => pane,
            Err(error)
                if error.downcast_ref::<NewSessionFailure>()
                    == Some(&NewSessionFailure::DuplicateName) =>
            {
                occupied.insert(naming::sanitize(&name));
                tmux_taken.insert(tmux_name);
                continue;
            }
            Err(error)
                if error.downcast_ref::<NewSessionFailure>()
                    == Some(&NewSessionFailure::Unconfirmed) =>
            {
                if json_out {
                    println!(
                        "{}",
                        json!({"schema":1,"result":"unconfirmed","session_id":session_id,"name":name,"cwd":cwd,"slot_id":null,"tmux":null})
                    );
                    return Ok(());
                }
                bail!("new session {session_id} ({name}) has an unconfirmed launch result; check that UUID before trying again");
            }
            Err(error) => return Err(error).context("start tmux session"),
        };
        if super::launch::exited_at_start(&tmux, &pane) {
            bail!(super::launch::exited_message(
                &name,
                &claude,
                &cwd.to_string_lossy()
            ));
        }
        let _ = tmux.set_display_name(&pane.session_id, &name);
        if json_out {
            println!(
                "{}",
                json!({"schema":1,"result":"starting","session_id":session_id,"name":name,"cwd":cwd,"slot_id":null,
                "tmux":{"session_id":pane.session_id,"pane_id":pane.pane_id,"socket":tmux.socket_path()}})
            );
        } else {
            println!(
                "{}",
                tr!(
                    format!("Started {name} in tmux session {tmux_name}."),
                    format!("已启动 {name}（tmux 会话 {tmux_name}）。")
                )
            );
        }
        if args.detach {
            return Ok(());
        }
        if let Some(current) = std::env::var_os("TMUX") {
            if !tmux.matches_client(&current.to_string_lossy()) {
                bail!("created session {session_id}, but the current terminal uses another tmux server; attach to the new session explicitly");
            }
        }
        return super::attach::attach_tmux(&socket, &pane.session_id);
    }
    bail!("could not reserve a new session name after 32 confirmed collisions; no existing session was opened")
}

const BYPASS_FLAG: &str = "--dangerously-skip-permissions";

fn wants_bypass(config: &Config, safe: bool, claude_args: &[String]) -> bool {
    !safe
        && config.claude.permission_mode == PermissionMode::Bypass
        && !sets_permissions(&config.claude.default_args)
        && !sets_permissions(claude_args)
        && super::setup::bypass_blocked_by().is_none()
}
