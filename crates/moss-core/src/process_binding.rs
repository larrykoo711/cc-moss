//! Durable process origin, separate from conversation history and event retention.

use crate::event::{Context, EndReason, Event, StartSource};
use crate::slot::Slot;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessBinding {
    pub generation: String,
    pub pid: u32,
    pub pid_start: String,
    pub origin_session_id: String,
    pub owner_slot: String,
    pub current_session_id: String,
    pub socket: Option<String>,
    pub pane: Option<String>,
    pub state: String,
    pub hops: u32,
}

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE process_bindings (
        pid INTEGER NOT NULL, pid_start TEXT NOT NULL CHECK(length(pid_start)>0),
        generation TEXT NOT NULL UNIQUE, origin_session_id TEXT NOT NULL,
        owner_slot TEXT NOT NULL, current_session_id TEXT NOT NULL,
        socket TEXT, pane TEXT,
        state TEXT NOT NULL CHECK(state IN ('root','pending_resume','transferred','invalid')),
        hops INTEGER NOT NULL CHECK(hops IN (0,1)), PRIMARY KEY(pid,pid_start));
        CREATE UNIQUE INDEX process_binding_owner ON process_bindings(owner_slot) WHERE state <> 'invalid';
        CREATE TRIGGER process_binding_identity_changed AFTER UPDATE ON slots
        WHEN OLD.pid IS NOT NEW.pid OR OLD.pid_start IS NOT NEW.pid_start
          OR OLD.session_id IS NOT NEW.session_id OR OLD.tmux_socket IS NOT NEW.tmux_socket
          OR OLD.tmux_pane IS NOT NEW.tmux_pane OR OLD.status IS NOT NEW.status
          OR OLD.archived_at_ms IS NOT NEW.archived_at_ms
        BEGIN UPDATE process_bindings SET state='invalid' WHERE owner_slot=OLD.slot_id; END;
        CREATE TRIGGER process_binding_owner_deleted AFTER DELETE ON slots
        BEGIN UPDATE process_bindings SET state='invalid' WHERE owner_slot=OLD.slot_id; END;")?;
    Ok(())
}

pub(crate) fn read(conn: &Connection, pid: u32, start: &str) -> Result<Option<ProcessBinding>> {
    Ok(conn.query_row("SELECT generation,pid,pid_start,origin_session_id,owner_slot,current_session_id,socket,pane,state,hops FROM process_bindings WHERE pid=?1 AND pid_start=?2",
        params![pid,start], |r| Ok(ProcessBinding {generation:r.get(0)?,pid:r.get(1)?,pid_start:r.get(2)?,origin_session_id:r.get(3)?,owner_slot:r.get(4)?,current_session_id:r.get(5)?,socket:r.get(6)?,pane:r.get(7)?,state:r.get(8)?,hops:r.get(9)?})).optional()?)
}

pub(crate) struct Before {
    binding: Option<ProcessBinding>,
    owner_matches: bool,
    had_generation_slot: bool,
    running_count: i64,
}

pub(crate) fn before(conn: &Connection, ctx: &Context) -> Result<Before> {
    let Some((pid, start)) = ctx
        .pid
        .zip(ctx.pid_start.as_deref().filter(|s| !s.is_empty()))
    else {
        return Ok(Before {
            binding: None,
            owner_matches: false,
            had_generation_slot: true,
            running_count: 0,
        });
    };
    let binding = read(conn, pid, start)?;
    let had_generation_slot = conn
        .prepare("SELECT 1 FROM slots WHERE pid=?1 AND pid_start=?2")?
        .exists(params![pid, start])?;
    let running_count = conn.query_row(
        "SELECT count(*) FROM slots WHERE pid=?1 AND pid_start=?2 AND status='running'",
        params![pid, start],
        |r| r.get(0),
    )?;
    let owner_matches = match &binding {
        Some(b) => conn.prepare("SELECT 1 FROM slots WHERE slot_id=?1 AND pid=?2 AND pid_start=?3 AND session_id=?4 AND tmux_socket IS ?5 AND tmux_pane IS ?6 AND ((status='running' AND ?7 IN ('root','transferred')) OR (status='closed' AND end_reason='resume' AND ?7='pending_resume'))")?
            .exists(params![b.owner_slot,pid,start,b.current_session_id,b.socket,b.pane,b.state])?,
        None => false,
    };
    Ok(Before {
        binding,
        owner_matches,
        had_generation_slot,
        running_count,
    })
}

/// Called in the same transaction as the slot transition. `origin` is supplied
/// only by a caller that freshly verified the OS process and exact launch ID.
pub(crate) fn after(
    conn: &Connection,
    event: &Event,
    ctx: &Context,
    origin: Option<&str>,
    before: Before,
) -> Result<()> {
    let Some((pid, start)) = ctx
        .pid
        .zip(ctx.pid_start.as_deref().filter(|s| !s.is_empty()))
    else {
        if let Event::Start { session_id, .. } | Event::End { session_id, .. } = event {
            conn.execute(
                "UPDATE process_bindings SET state='invalid' WHERE current_session_id=?1",
                [session_id],
            )?;
        }
        return Ok(());
    };
    let id = match event {
        Event::Start { session_id, .. } | Event::End { session_id, .. } => session_id,
        _ => {
            if before
                .binding
                .as_ref()
                .is_some_and(|b| b.state == "pending_resume")
            {
                conn.execute(
                    "UPDATE process_bindings SET state='invalid' WHERE pid=?1 AND pid_start=?2",
                    params![pid, start],
                )?;
            }
            return Ok(());
        }
    };
    let b = before.binding;
    let same =
        |b: &ProcessBinding| before.owner_matches && origin == Some(b.origin_session_id.as_str());
    let next = match (event, b.as_ref()) {
        (
            Event::Start {
                source: StartSource::Startup,
                ..
            },
            None,
        ) if origin == Some(id.as_str()) && !before.had_generation_slot => {
            let owner: Option<(String,Option<String>,Option<String>)> = conn.query_row("SELECT slot_id,tmux_socket,tmux_pane FROM slots WHERE session_id=?1 AND pid=?2 AND pid_start=?3 AND status='running'",params![id,pid,start],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            owner.map(|(owner_slot, socket, pane)| ProcessBinding {
                generation: ulid::Ulid::generate().to_string(),
                pid,
                pid_start: start.into(),
                origin_session_id: id.clone(),
                owner_slot,
                current_session_id: id.clone(),
                socket,
                pane,
                state: "root".into(),
                hops: 0,
            })
        }
        (
            Event::Start {
                source: StartSource::Startup,
                ..
            },
            Some(b),
        ) if same(b)
            && b.state == "root"
            && b.current_session_id == *id
            && before.running_count == 1 =>
        {
            Some(b.clone())
        }
        (
            Event::End {
                reason: EndReason::Closed(reason),
                ..
            },
            Some(b),
        ) if reason == "resume"
            && same(b)
            && b.state == "root"
            && b.current_session_id == *id
            && before.running_count == 1 =>
        {
            let mut next = b.clone();
            next.state = "pending_resume".into();
            Some(next)
        }
        (
            Event::End {
                reason: EndReason::Closed(reason),
                ..
            },
            Some(b),
        ) if reason == "resume"
            && same(b)
            && b.state == "pending_resume"
            && b.current_session_id == *id
            && b.hops == 0
            && before.running_count == 0 =>
        {
            Some(b.clone())
        }
        (
            Event::Start {
                source: StartSource::Resume,
                ..
            },
            Some(b),
        ) if same(b)
            && b.state == "pending_resume"
            && b.hops == 0
            && b.current_session_id != *id
            && before.running_count == 0
            && ctx.tmux.as_ref().map(|t| t.socket.as_str()) == b.socket.as_deref()
            && ctx.tmux.as_ref().map(|t| t.pane_id.as_str()) == b.pane.as_deref() =>
        {
            let owner: Option<String> = conn.query_row("SELECT slot_id FROM slots WHERE session_id=?1 AND pid=?2 AND pid_start=?3 AND status='running'",params![id,pid,start],|r|r.get(0)).optional()?;
            owner.map(|owner| {
                let mut next = b.clone();
                next.owner_slot = owner;
                next.current_session_id = id.clone();
                next.state = "transferred".into();
                next.hops = 1;
                next
            })
        }
        (
            Event::Start {
                source: StartSource::Resume,
                ..
            },
            Some(b),
        ) if same(b)
            && b.state == "transferred"
            && b.current_session_id == *id
            && before.running_count == 1 =>
        {
            Some(b.clone())
        }
        _ => None,
    };
    if let Some(next) = next {
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM slots WHERE pid=?1 AND pid_start=?2 AND status='running'",
            params![pid, start],
            |r| r.get(0),
        )?;
        if (next.state == "pending_resume" && count != 0)
            || (next.state != "pending_resume" && count != 1)
        {
            conn.execute(
                "UPDATE process_bindings SET state='invalid' WHERE pid=?1 AND pid_start=?2",
                params![pid, start],
            )?;
            return Ok(());
        }
        conn.execute("INSERT INTO process_bindings(pid,pid_start,generation,origin_session_id,owner_slot,current_session_id,socket,pane,state,hops) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
            ON CONFLICT(pid,pid_start) DO UPDATE SET owner_slot=excluded.owner_slot,current_session_id=excluded.current_session_id,state=excluded.state,hops=excluded.hops",
            params![next.pid,next.pid_start,next.generation,next.origin_session_id,next.owner_slot,next.current_session_id,next.socket,next.pane,next.state,next.hops])?;
    } else if matches!(event, Event::Start { .. } | Event::End { .. }) {
        conn.execute(
            "UPDATE process_bindings SET state='invalid' WHERE pid=?1 AND pid_start=?2",
            params![pid, start],
        )?;
    }
    Ok(())
}

pub(crate) fn verified_for(conn: &Connection, slot: &Slot) -> Result<Option<ProcessBinding>> {
    let Some((pid, start)) = slot
        .pid
        .zip(slot.pid_start.as_deref().filter(|s| !s.is_empty()))
    else {
        return Ok(None);
    };
    let Some(b) = read(conn, pid, start)? else {
        return Ok(None);
    };
    if !matches!(b.state.as_str(), "root" | "transferred")
        || b.owner_slot != slot.slot_id
        || b.current_session_id != slot.session_id
        || b.socket.as_deref() != slot.tmux.as_ref().map(|t| t.socket.as_str())
        || b.pane.as_deref() != slot.tmux.as_ref().map(|t| t.pane_id.as_str())
    {
        return Ok(None);
    }
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM slots WHERE pid=?1 AND pid_start=?2 AND status='running'",
        params![pid, start],
        |r| r.get(0),
    )?;
    let current=conn.prepare("SELECT 1 FROM slots WHERE slot_id=?1 AND pid=?2 AND pid_start=?3 AND session_id=?4 AND status='running' AND tmux_socket IS ?5 AND tmux_pane IS ?6")?.exists(params![slot.slot_id,pid,start,slot.session_id,b.socket,b.pane])?;
    Ok((count == 1 && current).then_some(b))
}
