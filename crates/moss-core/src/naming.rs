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
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_control() && !c.is_whitespace() {
            continue;
        }
        if hidden_at(&chars, i) {
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

/// Whether `name` holds characters that change how text shows without
/// showing themselves: Unicode format characters (category Cf: direction
/// overrides such as U+202E, zero-width spaces and joiners, soft hyphens,
/// tags) and line or paragraph separators (Zl, Zp). A zero-width joiner
/// between two emoji (👩‍💻) is part of the emoji and stays.
pub fn has_hidden_chars(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    (0..chars.len()).any(|i| hidden_at(&chars, i))
}

fn hidden_at(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    if c == '\u{200D}' {
        let before = i.checked_sub(1).map(|j| chars[j]);
        let after = chars.get(i + 1).copied();
        return !(before.is_some_and(emoji_part) && after.is_some_and(emoji_part));
    }
    format_or_separator(c)
}

/// Emoji (skin tones among them), and the variation selector that ends one.
fn emoji_part(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE0F)
}

/// Unicode categories Cf, Zl and Zp.
fn format_or_separator(c: char) -> bool {
    matches!(c as u32,
        0x00AD
        | 0x0600..=0x0605
        | 0x061C
        | 0x06DD
        | 0x070F
        | 0x0890..=0x0891
        | 0x08E2
        | 0x180E
        | 0x200B..=0x200F
        | 0x2028..=0x202E
        | 0x2060..=0x2064
        | 0x2066..=0x206F
        | 0xFEFF
        | 0xFFF9..=0xFFFB
        | 0x110BD
        | 0x110CD
        | 0x13430..=0x1343F
        | 0x1BCA0..=0x1BCA3
        | 0x1D173..=0x1D17A
        | 0xE0001
        | 0xE0020..=0xE007F)
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

/// The longest standard session name, in characters.
pub const STANDARD_CHARS: usize = 24;

/// Whether `name` is a session id or its first letters rather than a name:
/// eight or more hex digits (with a digit among them) before any `-`, and
/// nothing but hex digits and `-`.
pub fn looks_like_id(name: &str) -> bool {
    let head = name.split('-').next().unwrap_or("");
    head.len() >= 8
        && head.chars().any(|c| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// `name` cut to [`STANDARD_CHARS`] characters, at a `-` when there is one
/// to cut at, with no `-` left at the end.
fn cut(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= STANDARD_CHARS {
        return name.to_string();
    }
    let mut kept: String = chars[..STANDARD_CHARS].iter().collect();
    // Cut mid-word: back to the last `-`, if any.
    if chars[STANDARD_CHARS] != '-' {
        if let Some(at) = kept.rfind('-').filter(|at| *at > 0) {
            kept.truncate(at);
        }
    }
    kept.trim_end_matches('-').to_string()
}

/// `None` when `name` is a standard session name: as tmux takes it
/// ([`sanitize`] keeps it as it is), at most [`STANDARD_CHARS`] characters.
/// Else the standard name. An id is no name to correct: it gets a name from
/// its folder ([`short_name`]), so `None` here too.
pub fn standard_name(name: &str) -> Option<String> {
    if looks_like_id(name) {
        return None;
    }
    let standard = cut(&sanitize(name));
    (standard != name).then_some(standard)
}

/// A short name for a session from its folder: the folder's own name (a
/// worktree's too), made standard. Never the session id. The caller makes
/// it unique ([`unique`]).
pub fn short_name(folder: &str) -> String {
    let last = folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    cut(&sanitize(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_names_stay() {
        for name in ["api", "接口评审", "API-Review", "修复-12", "emoji🚀"] {
            assert_eq!(standard_name(name), None, "{name}");
        }
    }

    #[test]
    fn standard_name_corrects_what_tmux_cannot_take() {
        assert_eq!(standard_name("接口 评审").as_deref(), Some("接口-评审"));
        assert_eq!(
            standard_name("API Review v1.2").as_deref(),
            Some("API-Review-v1-2")
        );
        assert_eq!(standard_name(" fix: login ").as_deref(), Some("fix-login"));
    }

    #[test]
    fn standard_name_cuts_long_names_at_a_dash() {
        let long = "the-new-billing-page-for-enterprise-customers";
        let fixed = standard_name(long).unwrap();
        assert_eq!(fixed, "the-new-billing-page-for");
        assert!(fixed.chars().count() <= STANDARD_CHARS);
        // No dash to cut at: cut at 24 characters, never mid-character.
        let han = "一二三四五六七八九十一二三四五六七八九十一二三四五六";
        let fixed = standard_name(han).unwrap();
        assert_eq!(fixed.chars().count(), STANDARD_CHARS);
        assert!(han.starts_with(&fixed));
        // Exactly 24 is fine.
        assert_eq!(standard_name(&"a".repeat(24)), None);
    }

    #[test]
    fn ids_are_not_names() {
        assert!(looks_like_id("a1b2c3d4"));
        assert!(looks_like_id("a1b2c3d4-0000-4000-8000-000000000001"));
        assert!(!looks_like_id("api"));
        assert!(!looks_like_id("deadbeef-notes"));
        assert!(!looks_like_id("cafe"));
        // An id is not "corrected": it gets a name from its folder instead.
        assert_eq!(standard_name("a1b2c3d4-0000-4000-8000-000000000001"), None);
    }

    #[test]
    fn short_names_come_from_the_folder() {
        assert_eq!(short_name("/Users/me/work/ofoxai"), "ofoxai");
        assert_eq!(short_name("/Users/me/work/ofoxai/"), "ofoxai");
        assert_eq!(
            short_name("/r/cc-moss/.claude/worktrees/97-t7.1"),
            "97-t7-1"
        );
        assert_eq!(short_name("/Users/me/项目 资料"), "项目-资料");
        assert_eq!(short_name("/"), "session");
        assert!(
            short_name("/w/an-extremely-long-folder-name-for-a-project")
                .chars()
                .count()
                <= STANDARD_CHARS
        );
    }
}
