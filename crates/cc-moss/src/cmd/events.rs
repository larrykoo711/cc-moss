//! `cc-moss events`: what just happened to the sessions, in order.
//!
//! Cheap on purpose: it reads the event table and nothing else (no
//! reconcile, no process checks), so an app can poll it.

use anyhow::Result;
use moss_core::registry::{Registry, SessionEvent};
use moss_core::slot::rfc3339;
use moss_core::store_dir;
use serde_json::{json, Value};

pub const JSON_SCHEMA: u32 = 1;

pub fn run(since: Option<i64>, limit: usize, as_json: bool) -> Result<()> {
    let path = store_dir::registry_path();
    let (events, last_seq) = if path.exists() {
        let registry = Registry::open(&path)?;
        let events = registry.events(since, limit)?;
        // Where to continue from: the last event returned, else the newest
        // one there is (so a caller ahead of a reset registry catches up).
        let last = match events.last() {
            Some(e) => e.seq,
            None => registry.last_event_seq()?,
        };
        (events, last)
    } else {
        (Vec::new(), 0)
    };

    if as_json {
        let events: Vec<Value> = events.iter().map(to_json).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": JSON_SCHEMA,
                "events": events,
                "last_seq": last_seq,
            }))?
        );
        return Ok(());
    }
    if events.is_empty() {
        println!("{}", tr!("No events yet.", "还没有事件。"));
    }
    for e in &events {
        println!(
            "{:>6}  {}  {:<12} {}",
            e.seq,
            rfc3339(e.at_ms),
            e.kind,
            e.name.as_deref().unwrap_or(&e.slot_id)
        );
    }
    Ok(())
}

fn to_json(e: &SessionEvent) -> Value {
    json!({
        "seq": e.seq,
        "slot_id": e.slot_id,
        "name": e.name,
        "kind": e.kind,
        "detail": e.detail,
        "at": rfc3339(e.at_ms),
    })
}
