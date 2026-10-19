//! `cc-moss ls`: what is running, and what needs to be restored.

use anyhow::Result;
use moss_core::slot::{now_ms, rfc3339, Slot, Status};
use moss_core::{proc, store_dir, worktree};
use serde_json::{json, Value};
use unicode_width::UnicodeWidthStr;

pub const JSON_SCHEMA: u32 = 1;

pub fn run(all: bool, as_json: bool) -> Result<()> {
    let mut rows: Vec<(Status, Slot)> = crate::reconcile::run()?
        .slots()?
        .into_iter()
        .map(|s| (s.effective_status(proc::is_alive), s))
        .filter(|(st, _)| all || *st != Status::Closed)
        .collect();
    // Most urgent first: interrupted, then running, then closed; newest activity first.
    rows.sort_by_key(|(st, s)| (rank(*st), std::cmp::Reverse(s.last_active_at_ms)));

    if as_json {
        let sessions: Vec<Value> = rows.iter().map(|(st, s)| to_json(*st, s)).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": JSON_SCHEMA,
                "state_dir": store_dir::state_dir(),
                "sessions": sessions,
            }))?
        );
    } else {
        print_table(&rows);
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
    })
}

fn print_table(rows: &[(Status, Slot)]) {
    if rows.is_empty() {
        println!(
            "{}",
            tr!(
                "No sessions yet. Start one with `cc-moss new <name>`, or just type `t`.",
                "还没有会话。用 `cc-moss new <名字>` 开一个，或者直接敲 `t`。"
            )
        );
        return;
    }
    let now = now_ms();
    let lines: Vec<[String; 6]> = rows
        .iter()
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
        .collect();
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
