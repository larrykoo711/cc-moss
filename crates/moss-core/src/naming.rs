//! tmux session names derived from Claude Code session names (DESIGN §6).

use std::process::Command;
use std::time::Duration;

use crate::command::output_with_timeout;

/// The longest tmux name part, in characters.
pub const MAX_CHARS: usize = 40;

/// A session name as a tmux name: what people name things with stays (any
/// script, case, emoji); only what tmux or a shell can't take becomes `-`:
/// white space, `.` and `:` (tmux's target separators), and characters a
/// shell would read (`'"$\`!#;&|<>(){}[]*?~^,=/`). Control characters go;
/// so do a leading `-` and the `$` `@` `%` that tmux reads as ids. Runs of
/// `-` collapse, at most [`MAX_CHARS`] characters. Never empty.
/// (notes/session-names.md, 2026-10-19: Chinese names used to vanish.)
pub fn sanitize(name: &str) -> String {
    const SHELL: &str = ".:'\"$`\\!#;&|<>(){}[]*?~^,=/";
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_control() && !c.is_whitespace() {
            continue;
        }
        let c = if c.is_whitespace() || SHELL.contains(c) {
            '-'
        } else {
            c
        };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        if out.is_empty() && matches!(c, '@' | '%') {
            continue;
        }
        out.push(c);
    }
    let cut: String = out.chars().take(MAX_CHARS).collect();
    let trimmed = cut.trim_end_matches('-');
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `<prefix>_<name>`, or just the name without a prefix. The machine's
/// prefix stays lowercase, as it always was.
pub fn session_name(prefix: Option<&str>, name: &str) -> String {
    match prefix {
        Some(p) => format!("{}_{}", sanitize(p).to_lowercase(), sanitize(name)),
        None => sanitize(name),
    }
}

/// `desired`, or `desired-2`, `desired-3`… if another session already has it.
/// `current` is the session being renamed; its own name is not a conflict.
pub fn unique(desired: &str, taken: &[String], current: Option<&str>) -> String {
    let is_taken = |n: &str| current != Some(n) && taken.iter().any(|t| t == n);
    if !is_taken(desired) {
        return desired.to_string();
    }
    (2..)
        .map(|i| format!("{desired}-{i}"))
        .find(|n| !is_taken(n))
        .unwrap_or_else(|| desired.to_string())
}

/// The machine's short name: `$CC_MOSS_HOST_PREFIX` if set, else
/// `LocalHostName` on macOS (`hostname -s` often returns a generic name
/// there), else the kernel's host name on Linux, else `hostname -s`.
pub fn host_prefix() -> Option<String> {
    if let Some(p) = std::env::var("CC_MOSS_HOST_PREFIX")
        .ok()
        .filter(|v| !v.is_empty())
    {
        return Some(p);
    }
    let run = |cmd: &mut Command| {
        output_with_timeout(cmd, Duration::from_millis(500))
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    if cfg!(target_os = "macos") {
        if let Some(name) = run(Command::new("scutil").args(["--get", "LocalHostName"])) {
            return Some(name);
        }
    }
    if cfg!(target_os = "linux") {
        if let Ok(name) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
            let short = name.trim().split('.').next().unwrap_or_default();
            if !short.is_empty() {
                return Some(short.to_string());
            }
        }
    }
    run(Command::new("hostname").arg("-s"))
}

/// [`host_prefix`], cached in `state_dir` for a day: the hook runs it on
/// every prompt and `scutil` costs a process spawn.
pub fn cached_host_prefix(state_dir: &std::path::Path) -> Option<String> {
    if let Some(p) = std::env::var("CC_MOSS_HOST_PREFIX")
        .ok()
        .filter(|v| !v.is_empty())
    {
        return Some(p);
    }
    let path = state_dir.join("host_prefix");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(86_400));
    if fresh {
        if let Ok(cached) = std::fs::read_to_string(&path) {
            // Empty means "no name found": also cached, so a machine without
            // `hostname` does not spawn it on every prompt.
            let cached = cached.trim();
            return (!cached.is_empty()).then(|| cached.to_string());
        }
    }
    let prefix = host_prefix();
    if crate::store_dir::ensure_private_dir(state_dir).is_ok() {
        let _ = std::fs::write(&path, prefix.as_deref().unwrap_or(""));
    }
    prefix
}
