//! SQLite-backed slot registry.
//!
//! Every hook invocation is a separate short-lived process, so all writes go
//! through one `BEGIN IMMEDIATE` transaction and rely on WAL plus a busy
//! timeout for concurrency.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result};
use rusqlite::{named_params, params, Connection, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

use crate::event::{Context, EndReason, Event, StartSource, TmuxLoc};
use crate::slot::{Notes, Slot, Status};

const SCHEMA_VERSION: i64 = 1;
/// `needs_you` keeps at most this many characters of the agent's message.
pub const MESSAGE_MAX: usize = 200;
/// Events older than this are pruned when `ls` or the picker opens.
pub const EVENT_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// What `events` returns without `--since`.
pub const RECENT_EVENTS: usize = 50;
/// Writes give up after this long. Hooks have their own timeout (T1.2 sets
/// 10s), and uncontended writes take a few milliseconds; no process is
/// spawned while the lock is held, so waiting this long means a real stall.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Liveness check for a recorded process: `(pid, recorded start time)`.
pub type IsAlive<'a> = &'a dyn Fn(u32, Option<&str>) -> bool;

pub struct Registry {
    conn: Connection,
    /// The schema version on disk when it is newer than this binary's.
    newer: Option<i64>,
    /// Touched when this handle closes after changing a row (see
    /// [`CHANGED_FILE`]); none for an in-memory registry.
    stamp: Option<std::path::PathBuf>,
    /// Rows changed by opening (migrations), so they don't count.
    opened_changes: u64,
    /// Opened for Moss: reads only ([`opens_read_only`]).
    read_only: bool,
}

/// Beside the registry: touched whenever a cc-moss process closes the
/// registry after writing to it. The app watches this one file instead of
/// the database, whose SQLite side files come and go on every read.
pub const CHANGED_FILE: &str = "registry.changed";

impl Drop for Registry {
    fn drop(&mut self) {
        if let Some(stamp) = &self.stamp {
            if self.conn.total_changes() > self.opened_changes {
                let _ = std::fs::write(stamp, b"");
            }
        }
    }
}

/// A write was refused because the registry was written by a newer cc-moss,
/// whose rows this version might break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewerSchema {
    pub found: i64,
    pub supported: i64,
}

impl std::fmt::Display for NewerSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the registry was written by a newer cc-moss (schema {}, this one knows {}); upgrade cc-moss",
            self.found, self.supported
        )
    }
}

impl std::error::Error for NewerSchema {}

/// Set by cc-moss's hooks: they run for Moss too (with its role in their
/// environment) and record it, outside its sandbox.
static AS_HOOK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// This process is a cc-moss hook: it writes the registry even for Moss.
pub fn as_hook() {
    AS_HOOK.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the registry is opened read-only: in Moss's processes (its role
/// in the environment), except cc-moss's own hooks. Moss changes sessions
/// only through `cc-moss moss act`, which the daemon checks.
pub fn opens_read_only(role: Option<&str>, hook: bool) -> bool {
    role == Some(crate::moss::ROLE) && !hook
}

/// What a write says when Moss tries one.
const READ_ONLY: &str =
    "Moss can't change the session records itself; ask through `cc-moss moss act`";

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        let role = std::env::var(crate::moss::ROLE_ENV).ok();
        if opens_read_only(
            role.as_deref(),
            AS_HOOK.load(std::sync::atomic::Ordering::Relaxed),
        ) {
            return Self::open_read_only(path);
        }
        if !path.is_absolute() {
            anyhow::bail!(crate::store_dir::NOT_ABSOLUTE);
        }
        if let Some(dir) = path.parent() {
            crate::store_dir::ensure_private_dir(dir)
                .with_context(|| format!("create state dir {}", dir.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open registry {}", path.display()))?;
        for suffix in ["", "-wal", "-shm"] {
            let mut file = path.as_os_str().to_owned();
            file.push(suffix);
            crate::store_dir::make_private(Path::new(&file), 0o600);
        }
        conn.busy_timeout(BUSY_TIMEOUT)?;
        enable_wal(&conn)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let mut registry = Self::init(conn)?;
        registry.stamp = path.parent().map(|dir| dir.join(CHANGED_FILE));
        registry.opened_changes = registry.conn.total_changes();
        Ok(registry)
    }

    /// For reading only: no directory made, no migration, every write
    /// refused. Fails when there is no registry yet.
    pub fn open_read_only(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("open registry {}", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "query_only", true)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        Ok(Registry {
            conn,
            newer: (version > SCHEMA_VERSION).then_some(version),
            stamp: None,
            opened_changes: 0,
            read_only: true,
        })
    }

    /// Opened for reading only (for Moss).
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            // Read what we can, write nothing: a newer schema may have
            // invariants this version does not know. SQLite enforces it too.
            conn.pragma_update(None, "query_only", true)?;
            return Ok(Registry {
                conn,
                newer: Some(version),
                stamp: None,
                opened_changes: 0,
                read_only: false,
            });
        }
        migrate(&mut conn)?;
        ensure_columns(&conn)?;
        Ok(Registry {
            conn,
            newer: None,
            stamp: None,
            opened_changes: 0,
            read_only: false,
        })
    }

    /// The on-disk schema version when a newer cc-moss wrote this registry.
    /// Such a registry is opened read-only; every write fails with
    /// [`NewerSchema`].
    pub fn newer_schema(&self) -> Option<i64> {
        self.newer
    }

    /// Liveness of every other process running a slot that has used
    /// `session_id`: the only owners `apply_with` may need to check.
    fn prefetch_liveness(
        &self,
        session_id: &str,
        ctx: &Context,
        is_alive: IsAlive,
    ) -> Result<Vec<(u32, Option<String>, bool)>> {
        let mut stmt = self.conn.prepare(&sql(
            "SELECT DISTINCT pid, pid_start FROM slots
              WHERE status = 'running' AND pid IS NOT NULL AND NOT COALESCE({SAME}, 0)
                AND (session_id = :sid
                     OR slot_id IN (SELECT slot_id FROM slot_sessions WHERE session_id = :sid))",
        ))?;
        let owners = stmt
            .query_map(
                named_params! {":sid": session_id, ":pid": ctx.pid, ":pid_start": ctx.pid_start},
                |r| Ok((r.get::<_, u32>(0)?, r.get::<_, Option<String>>(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(owners
            .into_iter()
            .map(|(pid, start)| {
                let alive = is_alive(pid, start.as_deref());
                (pid, start, alive)
            })
            .collect())
    }

    /// The connection for a write, refused on a newer schema.
    fn write(&self) -> Result<&Connection> {
        if self.read_only {
            anyhow::bail!(READ_ONLY);
        }
        match self.newer {
            Some(found) => Err(NewerSchema {
                found,
                supported: SCHEMA_VERSION,
            }
            .into()),
            None => Ok(&self.conn),
        }
    }

    /// A write transaction that takes the lock up front, so a read inside it
    /// cannot leave it unable to write under a concurrent hook.
    fn write_tx(&self) -> Result<rusqlite::Transaction<'_>> {
        Ok(rusqlite::Transaction::new_unchecked(
            self.write()?,
            TransactionBehavior::Immediate,
        )?)
    }

    /// Apply one event, treating every other recorded process as dead.
    /// Convenient for tests; the hook uses [`Registry::apply_with`].
    pub fn apply(&mut self, event: &Event, ctx: &Context) -> Result<()> {
        self.apply_with(event, ctx, &|_, _| false)
    }

    /// Apply one event. Events for sessions we never saw are ignored.
    pub fn apply_with(&mut self, event: &Event, ctx: &Context, is_alive: IsAlive) -> Result<()> {
        self.write()?;
        if let Event::ToolDone { session_id } = event {
            // Runs after every tool call: read one row, write only when a
            // mark is there to clear, and check no process.
            return self.clear_attention_of(session_id);
        }
        // A liveness check spawns `ps`; doing it under the write lock would
        // make concurrent hooks wait on it and possibly drop their events.
        // So check every process the event could be compared with first,
        // and answer from that inside the lock. Only a row that changed in
        // between is checked under the lock.
        let known = self.prefetch_liveness(event_session_id(event), ctx, is_alive)?;
        let cached = |pid: u32, start: Option<&str>| match known
            .iter()
            .find(|(p, s, _)| *p == pid && s.as_deref() == start)
        {
            Some((_, _, alive)) => *alive,
            None => is_alive(pid, start),
        };
        let is_alive: IsAlive = &cached;
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
                let created = existing.is_none();
                // `cc-moss restore` launched this one: its `restored` event
                // already says the session is back.
                let restoring = *source == StartSource::Resume && match &existing {
                    Some(id) => tx
                        .query_row(
                            "SELECT restore_started_at IS NOT NULL FROM slots WHERE slot_id = ?1",
                            [id],
                            |r| r.get::<_, bool>(0),
                        )
                        .optional()?
                        .unwrap_or(false),
                    None => false,
                };
                // A session taken up again leaves the archive.
                let was_archived = match &existing {
                    Some(id) => tx
                        .query_row(
                            "SELECT archived_at_ms IS NOT NULL FROM slots WHERE slot_id = ?1",
                            [id],
                            |r| r.get::<_, bool>(0),
                        )
                        .optional()?
                        .unwrap_or(false),
                    None => false,
                };
                let slot_id = match existing {
                    Some(id) => {
                        tx.execute(
                            "UPDATE slots SET session_id=?2, cwd=?3,
                               pid=COALESCE(?4, pid), pid_start=COALESCE(?5, pid_start),
                               status='running', end_reason=NULL, ended_at=NULL, last_active_at=?6,
                               restore_started_at=NULL, busy=0, attention=NULL,
                               lost_reported_at=NULL, archived_at_ms=NULL
                             WHERE slot_id=?1",
                            params![id, session_id, cwd, ctx.pid, ctx.pid_start, ctx.now_ms],
                        )?;
                        id
                    }
                    None => {
                        let id = Ulid::generate().to_string();
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
                let started = match source {
                    StartSource::Resume if restoring => None,
                    StartSource::Startup => Some("startup"),
                    StartSource::Resume => Some("resume"),
                    StartSource::Clear | StartSource::Compact => None,
                };
                if let Some(source) = started {
                    let detail = serde_json::json!({ "source": source });
                    record_event(&tx, &slot_id, "started", &detail, ctx.now_ms)?;
                }
                if was_archived {
                    let detail = serde_json::json!({ "by": "resume" });
                    record_event(&tx, &slot_id, "unarchived", &detail, ctx.now_ms)?;
                }
                if let Some(t) = title.as_deref().filter(|t| !t.is_empty()) {
                    // The name a new slot is born with is part of `started`.
                    rename(&tx, &slot_id, t, !created, ctx.now_ms)?;
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
                    let began = tx.execute(
                        "UPDATE slots SET busy = 1 WHERE slot_id = ?1 AND busy = 0",
                        [&id],
                    )?;
                    if began == 1 {
                        record_event(&tx, &id, "working", &empty(), ctx.now_ms)?;
                    }
                    clear_attention(&tx, &id)?;
                    if let Some(preview) = prompt.as_deref().and_then(preview) {
                        tx.execute(
                            "UPDATE slots SET last_prompt = ?2 WHERE slot_id = ?1",
                            params![id, preview],
                        )?;
                    }
                    if let Some(t) = title.as_deref().filter(|t| !t.is_empty()) {
                        rename(&tx, &id, t, true, ctx.now_ms)?;
                    }
                }
            }
            Event::Stop { session_id } => {
                if let Some(id) = current_slot(&tx, session_id, ctx)? {
                    touch_and_repair(&tx, &id, ctx, is_alive)?;
                    let done = tx.execute(
                        "UPDATE slots SET busy = 0 WHERE slot_id = ?1 AND busy = 1",
                        [&id],
                    )?;
                    if done == 1 {
                        record_event(&tx, &id, "finished", &empty(), ctx.now_ms)?;
                    }
                    clear_attention(&tx, &id)?;
                }
            }
            Event::End { session_id, reason } => {
                // Only the slot's current id may end it; a late End for a
                // pre-/clear id must not close the live process.
                if let Some(id) = current_slot(&tx, session_id, ctx)? {
                    // A late SessionEnd from a process the session has since
                    // left (resumed elsewhere) ends nothing: the new one runs.
                    let recorded: Option<(Option<u32>, Option<String>)> = tx
                        .query_row(
                            "SELECT pid, pid_start FROM slots WHERE slot_id = ?1",
                            [&id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    let other_process = match (recorded, ctx.pid) {
                        (Some((Some(pid), start)), Some(caller)) => {
                            pid != caller
                                || matches!((&start, &ctx.pid_start), (Some(a), Some(b)) if a != b)
                        }
                        _ => false,
                    };
                    if other_process {
                        tx.commit()?;
                        return Ok(());
                    }
                    tx.execute("UPDATE slots SET busy = 0 WHERE slot_id = ?1", [&id])?;
                    clear_attention(&tx, &id)?;
                    let (status, raw) = match reason {
                        EndReason::Clear => (None, None),
                        EndReason::Closed(r) => (Some(Status::Closed), Some(r)),
                        EndReason::Abnormal(r) => (Some(Status::Interrupted), Some(r)),
                    };
                    if let Some(status) = status {
                        // A slot the user closed stays closed: killing its tmux
                        // session makes claude report SessionEnd(other) afterwards.
                        let changed = tx.execute(
                            "UPDATE slots SET status=?2, end_reason=?3, ended_at=?4
                             WHERE slot_id=?1 AND status <> 'closed'",
                            params![id, status.as_str(), raw, ctx.now_ms],
                        )?;
                        let detail = serde_json::json!({ "reason": raw });
                        // `cc-moss restart` (or restore) ended it on purpose
                        // and brings it back right away: no news.
                        let restarting = tx
                            .query_row(
                                "SELECT restore_started_at FROM slots WHERE slot_id = ?1",
                                [&id],
                                |r| r.get::<_, Option<i64>>(0),
                            )?
                            .is_some_and(|t| ctx.now_ms - t < crate::restore::PENDING_WINDOW_MS);
                        if changed == 1 && status == Status::Closed {
                            record_event(&tx, &id, "ended", &detail, ctx.now_ms)?;
                        } else if changed == 1 && !restarting {
                            record_interrupted(&tx, &id, &detail, ctx.now_ms)?;
                        }
                    }
                }
            }
            Event::Attention {
                session_id,
                reason,
                message,
            } => {
                if let Some(id) = current_slot(&tx, session_id, ctx)? {
                    tx.execute(
                        "UPDATE slots SET attention = ?2 WHERE slot_id = ?1",
                        params![id, reason.as_str()],
                    )?;
                    let message = message.as_deref().map(truncate_message);
                    let detail =
                        serde_json::json!({ "reason": reason.as_str(), "message": message });
                    record_event(&tx, &id, "needs_you", &detail, ctx.now_ms)?;
                }
            }
            // Handled before the transaction.
            Event::ToolDone { .. } => {}
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
        let tx = self.write_tx()?;
        rename(&tx, slot_id, name, true, crate::slot::now_ms())?;
        tx.commit()?;
        Ok(())
    }

    /// Take each open slot's name from its transcript's last `/rename`,
    /// and return the slots whose name changed. Claude Code leaves the name
    /// out of hook payloads after `--resume`, and `/rename` itself fires no
    /// hook, so an idle session renamed from the panel would keep its old
    /// name until it next did something.
    ///
    /// `scanned` says how far each slot's transcript was read (slot id →
    /// byte offset); the caller keeps it outside the registry. Writing the
    /// offsets here would change the registry on every look while a session
    /// works, and the app, which refreshes when the registry changes, would
    /// never rest.
    pub fn backfill_names(&self, scanned: &mut HashMap<String, u64>) -> Result<Vec<String>> {
        self.write()?;
        let mut renamed = Vec::new();
        for slot in self.slots()? {
            // Moss keeps the name it was started with.
            if slot.status == Status::Closed || slot.is_moss() {
                continue;
            }
            let Some(path) = slot.transcript_path.as_deref() else {
                continue;
            };
            // Transcripts grow to tens of megabytes: only read what was
            // appended since the last look (an older cc-moss kept the
            // offset in the registry; start from there).
            let from = match scanned.get(&slot.slot_id) {
                Some(&at) => at,
                None => self
                    .conn
                    .query_row(
                        "SELECT name_scanned_to FROM slots WHERE slot_id = ?1",
                        [&slot.slot_id],
                        |r| r.get::<_, i64>(0),
                    )?
                    .max(0) as u64,
            };
            let (title, to) = crate::transcript::custom_title_since(Path::new(path), from);
            scanned.insert(slot.slot_id.clone(), to);
            if let Some(name) = title.filter(|t| slot.name.as_deref() != Some(t.as_str())) {
                self.set_name(&slot.slot_id, &name)?;
                renamed.push(slot.slot_id);
            }
        }
        Ok(renamed)
    }

    /// What a session last did, from its transcript when the hooks missed
    /// it: the prompt's preview and when, if later than what is recorded.
    /// True when it changed anything.
    pub fn refresh_activity(&self, slot_id: &str, prompt: &str, at_ms: i64) -> Result<bool> {
        let preview = crate::slot::prompt_preview(prompt);
        let changed = self.write()?.execute(
            "UPDATE slots SET last_prompt = COALESCE(?2, last_prompt), last_active_at = ?3
             WHERE slot_id = ?1 AND last_active_at < ?3",
            params![slot_id, preview, at_ms],
        )?;
        Ok(changed == 1)
    }

    /// Whether Claude is working on a prompt (from outside the hooks).
    pub fn set_busy(&self, slot_id: &str, busy: bool) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET busy = ?2 WHERE slot_id = ?1",
            params![slot_id, busy],
        )?;
        Ok(())
    }

    /// Replace a slot's notes.
    pub fn set_notes(&self, slot_id: &str, notes: &Notes) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET note_goal = ?2, note_state = ?3, note_text = ?4 WHERE slot_id = ?1",
            params![slot_id, notes.goal, notes.state, notes.note],
        )?;
        Ok(())
    }

    /// The user does not want this session back: drop it from restore candidates.
    pub fn close_by_user(&self, slot_id: &str, now_ms: i64) -> Result<()> {
        let tx = self.write_tx()?;
        let changed = tx.execute(
            "UPDATE slots SET status = 'closed', end_reason = 'closed-by-user', ended_at = ?2,
               attention = NULL
             WHERE slot_id = ?1",
            params![slot_id, now_ms],
        )?;
        if changed == 1 {
            record_event(&tx, slot_id, "closed", &empty(), now_ms)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Record what cc-moss runs this session as (`moss`).
    pub fn set_role(&self, slot_id: &str, role: &str) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET role = ?2 WHERE slot_id = ?1",
            params![slot_id, role],
        )?;
        Ok(())
    }

    /// Close every open slot with `role` that ran on the tmux server named
    /// `socket` (`tmux -L`), except the session `keep` (an old Moss, once a
    /// new one runs): they are never offered for restore. A slot of that role
    /// elsewhere is not Moss's to close. How many were closed.
    pub fn close_role(
        &self,
        role: &str,
        socket: &str,
        keep: Option<&str>,
        reason: &str,
        now_ms: i64,
    ) -> Result<usize> {
        let conn = self.write()?;
        let mut stmt = conn.prepare(
            "SELECT slot_id, tmux_socket, session_id FROM slots
             WHERE role = ?1 AND status <> 'closed'",
        )?;
        let open: Vec<(String, Option<String>, String)> = stmt
            .query_map(params![role], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut closed = 0;
        for (slot_id, tmux_socket, session_id) in open {
            // `$TMUX` names the server by its socket path: …/tmux-501/<socket>.
            let on_socket = tmux_socket.as_deref().is_some_and(|path| {
                std::path::Path::new(path).file_name() == Some(std::ffi::OsStr::new(socket))
            });
            if !on_socket || keep == Some(session_id.as_str()) {
                continue;
            }
            closed += conn.execute(
                "UPDATE slots SET status = 'closed', end_reason = ?2, ended_at = ?3,
                   attention = NULL, busy = 0
                 WHERE slot_id = ?1 AND status <> 'closed'",
                params![slot_id, reason, now_ms],
            )?;
        }
        Ok(closed)
    }

    /// `cc-moss restore` launched this slot; SessionStart clears the mark.
    pub fn mark_restore_started(&self, slot_id: &str, now_ms: i64) -> Result<()> {
        self.write()?.execute(
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
        let changed = self.write()?.execute(
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
        let tx = self.write_tx()?;
        let changed = tx.execute(
            "UPDATE slots SET pid = ?2, pid_start = ?3, status = 'running', end_reason = NULL,
               ended_at = NULL, tmux_socket = ?4, tmux_session_id = ?5, tmux_session_name = ?6,
               tmux_pane = ?7, last_active_at = ?8, attention = NULL, lost_reported_at = NULL,
               archived_at_ms = NULL
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
        if changed == 1 {
            record_event(&tx, slot_id, "restored", &empty(), now_ms)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// After a restore, older interrupted copies of the same conversation are
    /// superseded: close them so they stop being listed.
    pub fn close_superseded(&self, keep: &str, session_id: &str, now_ms: i64) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET status = 'closed', end_reason = 'switched', ended_at = ?3
             WHERE slot_id <> ?1 AND session_id = ?2 AND status <> 'closed'
               AND restore_started_at IS NULL",
            params![keep, session_id, now_ms],
        )?;
        Ok(())
    }

    /// Undo [`Registry::mark_restore_started`] when the launch failed.
    pub fn clear_restore_started(&self, slot_id: &str) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET restore_started_at = NULL WHERE slot_id = ?1",
            [slot_id],
        )?;
        Ok(())
    }

    /// Record the tmux session name after cc-moss renamed it.
    pub fn set_tmux_session_name(&self, slot_id: &str, name: &str) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET tmux_session_name = ?2 WHERE slot_id = ?1",
            params![slot_id, name],
        )?;
        Ok(())
    }

    /// Put these sessions in the archive; `by` is `auto` or `you`. Running
    /// ones and Moss are never archived, whatever the caller says. One
    /// `archived {count, by}` event for the lot (not tied to one session:
    /// its slot id is empty). Returns the slot ids archived.
    pub fn archive(
        &self,
        slot_ids: &[String],
        by: &str,
        is_running: &dyn Fn(&Slot) -> bool,
        now_ms: i64,
    ) -> Result<Vec<String>> {
        self.write()?;
        let slots = self.slots()?;
        let tx = self.write_tx()?;
        let mut done = Vec::new();
        for slot in slots.iter().filter(|s| slot_ids.contains(&s.slot_id)) {
            if slot.is_moss() || slot.is_archived() || is_running(slot) {
                continue;
            }
            let changed = tx.execute(
                "UPDATE slots SET archived_at_ms = ?2 WHERE slot_id = ?1 AND archived_at_ms IS NULL",
                params![slot.slot_id, now_ms],
            )?;
            if changed == 1 {
                done.push(slot.slot_id.clone());
            }
        }
        if !done.is_empty() {
            let detail = serde_json::json!({ "count": done.len(), "by": by, "slot_ids": done });
            record_event(&tx, GLOBAL, "archived", &detail, now_ms)?;
        }
        tx.commit()?;
        Ok(done)
    }

    /// Take these sessions out of the archive, back into the list; one
    /// `unarchived {by}` event each. Returns the slot ids brought back.
    pub fn unarchive(&self, slot_ids: &[String], by: &str, now_ms: i64) -> Result<Vec<String>> {
        let tx = self.write_tx()?;
        let mut done = Vec::new();
        for id in slot_ids {
            // One that had ended comes back as something to bring back:
            // the list leaves ended sessions out, so it would vanish. Quietly:
            // it is marked as told, so nobody hears it dropped.
            let changed = tx.execute(
                "UPDATE slots SET archived_at_ms = NULL,
                   status = CASE status WHEN 'closed' THEN 'interrupted' ELSE status END,
                   lost_reported_at = CASE status WHEN 'closed' THEN COALESCE(lost_reported_at, ?2)
                                      ELSE lost_reported_at END
                 WHERE slot_id = ?1 AND archived_at_ms IS NOT NULL",
                params![id, now_ms],
            )?;
            if changed == 1 {
                record_event(
                    &tx,
                    id,
                    "unarchived",
                    &serde_json::json!({ "by": by }),
                    now_ms,
                )?;
                done.push(id.clone());
            }
        }
        tx.commit()?;
        Ok(done)
    }

    /// Forget archived sessions for good: their rows, their session ids
    /// and their events go. Only archived ones are touched, whatever the
    /// caller passes. Returns the slot ids removed.
    pub fn purge(&self, slot_ids: &[String]) -> Result<Vec<String>> {
        let tx = self.write_tx()?;
        let mut done = Vec::new();
        for id in slot_ids {
            let archived = tx
                .query_row(
                    "SELECT archived_at_ms IS NOT NULL AND status <> 'running' FROM slots WHERE slot_id = ?1",
                    [id],
                    |r| r.get::<_, bool>(0),
                )
                .optional()?
                .unwrap_or(false);
            if !archived {
                continue;
            }
            tx.execute("DELETE FROM events WHERE slot_id = ?1", [id])?;
            tx.execute("DELETE FROM slot_sessions WHERE slot_id = ?1", [id])?;
            tx.execute("DELETE FROM slots WHERE slot_id = ?1", [id])?;
            done.push(id.clone());
        }
        tx.commit()?;
        Ok(done)
    }

    /// The transcripts of `slot_id` (one per session id it ran) that no
    /// other session in the registry uses: what may go with it.
    pub fn own_transcripts(&self, slot_id: &str, leaving: &[String]) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT ss.session_id, ss.transcript_path FROM slot_sessions ss
              WHERE ss.slot_id = ?1 AND ss.transcript_path IS NOT NULL",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map([slot_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut shared = self.conn.prepare(
            "SELECT slot_id FROM slot_sessions WHERE session_id = ?1
             UNION SELECT slot_id FROM slots WHERE session_id = ?1",
        )?;
        let mut out = Vec::new();
        for (session_id, path) in rows {
            let users: Vec<String> = shared
                .query_map([&session_id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            if users.iter().all(|u| u == slot_id || leaving.contains(u)) && !out.contains(&path) {
                out.push(path);
            }
        }
        Ok(out)
    }

    /// The provider a session started with, and the keys seen so far
    /// (both JSON, from [`crate::provider`]: never a key's value).
    pub fn set_provider(&self, slot_id: &str, started: &str, seen: &str) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET provider_started = ?2, provider_seen_keys = ?3 WHERE slot_id = ?1",
            params![slot_id, started, seen],
        )?;
        Ok(())
    }

    /// The provider key names a session has seen (JSON array).
    pub fn set_provider_seen(&self, slot_id: &str, seen: &str) -> Result<()> {
        self.write()?.execute(
            "UPDATE slots SET provider_seen_keys = ?2 WHERE slot_id = ?1",
            params![slot_id, seen],
        )?;
        Ok(())
    }

    /// Record one event for a session.
    pub fn record(
        &self,
        slot_id: &str,
        kind: &str,
        detail: &serde_json::Value,
        now_ms: i64,
    ) -> Result<()> {
        record_event(self.write()?, slot_id, kind, detail, now_ms)
    }

    /// Record an event about no one session (`slot_id` empty).
    pub fn record_global(&self, kind: &str, detail: &serde_json::Value, now_ms: i64) -> Result<()> {
        record_event(self.write()?, GLOBAL, kind, detail, now_ms)
    }

    /// All slots, oldest first.
    pub fn slots(&self) -> Result<Vec<Slot>> {
        // One read transaction so slots and their session ids are one snapshot.
        let tx = self.conn.unchecked_transaction()?;
        let slots = read_slots(&tx)?;
        tx.commit()?;
        Ok(slots)
    }

    /// The PostToolUse fast path: clear the attention mark of the slot(s)
    /// whose current id is `session_id`, writing only when one is set.
    fn clear_attention_of(&self, session_id: &str) -> Result<()> {
        let marked = self
            .conn
            .prepare_cached(
                "SELECT 1 FROM slots WHERE session_id = ?1 AND attention IS NOT NULL LIMIT 1",
            )?
            .exists([session_id])?;
        if marked {
            self.conn.execute(
                "UPDATE slots SET attention = NULL WHERE session_id = ?1 AND attention IS NOT NULL",
                [session_id],
            )?;
        }
        Ok(())
    }

    /// Record that a running slot's process is gone without a SessionEnd
    /// (SIGKILL, power loss). Once per interruption: nothing is recorded when
    /// the slot's latest event already is `interrupted`.
    pub fn record_lost(&self, slot_id: &str, now_ms: i64) -> Result<bool> {
        self.record_lost_of(slot_id, None, now_ms)
    }

    /// [`Registry::record_lost`] for the process `pid` that was seen gone:
    /// nothing when the slot runs another process by now (restarted).
    pub fn record_lost_of(&self, slot_id: &str, pid: Option<u32>, now_ms: i64) -> Result<bool> {
        let tx = self.write_tx()?;
        if let Some(seen) = pid {
            let now: Option<u32> = tx
                .query_row("SELECT pid FROM slots WHERE slot_id = ?1", [slot_id], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten();
            if now != Some(seen) {
                return Ok(false);
            }
        }
        // On its way back (`restart`, `restore`): not lost.
        let returning = tx
            .query_row(
                "SELECT restore_started_at FROM slots WHERE slot_id = ?1",
                [slot_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .is_some_and(|t| now_ms - t < crate::restore::PENDING_WINDOW_MS);
        if returning {
            return Ok(false);
        }
        let detail = serde_json::json!({ "reason": "process-gone" });
        let recorded = record_interrupted(&tx, slot_id, &detail, now_ms)?;
        clear_attention(&tx, slot_id)?;
        tx.commit()?;
        Ok(recorded)
    }

    /// Drop events recorded before `before_ms`. Returns how many went.
    pub fn prune_events(&self, before_ms: i64) -> Result<usize> {
        Ok(self
            .write()?
            .execute("DELETE FROM events WHERE at < ?1", [before_ms])?)
    }

    /// [`Registry::prune_events`], but only once the oldest event is `slack_ms`
    /// past the cut, so pruning happens in batches instead of on every look
    /// (each prune changes the registry and wakes the app).
    pub fn prune_events_batched(&self, before_ms: i64, slack_ms: i64) -> Result<usize> {
        if !has_events_table(&self.conn)? {
            return Ok(0);
        }
        let oldest: Option<i64> = self
            .conn
            .query_row("SELECT MIN(at) FROM events", [], |r| r.get(0))?;
        match oldest {
            Some(at) if at < before_ms - slack_ms => self.prune_events(before_ms),
            _ => Ok(0),
        }
    }

    /// Events after `since` in order, at most `limit`; without `since`, the
    /// latest [`RECENT_EVENTS`] (also capped by `limit`), oldest first.
    pub fn events(&self, since: Option<i64>, limit: usize) -> Result<Vec<SessionEvent>> {
        if !has_events_table(&self.conn)? {
            // A registry from a newer cc-moss, opened read-only, may lack it.
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let select = "SELECT e.seq, e.slot_id, s.name, e.kind, e.detail, e.at
                        FROM events e LEFT JOIN slots s ON s.slot_id = e.slot_id";
        let mut rows = match since {
            Some(since) => {
                let mut stmt = self.conn.prepare(&format!(
                    "{select} WHERE e.seq > ?1 ORDER BY e.seq LIMIT ?2"
                ))?;
                let rows = stmt.query_map(params![since, limit], read_event)?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let recent = limit.min(RECENT_EVENTS as i64);
                let mut stmt = self
                    .conn
                    .prepare(&format!("{select} ORDER BY e.seq DESC LIMIT ?1"))?;
                let rows = stmt.query_map([recent], read_event)?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        rows.sort_by_key(|e| e.seq);
        Ok(rows)
    }

    /// One slot's events between two times (Unix ms, both included), oldest
    /// first. Bounded by slot and time, for looking back at one moment.
    pub fn events_between(
        &self,
        slot_id: &str,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<SessionEvent>> {
        if !has_events_table(&self.conn)? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT e.seq, e.slot_id, s.name, e.kind, e.detail, e.at
               FROM events e LEFT JOIN slots s ON s.slot_id = e.slot_id
              WHERE e.slot_id = ?1 AND e.at BETWEEN ?2 AND ?3 ORDER BY e.seq LIMIT 50",
        )?;
        let rows = stmt.query_map(params![slot_id, from_ms, to_ms], read_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The highest sequence number ever handed out, 0 when there is none.
    /// Pruning does not lower it: SQLite keeps AUTOINCREMENT's high-water
    /// mark in `sqlite_sequence`.
    pub fn last_event_seq(&self) -> Result<i64> {
        if !has_events_table(&self.conn)? {
            return Ok(0);
        }
        let max: i64 =
            self.conn
                .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?;
        let has_sequence = self
            .conn
            .prepare_cached(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence'",
            )?
            .exists([])?;
        let high: Option<i64> = if has_sequence {
            self.conn
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name = 'events'",
                    [],
                    |r| r.get(0),
                )
                .optional()?
        } else {
            None
        };
        Ok(max.max(high.unwrap_or(0)))
    }
}

/// One row of the event stream.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionEvent {
    /// Increases by one or more with every event; never reused.
    pub seq: i64,
    pub slot_id: String,
    /// The slot's current name.
    pub name: Option<String>,
    pub kind: String,
    pub detail: serde_json::Value,
    pub at_ms: i64,
}

fn read_event(r: &rusqlite::Row) -> rusqlite::Result<SessionEvent> {
    let detail: String = r.get(4)?;
    Ok(SessionEvent {
        seq: r.get(0)?,
        slot_id: r.get(1)?,
        name: r.get(2)?,
        kind: r.get(3)?,
        detail: serde_json::from_str(&detail).unwrap_or_else(|_| serde_json::json!({})),
        at_ms: r.get(5)?,
    })
}

fn has_events_table(conn: &Connection) -> Result<bool> {
    Ok(conn
        .prepare_cached("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'events'")?
        .exists([])?)
}

/// The slot id of an event about no one session (`provider-changed`, an
/// automatic `archived`).
pub const GLOBAL: &str = "";

fn empty() -> serde_json::Value {
    serde_json::json!({})
}

/// The first [`MESSAGE_MAX`] characters of an agent message.
pub fn truncate_message(message: &str) -> String {
    message.chars().take(MESSAGE_MAX).collect()
}

/// Append one event for `slot_id`, in the caller's transaction.
fn record_event(
    conn: &Connection,
    slot_id: &str,
    kind: &str,
    detail: &serde_json::Value,
    at_ms: i64,
) -> Result<()> {
    conn.prepare_cached("INSERT INTO events (slot_id, kind, detail, at) VALUES (?1, ?2, ?3, ?4)")?
        .execute(params![slot_id, kind, detail.to_string(), at_ms])?;
    Ok(())
}

/// Record `interrupted` once per interruption, whoever notices it first.
/// The mark lives on the slot (`lost_reported_at`), not in the events, so
/// pruning old events does not make an old crash news again; a new process
/// for the slot (SessionStart, restore, adoption) clears it.
fn record_interrupted(
    conn: &Connection,
    slot_id: &str,
    detail: &serde_json::Value,
    at_ms: i64,
) -> Result<bool> {
    let fresh = conn.execute(
        "UPDATE slots SET lost_reported_at = ?2 WHERE slot_id = ?1 AND lost_reported_at IS NULL",
        params![slot_id, at_ms],
    )?;
    if fresh == 0 {
        return Ok(false);
    }
    record_event(conn, slot_id, "interrupted", detail, at_ms)?;
    Ok(true)
}

/// Set a slot's name; a change is recorded as `renamed` when `announce`.
fn rename(conn: &Connection, slot_id: &str, name: &str, announce: bool, at_ms: i64) -> Result<()> {
    let from: Option<String> = conn
        .query_row(
            "SELECT name FROM slots WHERE slot_id = ?1",
            [slot_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if from.as_deref() == Some(name) {
        return Ok(());
    }
    conn.execute(
        "UPDATE slots SET name = ?2 WHERE slot_id = ?1",
        params![slot_id, name],
    )?;
    if announce {
        let detail = serde_json::json!({ "from": from, "to": name });
        record_event(conn, slot_id, "renamed", &detail, at_ms)?;
    }
    Ok(())
}

fn clear_attention(conn: &Connection, slot_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE slots SET attention = NULL WHERE slot_id = ?1 AND attention IS NOT NULL",
        [slot_id],
    )?;
    Ok(())
}

fn read_slots(conn: &Connection) -> Result<Vec<Slot>> {
    let mut stmt = conn.prepare(
        "SELECT slot_id, agent, session_id, name, cwd, pid, pid_start,
                    tmux_socket, tmux_session_id, tmux_session_name, tmux_pane,
                    status, end_reason, created_at, last_active_at, ended_at,
                    note_goal, note_state, note_text, launch_args, restore_started_at, busy, last_prompt,
                    attention,
                    (SELECT transcript_path FROM slot_sessions ss
                      WHERE ss.slot_id = slots.slot_id AND ss.session_id = slots.session_id),
                    role, archived_at_ms, provider_started, provider_seen_keys
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
            attention: r.get(23)?,
            transcript_path: r.get(24)?,
            role: r.get(25)?,
            created_at_ms: r.get(13)?,
            last_active_at_ms: r.get(14)?,
            ended_at_ms: r.get(15)?,
            archived_at_ms: r.get(26)?,
            provider_started: r.get(27)?,
            provider_seen_keys: r.get(28)?,
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
        // After 0.1.1: why the session waits for the user, if it does.
        ("attention", "attention TEXT"),
        // After 0.1.1: when this slot's current interruption was reported.
        ("lost_reported_at", "lost_reported_at INTEGER"),
        // After 0.2.2: what cc-moss runs the session as (`moss`).
        ("role", "role TEXT"),
        // After 0.3.0: when the session was archived (none: in the list).
        ("archived_at_ms", "archived_at_ms INTEGER"),
        // After 0.3.0: the API provider the session started with (key
        // names, host and models as JSON; never a key's value), and every
        // provider key name seen in the settings while it ran.
        ("provider_started", "provider_started TEXT"),
        ("provider_seen_keys", "provider_seen_keys TEXT"),
    ] {
        let present = conn
            .prepare("SELECT 1 FROM pragma_table_info('slots') WHERE name = ?1")?
            .exists([column])?;
        if !present {
            // Another process may add it first; that is fine.
            let _ = conn.execute_batch(&format!("ALTER TABLE slots ADD COLUMN {ddl}"));
        }
    }
    // After 0.1.1: the event stream. Added without a version bump: 0.1.x reads and
    // writes the slots as before and simply records no events.
    if !has_events_table(conn)? {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
               seq INTEGER PRIMARY KEY AUTOINCREMENT,
               slot_id TEXT NOT NULL,
               kind TEXT NOT NULL,
               detail TEXT NOT NULL DEFAULT '{}',
               at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS events_slot ON events(slot_id, seq);
             CREATE INDEX IF NOT EXISTS events_at ON events(at);",
        )?;
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
fn event_session_id(event: &Event) -> &str {
    match event {
        Event::Start { session_id, .. }
        | Event::Prompt { session_id, .. }
        | Event::Stop { session_id }
        | Event::End { session_id, .. }
        | Event::Attention { session_id, .. }
        | Event::ToolDone { session_id } => session_id,
    }
}

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
        "UPDATE slots SET pid = ?2, pid_start = ?3, status = 'running', end_reason = NULL, ended_at = NULL,
           lost_reported_at = NULL, archived_at_ms = NULL
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

fn preview(prompt: &str) -> Option<String> {
    crate::slot::prompt_preview(prompt)
}
