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
