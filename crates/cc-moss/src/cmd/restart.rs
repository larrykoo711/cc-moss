//! `cc-moss restart <name>…` (T2.6): start a session again in place, so it
//! picks up the API provider the settings name now. Same tmux pane, same
//! window and session, same conversation (`claude --resume <id>`).
//!
//! The pane is kept open across the exit with `remain-on-exit` (on that
//! pane only, put back afterwards); Claude Code is asked to quit (SIGTERM,
//! as `adopt` does) and the pane runs it again (`respawn-pane`). When
//! Claude Code runs under a shell in the pane, the shell stays and the
//! same command is typed into it. A busy session is not interrupted: it is
//! queued, and the daemon restarts it after its turn, unless `--now`.

use std::io::IsTerminal;
use std::time::{Duration, Instant};

use anyhow::Result;
use moss_core::proc::{self, Liveness};
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, Slot, Status};
use moss_core::store_dir;
use moss_core::tmux::{ProcessRunner, Runner, Socket, Tmux};
use serde_json::{json, Value};

use super::ls::JSON_SCHEMA;

/// How long Claude Code has to quit (debug builds may say otherwise).
fn quit_wait() -> Duration {
    super::app::test_override("CC_MOSS_RESTART_QUIT_MS")
        .and_then(|v| v.to_str()?.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(10))
}

/// Exit uncertainty is terminal: never start another copy or silently retry.
#[derive(Debug)]
enum QuitFailure {
    Timeout,
    Signal(std::io::Error),
    Unknown,
}
impl QuitFailure {
    fn reason(&self) -> &'static str {
        match self {
            Self::Timeout => "quit-timeout",
            Self::Signal(_) => "signal-failed",
            Self::Unknown => "process-unknown",
        }
    }
}
impl std::fmt::Display for QuitFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => f.write_str("the old session did not exit within the wait limit"),
            Self::Signal(error) => write!(f, "could not ask the old session to exit: {error}"),
            Self::Unknown => f.write_str("could not verify that the old process has exited"),
        }
    }
}
impl std::error::Error for QuitFailure {}

pub struct RestartArgs {
    pub names: Vec<String>,
    pub now: bool,
    pub yes: bool,
    pub stale: bool,
    pub json: bool,
    pub provider_default: bool,
    pub cancel: bool,
    pub from_queue: bool,
    pub rebuild_pane: bool,
    pub rebuild_identity: Option<String>,
}

/// One session's result.
struct Outcome {
    name: String,
    session_id: Option<String>,
    result: &'static str,
    reason: Option<&'static str>,
    message: String,
    /// For `ambiguous`: the id prefixes of the sessions the name matches.
    candidates: Vec<String>,
    rebuild_identity: Option<RebuildIdentity>,
}

impl Outcome {
    fn to_json(&self) -> Value {
        let mut v = json!({
            "name": self.name,
            "session_id": self.session_id,
            "result": self.result,
        });
        if let Some(reason) = self.reason {
            v["reason"] = reason.into();
        }
        v["message"] = self.message.clone().into();
        if let Some(identity) = &self.rebuild_identity {
            v["rebuild_identity"] = identity.to_json();
        }
        if !self.candidates.is_empty() {
            v["candidates"] = self.candidates.clone().into();
        }
        v
    }
}

/// The same conversation, name and whitelisted launch arguments.
/// Selecting a different model is an explicit operation in `resume_argv`.
pub fn restart_argv(claude: &str, slot: &Slot) -> Vec<String> {
    let replay = moss_core::launch_args::whitelist(
        &std::iter::once("claude")
            .chain(slot.launch_args.iter().map(String::as_str))
            .collect::<Vec<_>>(),
    );
    let mut argv = vec![
        claude.to_string(),
        "--resume".to_string(),
        slot.session_id.clone(),
    ];
    if let Some(name) = slot.name.as_deref().filter(|n| !n.is_empty()) {
        argv.push(format!("--name={name}"));
    }
    argv.extend(replay);
    argv
}

/// Resume with an explicit target only for an explicit channel application.
/// The recorded config directory is replayed with env, never its credentials.
pub fn resume_argv(claude: &str, slot: &Slot, provider_default: bool) -> Result<Vec<String>> {
    if !std::path::Path::new(&slot.cwd).is_absolute() || slot.cwd.chars().any(char::is_control) {
        anyhow::bail!("the session's working directory is not safe to replay");
    }
    // Neither old launch arguments nor the last response reveal the current
    // selection (for example opusplan or a new /model choice without a reply).
    // Let Claude resume its conversation; only explicit channel application
    // is allowed to impose a model override.
    let mut argv = without_model(&restart_argv(claude, slot));
    let path = if provider_default {
        let (plan, path) = super::provider::model_plan(slot)?;
        argv = without_model(&argv);
        argv.extend([
            "--model".to_string(),
            plan.model.expect("a resolved plan has a model"),
        ]);
        Some(path)
    } else {
        super::provider::settings_of(slot).or_else(super::provider::settings_path)
    };
    if let Some(path) = path {
        let config = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid settings folder"))?;
        if !config.is_absolute() || config.to_string_lossy().chars().any(char::is_control) {
            anyhow::bail!("the settings folder is not safe to replay");
        }
        let mut with_env = vec![
            "/usr/bin/env".to_string(),
            format!("CLAUDE_CONFIG_DIR={}", config.display()),
        ];
        with_env.extend(argv);
        argv = with_env;
    }
    if has_control(&argv) {
        anyhow::bail!("the session name or arguments contain control characters");
    }
    Ok(argv)
}

/// Whitelisted arguments (flag and value as separate words) without
/// `--model <value>`.
fn without_model(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--model" {
            i += 2;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out
}

/// A word as a POSIX shell reads it back.
fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// `cc-moss restart`. False when a session was refused.
pub fn run(args: RestartArgs) -> Result<bool> {
    if args.rebuild_pane && args.names.len() != 1 {
        anyhow::bail!("pane rebuilding requires exactly one session");
    }
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let settings = super::provider::current();
    // Each target, and whether `--stale` picked it (rather than its name).
    let mut targets: Vec<(String, Option<Slot>, Option<super::Unresolved>)> = Vec::new();
    let mut stale_ids: Vec<String> = Vec::new();
    for name in &args.names {
        match super::resolve_item(&slots, name) {
            Ok(s) => targets.push((name.clone(), Some(s.clone()), None)),
            Err(u) => targets.push((name.clone(), None, Some(u))),
        }
    }
    if args.stale {
        let evidence = super::provider::current_model_evidence(&registry, &store_dir::state_dir())?;
        for slot in &slots {
            let status = slot.effective_status(proc::is_alive);
            let stale = !slot.is_moss()
                && super::provider::effective_reason(
                    slot,
                    status,
                    settings.as_ref(),
                    evidence.get(&slot.slot_id),
                )
                .is_some();
            let listed = targets
                .iter()
                .any(|(_, s, _)| s.as_ref().is_some_and(|s| s.slot_id == slot.slot_id));
            if stale && !listed {
                stale_ids.push(slot.slot_id.clone());
                targets.push((super::restore::display_name(slot), Some(slot.clone()), None));
            }
        }
    }
    let mut outcomes = Vec::new();
    for (name, slot, why) in targets {
        let Some(slot) = slot else {
            let u = why.expect("an unresolved name says why");
            outcomes.push(Outcome {
                reason: Some(u.code(false)),
                candidates: u.candidates,
                ..refused(&name, None, "not-found", u.message)
            });
            continue;
        };
        let stale = stale_ids.contains(&slot.slot_id);
        outcomes.push(restart_one(&registry, &name, &slot, &args, stale)?);
    }
    if args.json {
        let results: Vec<Value> = outcomes.iter().map(Outcome::to_json).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "schema": JSON_SCHEMA, "results": results }))?
        );
    } else if outcomes.is_empty() {
        println!(
            "{}",
            tr!("No session needs a restart.", "没有需要重开的会话。")
        );
    } else {
        for o in &outcomes {
            println!("{}", o.message);
        }
    }
    Ok(outcomes.iter().all(|o| o.result != "refused"))
}

fn refused(name: &str, slot: Option<&Slot>, reason: &'static str, message: String) -> Outcome {
    Outcome {
        name: name.to_string(),
        session_id: slot.map(|s| s.session_id.clone()),
        result: "refused",
        reason: Some(reason),
        message,
        candidates: Vec::new(),
        rebuild_identity: None,
    }
}

fn restart_one(
    registry: &Registry,
    name: &str,
    slot: &Slot,
    args: &RestartArgs,
    stale: bool,
) -> Result<Outcome> {
    let state = store_dir::state_dir();
    if args.cancel {
        let cancelled = super::provider::cancel(&state, &slot.slot_id)?;
        if cancelled == super::provider::CancelOutcome::InProgress {
            return Ok(refused(
                name,
                Some(slot),
                "in-progress",
                tr!(
                    "The restart has already started; it cannot be cancelled.",
                    "重开已经开始，不能取消。"
                )
                .into(),
            ));
        }
        return Ok(Outcome {
            name: name.into(),
            session_id: Some(slot.session_id.clone()),
            result: "cancelled",
            reason: (cancelled == super::provider::CancelOutcome::NotQueued)
                .then_some("not-queued"),
            message: if cancelled == super::provider::CancelOutcome::NotQueued {
                tr!(
                    "There is no queued operation right now.",
                    "当前没有排队操作。"
                )
                .into()
            } else {
                tr!("The queued restart was cancelled.", "已取消排队重开。").into()
            },
            candidates: Vec::new(),
            rebuild_identity: None,
        });
    }
    if args.from_queue && !super::provider::queue_claimed(&state, &slot.slot_id) {
        return Ok(refused(
            name,
            Some(slot),
            "cancelled",
            "The queued restart is no longer pending.".into(),
        ));
    }
    if !args.from_queue && super::provider::queue_claimed(&state, &slot.slot_id) {
        return Ok(refused(
            name,
            Some(slot),
            "in-progress",
            tr!("The restart has already started.", "重开已经开始。").into(),
        ));
    }
    let rebuild = if args.rebuild_pane {
        let identity = args
            .rebuild_identity
            .as_deref()
            .and_then(RebuildIdentity::from_json);
        let Some(identity) =
            identity.filter(|identity| args.yes && identity_matches(registry, slot, identity))
        else {
            return Ok(refused(
                name,
                Some(slot),
                "confirmation-stale",
                tr!(
                    "The pane or session changed. Confirm rebuilding again.",
                    "窗格或会话已变化，请重新确认重建。"
                )
                .into(),
            ));
        };
        if super::provider::in_turn(slot) {
            return Ok(refused(
                name,
                Some(slot),
                "busy",
                tr!(
                    "Wait for this turn to finish, then confirm rebuilding again.",
                    "请等本轮结束后，再确认重建窗格。"
                )
                .into(),
            ));
        }
        Some(identity)
    } else {
        None
    };
    if slot.is_moss() {
        return Ok(refused(
            name,
            Some(slot),
            "moss",
            tr!(
                "Moss restarts by itself; cc-moss looks after it.",
                "Moss 由 cc-moss 自己照看，它会自己重开。"
            )
            .into(),
        ));
    }
    if slot.effective_status(proc::is_alive) != Status::Running {
        let _ = super::provider::dequeue(&state, &slot.slot_id);
        return Ok(refused(
            name,
            Some(slot),
            "not-running",
            tr!(
                format!(
                    "{name} isn't running; restore it instead, it starts with the new settings."
                ),
                format!("{name} 没在运行；直接恢复它就会用上新设置。")
            ),
        ));
    }
    let Some(loc) = slot.tmux.clone() else {
        return Ok(refused(name, Some(slot), "not-in-tmux", tr!(
            format!("{name} isn't in tmux, so it can't be restarted in place; `cc-moss adopt {name}` moves it into tmux and starts it again."),
            format!("{name} 不在 tmux 里，没法原地重开；`cc-moss adopt {name}` 会把它移进 tmux 并重新启动。")
        )));
    };
    if rebuild.is_none() {
        let tmux = Tmux::with_timeout(Socket::Path(loc.socket.clone()), Duration::from_secs(5));
        match launch_target(slot, &tmux, &loc.pane_id) {
            Ok(LaunchTarget::Proxy(identity)) => {
                let mut outcome = refused(name, Some(slot), "pty-proxy",
                    tr!("This terminal forwards through another PTY. Rebuilding needs separate confirmation and ends this pane's integration and other tasks.", "此终端通过另一层 PTY 转发输入。重建需单独确认，会结束此窗格的终端集成及其他任务。").into());
                outcome.rebuild_identity = identity;
                return Ok(outcome);
            }
            Ok(_) => {}
            Err(error) => {
                return Ok(refused(
                    name,
                    Some(slot),
                    "unsafe-terminal",
                    format!("{error:#}"),
                ))
            }
        }
    }
    if args.provider_default {
        if let Err(error) = super::provider::model_plan(slot) {
            return Ok(refused(name, Some(slot), "settings", format!("{error:#}")));
        }
    }
    if super::provider::in_turn(slot) {
        if rebuild.is_some() {
            return Ok(refused(
                name,
                Some(slot),
                "busy",
                tr!(
                    "Wait for this turn to finish, then confirm rebuilding again.",
                    "请等本轮结束后，再确认重建窗格。"
                )
                .into(),
            ));
        }
        if !args.now {
            if !args.from_queue {
                if let Err(error) = super::provider::enqueue_policy(
                    &state,
                    &slot.slot_id,
                    stale,
                    args.provider_default,
                ) {
                    if error.kind() != std::io::ErrorKind::WouldBlock {
                        return Err(error.into());
                    }
                    return Ok(refused(
                        name,
                        Some(slot),
                        "in-progress",
                        tr!("The restart has already started.", "重开已经开始。").into(),
                    ));
                }
            }
            return Ok(Outcome {
                name: name.to_string(),
                session_id: Some(slot.session_id.clone()),
                result: "queued",
                reason: Some("busy"),
                candidates: Vec::new(),
                rebuild_identity: None,
                message: tr!(
                    format!("{name} is working; it restarts when this turn is done."),
                    format!("{name} 正在干活，这一轮结束后重开。")
                ),
            });
        }
        let question = tr!(
            format!("{name} is working. Restart it now? This turn is lost; send \"continue\" after the restart."),
            format!("{name} 正在干活。现在就重开吗？这一轮会丢，重开后可以发 continue 接着来。")
        );
        let agreed = if args.yes {
            true
        } else if std::io::stdin().is_terminal() {
            super::confirm::ask(false, &question, "")?
        } else {
            false
        };
        if !agreed {
            return Ok(refused(name, Some(slot), "busy", tr!(
                format!("{name} is working; it wasn't restarted. Add --yes to restart it now anyway, or leave out --now to restart it after this turn."),
                format!("{name} 正在干活，没有重开。加 --yes 立即重开，或者去掉 --now 等这一轮结束再重开。")
            )));
        }
    }
    let claude = super::launch::claude_bin()?;
    let argv = match resume_argv(&claude, slot, args.provider_default) {
        Ok(argv) => argv,
        Err(error) => return Ok(refused(name, Some(slot), "settings", format!("{error:#}"))),
    };
    let direct_execution = if args.from_queue {
        None
    } else {
        match super::provider::claim_direct(&state, &slot.slot_id, args.provider_default)? {
            Some(claim) => Some(claim),
            None => {
                return Ok(refused(
                    name,
                    Some(slot),
                    "in-progress",
                    tr!("The restart has already started.", "重开已经开始。").into(),
                ))
            }
        }
    };
    // One restart at a time: another (the daemon's after a turn, or a
    // second click) may have it already. `restore` keeps its hands off
    // too, and the exit is no news.
    // Claimed against the slot as it is now, not as listed at the start.
    let fresh = registry
        .slots()?
        .into_iter()
        .find(|s| s.slot_id == slot.slot_id)
        .unwrap_or_else(|| slot.clone());
    if rebuild.is_some() && super::provider::in_turn(&fresh) {
        return Ok(refused(
            name,
            Some(slot),
            "busy",
            tr!(
                "Wait for this turn to finish, then confirm rebuilding again.",
                "请等本轮结束后，再确认重建窗格。"
            )
            .into(),
        ));
    }
    let claimed_at = now_ms();
    if !registry.claim_restore(&fresh, claimed_at)? {
        drop(direct_execution);
        if rebuild.is_some() {
            return Ok(refused(
                name,
                Some(slot),
                "in-progress",
                tr!(
                    "Another operation owns this session. Confirm rebuilding again later.",
                    "此会话正在被另一项操作处理，请稍后重新确认重建。"
                )
                .into(),
            ));
        }
        // Preserve the request's intent: only automatic --stale targets
        // may be dropped when later model evidence says nothing is pending.
        if !args.from_queue {
            if let Err(error) =
                super::provider::enqueue_policy(&state, &slot.slot_id, stale, args.provider_default)
            {
                if error.kind() != std::io::ErrorKind::WouldBlock {
                    return Err(error.into());
                }
                return Ok(refused(
                    name,
                    Some(slot),
                    "in-progress",
                    tr!("The restart has already started.", "重开已经开始。").into(),
                ));
            }
        }
        return Ok(Outcome {
            name: name.to_string(),
            session_id: Some(slot.session_id.clone()),
            result: "queued",
            reason: Some("in-progress"),
            candidates: Vec::new(),
            rebuild_identity: None,
            message: tr!(
                format!("{name} is being restarted already; it is in the queue."),
                format!("{name} 已经在重开了，先排进队列。")
            ),
        });
    }
    match in_place(
        registry,
        slot,
        &loc.socket,
        &loc.pane_id,
        &argv,
        rebuild.as_ref(),
        claimed_at,
    ) {
        Ok(()) => {
            let confirmed = wait_for_start_in_pane(registry, slot, slot.tmux.as_ref())?;
            if args.from_queue {
                super::provider::dequeue(&state, &slot.slot_id)?;
            }
            drop(direct_execution);
            registry.record(
                &slot.slot_id,
                if confirmed {
                    "restarted"
                } else {
                    "restart-starting"
                },
                &json!({ "now": args.now }),
                now_ms(),
            )?;
            Ok(Outcome {
                name: name.to_string(),
                session_id: Some(slot.session_id.clone()),
                result: if confirmed { "restarted" } else { "starting" },
                reason: None,
                candidates: Vec::new(),
                rebuild_identity: None,
                message: tr!(
                    format!(
                        "{name}: {}. Model API availability has not been checked.",
                        if confirmed {
                            "session started again in its own pane"
                        } else {
                            "starting; waiting for the session to report ready"
                        }
                    ),
                    format!(
                        "{name}：{}；尚未验证模型 API 可用性。",
                        if confirmed {
                            "已在原窗格启动"
                        } else {
                            "正在启动，等待会话就绪"
                        }
                    )
                ),
            })
        }
        Err(e) => {
            registry.clear_restore_claim(&fresh, claimed_at)?;
            if let Some(failure) = e.downcast_ref::<QuitFailure>() {
                registry.record(
                    &slot.slot_id,
                    "restart-failed",
                    &json!({"reason":failure.reason()}),
                    now_ms(),
                )?;
                return Ok(refused(name, Some(slot), failure.reason(), tr!(
                    format!("Couldn't restart {name}: {failure}. The old session has not been confirmed stopped; no other copy was started. Open the session to handle its exit."),
                    format!("未能重开 {name}：{failure}。旧会话尚未确认退出，没有启动另一份。请打开原会话处理退出。")
                )));
            }
            Ok(refused(
                name,
                Some(slot),
                if e.downcast_ref::<StaleConfirmation>().is_some() {
                    "confirmation-stale"
                } else if e.downcast_ref::<RebuildBusy>().is_some() {
                    "busy"
                } else {
                    "failed"
                },
                tr!(
                    format!(
                        "Couldn't restart {name}: {e:#}. Open the session and check its current state before trying again."
                    ),
                    format!(
                        "没能重开 {name}：{e:#}。请打开会话确认当前状态后再操作。"
                    )
                ),
            ))
        }
    }
}

/// A launch command or a live pane is not a SessionStart acknowledgement.
pub fn wait_for_start(registry: &Registry, slot: &Slot) -> Result<bool> {
    wait_for_start_in_pane(registry, slot, None)
}

fn wait_for_start_in_pane(
    registry: &Registry,
    slot: &Slot,
    expected_pane: Option<&moss_core::event::TmuxLoc>,
) -> Result<bool> {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let fresh = registry
            .slots()?
            .into_iter()
            .find(|s| s.slot_id == slot.slot_id);
        if fresh.is_some_and(|s| {
            s.session_id == slot.session_id
                && s.restore_started_at_ms.is_none()
                && s.pid != slot.pid
                && s.effective_status(proc::is_alive) == Status::Running
                && slot.pid.is_none_or(|pid| {
                    proc::liveness(pid, slot.pid_start.as_deref()) == Liveness::Dead
                })
                && expected_pane.is_none_or(|expected| {
                    s.tmux.as_ref().is_some_and(|current| {
                        current.socket == expected.socket && current.pane_id == expected.pane_id
                    })
                })
        }) {
            return Ok(true);
        }
        if Instant::now() >= until {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether any word holds a control character.
fn has_control(argv: &[String]) -> bool {
    argv.iter().any(|w| w.chars().any(char::is_control))
}

fn shell_line(argv: &[String]) -> String {
    argv.iter()
        .map(|w| shell_word(w))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RebuildIdentity {
    slot_id: String,
    session_id: String,
    claude_pid: u32,
    claude_start: String,
    tmux_socket: String,
    pane_id: String,
    pane_pid: u32,
    pane_start: String,
}

impl RebuildIdentity {
    fn to_json(&self) -> Value {
        json!({"slot_id":self.slot_id,"session_id":self.session_id,
            "claude_pid":self.claude_pid,"claude_start":self.claude_start,
            "tmux_socket":self.tmux_socket,"pane_id":self.pane_id,
            "pane_pid":self.pane_pid,"pane_start":self.pane_start})
    }
    fn from_json(text: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(text).ok()?;
        if v.as_object()?.len() != 8 {
            return None;
        }
        let text = |key: &str| -> Option<String> {
            let value = v[key].as_str()?;
            (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_string())
        };
        Some(Self {
            slot_id: text("slot_id")?,
            session_id: text("session_id")?,
            claude_pid: u32::try_from(v["claude_pid"].as_u64()?).ok()?,
            claude_start: text("claude_start")?,
            tmux_socket: text("tmux_socket")?,
            pane_id: text("pane_id")?,
            pane_pid: u32::try_from(v["pane_pid"].as_u64()?).ok()?,
            pane_start: text("pane_start")?,
        })
    }
}

#[derive(Debug)]
struct StaleConfirmation;
impl std::fmt::Display for StaleConfirmation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the pane or session changed; confirm rebuilding again")
    }
}
impl std::error::Error for StaleConfirmation {}

#[derive(Debug)]
struct RebuildBusy;
impl std::fmt::Display for RebuildBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the session started working; wait and confirm rebuilding again")
    }
}
impl std::error::Error for RebuildBusy {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalProcess {
    pid: u32,
    parent: u32,
    group: i64,
    foreground: i64,
    tty: String,
}

fn terminal_process(pid: u32) -> Option<TerminalProcess> {
    let output = moss_core::command::output_with_timeout(
        std::process::Command::new("ps").args([
            "-p",
            &pid.to_string(),
            "-o",
            "pid=,ppid=,pgid=,tpgid=,tty=",
        ]),
        Duration::from_secs(2),
    )?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let values: Vec<_> = text.split_whitespace().collect();
    if values.len() != 5 {
        return None;
    }
    let process = TerminalProcess {
        pid: values[0].parse().ok()?,
        parent: values[1].parse().ok()?,
        group: values[2].parse().ok()?,
        foreground: values[3].parse().ok()?,
        tty: values[4].to_string(),
    };
    (process.pid == pid
        && process.group > 0
        && process.foreground > 0
        && !matches!(process.tty.as_str(), "?" | "??" | "-"))
    .then_some(process)
}

fn belongs_to_pane(pid: u32, pane: u32, parents: &std::collections::HashMap<u32, u32>) -> bool {
    let mut current = pid;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..16 {
        if current == pane {
            return true;
        }
        if !seen.insert(current) {
            return false;
        }
        let Some(parent) = parents.get(&current).copied() else {
            return false;
        };
        current = parent;
    }
    current == pane
}

fn rebuild_identity(
    slot: &Slot,
    tmux: &Tmux<ProcessRunner>,
    pane: &str,
) -> Option<RebuildIdentity> {
    let loc = slot.tmux.as_ref()?;
    let pid = slot.pid?;
    let (pane_pid, dead) = tmux.pane_process(pane)?;
    if dead
        || pane != loc.pane_id
        || !super::adopt::still_it(slot)
        || !belongs_to_pane(pid, pane_pid, &proc::parents())
    {
        return None;
    }
    let claude_start = proc::start_time(pid)?;
    if slot.pid_start.as_ref() != Some(&claude_start) {
        return None;
    }
    Some(RebuildIdentity {
        slot_id: slot.slot_id.clone(),
        session_id: slot.session_id.clone(),
        claude_pid: pid,
        claude_start,
        tmux_socket: loc.socket.clone(),
        pane_id: pane.to_string(),
        pane_pid,
        pane_start: proc::start_time(pane_pid)?,
    })
}

fn identity_matches(registry: &Registry, slot: &Slot, expected: &RebuildIdentity) -> bool {
    let Ok(slots) = registry.slots() else {
        return false;
    };
    let Some(fresh) = slots.iter().find(|s| s.slot_id == slot.slot_id) else {
        return false;
    };
    let Some(loc) = &fresh.tmux else {
        return false;
    };
    let tmux = Tmux::with_timeout(Socket::Path(loc.socket.clone()), Duration::from_secs(5));
    rebuild_identity(fresh, &tmux, &loc.pane_id).as_ref() == Some(expected)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneState {
    pane: String,
    session: String,
    window: String,
    server_pid: u32,
    server_start: String,
    root_pid: Option<u32>,
    dead: bool,
}
impl PaneState {
    fn parse(text: &str, pane: &str, server_start: String) -> Option<Self> {
        if text.lines().count() != 1 {
            return None;
        }
        let fields: Vec<_> = text.trim_end_matches('\n').split('|').collect();
        if fields.len() != 6 || fields[0] != pane {
            return None;
        }
        let identifier = |value: &str, prefix: char| {
            value.strip_prefix(prefix).is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
            })
        };
        if !identifier(fields[0], '%') || !identifier(fields[1], '$') || !identifier(fields[2], '@')
        {
            return None;
        }
        let dead = match fields[5] {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        let root_pid = if fields[4].is_empty() && dead {
            None
        } else {
            Some(fields[4].parse::<u32>().ok().filter(|pid| *pid > 0)?)
        };
        let server_pid = fields[3].parse::<u32>().ok().filter(|pid| *pid > 0)?;
        Some(Self {
            pane: fields[0].into(),
            session: fields[1].into(),
            window: fields[2].into(),
            server_pid,
            server_start,
            root_pid,
            dead,
        })
    }
    fn same_topology(&self, other: &Self) -> bool {
        self.pane == other.pane
            && self.session == other.session
            && self.window == other.window
            && self.server_pid == other.server_pid
            && self.server_start == other.server_start
    }
}

fn pane_state(tmux: &Tmux<ProcessRunner>, pane: &str) -> Option<PaneState> {
    let args = [
        "display-message",
        "-p",
        "-t",
        pane,
        "#{pane_id}|#{session_id}|#{window_id}|#{pid}|#{pane_pid}|#{pane_dead}",
    ];
    let text = tmux
        .runner()
        .run(&args.into_iter().map(str::to_string).collect::<Vec<_>>())
        .ok()?;
    let fields: Vec<_> = text.trim_end_matches('\n').split('|').collect();
    if fields.len() != 6 {
        return None;
    }
    let server_pid = fields[3].parse().ok()?;
    PaneState::parse(&text, pane, proc::start_time(server_pid)?)
}

/// A dead pane no longer exposes its root PID. Bind it to the exact server,
/// pane, session and window observed before exit, and prove both old processes
/// stopped. A live pane must still have the original root identity.
fn stopped_rebuild_matches(
    registry: &Registry,
    slot: &Slot,
    identity: &RebuildIdentity,
    claimed_at: i64,
    tmux: &Tmux<ProcessRunner>,
    before: &PaneState,
) -> bool {
    let until = Instant::now() + Duration::from_secs(1);
    loop {
        if proc::liveness(identity.claude_pid, Some(&identity.claude_start)) != Liveness::Dead {
            return false;
        }
        let Ok(slots) = registry.slots() else {
            return false;
        };
        let Some(current) = slots.iter().find(|s| s.slot_id == identity.slot_id) else {
            return false;
        };
        if current.session_id != identity.session_id
            || current.pid != slot.pid
            || current.pid_start != slot.pid_start
            || current.restore_started_at_ms != Some(claimed_at)
            || current.tmux.as_ref().is_none_or(|loc| {
                loc.socket != identity.tmux_socket || loc.pane_id != identity.pane_id
            })
        {
            return false;
        }
        let Some(now) = pane_state(tmux, &identity.pane_id) else {
            return false;
        };
        if !before.same_topology(&now) {
            return false;
        }
        let root = proc::liveness(identity.pane_pid, Some(&identity.pane_start));
        if root == Liveness::Unknown {
            return false;
        }
        if now.dead {
            return root == Liveness::Dead
                && now.root_pid.is_none_or(|pid| pid == identity.pane_pid);
        }
        if now.root_pid != Some(identity.pane_pid) {
            return false;
        }
        if root == Liveness::Alive {
            return true;
        }
        // tmux can observe child exit slightly later than the OS does.
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

struct ShellTarget {
    process: TerminalProcess,
    start: String,
    pane: TerminalProcess,
    pane_start: String,
}
impl ShellTarget {
    fn same_process(&self) -> bool {
        proc::start_time(self.process.pid).as_deref() == Some(self.start.as_str())
            && proc::start_time(self.pane.pid).as_deref() == Some(self.pane_start.as_str())
    }
    fn ready(&self) -> bool {
        self.same_process()
            && interactive_shell(self.process.pid)
            && terminal_process(self.process.pid).is_some_and(|p| {
                p.tty == self.process.tty
                    && p.group == self.process.group
                    && p.foreground == p.group
            })
    }
    fn before_quit(&self, pid: u32) -> bool {
        self.same_process()
            && interactive_shell(self.process.pid)
            && belongs_to_pane(self.process.pid, self.pane.pid, &proc::parents())
            && terminal_process(self.pane.pid).is_some_and(|p| p.tty == self.process.tty)
            && terminal_process(pid).is_some_and(|p| {
                p.parent == self.process.pid
                    && p.tty == self.process.tty
                    && p.group == p.foreground
                    && p.group != self.process.group
            })
    }
}

enum LaunchTarget {
    Direct,
    Shell(ShellTarget),
    Proxy(Option<RebuildIdentity>),
}

fn interactive_shell(pid: u32) -> bool {
    if !program_of(pid).is_some_and(|p| is_shell(&p)) {
        return false;
    }
    let Some(argv) = moss_core::launch_args::argv(pid) else {
        return false;
    };
    interactive_shell_argv(&argv)
}

fn interactive_shell_argv(argv: &[String]) -> bool {
    !argv.is_empty()
        && is_shell(&argv[0])
        && argv.iter().skip(1).all(|arg| {
            matches!(
                arg.as_str(),
                "-i" | "-l" | "-il" | "-li" | "--login" | "--norc" | "--noprofile" | "--no-config"
            )
        })
}

fn same_foreground_terminal(
    claude: &TerminalProcess,
    shell: &TerminalProcess,
    pane: &TerminalProcess,
) -> bool {
    claude.parent == shell.pid
        && claude.tty == shell.tty
        && claude.tty == pane.tty
        && claude.group > 0
        && claude.group == claude.foreground
        && claude.group != shell.group
}

fn launch_target(slot: &Slot, tmux: &Tmux<ProcessRunner>, pane: &str) -> Result<LaunchTarget> {
    let (pane_pid, dead) = tmux
        .pane_process(pane)
        .ok_or_else(|| anyhow::anyhow!("the recorded pane is gone"))?;
    let pid = slot
        .pid
        .ok_or_else(|| anyhow::anyhow!("unknown session process"))?;
    if dead || !super::adopt::still_it(slot) {
        anyhow::bail!("the session process changed");
    }
    if pid == pane_pid {
        return Ok(LaunchTarget::Direct);
    }
    if slot.pid_start.is_none() || proc::start_time(pid).as_ref() != slot.pid_start.as_ref() {
        anyhow::bail!("the session's recorded process identity cannot be verified");
    }
    if !belongs_to_pane(pid, pane_pid, &proc::parents()) {
        anyhow::bail!("the session is not a descendant of its recorded pane");
    }
    let claude = terminal_process(pid)
        .ok_or_else(|| anyhow::anyhow!("cannot verify the session terminal"))?;
    let root = terminal_process(pane_pid)
        .ok_or_else(|| anyhow::anyhow!("cannot verify the pane terminal"))?;
    if claude.tty != root.tty {
        return Ok(LaunchTarget::Proxy(rebuild_identity(slot, tmux, pane)));
    }
    let shell = terminal_process(claude.parent)
        .ok_or_else(|| anyhow::anyhow!("cannot verify the parent terminal"))?;
    if !interactive_shell(shell.pid) || !same_foreground_terminal(&claude, &shell, &root) {
        anyhow::bail!("the session is not the foreground job of a supported interactive shell");
    }
    let start =
        proc::start_time(shell.pid).ok_or_else(|| anyhow::anyhow!("unknown shell identity"))?;
    let pane_start =
        proc::start_time(root.pid).ok_or_else(|| anyhow::anyhow!("unknown pane identity"))?;
    Ok(LaunchTarget::Shell(ShellTarget {
        process: shell,
        start,
        pane: root,
        pane_start,
    }))
}

/// Quit Claude Code in its pane and start `argv` there again.
fn in_place(
    registry: &Registry,
    slot: &Slot,
    socket: &str,
    pane: &str,
    argv: &[String],
    rebuild: Option<&RebuildIdentity>,
    claimed_at: i64,
) -> Result<()> {
    let tmux = Tmux::with_timeout(Socket::Path(socket.to_string()), Duration::from_secs(5));
    if let Some(identity) = rebuild {
        if !identity_matches(registry, slot, identity) {
            return Err(StaleConfirmation.into());
        }
        let fresh = registry
            .slots()?
            .into_iter()
            .find(|s| s.slot_id == slot.slot_id)
            .ok_or(StaleConfirmation)?;
        if super::provider::in_turn(&fresh) {
            return Err(RebuildBusy.into());
        }
        let topology = pane_state(&tmux, pane)
            .filter(|state| !state.dead && state.root_pid == Some(identity.pane_pid))
            .ok_or(StaleConfirmation)?;
        let before = tmux.pane_option(pane, "remain-on-exit");
        tmux.set_pane_option(pane, "remain-on-exit", Some("on"))?;
        if !identity_matches(registry, &fresh, identity) {
            let _ = tmux.set_pane_option(pane, "remain-on-exit", before.as_deref());
            return Err(StaleConfirmation.into());
        }
        // Ending a PTY wrapper is not evidence that its detached child exited.
        // Ask the identity-bound Claude itself to exit before touching the wrapper.
        quit(&fresh, identity.claude_pid)?;
        if !stopped_rebuild_matches(registry, &fresh, identity, claimed_at, &tmux, &topology) {
            return Err(StaleConfirmation.into());
        }
        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
        let done = tmux.respawn_pane(pane, &slot.cwd, &words, true);
        let _ = tmux.set_pane_option(pane, "remain-on-exit", before.as_deref());
        return done;
    }
    let pid = slot
        .pid
        .ok_or_else(|| anyhow::anyhow!("unknown session process"))?;
    match launch_target(slot, &tmux, pane)? {
        LaunchTarget::Shell(shell) => {
            // Re-check immediately before signalling; a PID match alone is not identity.
            if tmux.pane_process(pane) != Some((shell.pane.pid, false))
                || !super::adopt::still_it(slot)
                || !shell.before_quit(pid)
            {
                anyhow::bail!("the foreground shell or session changed; nothing was stopped");
            }
            quit(slot, pid)?;
            let until = Instant::now() + Duration::from_secs(2);
            while !shell.ready() {
                if tmux.pane_process(pane) != Some((shell.pane.pid, false))
                    || !shell.same_process()
                    || Instant::now() >= until
                {
                    anyhow::bail!(
                        "the original shell did not regain the foreground; no command was entered"
                    );
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            let line = format!("cd {} && {}", shell_word(&slot.cwd), shell_line(argv));
            if tmux.pane_process(pane) != Some((shell.pane.pid, false)) || !shell.ready() {
                anyhow::bail!("the foreground or pane mapping changed; no command was entered");
            }
            return tmux.type_line(pane, &line);
        }
        LaunchTarget::Proxy(_) => {
            anyhow::bail!("PTY forwarding requires explicit pane rebuilding confirmation")
        }
        LaunchTarget::Direct => {}
    }
    let identity = rebuild_identity(slot, &tmux, pane).ok_or(StaleConfirmation)?;
    let topology = pane_state(&tmux, pane)
        .filter(|state| !state.dead && state.root_pid == Some(identity.pane_pid))
        .ok_or(StaleConfirmation)?;
    let before = tmux.pane_option(pane, "remain-on-exit");
    tmux.set_pane_option(pane, "remain-on-exit", Some("on"))?;
    if !identity_matches(registry, slot, &identity) {
        return Err(StaleConfirmation.into());
    }
    quit(slot, pid)?;
    // A different controller may have replaced the pane during the wait.
    // The old PID being dead does not authorize killing that replacement.
    if !stopped_rebuild_matches(registry, slot, &identity, claimed_at, &tmux, &topology) {
        return Err(StaleConfirmation.into());
    }
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let done = tmux.respawn_pane(pane, &slot.cwd, &words, true);
    // Put back once the pane runs again, or after a failed respawn (a dead
    // pane stays until it is respawned or closed).
    let _ = tmux.set_pane_option(pane, "remain-on-exit", before.as_deref());
    done
}

/// The program a process runs (`ps -o comm=`).
fn program_of(pid: u32) -> Option<String> {
    let out = moss_core::command::output_with_timeout(
        std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]),
        Duration::from_secs(2),
    )?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether `comm` (a path or a name, `-zsh` for a login shell) is an
/// interactive shell a line can be typed into.
fn is_shell(comm: &str) -> bool {
    let name = comm.rsplit('/').next().unwrap_or(comm);
    let name = name.strip_prefix('-').unwrap_or(name);
    matches!(
        name,
        "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "nu"
    )
}

/// SIGTERM, then wait for it to go (Claude Code exits from idle in about
/// half a second).
fn quit(slot: &Slot, pid: u32) -> Result<()> {
    if slot.pid != Some(pid)
        || slot.pid_start.is_none()
        || proc::liveness(pid, slot.pid_start.as_deref()) != Liveness::Alive
        || !super::adopt::still_it(slot)
    {
        return Err(QuitFailure::Unknown.into());
    }
    stop_and_wait(
        quit_wait(),
        || {
            // SAFETY: only the identity-checked session PID, never a group or wrapper.
            if unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        },
        || proc::liveness(pid, slot.pid_start.as_deref()),
    )
    .map_err(Into::into)
}

fn stop_and_wait(
    wait: Duration,
    signal: impl FnOnce() -> std::io::Result<()>,
    mut observe: impl FnMut() -> Liveness,
) -> std::result::Result<(), QuitFailure> {
    signal().map_err(QuitFailure::Signal)?;
    let until = Instant::now() + wait;
    loop {
        match observe() {
            Liveness::Dead => return Ok(()),
            Liveness::Unknown => return Err(QuitFailure::Unknown),
            Liveness::Alive if Instant::now() >= until => return Err(QuitFailure::Timeout),
            Liveness::Alive => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(args: &[&str]) -> Slot {
        Slot {
            slot_id: "s1".into(),
            agent: "claude".into(),
            session_id: "0837a145-0000".into(),
            session_ids: vec![],
            name: Some("api".into()),
            cwd: "/w".into(),
            pid: Some(42),
            pid_start: Some("x".into()),
            tmux: None,
            status: Status::Running,
            end_reason: None,
            notes: Default::default(),
            launch_args: args.iter().map(|a| a.to_string()).collect(),
            transcript_path: None,
            busy: false,
            attention: None,
            last_prompt: None,
            restore_started_at_ms: None,
            role: None,
            created_at_ms: 0,
            last_active_at_ms: 0,
            ended_at_ms: None,
            archived_at_ms: None,
            archived_by: None,
            provider_started: None,
            provider_seen_keys: None,
        }
    }

    #[test]
    fn restart_drops_model_when_settings_pick_one() {
        let s = slot(&[
            "--dangerously-skip-permissions",
            "--model",
            "opus",
            "--effort",
            "high",
        ]);
        // Without a model in the settings, the session keeps its own.
        assert_eq!(
            restart_argv("claude", &s),
            [
                "claude",
                "--resume",
                "0837a145-0000",
                "--name=api",
                "--dangerously-skip-permissions",
                "--model",
                "opus",
                "--effort",
                "high"
            ]
        );
    }

    #[test]
    fn restart_terminal_failure_signal_error_is_immediate() {
        let result = stop_and_wait(
            Duration::ZERO,
            || Err(std::io::Error::from_raw_os_error(libc::EPERM)),
            || panic!("signal failure must not start waiting"),
        );
        assert_eq!(result.unwrap_err().reason(), "signal-failed");
    }

    #[test]
    fn restart_terminal_failure_unknown_is_not_exit_or_timeout() {
        assert_eq!(
            stop_and_wait(Duration::ZERO, || Ok(()), || Liveness::Unknown)
                .unwrap_err()
                .reason(),
            "process-unknown"
        );
        assert_eq!(
            stop_and_wait(Duration::ZERO, || Ok(()), || Liveness::Alive)
                .unwrap_err()
                .reason(),
            "quit-timeout"
        );
        assert!(stop_and_wait(Duration::ZERO, || Ok(()), || Liveness::Dead).is_ok());
    }

    #[test]
    fn restart_terminal_failure_dead_pane_parser_and_topology_are_strict() {
        let before = PaneState::parse("%1|$2|@3|10|20|0\n", "%1", "server-start".into()).unwrap();
        let dead = PaneState::parse("%1|$2|@3|10||1\n", "%1", "server-start".into()).unwrap();
        assert!(before.same_topology(&dead));
        assert!(dead.dead && dead.root_pid.is_none());
        for changed in [
            "%9|$2|@3|10||1",
            "%1|$9|@3|10||1",
            "%1|$2|@9|10||1",
            "%1|$2|@3|99||1",
        ] {
            let now = PaneState::parse(
                changed,
                changed.split('|').next().unwrap(),
                "server-start".into(),
            )
            .unwrap();
            assert!(!before.same_topology(&now));
        }
        let reused = PaneState::parse("%1|$2|@3|10||1", "%1", "new-server-start".into()).unwrap();
        assert!(!before.same_topology(&reused));
        for invalid in [
            "%1|$2|@3|10||0",
            "%1|$2|@3|0||1",
            "%1|bad|@3|10||1",
            "%1|$2|@3|10||1|extra",
            "%1|$2|@3|10||1\n%1|$2|@3|10||1",
        ] {
            assert!(PaneState::parse(invalid, "%1", "start".into()).is_none());
        }
    }

    #[test]
    fn nested_restart_ancestor_guard_is_bounded_and_detects_cycles() {
        let mut parents = std::collections::HashMap::from([(3, 2), (2, 1)]);
        assert!(belongs_to_pane(3, 1, &parents));
        assert!(!belongs_to_pane(3, 99, &parents));
        parents.insert(1, 3);
        assert!(!belongs_to_pane(3, 99, &parents));
        let parents = (1..=18).map(|pid| (pid, pid + 1)).collect();
        assert!(belongs_to_pane(1, 17, &parents));
        assert!(!belongs_to_pane(1, 18, &parents));
    }

    #[test]
    fn nested_restart_shell_argv_is_not_a_script_or_unknown_wrapper() {
        for args in [
            vec!["bash", "--norc", "--noprofile"],
            vec!["/bin/zsh", "-i"],
            vec!["-bash"],
        ] {
            assert!(interactive_shell_argv(
                &args.into_iter().map(str::to_string).collect::<Vec<_>>()
            ));
        }
        for args in [
            vec!["bash", "-c", "cat"],
            vec!["bash", "script.sh"],
            vec!["python3"],
            vec!["zsh (kiro-cli-term)"],
        ] {
            assert!(!interactive_shell_argv(
                &args.into_iter().map(str::to_string).collect::<Vec<_>>()
            ));
        }
    }

    #[test]
    fn nested_restart_foreground_and_tty_guard() {
        let shell = TerminalProcess {
            pid: 2,
            parent: 1,
            group: 2,
            foreground: 3,
            tty: "pts/1".into(),
        };
        let pane = TerminalProcess {
            pid: 1,
            parent: 0,
            group: 1,
            foreground: 3,
            tty: "pts/1".into(),
        };
        let mut claude = TerminalProcess {
            pid: 3,
            parent: 2,
            group: 3,
            foreground: 3,
            tty: "pts/1".into(),
        };
        assert!(same_foreground_terminal(&claude, &shell, &pane));
        claude.foreground = 4;
        assert!(!same_foreground_terminal(&claude, &shell, &pane));
        claude.foreground = 3;
        claude.tty = "pts/2".into();
        assert!(!same_foreground_terminal(&claude, &shell, &pane));
    }

    #[test]
    fn nested_restart_shell_identity_change_is_rejected() {
        let pid = std::process::id();
        let process = TerminalProcess {
            pid,
            parent: 1,
            group: 1,
            foreground: 1,
            tty: "pts/1".into(),
        };
        let target = ShellTarget {
            process: process.clone(),
            start: "wrong-start".into(),
            pane: process,
            pane_start: "wrong-start".into(),
        };
        assert!(!target.same_process());
        assert!(!target.ready());
    }

    #[test]
    fn rebuild_identity_rejects_missing_and_unknown_fields() {
        assert!(RebuildIdentity::from_json("{}").is_none());
        assert!(RebuildIdentity::from_json(r#"{"slot_id":"s","session_id":"id","claude_pid":1,"claude_start":"start","tmux_socket":"/s","pane_id":"%1","pane_pid":2,"pane_start":"start","extra":true}"#).is_none());
    }

    #[test]
    fn provider_model_opusplan_is_not_replaced_by_observed_model() {
        let mut s = slot(&["--model", "opusplan"]);
        s.provider_started =
            Some(r#"{"keys":[],"source":"/nonexistent/channel/settings.json"}"#.into());
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            r#"{"type":"assistant","message":{"model":"sonnet"}}"#,
        )
        .unwrap();
        s.transcript_path = Some(file.path().to_string_lossy().into_owned());
        assert!(!resume_argv("claude", &s, false)
            .unwrap()
            .iter()
            .any(|arg| arg == "--model"));
    }

    #[test]
    fn provider_model_control_cwd_is_rejected() {
        let mut s = slot(&[]);
        s.cwd = "/nonexistent/work\nunsafe".into();
        s.provider_started =
            Some(r#"{"keys":[],"source":"/nonexistent/channel/settings.json"}"#.into());
        assert!(super::super::provider::model_plan(&s).is_err());
    }

    #[test]
    fn restart_types_only_into_a_shell() {
        for shell in [
            "zsh",
            "-zsh",
            "/bin/bash",
            "fish",
            "/usr/local/bin/nu",
            "sh",
        ] {
            assert!(is_shell(shell), "{shell}");
        }
        for other in ["node", "python3", "claude", "tmux", "vim", ""] {
            assert!(!is_shell(other), "{other}");
        }
    }

    #[test]
    fn restart_refuses_a_control_character() {
        let mut s = slot(&[]);
        assert!(!has_control(&restart_argv("claude", &s)));
        s.name = Some("api\u{1b}[2J".into());
        assert!(has_control(&restart_argv("claude", &s)));
    }

    #[test]
    fn restart_types_a_line_a_shell_reads_back() {
        assert_eq!(shell_word("--name=api"), "--name=api");
        assert_eq!(shell_word("my notes"), "'my notes'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("a;rm -rf"), "'a;rm -rf'");
    }
}
