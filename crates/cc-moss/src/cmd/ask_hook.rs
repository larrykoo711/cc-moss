//! The hooks that let a question a session waits on be answered from
//! outside (T1.3; design in `.sdd/004-moss-agent/notes/T1.3-design.md`).
//!
//! - PreToolUse (AskUserQuestion): notes the question and returns at once.
//! - PermissionRequest: the only hook that waits. It waits only when asking
//!   is on and someone takes questions; it prints one decision when a valid
//!   answer arrives, and nothing otherwise, so the session falls back to its
//!   own dialog, which stays usable all along.
//! - [`withdraw`]: called from the observing hooks; the session moved on, so
//!   its open questions are withdrawn.
//!
//! Every error is returned to `hook::run`, which logs it and exits 0 silently.

use std::io::Write;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Result};
use moss_core::agent::HookKind;
use moss_core::ask::{self, Ask, Moved, NewAsk, Poll, Reason, Store};
use moss_core::config::Config;
use moss_core::registry::Registry;
use moss_core::slot::now_ms;
use moss_core::store_dir;
use serde_json::Value;

/// The fields of a PreToolUse / PermissionRequest payload cc-moss uses.
struct Payload {
    session_id: String,
    cwd: String,
    prompt_id: String,
    tool_name: String,
    tool_input: Value,
    tool_use_id: Option<String>,
}

impl Payload {
    fn parse(text: &str) -> Result<Self> {
        let mut v: Value = serde_json::from_str(text)?;
        let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(String::from);
        let session_id = s(&v, "session_id").unwrap_or_default();
        if session_id.is_empty() {
            bail!("hook payload has no session_id");
        }
        Ok(Payload {
            cwd: s(&v, "cwd").unwrap_or_default(),
            prompt_id: s(&v, "prompt_id").unwrap_or_default(),
            tool_name: s(&v, "tool_name").unwrap_or_default(),
            tool_use_id: s(&v, "tool_use_id"),
            tool_input: v
                .get_mut("tool_input")
                .map(Value::take)
                .unwrap_or(Value::Null),
            session_id,
        })
    }
}

/// PreToolUse or PermissionRequest.
pub fn handle(kind: HookKind, payload: &str) -> Result<()> {
    let config = Config::load()?;
    if !config.ask.enabled {
        return Ok(());
    }
    let state = store_dir::state_dir();
    if !state.is_absolute() {
        return Ok(());
    }
    let store = Store::new(&state);
    if !store.has_taker(SystemTime::now()) {
        return Ok(());
    }
    // Someone else's or open to others: anyone could plant an answer.
    if !store.is_private() {
        bail!(
            "{} is not private to this user; not taking questions",
            store.dir().display()
        );
    }
    let p = Payload::parse(payload)?;
    let now = now_ms();
    let wait_secs = ask::clamp_wait(config.ask.wait_secs);
    let fresh = |session_name| NewAsk {
        id: ask::new_id(now, std::process::id()),
        session_id: &p.session_id,
        prompt_id: &p.prompt_id,
        tool_use_id: p.tool_use_id.as_deref(),
        session_name,
        cwd: &p.cwd,
        now_ms: now,
        wait_secs,
    };
    let is_question = p.tool_name == ask::ASK_TOOL;
    match kind {
        HookKind::PreToolUse => {
            // Note the question; never wait here: the session's dialog only
            // appears once this hook is done. The session's name is filled
            // in by PermissionRequest, so this stays off the registry.
            if let Some(ask) = is_question
                .then(|| fresh(None).question(&p.tool_input))
                .flatten()
            {
                store.create(&ask)?;
            }
            Ok(())
        }
        HookKind::PermissionRequest => {
            let name = session_name(&p.session_id);
            let ask = if is_question {
                let Some(questions) = p.tool_input.get("questions").and_then(Value::as_array)
                else {
                    return Ok(());
                };
                match store.recent_question(&p.session_id, &p.prompt_id, questions, now) {
                    Some(mut seen) => {
                        if seen.session_name.is_none() && name.is_some() {
                            seen.session_name = name;
                            store.create(&seen)?;
                        }
                        seen
                    }
                    None => {
                        let Some(ask) = fresh(name).question(&p.tool_input) else {
                            return Ok(());
                        };
                        store.create(&ask)?;
                        ask
                    }
                }
            } else {
                let ask = fresh(name).permission(&p.tool_name, &p.tool_input);
                store.create(&ask)?;
                ask
            };
            store.sweep(now);
            wait(&store, &ask, Duration::from_secs(wait_secs))
        }
        _ => Ok(()),
    }
}

/// Wait for an answer, a withdrawal or the deadline, looking every
/// [`ask::POLL`]. Prints the decision only for an answer that fits, and
/// only after marking the question answered.
fn wait(store: &Store, ask: &Ask, wait: Duration) -> Result<()> {
    let deadline = Instant::now() + wait;
    loop {
        match store.poll(ask, now_ms())? {
            Poll::Deliver(out) => {
                let mut stdout = std::io::stdout().lock();
                writeln!(stdout, "{out}")?;
                stdout.flush()?;
                return Ok(());
            }
            Poll::Gone => return Ok(()),
            Poll::Wait => {}
        }
        if Instant::now() >= deadline {
            store.close(&ask.id, Reason::Timeout)?;
            return Ok(());
        }
        std::thread::sleep(ask::POLL);
    }
}

/// The session's name in the registry, if it has one. Best effort.
fn session_name(session_id: &str) -> Option<String> {
    let registry = Registry::open(&store_dir::registry_path()).ok()?;
    registry
        .slots()
        .ok()?
        .into_iter()
        .find(|s| s.session_id == session_id || s.session_ids.iter().any(|i| i == session_id))?
        .name
}

/// The session moved on: withdraw its open questions. Cheap when none is
/// standing, which is nearly always (closed ones are moved out of the way).
pub fn withdraw(kind: HookKind, payload: &str) -> Result<()> {
    let reason = match kind {
        HookKind::PostToolUse | HookKind::UserPromptSubmit | HookKind::Stop => {
            Reason::AnsweredInSession
        }
        HookKind::SessionEnd => Reason::SessionEnded,
        _ => return Ok(()),
    };
    let state = store_dir::state_dir();
    if !state.is_absolute() {
        return Ok(());
    }
    let store = Store::new(&state);
    if !store.any() {
        return Ok(());
    }
    let v: Value = serde_json::from_str(payload)?;
    let field = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
    let null = Value::Null;
    // After a tool ran, only the question or request for that call.
    let moved = if kind == HookKind::PostToolUse {
        Moved::Tool {
            name: field("tool_name"),
            input: v.get("tool_input").unwrap_or(&null),
        }
    } else {
        Moved::All
    };
    store.withdraw_session(field("session_id"), moved, reason, now_ms())?;
    Ok(())
}
