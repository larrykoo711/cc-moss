//! SQLite-backed slot registry.
//!
//! Every hook invocation is a separate short-lived process, so all writes go
//! through one `BEGIN IMMEDIATE` transaction and rely on WAL plus a busy
//! timeout for concurrency.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result};
use rusqlite::{named_params, params, Connection, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use crate::event::{Context, EndReason, Event, StartSource, TmuxLoc};
use crate::slot::{Notes, Slot, Status};

const SCHEMA_VERSION: i64 = 1;
/// Writes give up after this long. Hooks have their own timeout (T1.2 sets
/// 10s), and uncontended writes take a few milliseconds.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(1);

/// Liveness check for a recorded process: `(pid, recorded start time)`.
pub type IsAlive<'a> = &'a dyn Fn(u32, Option<&str>) -> bool;

pub struct Registry {
    conn: Connection,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("create state dir {}", dir.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open registry {}", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        enable_wal(&conn)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        migrate(&mut conn)?;
        ensure_columns(&conn)?;
        Ok(Registry { conn })
    }

    /// Apply one event, treating every other recorded process as dead.
    /// Convenient for tests; the hook uses [`Registry::apply_with`].
    pub fn apply(&mut self, event: &Event, ctx: &Context) -> Result<()> {
        self.apply_with(event, ctx, &|_, _| false)
    }

    /// Apply one event. Events for sessions we never saw are ignored.
    pub fn apply_with(&mut self, event: &Event, ctx: &Context, is_alive: IsAlive) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match event {
            Event::Start {
                source,
                session_id,
                cwd,
                title,
                transcript_path,
            } => {
                let existing = match source {
                    // Same process, new id: rotate the id inside this process's slot.
                    StartSource::Clear | StartSource::Compact => {
                        match running_slot_of_process(&tx, ctx)? {
                            Some(id) => Some(id),
                            None => slot_by_session(&tx, session_id, ctx)?,
                        }
                    }
                    StartSource::Startup | StartSource::Resume => {
                        match slot_by_session(&tx, session_id, ctx)? {
                            // The id belongs to a slot whose own process is still
                            // alive: this is a second process on the same
                            // conversation. Give it its own slot instead of
                            // stealing the live one.
                            Some(id)
                                if owned_by_other_live_process(
                                    &tx,
                                    &id,
                                    ctx,
                                    is_alive,
                                    *source == StartSource::Resume,
                                )? =>
                            {
                                None
                            }
                            other => other,
                        }
                    }
                };
                let slot_id = match existing {
                    Some(id) => {
                        tx.execute(
                            "UPDATE slots SET session_id=?2, cwd=?3,
                               pid=COALESCE(?4, pid), pid_start=COALESCE(?5, pid_start),
                               status='running', end_reason=NULL, ended_at=NULL, last_active_at=?6,
                               restore_started_at=NULL, busy=0
                             WHERE slot_id=?1",
                            params![id, session_id, cwd, ctx.pid, ctx.pid_start, ctx.now_ms],
                        )?;
                        id
                    }
                    None => {
                        let id = Ulid::new().to_string();
                        tx.execute(
                            "INSERT INTO slots (slot_id, agent, session_id, cwd, pid, pid_start,
                               status, created_at, last_active_at)
                             VALUES (?1, 'claude', ?2, ?3, ?4, ?5, 'running', ?6, ?6)",
                            params![id, session_id, cwd, ctx.pid, ctx.pid_start, ctx.now_ms],
                        )?;
                        id
                    }
                };
                if ctx.tmux.is_some() || ctx.pid.is_some() {
                    set_tmux(&tx, &slot_id, ctx.tmux.as_ref())?;
                }
                if let Some(t) = title.as_deref().filter(|t| !t.is_empty()) {
                    tx.execute(
                        "UPDATE slots SET name=?2 WHERE slot_id=?1",
                        params![slot_id, t],
                    )?;
                }
                record_session(&tx, &slot_id, session_id, transcript_path.as_deref())?;
                if let Some(args) = &ctx.launch_args {
                    tx.execute(
                        "UPDATE slots SET launch_args=?2 WHERE slot_id=?1",
                        params![slot_id, serde_json::to_string(args)?],
                    )?;
                }
                // One process runs one conversation: any other slot still marked
                // running for this exact process was switched away from (F11).
                close_other_slots_of_process(&tx, &slot_id, ctx)?;
            }
            Event::Prompt {
                session_id,
                title,
                prompt,
            } => {
                if let Some(id) = current_slot(&tx, session_id, ctx)? {
                    touch_and_repair(&tx, &id, ctx, is_alive)?;
                    tx.execute("UPDATE slots SET busy = 1 WHERE slot_id = ?1", [&id])?;
                    if let Some(preview) = prompt.as_deref().and_then(preview) {
                        tx.execute(
                            "UPDATE slots SET last_prompt = ?2 WHERE slot_id = ?1",
                            params![id, preview],
                        )?;
                    }
                    if let Some(t) = title.as_deref().filter(|t| !t.is_empty()) {
                        tx.execute("UPDATE slots SET name=?2 WHERE slot_id=?1", params![id, t])?;
                    }
                }
            }
            Event::Stop { session_id } => {
                if let Some(id) = current_slot(&tx, session_id, ctx)? {
                    touch_and_repair(&tx, &id, ctx, is_alive)?;
                    tx.execute("UPDATE slots SET busy = 0 WHERE slot_id = ?1", [&id])?;
                }
            }
            Event::End { session_id, reason } => {
                // Only the slot's current id may end it; a late End for a
                // pre-/clear id must not close the live process.
                if let Some(id) = current_slot(&tx, session_id, ctx)? {
                    tx.execute("UPDATE slots SET busy = 0 WHERE slot_id = ?1", [&id])?;
                    let (status, raw) = match reason {
                        EndReason::Clear => (None, None),
                        EndReason::Closed(r) => (Some(Status::Closed), Some(r)),
                        EndReason::Abnormal(r) => (Some(Status::Interrupted), Some(r)),
                    };
                    if let Some(status) = status {
                        // A slot the user closed stays closed: killing its tmux
                        // session makes claude report SessionEnd(other) afterwards.
                        tx.execute(
                            "UPDATE slots SET status=?2, end_reason=?3, ended_at=?4
                             WHERE slot_id=?1 AND status <> 'closed'",
                            params![id, status.as_str(), raw, ctx.now_ms],
                        )?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The slot an event for `session_id` from this process would act on.
    pub fn slot_for_event(&self, session_id: &str, ctx: &Context) -> Result<Option<Slot>> {
        let Some(id) = current_slot(&self.conn, session_id, ctx)? else {
            return Ok(None);
        };
        Ok(self.slots()?.into_iter().find(|s| s.slot_id == id))
    }

    /// The slot that owns its tmux session's name: the earliest-created running
    /// slot in that tmux session (R9: normally the only one).
    pub fn is_tmux_primary(&self, slot: &Slot) -> Result<bool> {
        let Some(t) = slot.tmux.as_ref() else {
            return Ok(false);
        };
        let Some(sid) = t.session_id.as_deref() else {
            return Ok(true);
        };
        let first: Option<String> = self
            .conn
            .query_row(
                "SELECT slot_id FROM slots
                 WHERE tmux_socket = ?1 AND tmux_session_id = ?2 AND status = 'running'
                 ORDER BY created_at, slot_id LIMIT 1",
                params![t.socket, sid],
                |r| r.get(0),
            )
            .optional()?;
        Ok(first.as_deref() == Some(slot.slot_id.as_str()))
    }

    /// Whether a slot other than `slot_id` that is not closed is named `name`.
    pub fn name_taken_by_other(&self, slot_id: &str, name: &str) -> Result<bool> {
        Ok(self
            .conn
            .prepare(
                "SELECT 1 FROM slots WHERE slot_id <> ?1 AND name = ?2 AND status <> 'closed' LIMIT 1",
            )?
            .exists(params![slot_id, name])?)
    }

    /// Set a slot's name (found outside the hook payload, e.g. in its transcript).
    pub fn set_name(&self, slot_id: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET name = ?2 WHERE slot_id = ?1",
            params![slot_id, name],
        )?;
        Ok(())
    }

    /// Name open slots that have none yet from their transcript's last
    /// `/rename`. Claude Code leaves the name out of hook payloads after
    /// `--resume`, so a resumed session can stay nameless otherwise.
    pub fn backfill_names(&self) -> Result<usize> {
        let mut named = 0;
        for slot in self.slots()? {
            if slot.name.is_some() || slot.status == Status::Closed {
                continue;
            }
            let Some(path) = slot.transcript_path.as_deref() else {
                continue;
            };
            // Transcripts grow to tens of megabytes: only read what was
            // appended since the last look.
            let from: i64 = self.conn.query_row(
                "SELECT name_scanned_to FROM slots WHERE slot_id = ?1",
                [&slot.slot_id],
                |r| r.get(0),
            )?;
            let (title, scanned) =
                crate::transcript::custom_title_since(Path::new(path), from.max(0) as u64);
            self.conn.execute(
                "UPDATE slots SET name_scanned_to = ?2 WHERE slot_id = ?1",
                params![slot.slot_id, scanned as i64],
            )?;
            if let Some(name) = title {
                self.set_name(&slot.slot_id, &name)?;
                named += 1;
            }
        }
        Ok(named)
    }

    /// Whether Claude is working on a prompt (from outside the hooks).
    pub fn set_busy(&self, slot_id: &str, busy: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET busy = ?2 WHERE slot_id = ?1",
            params![slot_id, busy],
        )?;
        Ok(())
    }

    /// Replace a slot's notes.
    pub fn set_notes(&self, slot_id: &str, notes: &Notes) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET note_goal = ?2, note_state = ?3, note_text = ?4 WHERE slot_id = ?1",
            params![slot_id, notes.goal, notes.state, notes.note],
        )?;
        Ok(())
    }

    /// The user does not want this session back: drop it from restore candidates.
    pub fn close_by_user(&self, slot_id: &str, now_ms: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET status = 'closed', end_reason = 'closed-by-user', ended_at = ?2
             WHERE slot_id = ?1",
            params![slot_id, now_ms],
        )?;
        Ok(())
    }

    /// `cc-moss restore` launched this slot; SessionStart clears the mark.
    pub fn mark_restore_started(&self, slot_id: &str, now_ms: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET restore_started_at = ?2 WHERE slot_id = ?1",
            params![slot_id, now_ms],
        )?;
        Ok(())
    }

    /// Atomically claim a slot for restoring, against the snapshot `planned`
    /// was taken from: any hook activity since then (a resume elsewhere, a
    /// prompt) or a claim within the pending window makes the claim fail, so
    /// a conversation that came back on its own is never launched twice.
    pub fn claim_restore(&self, planned: &Slot, now_ms: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE slots SET restore_started_at = :now
             WHERE slot_id = :slot AND session_id = :sid
               AND pid IS :pid AND pid_start IS :pid_start AND last_active_at = :active
               AND (restore_started_at IS NULL OR restore_started_at < :now - :window)",
            named_params! {
                ":now": now_ms,
                ":slot": planned.slot_id,
                ":sid": planned.session_id,
                ":pid": planned.pid,
                ":pid_start": planned.pid_start,
                ":active": planned.last_active_at_ms,
                ":window": crate::restore::PENDING_WINDOW_MS,
            },
        )?;
        Ok(changed == 1)
    }

    /// Record the process `cc-moss restore` just launched, so liveness (not a
    /// timer) protects it from being restored again.
    pub fn set_restored_process(
        &self,
        slot_id: &str,
        pid: u32,
        pid_start: Option<&str>,
        tmux: &TmuxLoc,
        now_ms: i64,
    ) -> Result<()> {
        // last_active_at moves too: a pid recorded after boot must look it.
        self.conn.execute(
            "UPDATE slots SET pid = ?2, pid_start = ?3, status = 'running', end_reason = NULL,
               ended_at = NULL, tmux_socket = ?4, tmux_session_id = ?5, tmux_session_name = ?6,
               tmux_pane = ?7, last_active_at = ?8
             WHERE slot_id = ?1",
            params![
                slot_id,
                pid,
                pid_start,
                tmux.socket,
                tmux.session_id,
                tmux.session_name,
                tmux.pane_id,
                now_ms
            ],
        )?;
        Ok(())
    }

    /// After a restore, older interrupted copies of the same conversation are
    /// superseded: close them so they stop being listed.
    pub fn close_superseded(&self, keep: &str, session_id: &str, now_ms: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET status = 'closed', end_reason = 'switched', ended_at = ?3
             WHERE slot_id <> ?1 AND session_id = ?2 AND status <> 'closed'
               AND restore_started_at IS NULL",
            params![keep, session_id, now_ms],
        )?;
        Ok(())
    }

    /// Undo [`Registry::mark_restore_started`] when the launch failed.
    pub fn clear_restore_started(&self, slot_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET restore_started_at = NULL WHERE slot_id = ?1",
            [slot_id],
        )?;
        Ok(())
    }

    /// Record the tmux session name after cc-moss renamed it.
    pub fn set_tmux_session_name(&self, slot_id: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE slots SET tmux_session_name = ?2 WHERE slot_id = ?1",
            params![slot_id, name],
        )?;
        Ok(())
    }

    /// All slots, oldest first.
    pub fn slots(&self) -> Result<Vec<Slot>> {
        // One read transaction so slots and their session ids are one snapshot.
        let tx = self.conn.unchecked_transaction()?;
        let slots = read_slots(&tx)?;
        tx.commit()?;
        Ok(slots)
    }
}

fn read_slots(conn: &Connection) -> Result<Vec<Slot>> {
    let mut stmt = conn.prepare(
        "SELECT slot_id, agent, session_id, name, cwd, pid, pid_start,
                    tmux_socket, tmux_session_id, tmux_session_name, tmux_pane,
                    status, end_reason, created_at, last_active_at, ended_at,
                    note_goal, note_state, note_text, launch_args, restore_started_at, busy, last_prompt,
                    (SELECT transcript_path FROM slot_sessions ss
                      WHERE ss.slot_id = slots.slot_id AND ss.session_id = slots.session_id)
             FROM slots ORDER BY created_at, slot_id",
    )?;
    let rows = stmt.query_map([], |r| {
        let socket: Option<String> = r.get(7)?;
        let pane: Option<String> = r.get(10)?;
        let tmux = match (socket, pane) {
            (Some(socket), Some(pane_id)) => Some(TmuxLoc {
                socket,
                session_id: r.get(8)?,
                session_name: r.get(9)?,
                pane_id,
            }),
            _ => None,
        };
        let status: String = r.get(11)?;
        Ok(Slot {
            slot_id: r.get(0)?,
            agent: r.get(1)?,
            session_id: r.get(2)?,
            session_ids: Vec::new(),
            name: r.get(3)?,
            cwd: r.get(4)?,
            pid: r.get(5)?,
            pid_start: r.get(6)?,
            tmux,
            status: Status::parse(&status),
            end_reason: r.get(12)?,
            notes: Notes {
                goal: r.get(16)?,
                state: r.get(17)?,
                note: r.get(18)?,
            },
            launch_args: serde_json::from_str(&r.get::<_, String>(19)?).unwrap_or_default(),
            restore_started_at_ms: r.get(20)?,
            busy: r.get(21)?,
            last_prompt: r.get(22)?,
            transcript_path: r.get(23)?,
            created_at_ms: r.get(13)?,
            last_active_at_ms: r.get(14)?,
            ended_at_ms: r.get(15)?,
        })
    })?;
    let mut slots = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut ids =
        conn.prepare("SELECT session_id FROM slot_sessions WHERE slot_id=?1 ORDER BY seq")?;
    for s in &mut slots {
        s.session_ids = ids
            .query_map([&s.slot_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
    }
    Ok(slots)
}

/// Switch to WAL once. Changing the journal mode needs an exclusive lock and
/// does not wait on the busy timeout, so concurrent first opens retry.
fn enable_wal(conn: &Connection) -> Result<()> {
    let mut last_err = None;
    for attempt in 0..20 {
        let mode: rusqlite::Result<String> =
            conn.pragma_query_value(None, "journal_mode", |r| r.get(0));
        match mode {
            Ok(m) if m.eq_ignore_ascii_case("wal") => return Ok(()),
            Ok(_) => match conn
                .pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))
            {
                Ok(m) if m.eq_ignore_ascii_case("wal") => return Ok(()),
                Ok(m) => last_err = Some(anyhow::anyhow!("journal_mode stayed {m}")),
                Err(e) => last_err = Some(e.into()),
            },
            Err(e) => last_err = Some(e.into()),
        }
        std::thread::sleep(Duration::from_millis(10 + attempt * 5));
    }
    Err(last_err
        .unwrap_or_else(|| anyhow::anyhow!("could not enable WAL"))
        .context("enable WAL"))
}

/// Earlier builds created schema v1 without later columns; v1 was never
/// released, so missing columns are added in place.
fn ensure_columns(conn: &Connection) -> Result<()> {
    for (column, ddl) in [
        ("restore_started_at", "restore_started_at INTEGER"),
        ("busy", "busy INTEGER NOT NULL DEFAULT 0"),
        ("last_prompt", "last_prompt TEXT"),
        (
            "name_scanned_to",
            "name_scanned_to INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        let present = conn
            .prepare("SELECT 1 FROM pragma_table_info('slots') WHERE name = ?1")?
            .exists([column])?;
        if !present {
            // Another process may add it first; that is fine.
            let _ = conn.execute_batch(&format!("ALTER TABLE slots ADD COLUMN {ddl}"));
        }
    }
    Ok(())
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Re-check inside the write lock: another hook may have migrated first.
    let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS slots (
               slot_id TEXT PRIMARY KEY,
               agent TEXT NOT NULL,
               session_id TEXT NOT NULL,
               name TEXT,
               cwd TEXT NOT NULL,
               pid INTEGER,
               pid_start TEXT,
               tmux_socket TEXT,
               tmux_session_id TEXT,
               tmux_session_name TEXT,
               tmux_pane TEXT,
               launch_args TEXT NOT NULL DEFAULT '[]',
               note_goal TEXT,
               note_state TEXT,
               note_text TEXT,
               status TEXT NOT NULL CHECK (status IN ('running','closed','interrupted')),
               restore_started_at INTEGER,
               busy INTEGER NOT NULL DEFAULT 0,
               last_prompt TEXT,
               name_scanned_to INTEGER NOT NULL DEFAULT 0,
               end_reason TEXT,
               created_at INTEGER NOT NULL,
               last_active_at INTEGER NOT NULL,
               ended_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS slots_pid ON slots(pid, status);
             CREATE TABLE IF NOT EXISTS slot_sessions (
               slot_id TEXT NOT NULL REFERENCES slots(slot_id),
               session_id TEXT NOT NULL,
               seq INTEGER NOT NULL,
               transcript_path TEXT,
               PRIMARY KEY (slot_id, session_id)
             );
             CREATE INDEX IF NOT EXISTS slot_sessions_session ON slot_sessions(session_id);
             PRAGMA user_version = 1;",
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// SQL predicate "this row belongs to the calling process". A process is its
/// pid *and* its start time: if either side's start time is unknown we cannot
/// tell a reused pid from the same process, so the answer is no.
const SAME_PROCESS: &str =
    "(pid = :pid AND pid_start IS NOT NULL AND :pid_start IS NOT NULL AND pid_start = :pid_start)";

fn sql(template: &str) -> String {
    template.replace("{SAME}", SAME_PROCESS)
}

/// A slot that has ever used `session_id`. If several have (two processes on
/// one conversation), prefer the caller's own, then the most recently active.
fn slot_by_session(conn: &Connection, session_id: &str, ctx: &Context) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            &sql(
                "SELECT s.slot_id FROM slot_sessions ss JOIN slots s ON s.slot_id = ss.slot_id
                  WHERE ss.session_id = :sid
                  ORDER BY {SAME} DESC, s.last_active_at DESC LIMIT 1",
            ),
            named_params! {":sid": session_id, ":pid": ctx.pid, ":pid_start": ctx.pid_start},
            |r| r.get(0),
        )
        .optional()?)
}

/// The slot whose *current* id is `session_id`, preferring the caller's own
/// slot when two processes share a conversation.
fn current_slot(conn: &Connection, session_id: &str, ctx: &Context) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            &sql("SELECT slot_id FROM slots WHERE session_id = :sid
                  ORDER BY {SAME} DESC, last_active_at DESC LIMIT 1"),
            named_params! {":sid": session_id, ":pid": ctx.pid, ":pid_start": ctx.pid_start},
            |r| r.get(0),
        )
        .optional()?)
}

/// The running slot of the calling process (strict identity), else the
/// running slot in the same tmux pane.
fn running_slot_of_process(conn: &Connection, ctx: &Context) -> Result<Option<String>> {
    let by_process = conn
        .query_row(
            &sql(
                "SELECT slot_id FROM slots WHERE status = 'running' AND {SAME}
                  ORDER BY last_active_at DESC LIMIT 1",
            ),
            named_params! {":pid": ctx.pid, ":pid_start": ctx.pid_start},
            |r| r.get(0),
        )
        .optional()?;
    if by_process.is_some() {
        return Ok(by_process);
    }
    let Some(t) = ctx.tmux.as_ref() else {
        return Ok(None);
    };
    // Pane ids restart at %0 with a new tmux server, so the pane alone is not
    // an identity: use it only when the pids cannot prove a different process.
    Ok(conn
        .query_row(
            "SELECT slot_id FROM slots
             WHERE tmux_socket = :socket AND tmux_pane = :pane AND status = 'running'
               AND (:pid IS NULL OR pid IS NULL
                    OR (pid = :pid AND (pid_start IS NULL OR :pid_start IS NULL)))
             ORDER BY last_active_at DESC LIMIT 1",
            named_params! {":socket": t.socket, ":pane": t.pane_id, ":pid": ctx.pid, ":pid_start": ctx.pid_start},
            |r| r.get(0),
        )
        .optional()?)
}

struct Owner {
    status: String,
    pid: Option<u32>,
    pid_start: Option<String>,
    same_process: bool,
    restore_started_at: Option<i64>,
}

fn owner(conn: &Connection, slot_id: &str, ctx: &Context) -> Result<Owner> {
    Ok(conn.query_row(
        &sql("SELECT status, pid, pid_start, {SAME}, restore_started_at
              FROM slots WHERE slot_id = :slot"),
        named_params! {":slot": slot_id, ":pid": ctx.pid, ":pid_start": ctx.pid_start},
        |r| {
            Ok(Owner {
                status: r.get(0)?,
                pid: r.get(1)?,
                pid_start: r.get(2)?,
                same_process: r.get::<_, Option<bool>>(3)?.unwrap_or(false),
                restore_started_at: r.get(4)?,
            })
        },
    )?)
}

/// True if the slot is running under a different process that is still alive.
fn owned_by_other_live_process(
    conn: &Connection,
    slot_id: &str,
    ctx: &Context,
    is_alive: IsAlive,
    adopt_restore: bool,
) -> Result<bool> {
    let o = owner(conn, slot_id, ctx)?;
    let just_restored = o
        .restore_started_at
        .is_some_and(|t| ctx.now_ms - t < crate::restore::PENDING_WINDOW_MS);
    if adopt_restore && just_restored {
        // `cc-moss restore` launched this slot moments ago; its SessionStart
        // may come from a wrapper's child rather than the pane process we
        // recorded. Only that resume, and only within the window, may adopt.
        return Ok(false);
    }
    Ok(o.status == "running"
        && !o.same_process
        && o.pid
            .is_some_and(|pid| is_alive(pid, o.pid_start.as_deref())))
}

/// One process runs one conversation: other running slots of exactly this
/// process were switched away from (F11). Unknown start times close nothing.
fn close_other_slots_of_process(conn: &Connection, keep: &str, ctx: &Context) -> Result<()> {
    conn.execute(
        &sql("UPDATE slots SET status = 'closed', end_reason = 'switched', ended_at = :now
              WHERE slot_id <> :keep AND status = 'running' AND {SAME}"),
        named_params! {":keep": keep, ":now": ctx.now_ms, ":pid": ctx.pid, ":pid_start": ctx.pid_start},
    )?;
    Ok(())
}

/// Record activity. An event from a *different* process on a slot that is not
/// held by a live process means a SessionStart(resume) was lost: adopt the
/// caller so the slot is not offered for restore again. The slot's own
/// process never revives a closed slot.
fn touch_and_repair(
    conn: &Connection,
    slot_id: &str,
    ctx: &Context,
    is_alive: IsAlive,
) -> Result<()> {
    conn.execute(
        "UPDATE slots SET last_active_at = ?2 WHERE slot_id = ?1",
        params![slot_id, ctx.now_ms],
    )?;
    let Some(pid) = ctx.pid else { return Ok(()) };
    let o = owner(conn, slot_id, ctx)?;
    if o.same_process || owned_by_other_live_process(conn, slot_id, ctx, is_alive, false)? {
        return Ok(());
    }
    if o.status == "running"
        && o.pid == Some(pid)
        && (o.pid_start.is_none() || ctx.pid_start.is_none())
    {
        // Same pid, identity unprovable: leave it alone rather than guess.
        return Ok(());
    }
    conn.execute(
        "UPDATE slots SET pid = ?2, pid_start = ?3, status = 'running', end_reason = NULL, ended_at = NULL
         WHERE slot_id = ?1",
        params![slot_id, pid, ctx.pid_start],
    )?;
    Ok(())
}

fn set_tmux(conn: &Connection, slot_id: &str, tmux: Option<&TmuxLoc>) -> Result<()> {
    conn.execute(
        "UPDATE slots SET tmux_socket=?2, tmux_session_id=?3, tmux_session_name=?4, tmux_pane=?5
         WHERE slot_id=?1",
        params![
            slot_id,
            tmux.map(|t| &t.socket),
            tmux.and_then(|t| t.session_id.as_ref()),
            tmux.and_then(|t| t.session_name.as_ref()),
            tmux.map(|t| &t.pane_id),
        ],
    )?;
    Ok(())
}

fn record_session(
    conn: &Connection,
    slot_id: &str,
    session_id: &str,
    transcript_path: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO slot_sessions (session_id, slot_id, seq, transcript_path)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(seq), 0) + 1 FROM slot_sessions WHERE slot_id=?2), ?3)
         ON CONFLICT(slot_id, session_id) DO UPDATE SET
           transcript_path=COALESCE(excluded.transcript_path, slot_sessions.transcript_path)",
        params![session_id, slot_id, transcript_path],
    )?;
    Ok(())
}

/// The first non-empty line of a prompt, at most 200 characters.
fn preview(prompt: &str) -> Option<String> {
    let line = prompt.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(200).collect())
}
