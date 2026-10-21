//! `cc-moss restore`: bring interrupted sessions back after a crash or reboot.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Result};
use moss_core::config::Config;
use moss_core::event::TmuxLoc;
use moss_core::registry::Registry;
use moss_core::restore::{plan, Verdict};
use moss_core::slot::{now_ms, Slot, Status};
use moss_core::tmux::{ProcessRunner, Socket, Tmux};
use moss_core::{naming, proc, store_dir};

use super::confirm;
use super::ls::JSON_SCHEMA;

pub struct RestoreArgs {
    pub provider_default: bool,
    pub yes: bool,
    pub dry_run: bool,
    /// Print one result per session as JSON instead of text (for the app).
    pub json: bool,
    pub names: Vec<String>,
}

/// Why a session did not come back, as a stable code for `--json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Killed before the first message: nothing to resume.
    Empty,
    /// Its folder is gone.
    MissingDir,
    /// Claude Code exited right after it started.
    ExitedAtOnce,
    /// tmux could not start (or reach) the session.
    TmuxFailed,
    /// It is running, on its way back, or comes back through another copy.
    AlreadyRunning,
    /// The name matches several sessions (`--json` only).
    Ambiguous,
    /// The name matches no session (`--json` only).
    NotFound,
}

impl Reason {
    pub fn code(self) -> &'static str {
        match self {
            Reason::Empty => "empty",
            Reason::MissingDir => "missing_dir",
            Reason::ExitedAtOnce => "exited_at_once",
            Reason::TmuxFailed => "tmux_failed",
            Reason::AlreadyRunning => "already_running",
            Reason::Ambiguous => "ambiguous",
            Reason::NotFound => "not_found",
        }
    }
}

/// One line of `restore --json`.
struct Outcome {
    id: String,
    name: String,
    failure: Option<(Reason, String)>,
    /// For `ambiguous`: the id prefixes of the sessions the name matches.
    candidates: Vec<String>,
    result: Option<&'static str>,
}

impl Outcome {
    fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "id": self.id,
            "name": self.name,
            "ok": self.failure.is_none(),
        });
        if let Some(result) = self.result {
            v["result"] = result.into();
        }
        if let Some((reason, message)) = &self.failure {
            v["reason"] = reason.code().into();
            v["message"] = message.as_str().into();
        }
        if !self.candidates.is_empty() {
            v["candidates"] = self.candidates.clone().into();
        }
        v
    }
}

fn print_json(outcomes: &[Outcome]) -> Result<()> {
    let results: Vec<_> = outcomes.iter().map(Outcome::to_json).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": JSON_SCHEMA,
            "results": results,
        }))?
    );
    Ok(())
}

pub fn run(args: RestoreArgs) -> Result<()> {
    let config = Config::load()?;
    let path = store_dir::registry_path();
    if !path.exists() {
        if args.json {
            return print_json(&[]);
        }
        println!("{}", nothing_to_restore());
        return Ok(());
    }
    let registry = Registry::open(&path)?;
    let slots = registry.slots()?;
    // Names select exact slots (by id), whatever form the user typed.
    // With --json a name that picks no one session is a result of its own.
    let mut unresolved = Vec::new();
    let only: Option<HashSet<String>> = if args.names.is_empty() {
        None
    } else if args.json {
        let mut ids = HashSet::new();
        for n in &args.names {
            match super::resolve_item(&slots, n) {
                Ok(s) => {
                    ids.insert(s.slot_id.clone());
                }
                Err(u) => unresolved.push(Outcome {
                    id: String::new(),
                    name: n.clone(),
                    failure: Some((
                        if u.ambiguous {
                            Reason::Ambiguous
                        } else {
                            Reason::NotFound
                        },
                        u.message,
                    )),
                    candidates: u.candidates,
                    result: None,
                }),
            }
        }
        Some(ids)
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
    // Archived sessions, and ones the user ended on purpose (shown again
    // after the archive), come back only when named.
    let pool = only.clone().or_else(|| {
        Some(
            rows.iter()
                .filter(|(_, s)| !s.is_archived() && !s.ended_on_purpose())
                .map(|(_, s)| s.slot_id.clone())
                .collect(),
        )
    });
    let candidates = plan(&rows, pool.as_ref(), now_ms(), |p: &Path| p.exists());
    if args.json {
        return run_json(
            &config,
            &registry,
            &rows,
            only.as_ref(),
            &candidates,
            unresolved,
            args.provider_default,
        );
    }
    if candidates.is_empty() {
        println!("{}", nothing_to_restore());
        return Ok(());
    }

    for c in &candidates {
        let why = verdict_text(c.verdict);
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

    let mut launcher = Launcher::new(&config)?.provider_default(args.provider_default);
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
            Launched::Failed(..) => failed += 1,
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

fn verdict_text(verdict: Verdict) -> &'static str {
    match verdict {
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
    }
}

/// `restore --yes --json`: restore every restorable candidate without asking
/// and report each session, including the ones named but already running.
/// Failures are results, not an error: the JSON is always printed.
fn run_json(
    config: &Config,
    registry: &Registry,
    rows: &[(Status, Slot)],
    only: Option<&HashSet<String>>,
    candidates: &[moss_core::restore::Candidate<'_>],
    unresolved: Vec<Outcome>,
    provider_default: bool,
) -> Result<()> {
    let mut outcomes = unresolved;
    if let Some(ids) = only {
        for (_, slot) in rows
            .iter()
            .filter(|(st, s)| *st == Status::Running && ids.contains(&s.slot_id))
        {
            outcomes.push(Outcome {
                id: slot.slot_id.clone(),
                name: display_name(slot),
                failure: Some((
                    Reason::AlreadyRunning,
                    tr!("it is already running", "它已经在运行了").to_string(),
                )),
                candidates: Vec::new(),
                result: None,
            });
        }
    }
    let mut launcher: Option<Launcher> = None;
    for c in candidates {
        let reason = match c.verdict {
            Verdict::Restore => None,
            Verdict::EmptySession => Some(Reason::Empty),
            Verdict::MissingCwd => Some(Reason::MissingDir),
            Verdict::RestorePending | Verdict::OpenElsewhere | Verdict::Duplicate => {
                Some(Reason::AlreadyRunning)
            }
        };
        let failure = match reason {
            Some(r) => Some((r, verdict_text(c.verdict).to_string())),
            None => {
                if launcher.is_none() {
                    launcher = Some(
                        Launcher::new(config)?
                            .provider_default(provider_default)
                            .quiet(),
                    );
                }
                let l = launcher.as_mut().expect("just created");
                match l.launch(registry, c.slot)? {
                    Launched::Started(_) => None,
                    Launched::Changed => Some((
                        Reason::AlreadyRunning,
                        tr!(
                            "it changed meanwhile (resumed elsewhere?)",
                            "它刚刚有了变化（可能已在别处恢复？）"
                        )
                        .to_string(),
                    )),
                    Launched::Failed(r, message) => Some((r, message)),
                }
            }
        };
        let result = if failure.is_none() {
            let confirmed = registry.slots()?.iter().any(|s| {
                s.slot_id == c.slot.slot_id
                    && s.restore_started_at_ms.is_none()
                    && s.effective_status(proc::is_alive) == Status::Running
            });
            Some(if confirmed { "restored" } else { "starting" })
        } else {
            None
        };
        outcomes.push(Outcome {
            id: c.slot.slot_id.clone(),
            name: display_name(c.slot),
            failure,
            candidates: Vec::new(),
            result,
        });
    }
    print_json(&outcomes)
}

pub enum Launched {
    /// Running in this tmux session.
    Started(String),
    /// Some hook touched the slot after planning; left alone.
    Changed,
    /// Did not come back: why, and the words already printed to stderr.
    Failed(Reason, String),
}

/// Relaunches interrupted slots in tmux: claim, start, record the process.
pub struct Launcher {
    socket: Socket,
    tmux: Tmux<ProcessRunner>,
    prefix: Option<String>,
    taken: Vec<String>,
    claude: String,
    /// Keep stdout clean (for `--json`): no "Started …" lines.
    quiet: bool,
    provider_default: bool,
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
        super::launch::require_tmux()?;
        let claude = super::launch::claude_bin()?;
        let taken = tmux.session_names()?;
        Ok(Launcher {
            socket,
            tmux,
            prefix,
            taken,
            claude,
            quiet: false,
            provider_default: false,
        })
    }

    pub fn provider_default(mut self, apply: bool) -> Self {
        self.provider_default = apply;
        self
    }

    /// Print nothing on stdout; errors still go to stderr.
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub fn socket(&self) -> &Socket {
        &self.socket
    }

    /// Restore `slot` (planned as [`Verdict::Restore`]) in its own tmux session.
    pub fn launch(&mut self, registry: &Registry, slot: &Slot) -> Result<Launched> {
        let label = display_name(slot);
        let owned_argv =
            match super::restart::resume_argv(&self.claude, slot, self.provider_default) {
                Ok(argv) => argv,
                Err(error) => {
                    return Ok(Launched::Failed(Reason::TmuxFailed, format!("{error:#}")))
                }
            };
        // Claim with the time of launch, not of planning: a prompt may have waited.
        if !registry.claim_restore(slot, now_ms())? {
            return Ok(Launched::Changed);
        }
        let tmux_name = naming::unique(
            &naming::session_name(self.prefix.as_deref(), &label),
            &self.taken,
            None,
        );
        // Its folder has to be one: a session never starts anywhere else.
        if !Path::new(&slot.cwd).is_dir() {
            registry.clear_restore_started(&slot.slot_id)?;
            let message = tr!(
                format!("could not restore {label}: its folder {} is gone", slot.cwd),
                format!("没能恢复 {label}：它的目录 {} 已经不在了", slot.cwd)
            );
            return Ok(Launched::Failed(Reason::MissingDir, message));
        }
        let argv: Vec<&str> = owned_argv.iter().map(String::as_str).collect();
        if let Err(e) = moss_core::tmux::ensure_access(&self.tmux, &slot.cwd) {
            let message = tr!(
                format!("could not restore {label}: {e:#}"),
                format!("没能恢复 {label}：{e:#}")
            );
            eprintln!("cc-moss: {message}");
            registry.clear_restore_started(&slot.slot_id)?;
            return Ok(Launched::Failed(Reason::TmuxFailed, message));
        }
        let mut failure: Option<(Reason, String)> = None;
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
                    let message = tr!(
                        format!("could not restore {label}: {e:#}"),
                        format!("没能恢复 {label}：{e:#}")
                    );
                    eprintln!("cc-moss: {message}");
                    failure = Some((Reason::TmuxFailed, message));
                    None
                }),
        };
        let pane = pane.filter(|pane| {
            let dead = super::launch::exited_at_start(&self.tmux, pane);
            if dead {
                let message = super::launch::exited_message(&label, &self.claude, &slot.cwd);
                eprintln!("cc-moss: {message}");
                failure = Some((Reason::ExitedAtOnce, message));
            }
            !dead
        });
        let Some(pane) = pane else {
            if let Err(e) = registry.clear_restore_started(&slot.slot_id) {
                eprintln!("cc-moss: {e:#}");
            }
            let (reason, message) = failure.unwrap_or_else(|| {
                (
                    Reason::TmuxFailed,
                    tr!(
                        format!("could not restore {label}"),
                        format!("没能恢复 {label}")
                    ),
                )
            });
            return Ok(Launched::Failed(reason, message));
        };
        let _ = self.tmux.set_display_name(&pane.session_id, &label);
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
        let confirmed = super::restart::wait_for_start(registry, slot)?;
        if !self.quiet {
            println!(
                "{}",
                tr!(
                    format!(
                        "{} {label} in tmux session {tmux_name}{}",
                        if confirmed { "Started" } else { "Starting" },
                        if confirmed {
                            "."
                        } else {
                            "; waiting for the session start event."
                        }
                    ),
                    format!(
                        "{} {label}（tmux 会话 {tmux_name}）{}",
                        if confirmed {
                            "已启动"
                        } else {
                            "正在启动"
                        },
                        if confirmed {
                            "。"
                        } else {
                            "，等待会话启动事件。"
                        }
                    )
                )
            );
        }
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
