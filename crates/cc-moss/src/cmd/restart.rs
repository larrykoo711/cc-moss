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
use moss_core::tmux::{Socket, Tmux};
use serde_json::{json, Value};

use super::ls::JSON_SCHEMA;

/// How long Claude Code has to quit.
const QUIT: Duration = Duration::from_secs(10);

pub struct RestartArgs {
    pub names: Vec<String>,
    pub now: bool,
    pub yes: bool,
    pub stale: bool,
    pub json: bool,
}

/// One session's result.
struct Outcome {
    name: String,
    session_id: Option<String>,
    result: &'static str,
    reason: Option<&'static str>,
    message: String,
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
        v
    }
}

/// The command Claude Code is started with again: the same conversation,
/// its name, and the launch arguments worth replaying, without `--model`
/// when the settings pick a model (it would override the provider's).
pub fn restart_argv(claude: &str, slot: &Slot, settings_pick_model: bool) -> Vec<String> {
    let mut replay = moss_core::launch_args::whitelist(
        &std::iter::once("claude")
            .chain(slot.launch_args.iter().map(String::as_str))
            .collect::<Vec<_>>(),
    );
    if settings_pick_model {
        replay = without_model(&replay);
    }
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
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let settings = super::provider::current();
    let mut targets: Vec<(String, Option<Slot>, String)> = Vec::new();
    for name in &args.names {
        match super::resolve(&slots, name) {
            Ok(s) => targets.push((name.clone(), Some(s.clone()), String::new())),
            Err(e) => targets.push((name.clone(), None, e.to_string())),
        }
    }
    if args.stale {
        for slot in &slots {
            let status = slot.effective_status(proc::is_alive);
            let stale = !slot.is_moss()
                && super::provider::restart_reason(slot, status, settings.as_ref()).is_some();
            let listed = targets
                .iter()
                .any(|(_, s, _)| s.as_ref().is_some_and(|s| s.slot_id == slot.slot_id));
            if stale && !listed {
                targets.push((
                    super::restore::display_name(slot),
                    Some(slot.clone()),
                    String::new(),
                ));
            }
        }
    }
    let picks_model = settings.as_ref().is_some_and(|s| s.picks_model());
    let mut outcomes = Vec::new();
    for (name, slot, why) in targets {
        let Some(slot) = slot else {
            outcomes.push(refused(&name, None, "not-found", why));
            continue;
        };
        outcomes.push(restart_one(&registry, &name, &slot, &args, picks_model)?);
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
    }
}

fn restart_one(
    registry: &Registry,
    name: &str,
    slot: &Slot,
    args: &RestartArgs,
    picks_model: bool,
) -> Result<Outcome> {
    let state = store_dir::state_dir();
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
    if super::provider::in_turn(slot) {
        if !args.now {
            super::provider::enqueue(&state, &slot.slot_id)?;
            return Ok(Outcome {
                name: name.to_string(),
                session_id: Some(slot.session_id.clone()),
                result: "queued",
                reason: None,
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
    let argv = restart_argv(&claude, slot, picks_model);
    // One restart at a time: another (the daemon's after a turn, or a
    // second click) may have it already. `restore` keeps its hands off
    // too, and the exit is no news.
    if !registry.claim_restore(slot, now_ms())? {
        return Ok(Outcome {
            name: name.to_string(),
            session_id: Some(slot.session_id.clone()),
            result: "queued",
            reason: None,
            message: tr!(
                format!("{name} is being restarted already."),
                format!("{name} 已经在重开了。")
            ),
        });
    }
    match in_place(slot, &loc.socket, &loc.pane_id, &argv) {
        Ok(()) => {
            let _ = super::provider::dequeue(&state, &slot.slot_id);
            registry.record(
                &slot.slot_id,
                "restarted",
                &json!({ "now": args.now }),
                now_ms(),
            )?;
            Ok(Outcome {
                name: name.to_string(),
                session_id: Some(slot.session_id.clone()),
                result: "restarted",
                reason: None,
                message: tr!(
                    format!("Restarted {name} in its own pane; it uses the new settings."),
                    format!("已在原来的窗格里重开 {name}，它现在用的是新设置。")
                ),
            })
        }
        Err(e) => {
            let _ = registry.clear_restore_started(&slot.slot_id);
            Ok(refused(
                name,
                Some(slot),
                "failed",
                tr!(
                    format!(
                        "Couldn't restart {name}: {e:#}. In its pane, run: {}",
                        shell_line(&argv)
                    ),
                    format!(
                        "没能重开 {name}：{e:#}。可以在它的窗格里运行：{}",
                        shell_line(&argv)
                    )
                ),
            ))
        }
    }
}

fn shell_line(argv: &[String]) -> String {
    argv.iter()
        .map(|w| shell_word(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quit Claude Code in its pane and start `argv` there again.
fn in_place(slot: &Slot, socket: &str, pane: &str, argv: &[String]) -> Result<()> {
    let tmux = Tmux::with_timeout(Socket::Path(socket.to_string()), Duration::from_secs(5));
    let Some((pane_pid, dead)) = tmux.pane_process(pane) else {
        anyhow::bail!("its tmux pane {pane} is gone");
    };
    let Some(pid) = slot.pid else {
        anyhow::bail!("cc-moss doesn't know its process");
    };
    if dead || !super::adopt::still_it(slot) {
        anyhow::bail!("its process isn't that Claude Code any more");
    }
    // Claude Code is the pane's program, or a shell's child in it.
    let under_shell = if pane_pid == pid {
        false
    } else if proc::parents().get(&pid) == Some(&pane_pid) {
        true
    } else {
        anyhow::bail!("it doesn't run in the pane cc-moss recorded");
    };
    if under_shell {
        quit(slot, pid)?;
        let words: Vec<String> = argv.iter().map(|w| shell_word(w)).collect();
        return tmux.type_line(pane, &words.join(" "));
    }
    let before = tmux.pane_option(pane, "remain-on-exit");
    tmux.set_pane_option(pane, "remain-on-exit", Some("on"))?;
    let done = quit(slot, pid).and_then(|()| {
        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
        tmux.respawn_pane(pane, &slot.cwd, &words)
    });
    // Put back, also after a failure: a dead pane stays until it is
    // respawned or closed.
    let _ = tmux.set_pane_option(pane, "remain-on-exit", before.as_deref());
    done
}

/// SIGTERM, then wait for it to go (Claude Code exits from idle in about
/// half a second).
fn quit(slot: &Slot, pid: u32) -> Result<()> {
    // SAFETY: a signal to a pid just checked to be this Claude Code.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let until = Instant::now() + QUIT;
    while proc::liveness(pid, slot.pid_start.as_deref()) == Liveness::Alive {
        if Instant::now() >= until {
            anyhow::bail!("it didn't quit within 10 seconds");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
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
        assert_eq!(
            restart_argv("claude", &s, true),
            [
                "claude",
                "--resume",
                "0837a145-0000",
                "--name=api",
                "--dangerously-skip-permissions",
                "--effort",
                "high"
            ]
        );
        // Without a model in the settings, the session keeps its own.
        assert_eq!(
            restart_argv("claude", &s, false),
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
    fn restart_types_a_line_a_shell_reads_back() {
        assert_eq!(shell_word("--name=api"), "--name=api");
        assert_eq!(shell_word("my notes"), "'my notes'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("a;rm -rf"), "'a;rm -rf'");
    }
}
