//! `cc-moss restore`: bring interrupted sessions back after a crash or reboot.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Result};
use moss_core::config::Config;
use moss_core::event::TmuxLoc;
use moss_core::registry::Registry;
use moss_core::restore::{plan, Verdict};
use moss_core::slot::{now_ms, Slot};
use moss_core::tmux::{ProcessRunner, Socket, Tmux};
use moss_core::{naming, proc, store_dir};

use super::confirm;

pub struct RestoreArgs {
    pub yes: bool,
    pub dry_run: bool,
    pub names: Vec<String>,
}

pub fn run(args: RestoreArgs) -> Result<()> {
    let config = Config::load()?;
    let path = store_dir::registry_path();
    if !path.exists() {
        println!("{}", nothing_to_restore());
        return Ok(());
    }
    let registry = Registry::open(&path)?;
    let slots = registry.slots()?;
    // Names select exact slots (by id), whatever form the user typed.
    let only: Option<HashSet<String>> = if args.names.is_empty() {
        None
    } else {
        Some(
            args.names
                .iter()
                .map(|n| Ok(super::resolve(&slots, n)?.slot_id.clone()))
                .collect::<Result<_>>()?,
        )
    };
    let rows: Vec<_> = slots
        .into_iter()
        .map(|s| (s.effective_status(proc::is_alive), s))
        .collect();
    let candidates = plan(&rows, only.as_ref(), now_ms(), |p: &Path| p.exists());
    if candidates.is_empty() {
        println!("{}", nothing_to_restore());
        return Ok(());
    }

    for c in &candidates {
        let why = match c.verdict {
            Verdict::Restore => tr!("restore", "恢复"),
            Verdict::EmptySession => {
                tr!("skip: no messages were ever sent", "跳过：一句话都没发过")
            }
            Verdict::MissingCwd => tr!("skip: its folder is gone", "跳过：目录已经不在了"),
            Verdict::RestorePending => tr!("skip: already on its way back", "跳过：已经在恢复中"),
            Verdict::OpenElsewhere => tr!(
                "skip: this conversation is open in another session",
                "跳过：这段对话正在别处打开着"
            ),
            Verdict::Duplicate => tr!(
                "skip: a newer copy of this conversation is restored instead",
                "跳过：会恢复这段对话更新的那份"
            ),
        };
        let note = c
            .slot
            .notes
            .state
            .as_deref()
            .or(c.slot.notes.note.as_deref());
        println!(
            "{} {} {}{}",
            super::pad(&display_name(c.slot), 24),
            super::pad(why, 52),
            c.slot.cwd,
            note.map(|n| format!("  [{n}]")).unwrap_or_default()
        );
    }
    let todo: Vec<&Slot> = candidates
        .iter()
        .filter(|c| c.verdict == Verdict::Restore)
        .map(|c| c.slot)
        .collect();
    if todo.is_empty() {
        println!(
            "{}",
            tr!("Nothing here can be restored.", "这些会话都恢复不了。")
        );
        return Ok(());
    }
    if args.dry_run {
        println!(
            "{}",
            tr!(
                format!("Dry run: would restore {} session(s).", todo.len()),
                format!("演练模式：会恢复 {} 个会话。", todo.len())
            )
        );
        return Ok(());
    }
    let question = tr!(
        format!("Bring back {} session(s)?", todo.len()),
        format!("要把这 {} 个会话恢复回来吗？", todo.len())
    );
    let refused = tr!(
        "re-run with --yes to restore them",
        "加上 --yes 重新运行即可恢复"
    );
    if !confirm::ask(args.yes, &question, refused)? {
        println!("{}", tr!("Okay, nothing restored.", "好的，什么都没动。"));
        return Ok(());
    }

    let mut launcher = Launcher::new(&config)?;
    let mut failed = 0;
    for slot in todo {
        match launcher.launch(&registry, slot)? {
            Launched::Started(_) => {}
            Launched::Changed => println!(
                "{}",
                tr!(
                    format!(
                        "Skipped {}: it changed since the list above (resumed elsewhere?)",
                        display_name(slot)
                    ),
                    format!(
                        "跳过 {}：列出之后它有了变化（可能已在别处恢复？）",
                        display_name(slot)
                    )
                )
            ),
            Launched::Failed => failed += 1,
        }
    }
    println!(
        "{}",
        tr!(
            "Jump back in with `t`, or `cc-moss attach <name>`.",
            "敲 `t` 或 `cc-moss attach <名字>` 回到会话里。"
        )
    );
    if failed > 0 {
        bail!(tr!(
            format!("{failed} session(s) could not be restored"),
            format!("{failed} 个会话没能恢复")
        ));
    }
    Ok(())
}

pub enum Launched {
    /// Running in this tmux session.
    Started(String),
    /// Some hook touched the slot after planning; left alone.
    Changed,
    Failed,
}

/// Relaunches interrupted slots in tmux: claim, start, record the process.
pub struct Launcher {
    socket: Socket,
    tmux: Tmux<ProcessRunner>,
    prefix: Option<String>,
    taken: Vec<String>,
    claude: String,
}

impl Launcher {
    pub fn new(config: &Config) -> Result<Self> {
        let socket = Socket::from_config(config.tmux_socket().as_deref());
        let tmux = Tmux::with_timeout(socket.clone(), LAUNCH_TIMEOUT);
        let prefix = if config.naming.host_prefix {
            naming::host_prefix()
        } else {
            None
        };
        let taken = tmux.session_names()?;
        let claude = std::env::var("CC_MOSS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
        Ok(Launcher {
            socket,
            tmux,
            prefix,
            taken,
            claude,
        })
    }

    pub fn socket(&self) -> &Socket {
        &self.socket
    }

    /// Restore `slot` (planned as [`Verdict::Restore`]) in its own tmux session.
    pub fn launch(&mut self, registry: &Registry, slot: &Slot) -> Result<Launched> {
        let label = display_name(slot);
        // Claim with the time of launch, not of planning: a prompt may have waited.
        if !registry.claim_restore(slot, now_ms())? {
            return Ok(Launched::Changed);
        }
        let tmux_name = naming::unique(
            &naming::session_name(self.prefix.as_deref(), &label),
            &self.taken,
            None,
        );
        let name_arg = slot.name.as_ref().map(|n| format!("--name={n}"));
        let mut argv: Vec<&str> = vec![&self.claude, "--resume", &slot.session_id];
        argv.extend(name_arg.as_deref());
        argv.extend(slot.launch_args.iter().map(String::as_str));
        if let Err(e) = moss_core::tmux::ensure_access(&self.tmux, &slot.cwd) {
            eprintln!(
                "cc-moss: {}",
                tr!(
                    format!("could not restore {label}: {e:#}"),
                    format!("没能恢复 {label}：{e:#}")
                )
            );
            registry.clear_restore_started(&slot.slot_id)?;
            return Ok(Launched::Failed);
        }
        // A timeout or error does not prove the session was not created:
        // look for it before deciding.
        let pane = match self.tmux.new_session(&tmux_name, &slot.cwd, &argv) {
            Ok(pane) => Some(pane),
            // Only a pane that runs this very resume counts: a same-named
            // session someone else created must not be adopted.
            Err(e) => self
                .tmux
                .pane_of(&tmux_name)
                .filter(|p| {
                    p.start_command
                        .contains(&format!("--resume {}", slot.session_id))
                })
                .or_else(|| {
                    eprintln!(
                        "cc-moss: {}",
                        tr!(
                            format!("could not restore {label}: {e:#}"),
                            format!("没能恢复 {label}：{e:#}")
                        )
                    );
                    None
                }),
        };
        let Some(pane) = pane else {
            if let Err(e) = registry.clear_restore_started(&slot.slot_id) {
                eprintln!("cc-moss: {e:#}");
            }
            return Ok(Launched::Failed);
        };
        let loc = TmuxLoc {
            socket: socket_path(&self.tmux),
            session_id: Some(pane.session_id.clone()),
            session_name: Some(tmux_name.clone()),
            pane_id: pane.pane_id.clone(),
        };
        registry.set_restored_process(
            &slot.slot_id,
            pane.pid,
            proc::start_time(pane.pid).as_deref(),
            &loc,
            now_ms(),
        )?;
        registry.close_superseded(&slot.slot_id, &slot.session_id, now_ms())?;
        println!(
            "{}",
            tr!(
                format!("Started {label} in tmux session {tmux_name}."),
                format!("已启动 {label}（tmux 会话 {tmux_name}）。")
            )
        );
        self.taken.push(tmux_name.clone());
        Ok(Launched::Started(tmux_name))
    }
}

/// Launching may start the tmux server and is something the user waits on.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(5);

/// The server's socket path, recorded like the hooks do (from `$TMUX`).
fn socket_path(tmux: &Tmux<ProcessRunner>) -> String {
    tmux.socket_path().unwrap_or_default()
}

pub fn display_name(slot: &Slot) -> String {
    slot.name
        .clone()
        .unwrap_or_else(|| slot.session_id.chars().take(8).collect())
}

fn nothing_to_restore() -> &'static str {
    tr!(
        "Nothing to restore. Everything made it through.",
        "没有需要恢复的会话，全员健在。"
    )
}
