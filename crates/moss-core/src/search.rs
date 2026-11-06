//! Finding sessions by what was said in them (F44).
//!
//! A local index, `<state>/search.db` (owner-only), of the words people typed
//! and the text of replies: never tool input or output, thinking or
//! attachments, nor the output of `!` shell commands. Text is redacted
//! ([`crate::ask::redact_kept`]) before it is stored, so keys and passwords in
//! the shapes cc-moss knows never reach it. The daemon keeps it up to date
//! from where it last read each transcript; `cc-moss find` reads it.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::transcript::{said, Speaker};

/// `<state>/search.db`.
pub fn index_path(state: &Path) -> PathBuf {
    state.join("search.db")
}

/// Lines longer than this hold tool output, not words: skipped unread.
const MAX_LINE: u64 = 8 << 20;
/// The index's shape, and what goes into it: an index written by another
/// version is built again from the transcripts (it is only a cache).
/// 1: v0.3.8. 2: `!` command output left out, more keys redacted (never
/// released). 3: as 2, with a `meta` table and FTS secure-delete.
const SCHEMA: i64 = 3;
/// The longest a write holds the index, so a search catching it up gets
/// its turn while the daemon builds a large history.
const SLICE: Duration = Duration::from_millis(250);
/// Characters of context kept around a match.
const BEFORE: usize = 60;
const AFTER: usize = 100;

/// One transcript to index: the session it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub slot_id: String,
    pub session_id: String,
    pub path: PathBuf,
}

/// The transcripts of every session id a slot used, next to its current one.
pub fn sources_of(slot: &crate::slot::Slot) -> Vec<Source> {
    let Some(current) = slot.transcript_path.as_deref().map(Path::new) else {
        return Vec::new();
    };
    let Some(dir) = current.parent() else {
        return Vec::new();
    };
    let mut ids: Vec<&str> = slot.session_ids.iter().map(String::as_str).collect();
    if !ids.contains(&slot.session_id.as_str()) {
        ids.push(&slot.session_id);
    }
    ids.into_iter()
        // A session id names a file in that folder, nothing else.
        .filter(|sid| !sid.is_empty() && sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(|sid| Source {
            slot_id: slot.slot_id.clone(),
            session_id: sid.to_string(),
            path: dir.join(format!("{sid}.jsonl")),
        })
        .filter(|s| s.path.is_file())
        .collect()
}

/// What one pass of [`Index::update`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Progress {
    pub added: usize,
    pub removed: usize,
    /// The pass ran out of time; the next one carries on.
    pub unfinished: bool,
}

/// One session that matched: its newest matching line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub slot_id: String,
    pub session_id: String,
    pub at_ms: i64,
    pub speaker: Speaker,
    pub snippet: String,
}

/// What a search found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    pub hits: Vec<Hit>,
    /// It ran out of time: there may be more.
    pub partial: bool,
}

pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open (or create, owner-only) the index at `path`.
    pub fn open(path: &Path) -> Result<Index> {
        Self::open_waiting(path, Duration::from_secs(2))
    }

    /// [`Index::open`], waiting at most `wait` for another process writing
    /// the index (a search waits no longer than its catch-up may take).
    pub fn open_waiting(path: &Path, wait: Duration) -> Result<Index> {
        if let Some(dir) = path.parent() {
            crate::store_dir::ensure_private_dir(dir)?;
        }
        if !path.exists() {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("{}", path.display()))?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(wait)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::with(conn)
    }

    /// An index in memory (tests).
    pub fn open_in_memory() -> Result<Index> {
        Self::with(Connection::open_in_memory()?)
    }

    fn with(mut conn: Connection) -> Result<Index> {
        // What is deleted (a purged session's words) is overwritten, not
        // left behind in the file.
        conn.pragma_update(None, "secure_delete", "ON")?;
        // Reading the version needs no lock: a search while the daemon
        // writes must not wait here.
        if schema_version(&conn)? != SCHEMA {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if schema_version(&tx)? != SCHEMA {
                build(&tx)?;
            }
            tx.commit()?;
        }
        Ok(Index { conn })
    }

    /// The daemon's index. Built again, once, the first time a daemon of
    /// this version opens it: one of the previous version may have gone on
    /// writing its way into it while an upgrade was under way.
    pub fn open_for_daemon(path: &Path) -> Result<Index> {
        let mut index = Self::open(path)?;
        let tx = index
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ours: Option<i64> = tx
            .query_row(
                "SELECT value FROM meta WHERE key = 'daemon_schema'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if ours != Some(SCHEMA) {
            build(&tx)?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('daemon_schema', ?1)",
                [SCHEMA],
            )?;
        }
        tx.commit()?;
        Ok(index)
    }

    /// Bring the index up to date with `sources` (all of them: a transcript
    /// not listed any more, its session purged or forgotten, is dropped).
    /// Reads from where the last pass stopped, whole lines only; a file
    /// that got shorter is read again from the start. Stops at `deadline`.
    pub fn update(&mut self, sources: &[Source], deadline: Instant) -> Result<Progress> {
        let mut progress = Progress::default();
        let listed: HashSet<&Path> = sources.iter().map(|s| s.path.as_path()).collect();
        let known: Vec<String> = self
            .conn
            .prepare("SELECT path FROM files")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for path in known
            .iter()
            .filter(|p| !listed.contains(Path::new(p.as_str())))
        {
            progress.removed += self.drop_file(path)?;
        }
        'sources: for source in sources {
            loop {
                if Instant::now() >= deadline {
                    progress.unfinished = true;
                    break 'sources;
                }
                let slice = deadline.min(Instant::now() + SLICE);
                let (added, done) = self.read_file(source, slice)?;
                progress.added += added;
                if done {
                    break;
                }
            }
        }
        if progress.removed > 0 {
            // Out of the write-ahead log too, best effort.
            let _ = self
                .conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        }
        Ok(progress)
    }

    fn drop_file(&mut self, path: &str) -> Result<usize> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = tx.execute("DELETE FROM lines WHERE path = ?1", [path])?;
        tx.execute("DELETE FROM files WHERE path = ?1", [path])?;
        tx.commit()?;
        Ok(n)
    }

    /// Index what was added to one transcript; `(lines added, reached its end)`.
    fn read_file(&mut self, source: &Source, deadline: Instant) -> Result<(usize, bool)> {
        let path = source.path.to_string_lossy().into_owned();
        // Only a plain file: a folder can't be read, and opening a pipe
        // would wait for a writer forever.
        if !std::fs::metadata(&source.path).is_ok_and(|m| m.is_file()) {
            return Ok((0, true));
        }
        let Ok(file) = File::open(&source.path) else {
            return Ok((0, true));
        };
        // Where the last pass stopped is read inside the write, so two
        // processes catching up at once never both add the same lines.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let size = file.metadata()?.len();
        let mut from: u64 = tx
            .query_row("SELECT read_to FROM files WHERE path = ?1", [&path], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
            .map_or(0, |n| u64::try_from(n).unwrap_or(0));
        if size < from {
            // Rewritten: start over.
            tx.execute("DELETE FROM lines WHERE path = ?1", [&path])?;
            tx.execute("UPDATE files SET read_to = 0 WHERE path = ?1", [&path])?;
            from = 0;
        }
        if size == from {
            tx.commit()?;
            return Ok((0, true));
        }
        let mut reader = BufReader::with_capacity(256 << 10, file);
        reader.seek(SeekFrom::Start(from))?;
        let mut added = 0;
        let mut at = from;
        let mut done = true;
        let mut line = Vec::new();
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO lines (path, slot_id, session_id, at, speaker, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            loop {
                if Instant::now() >= deadline {
                    done = false;
                    break;
                }
                line.clear();
                let n = (&mut reader)
                    .take(MAX_LINE + 1)
                    .read_until(b'\n', &mut line)?;
                if n == 0 || line.last() != Some(&b'\n') {
                    if n as u64 > MAX_LINE {
                        // A huge line: skip to its end without keeping it.
                        let mut rest = 0u64;
                        let mut sink = Vec::new();
                        loop {
                            sink.clear();
                            let m = (&mut reader).take(MAX_LINE).read_until(b'\n', &mut sink)?;
                            rest += m as u64;
                            if m == 0 || sink.last() == Some(&b'\n') {
                                break;
                            }
                        }
                        if sink.last() == Some(&b'\n') {
                            at += n as u64 + rest;
                            continue;
                        }
                    }
                    // A line still being written: read it next time.
                    break;
                }
                at += n as u64;
                if let Some((speaker, text, at_ms)) = said(&line) {
                    let text = crate::ask::redact_kept(&text);
                    insert.execute(params![
                        path,
                        source.slot_id,
                        source.session_id,
                        at_ms,
                        speaker.as_str(),
                        text
                    ])?;
                    added += 1;
                }
            }
        }
        tx.execute(
            "INSERT INTO files (path, slot_id, session_id, read_to) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET slot_id = ?2, session_id = ?3, read_to = ?4",
            params![path, source.slot_id, source.session_id, i64::try_from(at)?],
        )?;
        tx.commit()?;
        Ok((added, done))
    }

    /// Sessions whose words contain `query` (case-insensitive), newest
    /// first, one hit each, at most `limit`. Stops at `deadline` and says so.
    pub fn find(&self, query: &str, limit: usize, deadline: Instant) -> Result<Found> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Found::default());
        }
        // Three characters and more: the trigram index. Fewer: a scan of
        // the words alone (the index has no shorter grams).
        let (sql, arg) = if query.chars().count() >= 3 {
            (
                "SELECT l.slot_id, l.session_id, l.at, l.speaker, l.text
                 FROM lines_fts f JOIN lines l ON l.id = f.rowid
                 WHERE lines_fts MATCH ?1 ORDER BY l.at DESC",
                format!("\"{}\"", query.replace('"', "\"\"")),
            )
        } else {
            (
                "SELECT slot_id, session_id, at, speaker, text FROM lines
                 WHERE text LIKE ?1 ESCAPE '\\' ORDER BY at DESC",
                format!("%{}%", escape_like(query)),
            )
        };
        let mut found = Found::default();
        if Instant::now() >= deadline {
            found.partial = true;
            return Ok(found);
        }
        // Past the deadline the running query is interrupted.
        let interrupt = self.conn.get_interrupt_handle();
        let (finished, wait) = std::sync::mpsc::channel::<()>();
        let left = deadline.saturating_duration_since(Instant::now());
        let timer = std::thread::spawn(move || {
            if wait.recv_timeout(left) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                interrupt.interrupt();
            }
        });
        let mut seen: HashSet<String> = HashSet::new();
        let result = (|| -> rusqlite::Result<()> {
            let mut stmt = self.conn.prepare(sql)?;
            let mut rows = stmt.query([&arg])?;
            while let Some(r) = rows.next()? {
                if Instant::now() >= deadline {
                    found.partial = true;
                    break;
                }
                let slot_id: String = r.get(0)?;
                if seen.contains(&slot_id) {
                    continue;
                }
                let text: String = r.get(4)?;
                let Some(snippet) = snippet(&text, query) else {
                    continue;
                };
                let speaker = match r.get::<_, String>(3)?.as_str() {
                    "you" => Speaker::You,
                    _ => Speaker::Reply,
                };
                seen.insert(slot_id.clone());
                found.hits.push(Hit {
                    slot_id,
                    session_id: r.get(1)?,
                    at_ms: r.get(2)?,
                    speaker,
                    snippet,
                });
                if found.hits.len() >= limit {
                    break;
                }
            }
            Ok(())
        })();
        drop(finished);
        let _ = timer.join();
        match result {
            Ok(()) => {}
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::OperationInterrupted =>
            {
                found.partial = true;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(found)
    }
}

/// The index from scratch: what was in it goes.
fn build(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DROP TRIGGER IF EXISTS lines_ai;
         DROP TRIGGER IF EXISTS lines_ad;
         DROP TABLE IF EXISTS lines_fts;
         DROP TABLE IF EXISTS lines;
         DROP TABLE IF EXISTS files;
         DROP TABLE IF EXISTS meta;
         CREATE TABLE files (
             path TEXT PRIMARY KEY,
             slot_id TEXT NOT NULL,
             session_id TEXT NOT NULL,
             read_to INTEGER NOT NULL
         );
         CREATE TABLE lines (
             id INTEGER PRIMARY KEY,
             path TEXT NOT NULL,
             slot_id TEXT NOT NULL,
             session_id TEXT NOT NULL,
             at INTEGER NOT NULL,
             speaker TEXT NOT NULL,
             text TEXT NOT NULL
         );
         CREATE INDEX lines_path ON lines(path);
         CREATE INDEX lines_at ON lines(at);
         CREATE TABLE meta (key TEXT PRIMARY KEY, value NOT NULL);
         CREATE VIRTUAL TABLE lines_fts USING fts5(
             text, content='lines', content_rowid='id', tokenize='trigram');
         -- A deleted line's words leave the full-text index itself, not
         -- only the table: otherwise they stay there until it is merged.
         INSERT INTO lines_fts(lines_fts, rank) VALUES ('secure-delete', 1);
         CREATE TRIGGER lines_ai AFTER INSERT ON lines BEGIN
             INSERT INTO lines_fts(rowid, text) VALUES (new.id, new.text);
         END;
         CREATE TRIGGER lines_ad AFTER DELETE ON lines BEGIN
             INSERT INTO lines_fts(lines_fts, rowid, text) VALUES ('delete', old.id, old.text);
         END;",
    )?;
    tx.pragma_update(None, "user_version", SCHEMA)
}

/// The error says another process holds the index for a write.
pub fn is_busy(e: &anyhow::Error) -> bool {
    e.downcast_ref::<rusqlite::Error>().is_some_and(|e| {
        matches!(
            e.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
        )
    })
}

fn schema_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// About 160 characters of `text` around the first case-insensitive
/// match of `query`, on one line; `None` if it isn't there.
pub fn snippet(text: &str, query: &str) -> Option<String> {
    let flat: Vec<char> = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect();
    let needle: Vec<char> = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect();
    if needle.is_empty() || needle.len() > flat.len() {
        return None;
    }
    // One lower-case character for each, so positions line up with `flat`;
    // then one linear search, however long the text.
    let lower = |c: &char| c.to_lowercase().next().unwrap_or(*c);
    let hay: String = flat.iter().map(lower).collect();
    let pin: String = needle.iter().map(lower).collect();
    let byte = hay.find(&pin)?;
    let start = hay[..byte].chars().count();
    let from = start.saturating_sub(BEFORE);
    let to = (start + needle.len() + AFTER).min(flat.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.extend(&flat[from..to]);
    if to < flat.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    fn user(text: &str, at: &str) -> String {
        serde_json::json!({"type":"user","timestamp":at,
            "message":{"role":"user","content":text}})
        .to_string()
    }

    fn reply(text: &str, at: &str) -> String {
        serde_json::json!({"type":"assistant","timestamp":at,
            "message":{"role":"assistant","content":[
                {"type":"text","text":text},
                {"type":"tool_use","name":"Bash","input":{"command":"grep needle-in-tool-input"}}]}})
        .to_string()
    }

    fn tool_result(text: &str, at: &str) -> String {
        serde_json::json!({"type":"user","timestamp":at,
            "message":{"role":"user","content":[{"type":"tool_result","content":text}]}})
        .to_string()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
    }

    impl Fixture {
        fn new(lines: &[String]) -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("s1.jsonl");
            let mut f = File::create(&path).unwrap();
            for l in lines {
                writeln!(f, "{l}").unwrap();
            }
            Fixture { _dir: dir, path }
        }

        fn source(&self) -> Source {
            Source {
                slot_id: "slot-1".into(),
                session_id: "s1".into(),
                path: self.path.clone(),
            }
        }

        fn append(&self, line: &str) {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&self.path)
                .unwrap();
            writeln!(f, "{line}").unwrap();
        }
    }

    #[test]
    fn finds_what_was_typed_and_replied_not_tool_output() {
        let fx = Fixture::new(&[
            user("把退款逻辑改成按订单分摊", "2026-11-01T10:00:00Z"),
            reply("Refund logic now splits per order.", "2026-11-01T10:01:00Z"),
            tool_result("secret-tool-output-words", "2026-11-01T10:02:00Z"),
        ]);
        let mut idx = Index::open_in_memory().unwrap();
        let p = idx.update(&[fx.source()], far()).unwrap();
        assert_eq!(p.added, 2);
        for q in ["退款逻辑", "退款", "splits per", "REFUND"] {
            let f = idx.find(q, 10, far()).unwrap();
            assert_eq!(f.hits.len(), 1, "{q}");
            assert_eq!(f.hits[0].slot_id, "slot-1");
        }
        assert!(idx
            .find("secret-tool-output", 10, far())
            .unwrap()
            .hits
            .is_empty());
        assert!(idx
            .find("needle-in-tool-input", 10, far())
            .unwrap()
            .hits
            .is_empty());
    }

    #[test]
    fn shell_commands_and_their_output_are_left_out() {
        let fx = Fixture::new(&[
            user("<bash-input>cat .env</bash-input>", "2026-11-01T10:00:00Z"),
            user(
                "<bash-stdout>DB_PASSWORD=hunter2-in-env</bash-stdout><bash-stderr></bash-stderr>",
                "2026-11-01T10:00:01Z",
            ),
            user("what does the env hold", "2026-11-01T10:01:00Z"),
        ]);
        let mut idx = Index::open_in_memory().unwrap();
        assert_eq!(idx.update(&[fx.source()], far()).unwrap().added, 1);
        for q in ["hunter2", "cat .env", "bash-stdout"] {
            assert!(idx.find(q, 10, far()).unwrap().hits.is_empty(), "{q}");
        }
    }

    #[test]
    fn writers_at_once_add_each_line_once() {
        let dir = tempfile::tempdir().unwrap();
        let lines: Vec<String> = (0..20_000)
            .map(|i| user(&format!("line number {i} of many"), "2026-11-01T10:00:00Z"))
            .collect();
        let fx = Fixture::new(&lines);
        let db = dir.path().join("search.db");
        Index::open(&db).unwrap();
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let (db, source) = (db.clone(), fx.source());
                std::thread::spawn(move || {
                    let mut idx = Index::open(&db).unwrap();
                    // A busy machine may make one give up waiting: it just
                    // tries again, as the daemon does.
                    loop {
                        let deadline = Instant::now() + Duration::from_millis(300);
                        if let Ok(p) = idx.update(std::slice::from_ref(&source), deadline) {
                            if !p.unfinished {
                                break;
                            }
                        }
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        let idx = Index::open(&db).unwrap();
        let (rows, distinct): (i64, i64) = idx
            .conn
            .query_row(
                "SELECT count(*), count(DISTINCT text) FROM lines",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((rows, distinct), (20_000, 20_000));
    }

    #[test]
    fn catching_up_waits_while_another_process_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("search.db");
        let fx = Fixture::new(&[user("said while it was busy", "2026-11-01T10:00:00Z")]);
        let mut idx = Index::open(&db).unwrap();
        // The daemon, mid-write: it holds the index for a moment.
        let mut other = Connection::open(&db).unwrap();
        let (held, release) = std::sync::mpsc::channel::<()>();
        let writer = std::thread::spawn(move || {
            let tx = other
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute(
                "INSERT INTO files (path, slot_id, session_id, read_to) VALUES ('x', 's', 's', 0)",
                [],
            )
            .unwrap();
            held.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            tx.commit().unwrap();
        });
        release.recv().unwrap();
        let p = idx.update(&[fx.source()], far()).unwrap();
        writer.join().unwrap();
        assert_eq!(p.added, 1);
    }

    #[test]
    fn an_index_from_another_version_is_built_again() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("search.db");
        let fx = Fixture::new(&[user("words from before", "2026-11-01T10:00:00Z")]);
        let mut idx = Index::open(&db).unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        idx.conn.pragma_update(None, "user_version", 1).unwrap();
        drop(idx);
        let mut idx = Index::open(&db).unwrap();
        assert!(idx.find("from before", 10, far()).unwrap().hits.is_empty());
        let version: i64 = idx
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA);
        assert_eq!(
            idx.update(&[fx.source()], far()).unwrap().added,
            1,
            "read again"
        );
    }

    #[test]
    fn what_a_dropped_session_said_is_gone_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("search.db");
        let fx = Fixture::new(&[user("UNIQUEPURGEMARKER please", "2026-11-01T10:00:00Z")]);
        let mut idx = Index::open(&db).unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        assert_eq!(idx.update(&[], far()).unwrap().removed, 1);
        drop(idx);
        for f in ["search.db", "search.db-wal"] {
            let bytes = std::fs::read(dir.path().join(f)).unwrap_or_default();
            assert!(
                !bytes.windows(17).any(|w| w == b"UNIQUEPURGEMARKER"),
                "{f} still holds it"
            );
            // Nor its pieces in the full-text index ("uniquepurgemarker"'s
            // three-letter grams that are stored whole).
            for gram in [&b"iqu"[..], b"niq", b"gem", b"rge"] {
                assert!(
                    !bytes.windows(3).any(|w| w == gram),
                    "{f} still holds {}",
                    String::from_utf8_lossy(gram)
                );
            }
        }
    }

    #[test]
    fn a_transcript_emptied_then_written_again_is_read_from_the_start() {
        let fx = Fixture::new(&[user("before it was emptied", "2026-11-01T10:00:00Z")]);
        let mut idx = Index::open_in_memory().unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        std::fs::write(&fx.path, "").unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        let longer = (0..5)
            .map(|i| user(&format!("written again, part {i}"), "2026-11-02T10:00:00Z") + "\n")
            .collect::<String>();
        std::fs::write(&fx.path, longer).unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        assert!(idx
            .find("before it was", 10, far())
            .unwrap()
            .hits
            .is_empty());
        assert_eq!(idx.find("part 0", 10, far()).unwrap().hits.len(), 1);
    }

    #[test]
    fn a_new_daemon_builds_the_index_again_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("search.db");
        let fx = Fixture::new(&[user("written by someone else", "2026-11-01T10:00:00Z")]);
        // A search (or an older daemon) filled it.
        Index::open(&db)
            .unwrap()
            .update(&[fx.source()], far())
            .unwrap();
        let idx = Index::open_for_daemon(&db).unwrap();
        assert!(
            idx.find("someone else", 10, far()).unwrap().hits.is_empty(),
            "built again"
        );
        let mut idx = idx;
        idx.update(&[fx.source()], far()).unwrap();
        drop(idx);
        // The next start of the same daemon keeps what it built.
        let idx = Index::open_for_daemon(&db).unwrap();
        assert_eq!(idx.find("someone else", 10, far()).unwrap().hits.len(), 1);
    }

    #[test]
    fn an_index_from_the_unreleased_shape_is_built_again() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("search.db");
        let idx = Index::open(&db).unwrap();
        // Version 2: no meta table, no secure-delete.
        idx.conn.execute_batch("DROP TABLE meta").unwrap();
        idx.conn.pragma_update(None, "user_version", 2).unwrap();
        drop(idx);
        Index::open_for_daemon(&db).unwrap();
        Index::open_for_daemon(&db).unwrap();
    }

    #[test]
    fn opening_while_another_process_rebuilds_says_it_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("search.db");
        let idx = Index::open(&db).unwrap();
        idx.conn.pragma_update(None, "user_version", 1).unwrap();
        drop(idx);
        let mut other = Connection::open(&db).unwrap();
        let tx = other
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let t = Instant::now();
        let err = Index::open_waiting(&db, Duration::from_millis(200))
            .err()
            .expect("busy");
        assert!(is_busy(&err), "{err:#}");
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        drop(tx);
    }

    #[test]
    fn a_source_that_is_not_a_file_is_passed_over() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("s0.jsonl");
        std::fs::create_dir(&folder).unwrap();
        let fx = Fixture::new(&[user("still found", "2026-11-01T10:00:00Z")]);
        let bad = Source {
            slot_id: "slot-0".into(),
            session_id: "s0".into(),
            path: folder,
        };
        let mut idx = Index::open_in_memory().unwrap();
        idx.update(&[bad, fx.source()], far()).unwrap();
        assert_eq!(idx.find("still found", 10, far()).unwrap().hits.len(), 1);
    }

    #[test]
    fn a_snippet_of_a_long_text_is_quick() {
        // A near miss all along a million characters: one linear search
        // takes a fraction of a second; comparing character by character,
        // allocating as it went, took seconds.
        let text = format!("{}needle at the end", "ab".repeat(500_000));
        let near = format!("{}c", "ab".repeat(100));
        let quickest = (0..3)
            .map(|_| {
                let t = Instant::now();
                assert!(snippet(&text, &near).is_none());
                assert!(snippet(&text, "NEEDLE")
                    .unwrap()
                    .contains("needle at the end"));
                t.elapsed()
            })
            .min()
            .unwrap();
        assert!(quickest < Duration::from_secs(3), "{quickest:?}");
    }

    #[test]
    fn newest_match_per_session_with_its_snippet() {
        let fx = Fixture::new(&[
            user("first look at the login page", "2026-11-01T10:00:00Z"),
            user(
                "again the login page, with the remembered email",
                "2026-11-02T10:00:00Z",
            ),
        ]);
        let mut idx = Index::open_in_memory().unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        let f = idx.find("login page", 10, far()).unwrap();
        assert_eq!(f.hits.len(), 1);
        assert!(
            f.hits[0].snippet.contains("remembered email"),
            "{:?}",
            f.hits[0]
        );
        assert_eq!(f.hits[0].speaker, Speaker::You);
    }

    #[test]
    fn keys_never_reach_the_index() {
        let fx = Fixture::new(&[user(
            "deploy with export OPENAI_API_KEY=sk-proj-abcdefghijklmnop1234 please",
            "2026-11-01T10:00:00Z",
        )]);
        let mut idx = Index::open_in_memory().unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        let f = idx.find("deploy with", 10, far()).unwrap();
        assert!(f.hits[0].snippet.contains("sk-***"), "{:?}", f.hits[0]);
        assert!(idx
            .find("abcdefghijklmnop1234", 10, far())
            .unwrap()
            .hits
            .is_empty());
        let stored: String = idx
            .conn
            .query_row("SELECT text FROM lines", [], |r| r.get(0))
            .unwrap();
        assert!(!stored.contains("abcdefghijklmnop1234"));
    }

    #[test]
    fn reads_on_from_where_it_stopped_and_drops_what_went() {
        let fx = Fixture::new(&[user("one apple", "2026-11-01T10:00:00Z")]);
        let mut idx = Index::open_in_memory().unwrap();
        assert_eq!(idx.update(&[fx.source()], far()).unwrap().added, 1);
        assert_eq!(
            idx.update(&[fx.source()], far()).unwrap().added,
            0,
            "nothing new"
        );
        fx.append(&user("two bananas", "2026-11-01T11:00:00Z"));
        assert_eq!(idx.update(&[fx.source()], far()).unwrap().added, 1);
        assert_eq!(idx.find("banana", 10, far()).unwrap().hits.len(), 1);
        // A line still being written is left for the next pass.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&fx.path)
            .unwrap();
        write!(
            f,
            "{}",
            &user("three cherries", "2026-11-01T12:00:00Z")[..20]
        )
        .unwrap();
        assert_eq!(idx.update(&[fx.source()], far()).unwrap().added, 0);
        // Rewritten shorter: read again from the start.
        std::fs::write(
            &fx.path,
            format!("{}\n", user("only grapes", "2026-11-01T13:00:00Z")),
        )
        .unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        assert!(idx.find("apple", 10, far()).unwrap().hits.is_empty());
        assert_eq!(idx.find("grapes", 10, far()).unwrap().hits.len(), 1);
        // The session went (purged): its words go too.
        let p = idx.update(&[], far()).unwrap();
        assert_eq!(p.removed, 1);
        assert!(idx.find("grapes", 10, far()).unwrap().hits.is_empty());
    }

    #[test]
    fn out_of_time_says_partial_not_nothing() {
        let fx = Fixture::new(&[user("some words", "2026-11-01T10:00:00Z")]);
        let mut idx = Index::open_in_memory().unwrap();
        let past = Instant::now() - Duration::from_millis(1);
        let p = idx.update(&[fx.source()], past).unwrap();
        assert!(p.unfinished);
        idx.update(&[fx.source()], far()).unwrap();
        let f = idx.find("some words", 10, past).unwrap();
        assert!(f.partial, "{f:?}");
    }

    #[test]
    fn like_wildcards_are_literal() {
        let fx = Fixture::new(&[user("100% done", "2026-11-01T10:00:00Z")]);
        let mut idx = Index::open_in_memory().unwrap();
        idx.update(&[fx.source()], far()).unwrap();
        assert_eq!(idx.find("0%", 10, far()).unwrap().hits.len(), 1);
        assert!(idx.find("_x", 10, far()).unwrap().hits.is_empty());
    }

    #[test]
    fn snippet_is_short_and_on_one_line() {
        let long = format!("{} needle {}", "a ".repeat(200), "b ".repeat(200));
        let s = snippet(&long, "NEEDLE").unwrap();
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(
            s.chars().count() <= BEFORE + AFTER + 8,
            "{}",
            s.chars().count()
        );
        assert!(!s.contains('\n'));
        assert_eq!(snippet("short", "missing"), None);
    }
}
