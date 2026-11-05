//! `cc-moss find <words>`: sessions by what was said in them (F44), from
//! the local index the daemon keeps (`moss_core::search`). Read-only on the
//! sessions, offline; snippets were redacted before they were stored.

use std::time::{Duration, Instant};

use anyhow::Result;
use moss_core::registry::Registry;
use moss_core::search::{self, Index};
use moss_core::slot::{now_ms, rfc3339, Slot};
use moss_core::store_dir;
use serde_json::json;

/// Catching the index up first, so what was just said is found too.
const CATCH_UP: Duration = Duration::from_millis(300);
/// The search itself; with the catch-up, a second at most.
const SEARCH: Duration = Duration::from_millis(700);

/// Search the index in `state` for `query` among `slots` (Moss's own left
/// out), catching it up first: what was found, and whether the index is
/// still being built.
pub fn search(
    state: &std::path::Path,
    slots: &[Slot],
    query: &str,
    limit: usize,
) -> Result<(search::Found, bool)> {
    let mut index = Index::open(&search::index_path(state))?;
    let sources: Vec<search::Source> = slots
        .iter()
        .filter(|s| !s.is_moss())
        .flat_map(search::sources_of)
        .collect();
    let indexing = index
        .update(&sources, Instant::now() + CATCH_UP)?
        .unfinished;
    let mut found = index.find(query, limit.max(1), Instant::now() + SEARCH)?;
    found
        .hits
        .retain(|h| slots.iter().any(|s| s.slot_id == h.slot_id && !s.is_moss()));
    Ok((found, indexing))
}

pub fn run(words: &[String], limit: usize, json: bool) -> Result<()> {
    let query = words.join(" ");
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let (found, indexing) = search(&store_dir::state_dir(), &slots, &query, limit)?;
    let now = now_ms();
    let matches: Vec<_> = found
        .hits
        .iter()
        .filter_map(|h| {
            let slot = slots.iter().find(|s| s.slot_id == h.slot_id)?;
            let st = slot.effective_status(moss_core::proc::is_alive);
            Some((slot, st, h))
        })
        .collect();
    if json {
        let list: Vec<_> = matches
            .iter()
            .map(|(slot, st, h)| {
                json!({
                    "id": slot.slot_id,
                    "name": slot.name,
                    "status": st.as_str(),
                    "archived": slot.archived_at_ms.is_some(),
                    "session_id": h.session_id,
                    "at": rfc3339(h.at_ms),
                    "speaker": h.speaker.as_str(),
                    "snippet": h.snippet,
                })
            })
            .collect();
        println!(
            "{}",
            json!({ "schema": 1, "query": query, "partial": found.partial,
                    "indexing": indexing, "matches": list })
        );
        return Ok(());
    }
    if matches.is_empty() {
        let line = if found.partial || indexing {
            tr!(
                "Nothing found yet; the search didn't finish (the index is still being built). Try again in a moment.",
                "暂时没找到；还没搜完（索引还在建）。稍后再试一次。"
            )
        } else {
            tr!("No session mentions that.", "没有会话提到过这个。")
        };
        println!("{line}");
        return Ok(());
    }
    for (slot, st, h) in &matches {
        let name = slot
            .name
            .clone()
            .unwrap_or_else(|| super::ls::short_id(&slot.slot_id));
        println!(
            "{name}  {}  {}\n    {}",
            super::ls::status_word(*st),
            super::ls::ago(now - h.at_ms),
            h.snippet
        );
    }
    if found.partial || indexing {
        println!(
            "{}",
            tr!(
                "(The search didn't finish; there may be more.)",
                "（没有搜完，可能还有更多。）"
            )
        );
    }
    Ok(())
}

/// How often the daemon catches the index up, and how long each pass may take.
const EVERY: Duration = Duration::from_secs(5);
const PASS: Duration = Duration::from_secs(2);

/// The daemon's indexer: keeps `search.db` up to date with every session's
/// transcripts (Moss's own left out), in its own thread so a first build of
/// a large history never holds up the rest of the daemon.
pub fn worker(stop: std::sync::Arc<std::sync::atomic::AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut index: Option<Index> = None;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let pass = (|| -> Result<search::Progress> {
                if index.is_none() {
                    index = Some(Index::open(&search::index_path(&store_dir::state_dir()))?);
                }
                let slots = Registry::open(&store_dir::registry_path())?.slots()?;
                let sources: Vec<search::Source> = slots
                    .iter()
                    .filter(|s| !s.is_moss())
                    .flat_map(search::sources_of)
                    .collect();
                let index = index.as_mut().expect("opened above");
                index.update(&sources, Instant::now() + PASS)
            })();
            let wait = match pass {
                Ok(p) if p.unfinished => Duration::from_millis(200),
                Ok(_) => EVERY,
                Err(e) => {
                    eprintln!("cc-moss: search index: {e:#}");
                    index = None;
                    EVERY
                }
            };
            std::thread::park_timeout(wait);
        }
    })
}
