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
        HookKind::Notification => "Notification",
        HookKind::PostToolUse => "PostToolUse",
    }
}

/// The shell command Claude Code runs for `kind`. Claude Code hands the
/// command to a shell, so any path beyond plain characters is single-quoted.
///
/// The `[ -x … ]` guard keeps a deleted binary (an uninstalled or upgraded
/// package) from turning into a hook error on every prompt: the command then
/// does nothing and exits 0.
pub fn hook_command(exe: &str, kind: HookKind) -> String {
    let exe = shell_quote(exe);
    format!("[ -x {exe} ] && {exe} hook {} || true", kind.cli_name())
}

fn shell_quote(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+@%,:=".contains(c));
    if plain {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Undo shell quoting of a single word. `None` when the text is not one
/// plain word: unquoted blanks or shell operators mean it is a longer command.
fn shell_unquote(word: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => loop {
                match chars.next()? {
                    '\'' => break,
                    c => out.push(c),
                }
            },
            '"' => loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => out.push(chars.next()?),
                    '$' | '`' => return None,
                    c => out.push(c),
                }
            },
            '\\' => out.push(chars.next()?),
            c if c.is_whitespace() || ";&|<>()$`*?[]#~{}!".contains(c) => return None,
            c => out.push(c),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The cc-moss binary a hook command runs, if the command is one cc-moss
/// wrote. Two shapes are recognised: the guarded one [`hook_command`] writes
/// today and the bare `<path> hook <event>` of earlier versions, so `setup`
/// can upgrade and `setup --uninstall` can remove either. The path must end in
/// `cc-moss`, not the exact current location, so a reinstall from elsewhere
/// replaces the old entry instead of adding one.
fn parse_command(command: &str) -> Option<(String, HookKind)> {
    let guarded = command
        .strip_prefix("[ -x ")
        .and_then(|rest| rest.strip_suffix(" || true"));
    HookKind::ALL.into_iter().find_map(|kind| {
        let tail = format!(" hook {}", kind.cli_name());
        let word = match guarded {
            // `<P> ] && <P>`: the tested path and the run path must agree.
            Some(rest) => {
                let both = rest.strip_suffix(&tail)?;
                let n = both.len().checked_sub(6)? / 2;
                let (tested, run) = (both.get(..n)?, both.get(n + 6..)?);
                if both.get(n..n + 6)? != " ] && " || tested != run {
                    return None;
                }
                run
            }
            None => command.strip_suffix(&tail)?,
        };
        let path = shell_unquote(word)?;
        (path == "cc-moss" || path.ends_with("/cc-moss")).then_some((path, kind))
    })
}

/// Whether one hook entry (`{"type": "command", "command": ...}`) was written
/// by cc-moss, in either the current or the earlier command shape.
pub fn is_ours(hook: &Value) -> bool {
    hook_binary(hook).is_some()
}

fn hook_binary(hook: &Value) -> Option<String> {
    parse_command(hook.get("command")?.as_str()?).map(|(path, _)| path)
}

/// The hooks cc-moss registers that `doc` has no cc-moss entry for, in
/// [`HookKind::ALL`] order. An install from an older version lacks the
/// newer ones (Notification and PostToolUse came after 0.1.1).
pub fn missing_hooks(doc: &Value) -> Vec<HookKind> {
    HookKind::ALL
        .into_iter()
        .filter(|&kind| {
            let present = doc
                .get("hooks")
                .and_then(|h| h.get(event_name(kind)))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                .flatten()
                .filter_map(|h| parse_command(h.get("command")?.as_str()?))
                .any(|(_, k)| k == kind);
            !present
        })
        .collect()
}

/// The binary path of every cc-moss hook entry in `doc`, in file order, so
/// `doctor` can tell when one of them no longer exists.
pub fn hook_binaries(doc: &Value) -> Vec<String> {
    let Some(hooks) = doc.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    hooks
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter_map(hook_binary)
        .collect()
}

/// The matcher cc-moss writes for `kind`, for events that take one. PostToolUse
/// runs after every tool; the other events are matched in code (Notification)
/// or have no matcher at all.
fn matcher(kind: HookKind) -> Option<&'static str> {
    (kind == HookKind::PostToolUse).then_some("*")
}

/// Add the cc-moss hooks, replacing any earlier cc-moss entries in place.
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
        let entry = json!([{
            "type": "command",
            "command": hook_command(exe, kind),
            "timeout": HOOK_TIMEOUT_SECS,
        }]);
        let ours = match matcher(kind) {
            Some(m) => json!({ "matcher": m, "hooks": entry }),
            None => json!({ "hooks": entry }),
        };
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
