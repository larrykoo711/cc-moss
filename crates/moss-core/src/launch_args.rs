//! The subset of a Claude Code command line worth replaying on restore.

use std::process::Command;
use std::time::Duration;

use crate::command::output_with_timeout;

/// Flags that take a value and are kept.
const WITH_VALUE: [&str; 5] = [
    "--permission-mode",
    "--model",
    "--effort",
    "--add-dir",
    "--agent",
];
/// Boolean flags that are kept.
const BOOLEAN: [&str; 1] = ["--dangerously-skip-permissions"];
/// Substrings that make a value look like a credential.
const SECRET_HINTS: [&str; 4] = ["sk-", "key", "token", "secret"];

/// Keep only whitelisted flags from `argv` (program name first). Everything
/// else, including session-specific flags and the prompt, is dropped.
pub fn whitelist(argv: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i];
        i += 1;
        if BOOLEAN.contains(&arg) {
            out.push(arg.to_string());
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (arg, None),
        };
        if !WITH_VALUE.contains(&flag) {
            continue;
        }
        let value = match inline {
            Some(v) => v,
            None if i < argv.len() && !argv[i].starts_with('-') => {
                i += 1;
                argv[i - 1]
            }
            None => continue,
        };
        let lower = value.to_ascii_lowercase();
        if SECRET_HINTS.iter().any(|h| lower.contains(h)) {
            continue;
        }
        out.push(flag.to_string());
        out.push(value.to_string());
    }
    out
}

/// Whether `args` already choose a permission mode (`--permission-mode …`
/// or `--dangerously-skip-permissions`), so `cc-moss new` adds none of its own.
pub fn sets_permissions(args: &[String]) -> bool {
    args.iter().any(|a| {
        let flag = a.split_once('=').map_or(a.as_str(), |(f, _)| f);
        BOOLEAN.contains(&flag) || flag == WITH_VALUE[0]
    })
}

/// `args` without the flags that pick a permission mode: `--dangerously-skip-permissions`,
/// `--permission-mode <value>` and `--permission-mode=<value>`. For `cc-moss new --safe`.
pub fn without_permissions(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if BOOLEAN.contains(&arg.as_str()) || arg.starts_with("--permission-mode=") {
            continue;
        }
        if arg == WITH_VALUE[0] {
            if i < args.len() && !args[i].starts_with('-') {
                i += 1;
            }
            continue;
        }
        out.push(arg.clone());
    }
    out
}

/// `cc-moss new`'s own flags; everything else that looks like a flag is
/// Claude Code's.
const NEW_OWN_WITH_VALUE: [&str; 2] = ["-C", "--dir"];
const NEW_OWN_BOOLEAN: [&str; 5] = ["--detach", "--yes", "--safe", "-h", "--help"];

/// Let `cc-moss new` take Claude Code's flags directly, anywhere on the line:
/// `new --dangerously-skip-permissions login-page` becomes
/// `new login-page -- --dangerously-skip-permissions`. `args` are the words
/// after `new`. Flags Claude Code takes a value for (`--model opus`) keep it;
/// anything after an explicit `--` follows the moved flags unchanged.
pub fn normalize_new(args: &[String]) -> Vec<String> {
    let mut own = Vec::new();
    let mut claude = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if arg == "--" {
            claude.extend(args[i..].iter().cloned());
            break;
        }
        let flag = arg.split_once('=').map_or(arg.as_str(), |(f, _)| f);
        if NEW_OWN_BOOLEAN.contains(&arg.as_str()) {
            own.push(arg.clone());
        } else if NEW_OWN_WITH_VALUE.contains(&flag) {
            own.push(arg.clone());
            if !arg.contains('=') && i < args.len() {
                own.push(args[i].clone());
                i += 1;
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            claude.push(arg.clone());
            if WITH_VALUE.contains(&flag) && !arg.contains('=') && i < args.len() {
                claude.push(args[i].clone());
                i += 1;
            }
        } else {
            own.push(arg.clone());
        }
    }
    if !claude.is_empty() {
        own.push("--".into());
        own.extend(claude);
    }
    own
}

/// Whitelisted arguments of a running process, from `ps`. Arguments that
/// contain spaces cannot be recovered exactly (see the T1.3 spec).
pub fn capture(pid: u32) -> Vec<String> {
    let Some(out) = output_with_timeout(
        Command::new("ps")
            .args(["-o", "args=", "-p", &pid.to_string()])
            .env("LC_ALL", "C"),
        Duration::from_millis(500),
    ) else {
        return Vec::new();
    };
    let line = String::from_utf8_lossy(&out.stdout);
    whitelist(&line.split_whitespace().collect::<Vec<_>>())
}
