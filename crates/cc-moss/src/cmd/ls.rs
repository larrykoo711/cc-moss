//! `cc-moss ls`: what is running, and what needs to be restored.

use anyhow::Result;
use moss_core::slot::{now_ms, rfc3339, Slot, Status};
use moss_core::{proc, store_dir, worktree};
use serde_json::{json, Value};
use unicode_width::UnicodeWidthStr;

pub const JSON_SCHEMA: u32 = 1;

/// Closed sessions stay in Moss's snapshot this long, as in the app's search.
const SNAPSHOT_CLOSED_MS: i64 = 30 * 24 * 60 * 60 * 1000;

pub fn run(all: bool, archived: bool, as_json: bool) -> Result<()> {
    // Moss's sandbox can't open the registry: it reads what the daemon
    // wrote for it, the same list without what the sessions said.
    if moss_core::registry::opens_read_only(
        std::env::var(moss_core::moss::ROLE_ENV).ok().as_deref(),
        false,
    ) {
        return run_from_snapshot(all, archived, as_json);
    }
    let registry = crate::reconcile::run()?;
    let mut rows: Vec<(Status, Slot)> = registry
        .slots()?
        .into_iter()
        .map(|s| (s.effective_status(proc::is_alive), s))
        .filter(|(st, s)| shown(*st, s, all, archived, as_json))
        .collect();
    // Most urgent first: interrupted, then running, then closed; newest activity first.
    rows.sort_by_key(|(st, s)| (rank(*st), std::cmp::Reverse(s.last_active_at_ms)));

    if as_json {
        let failures = registry.api_failures()?;
        let evidence = super::provider::current_model_evidence(&registry, &store_dir::state_dir())?;
        let settings = super::provider::current();
        let queue = super::provider::queued(&store_dir::state_dir());
        let mut source_planner = super::restore_source::Planner::current();
        let mut cwd_checks = super::working_directory::Checks::new();
        let sessions: Vec<Value> = rows
            .iter()
            .map(|(st, s)| {
                let mut v = to_json(*st, s);
                v["working_directory"] = cwd_checks.session(s);
                if *st == Status::Interrupted {
                    v["restore_source"] = serde_json::to_value(source_planner.plan(s))
                        .expect("restore source plan is serializable");
                }
                v["api_failure"] = serde_json::to_value(failures.get(&s.slot_id))
                    .expect("an API failure is serializable");
                // Running sessions the provider switch didn't reach (T2.6).
                v["provider_restart"] = super::provider::effective_reason(
                    s,
                    *st,
                    settings.as_ref(),
                    evidence.get(&s.slot_id),
                )
                .filter(|_| !s.is_moss())
                .into();
                v["restart_queued"] = (*st == Status::Running && queue.contains(&s.slot_id)).into();
                v["provider_model"] = serde_json::to_value(
                    super::provider::model_plan(s)
                        .map(|(plan, _)| plan)
                        .unwrap_or_else(|_| moss_core::provider::ModelPlan::unknown()),
                )
                .expect("a model plan is serializable");
                v
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": JSON_SCHEMA,
                "capabilities": {"working_directory": 1, "new_receipt": 1, "exact_restore": 1},
                "state_dir": store_dir::state_dir(),
                "provider": { "host": settings.as_ref().and_then(|s| s.host.clone()) },
                "sessions": sessions,
            }))?
        );
    } else {
        print_table(&rows);
    }
    Ok(())
}

/// Whether `ls` lists a session. The list leaves archived sessions out
/// (`--archived` lists only them); `--json` lists them too, whatever their
/// status, for the app to sort. Closed ones that are not archived need
/// `--all`.
pub fn shown(st: Status, s: &Slot, all: bool, archived: bool, as_json: bool) -> bool {
    shown_fields(st, s.is_archived(), all, archived, as_json)
}

fn shown_fields(st: Status, is_archived: bool, all: bool, archived: bool, as_json: bool) -> bool {
    if archived {
        return is_archived;
    }
    if is_archived {
        return as_json;
    }
    all || st != Status::Closed
}

/// `<Moss's home>/sessions.json`, which the daemon writes for Moss: every
/// session but Moss that is open, archived, or closed in the last 30 days,
/// as `ls --json` shows it, without `last_prompt` (no conversation). Nothing
/// in it changes with the clock, so an unchanged list writes nothing.
pub fn snapshot(rows: &[(Status, Slot)], now: i64, host: Option<&str>) -> String {
    let mut kept: Vec<&(Status, Slot)> = rows
        .iter()
        .filter(|(st, s)| {
            !s.is_moss()
                && (*st != Status::Closed
                    || s.is_archived()
                    || s.ended_at_ms.unwrap_or(s.last_active_at_ms) >= now - SNAPSHOT_CLOSED_MS)
        })
        .collect();
    kept.sort_by_key(|(st, s)| (rank(*st), std::cmp::Reverse(s.last_active_at_ms)));
    let sessions: Vec<Value> = kept
        .iter()
        .map(|(st, s)| {
            let mut v = to_json(*st, s);
            if let Some(o) = v.as_object_mut() {
                o.remove("last_prompt");
            }
            v
        })
        .collect();
    let mut text = serde_json::to_string_pretty(&json!({
        "schema": JSON_SCHEMA,
        "provider": { "host": host },
        "sessions": sessions,
    }))
    .unwrap_or_default();
    text.push('\n');
    text
}

/// `ls` as Moss: from its snapshot, with the same choices of what to list.
fn run_from_snapshot(all: bool, archived: bool, as_json: bool) -> Result<()> {
    use anyhow::Context;
    let path = moss_core::moss::home_dir(&store_dir::state_dir())
        .join(moss_core::moss::home::SESSIONS_JSON);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "cc-moss has not written {} yet (the daemon writes it every minute)",
            path.display()
        )
    })?;
    let mut snap: Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    let sessions: Vec<Value> = snap["sessions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|v| {
            let st = match v["status"].as_str() {
                Some("running") => Status::Running,
                Some("interrupted") => Status::Interrupted,
                _ => Status::Closed,
            };
            shown_fields(st, !v["archived_at"].is_null(), all, archived, as_json)
        })
        .collect();
    if as_json {
        snap["sessions"] = Value::Array(sessions);
        println!("{}", serde_json::to_string_pretty(&snap)?);
    } else {
        let now = now_ms();
        print_lines(
            sessions
                .iter()
                .map(|v| {
                    let text = |p: &str| v.pointer(p).and_then(Value::as_str).map(String::from);
                    let active = text("/last_active_at")
                        .and_then(|t| moss_core::ask::parse_time(&t))
                        .unwrap_or(now);
                    [
                        match v["status"].as_str() {
                            Some("running") => status_word(Status::Running),
                            Some("interrupted") => status_word(Status::Interrupted),
                            _ => status_word(Status::Closed),
                        }
                        .to_string(),
                        text("/name")
                            .unwrap_or_else(|| short_id(&text("/session_id").unwrap_or_default())),
                        ago(now - active),
                        text("/tmux/session_name").unwrap_or_else(|| "-".into()),
                        text("/notes/state")
                            .or_else(|| text("/notes/note"))
                            .unwrap_or_else(|| "-".into()),
                        text("/cwd").unwrap_or_default(),
                    ]
                })
                .collect(),
        );
    }
    Ok(())
}

fn rank(st: Status) -> u8 {
    match st {
        Status::Interrupted => 0,
        Status::Running => 1,
        Status::Closed => 2,
    }
}

fn to_json(st: Status, s: &Slot) -> Value {
    json!({
        "id": s.slot_id,
        "agent": s.agent,
        "name": s.name,
        "status": st.as_str(),
        "session_id": s.session_id,
        "session_ids": s.session_ids,
        "cwd": s.cwd,
        "tmux": s.tmux.as_ref().map(|t| json!({
            "socket": t.socket,
            "session_id": t.session_id,
            "session_name": t.session_name,
            "pane_id": t.pane_id,
        })),
        "pid": s.pid,
        // Claude Code runs it in the background, outside any terminal:
        // `cc-moss attach` opens it, nothing types into it.
        "background": st == Status::Running
            && s.tmux.is_none()
            && super::attach::background_job(s).is_some(),
        "end_reason": s.end_reason,
        "busy": s.busy,
        // Only a live session can be waiting for you.
        "attention": s.attention.as_deref().filter(|_| st == Status::Running),
        "last_prompt": s.last_prompt,
        "notes": { "goal": s.notes.goal, "state": s.notes.state, "note": s.notes.note },
        "created_at": rfc3339(s.created_at_ms),
        "last_active_at": rfc3339(s.last_active_at_ms),
        "ended_at": s.ended_at_ms.map(rfc3339),
        "worktree": worktree::of(&s.cwd).map(|w| json!({ "root": w.root, "repo": w.repo })),
        // `moss` for Moss (added after schema 1 was set; null otherwise).
        "role": s.role,
        // When it was archived; null while it is in the list.
        "archived_at": s.archived_at_ms.map(rfc3339),
    })
}

fn print_table(rows: &[(Status, Slot)]) {
    let now = now_ms();
    print_lines(
        rows.iter()
            .map(|(st, s)| {
                [
                    status_word(*st).to_string(),
                    s.name.clone().unwrap_or_else(|| short_id(&s.session_id)),
                    ago(now - s.last_active_at_ms),
                    s.tmux
                        .as_ref()
                        .and_then(|t| t.session_name.clone())
                        .unwrap_or_else(|| "-".into()),
                    s.notes
                        .state
                        .clone()
                        .or_else(|| s.notes.note.clone())
                        .unwrap_or_else(|| "-".into()),
                    s.cwd.clone(),
                ]
            })
            .collect(),
    );
}

fn print_lines(lines: Vec<[String; 6]>) {
    if lines.is_empty() {
        println!(
            "{}",
            tr!(
                "No sessions yet. Start one with `cc-moss new <name>`, or just type `t`.",
                "还没有会话。用 `cc-moss new <名字>` 开一个，或者直接敲 `t`。"
            )
        );
        return;
    }
    let header = tr!(
        ["STATUS", "NAME", "ACTIVE", "TMUX", "NOTE", "CWD"],
        ["状态", "名字", "活动", "TMUX", "备注", "目录"]
    )
    .map(String::from);
    let mut widths = header.clone().map(|h| h.width());
    for l in &lines {
        for (w, cell) in widths.iter_mut().zip(l) {
            *w = (*w).max(cell.width());
        }
    }
    for l in std::iter::once(&header).chain(&lines) {
        let cells: Vec<String> = l
            .iter()
            .zip(widths)
            .enumerate()
            .map(
                |(i, (c, w))| {
                    if i == 5 {
                        c.clone()
                    } else {
                        super::pad(c, w)
                    }
                },
            )
            .collect();
        println!("{}", cells.join("  "));
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn ago(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

fn status_word(st: Status) -> &'static str {
    match st {
        Status::Running => tr!("running", "运行中"),
        Status::Interrupted => tr!("interrupted", "已中断"),
        Status::Closed => tr!("closed", "已关闭"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(name: &str, status: Status, at: i64) -> Slot {
        Slot {
            slot_id: name.into(),
            agent: "claude".into(),
            session_id: format!("{name}-0000-id"),
            session_ids: vec![],
            name: Some(name.into()),
            cwd: format!("/p/{name}"),
            pid: None,
            pid_start: None,
            tmux: None,
            status,
            end_reason: None,
            notes: Default::default(),
            launch_args: vec![],
            transcript_path: None,
            busy: false,
            attention: None,
            last_prompt: Some(format!("secret words of {name}")),
            restore_started_at_ms: None,
            role: None,
            created_at_ms: 0,
            last_active_at_ms: at,
            ended_at_ms: Some(at),
            archived_at_ms: None,
            archived_by: None,
            provider_started: None,
            provider_seen_keys: None,
        }
    }

    #[test]
    fn snapshot_keeps_what_moss_may_look_for_and_no_conversation() {
        const DAY: i64 = 24 * 60 * 60 * 1000;
        let now = 100 * DAY;
        let mut archived = slot("archived", Status::Closed, now - 90 * DAY);
        archived.archived_at_ms = Some(now - 60 * DAY);
        let mut moss = slot("moss", Status::Running, now);
        moss.role = Some("moss".into());
        let rows = vec![
            (Status::Running, slot("running", Status::Running, now)),
            (
                Status::Closed,
                slot("recent", Status::Closed, now - 29 * DAY),
            ),
            (Status::Closed, slot("old", Status::Closed, now - 31 * DAY)),
            (Status::Closed, archived),
            (Status::Running, moss),
        ];
        let text = snapshot(&rows, now, Some("relay.example"));
        let v: Value = serde_json::from_str(&text).unwrap();
        let names: Vec<&str> = v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["running", "recent", "archived"]);
        assert_eq!(v["provider"]["host"], "relay.example");
        assert!(
            !text.contains("last_prompt") && !text.contains("secret words"),
            "{text}"
        );
        // Same rows, same text: an unchanged list writes nothing.
        assert_eq!(snapshot(&rows, now + 1, Some("relay.example")), text);
    }
}
