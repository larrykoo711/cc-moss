//! Edits Claude Code's user `settings.json` so it runs the cc-moss hooks.
//!
//! The file belongs to the user, so every function here touches only hook
//! entries that [`is_ours`] recognises. Everything else, including key order,
//! is carried through unchanged (serde_json is built with `preserve_order`).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};

use crate::agent::HookKind;

/// Seconds Claude Code waits for a hook. The hook itself takes milliseconds;
/// this only bounds the damage if something hangs.
pub const HOOK_TIMEOUT_SECS: u64 = 10;

/// Claude Code's event name for each hook cc-moss registers.
pub fn event_name(kind: HookKind) -> &'static str {
    match kind {
        HookKind::SessionStart => "SessionStart",
        HookKind::UserPromptSubmit => "UserPromptSubmit",
        HookKind::Stop => "Stop",
        HookKind::SessionEnd => "SessionEnd",
    }
}

/// The shell command Claude Code runs for `kind`. Claude Code hands the
/// command to a shell, so any path beyond plain characters is single-quoted.
pub fn hook_command(exe: &str, kind: HookKind) -> String {
    let plain = exe
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+@%,:=".contains(c));
    if plain {
        format!("{exe} hook {}", kind.cli_name())
    } else {
        format!("'{}' hook {}", exe.replace('\'', "'\\''"), kind.cli_name())
    }
}

/// Whether one hook entry (`{"type": "command", "command": ...}`) was written
/// by cc-moss. Matches on the command shape rather than the exact path, so a
/// reinstall from a new location replaces the old entry instead of adding one.
pub fn is_ours(hook: &Value) -> bool {
    let Some(command) = hook.get("command").and_then(Value::as_str) else {
        return false;
    };
    let bare: String = command
        .chars()
        .filter(|c| !matches!(c, '"' | '\''))
        .collect();
    HookKind::ALL.into_iter().any(|kind| {
        let tail = format!("cc-moss hook {}", kind.cli_name());
        match bare.strip_suffix(&tail) {
            Some(prefix) => prefix.is_empty() || prefix.ends_with('/'),
            None => false,
        }
    })
}

/// Add the four cc-moss hooks, replacing any earlier cc-moss entries in place.
pub fn install(doc: &Value, exe: &str) -> Result<Value> {
    let mut doc = doc.clone();
    let root = root_object(&mut doc)?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks
        .as_object_mut()
        .context("settings.json: `hooks` is not an object")?;
    for kind in HookKind::ALL {
        let event = event_name(kind);
        let ours = json!({ "hooks": [{
            "type": "command",
            "command": hook_command(exe, kind),
            "timeout": HOOK_TIMEOUT_SECS,
        }] });
        let groups = match hooks.get(event) {
            Some(v) => event_groups(event, v)?,
            None => Vec::new(),
        };
        let stripped = strip_ours(groups);
        let mut out = stripped.groups;
        let at = stripped.first_ours.unwrap_or(out.len());
        out.insert(at, ours);
        hooks.insert(event.to_string(), Value::Array(out));
    }
    Ok(doc)
}

/// Remove every cc-moss entry. Arrays and the `hooks` object that only held
/// cc-moss entries are removed too, so install + uninstall is a no-op.
pub fn uninstall(doc: &Value) -> Result<Value> {
    let mut doc = doc.clone();
    let root = root_object(&mut doc)?;
    let Some(hooks_value) = root.get_mut("hooks") else {
        return Ok(doc);
    };
    let hooks = hooks_value
        .as_object_mut()
        .context("settings.json: `hooks` is not an object")?;
    let mut removed_any = false;
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let stripped = strip_ours(event_groups(&event, &hooks[&event])?);
        if stripped.first_ours.is_none() {
            continue;
        }
        removed_any = true;
        if stripped.groups.is_empty() {
            hooks.shift_remove(&event);
        } else {
            hooks.insert(event, Value::Array(stripped.groups));
        }
    }
    if removed_any && hooks.is_empty() {
        root.shift_remove("hooks");
    }
    Ok(doc)
}

/// Serialise the way Claude Code does: `JSON.stringify(x, null, 2) + "\n"`.
/// serde_json's pretty printer uses the same indent and leaves non-ASCII as is.
pub fn to_claude_string(doc: &Value) -> String {
    let mut s = serde_json::to_string_pretty(doc).expect("a Value always serialises");
    s.push('\n');
    s
}

/// Where to keep a copy of `settings` before overwriting it. The UTC
/// timestamp keeps backups sortable; callers must not overwrite an existing one.
pub fn backup_path(settings: &Path, now_ms: i64) -> PathBuf {
    let t = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now_ms) * 1_000_000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let stamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    );
    let name = settings
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".into());
    settings.with_file_name(format!("{name}.cc-moss-{stamp}.bak"))
}

fn root_object(doc: &mut Value) -> Result<&mut Map<String, Value>> {
    doc.as_object_mut()
        .context("settings.json: top level is not an object")
}

fn event_groups(event: &str, v: &Value) -> Result<Vec<Value>> {
    match v.as_array() {
        Some(a) => Ok(a.clone()),
        None => bail!("settings.json: `hooks.{event}` is not an array"),
    }
}

struct Stripped {
    groups: Vec<Value>,
    /// Index in `groups` where the first cc-moss entry used to be, so a
    /// reinstall can put the new one back in the same place.
    first_ours: Option<usize>,
}

/// Drop cc-moss hook entries from an event's matcher groups. A group is
/// dropped only when cc-moss entries were all it held; user entries that
/// share a group with ours stay where they are.
fn strip_ours(groups: Vec<Value>) -> Stripped {
    let mut out = Vec::with_capacity(groups.len());
    let mut first_ours = None;
    for mut group in groups {
        let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            out.push(group);
            continue;
        };
        let before = inner.len();
        inner.retain(|h| !is_ours(h));
        if inner.len() == before {
            out.push(group);
            continue;
        }
        first_ours.get_or_insert(out.len());
        if !inner.is_empty() {
            out.push(group);
        }
    }
    Stripped {
        groups: out,
        first_ours,
    }
}
