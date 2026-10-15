//! tmux session names derived from Claude Code session names (DESIGN §6).

use std::process::Command;
use std::time::Duration;

use crate::command::output_with_timeout;

/// Lowercase; anything outside `[a-z0-9_-]` becomes `-` (tmux rewrites `.`
/// and `:` in session names itself, which would break name matching); runs of
/// `-` collapse; leading and trailing `-` are dropped. Never empty.
pub fn sanitize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars().flat_map(char::to_lowercase) {
        let c = if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            c
        } else {
            '-'
        };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `<prefix>_<name>`, both sanitized, or just the name without a prefix.
pub fn session_name(prefix: Option<&str>, name: &str) -> String {
    match prefix {
        Some(p) => format!("{}_{}", sanitize(p), sanitize(name)),
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
/// there), else `hostname -s`.
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
            let cached = cached.trim();
            if !cached.is_empty() {
                return Some(cached.to_string());
            }
        }
    }
    let prefix = host_prefix()?;
    let _ = std::fs::create_dir_all(state_dir);
    let _ = std::fs::write(&path, &prefix);
    Some(prefix)
}
