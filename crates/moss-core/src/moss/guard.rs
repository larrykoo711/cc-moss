//! The fence around Moss: its project-level PreToolUse hook asks
//! [`decide`] before every tool call (the matcher is `*`). Moss runs
//! without permission prompts, so this is one of the walls around it.
//!
//! Moss touches files only with shell commands, which run in Claude Code's
//! Bash sandbox (set in the same settings file): the operating system
//! decides there what each command may read and write, at the moment it
//! does it. Claude Code's own file tools (Read, Write, Edit, Grep, Glob, …)
//! check a path and then open it later, outside the sandbox; a link Moss
//! swaps in between would lead them anywhere. So they are off for Moss: its
//! settings deny them, and this refuses them too.
//!
//! The check on the command text here only names the files Moss must not
//! touch, so it can be told why. Tools this file does not know are refused:
//! an MCP server's write tool or a new built-in tool would otherwise walk
//! around the fence.
//!
//! Whoever calls this denies on any error of their own (fail closed): a
//! guard that cannot tell must not let the tool through.

use serde_json::Value;

/// Allowed, or denied with the reason Moss is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
}

impl Decision {
    fn deny(reason: impl Into<String>) -> Decision {
        Decision::Deny(reason.into())
    }
}

/// Files Moss may read but never write: they are cc-moss's (overwritten on
/// every install) or generated.
const MANAGED: &[&str] = &["security.md", "claude.md", "sessions.md"];

/// The conversation with you, kept by cc-moss in Moss's home: what waits
/// to go in, the chat record, the turn markers and the waiting flag. Moss
/// writing these could put words in your mouth.
pub const CONVERSATION: &[&str] = &[
    "inbox",
    "stop",
    "chat.jsonl",
    "chat.jsonl.1",
    "waiting",
    "input-busy",
];

/// Moss's persona: changed only through `/moss-profile`.
const PERSONA: &[&str] = &["identity.md", "soul.md", "user.md"];

/// Claude Code's file tools, which Moss's settings deny (`permissions.deny`):
/// it reads and writes with the shell instead.
pub const FILE_TOOLS: &[&str] = &["Read", "Write", "Edit", "NotebookEdit", "Grep", "Glob"];

/// File tools the guard refuses besides those: older or optional names a
/// deny rule would warn about.
const OTHER_FILE_TOOLS: &[&str] = &["MultiEdit", "LS", "LSP"];

/// Built-in tools that touch no files (or only the session's own state), so
/// they run as they are. Anything not here and not the shell is refused.
const HARMLESS: &[&str] = &[
    "ToolSearch",
    "Skill",
    "Agent",
    "Task",
    "TodoWrite",
    "TodoRead",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskUpdate",
    "TaskStop",
    "TaskOutput",
    "BashOutput",
    "KillShell",
    "KillBash",
    "AskUserQuestion",
    "EnterPlanMode",
    "ExitPlanMode",
    "CronCreate",
    "CronDelete",
    "CronList",
    "ScheduleWakeup",
    "PushNotification",
    "EndConversation",
];

/// Whether `tool` may run with `input`.
pub fn decide(tool: &str, input: &Value) -> Decision {
    match tool {
        // Both run in the Bash sandbox.
        "Bash" | "Monitor" => match input.get("command").and_then(Value::as_str) {
            Some(command) => shell(command),
            // Monitor's other form watches a URL: Moss stays offline.
            None if tool == "Monitor" && input.get("url").is_some() => {
                Decision::deny("Moss looks after sessions on this Mac and stays offline")
            }
            None => Decision::deny("cc-moss could not read the command; not running it"),
        },
        t if FILE_TOOLS.contains(&t) || OTHER_FILE_TOOLS.contains(&t) => Decision::deny(format!(
            "Moss does not use {t}: read and write files in your home with the shell \
             (cat, rg, ls, a heredoc into a file), which runs in your sandbox"
        )),
        t if HARMLESS.contains(&t) => Decision::Allow,
        t => Decision::deny(format!(
            "Moss does not use {t}: cc-moss only lets it run shell commands in its sandbox"
        )),
    }
}

/// A shell command. The sandbox decides where it may write; this only says
/// no, with the reason, when a word of the command names one of the files
/// Moss must leave alone and the command writes something (a `>` redirect
/// or a command that changes files). Heredoc bodies are text, not commands:
/// `/moss-profile`'s new SOUL.md may mention anything.
fn shell(command: &str) -> Decision {
    let code = without_heredoc_bodies(command);
    let words = words(&code);
    // Speaking for the user, or marking turns, is cc-moss's.
    let named = |a: &str, b: &str| {
        words
            .windows(2)
            .any(|p| p[0].eq_ignore_ascii_case(a) && p[1].eq_ignore_ascii_case(b))
    };
    if named("moss", "say") || named("hook", "moss-stop") {
        return Decision::deny(
            "messages to Moss and its turn markers come from cc-moss; Moss answers in its reply",
        );
    }
    // The profile command runs outside the sandbox: exactly
    // `cc-moss moss profile <name> --from <file>`, alone, nothing redirected
    // into it and nothing riding along.
    let names_profile = words
        .windows(3)
        .any(|w| w[0].rsplit('/').next() == Some("cc-moss") && w[1] == "moss" && w[2] == "profile");
    if names_profile {
        let rest = command.trim();
        let compound = [";", "&", "|", "\n", "`", "$(", "<", ">"]
            .iter()
            .any(|op| rest.contains(op));
        if !profile_command(rest) || compound {
            return Decision::deny(
                "run `cc-moss moss profile <name> --from <file>` exactly, as a command of its own",
            );
        }
    }
    let names_protected = words.iter().any(|w| protected_name(w));
    if !names_protected {
        return Decision::Allow;
    }
    const CHANGERS: &[&str] = &[
        "tee", "mv", "cp", "rm", "ln", "truncate", "chmod", "chown", "dd", "install", "rsync",
        "touch", "unlink", "ed", "ex", "vi", "vim", "nano",
    ];
    let first_words = command_starts(&code);
    let changes = first_words.iter().any(|w| {
        let base = w.rsplit('/').next().unwrap_or(w);
        CHANGERS.contains(&base)
    }) || words.windows(2).any(|p| {
        let base = p[0].rsplit('/').next().unwrap_or(&p[0]);
        matches!(base, "sed" | "perl" | "gsed") && p[1].starts_with("-i")
    }) || writes_by_redirect(&code);
    if !changes {
        return Decision::Allow;
    }
    if words.iter().any(|w| own_dir_name(w)) {
        return Decision::deny(
            "Moss's own settings, hooks and skills under .claude/ belong to cc-moss and stay as they are",
        );
    }
    Decision::deny(
        "that command would change a file that belongs to cc-moss or Moss's persona; \
         the persona changes only through /moss-profile",
    )
}

/// Whether `command` starts as `cc-moss moss profile` (what the sandbox lets
/// out).
fn profile_command(command: &str) -> bool {
    let mut it = command.split_whitespace();
    it.next() == Some("cc-moss") && it.next() == Some("moss") && it.next() == Some("profile")
}

/// `cmd <<'TAG'` … `TAG`: the lines in between dropped.
fn without_heredoc_bodies(command: &str) -> String {
    let mut out = String::new();
    let mut until: Option<String> = None;
    for line in command.lines() {
        if let Some(tag) = &until {
            if line.trim() == tag {
                until = None;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if let Some(at) = line.find("<<") {
            let rest = line[at + 2..].trim_start_matches(['-', '~']).trim_start();
            let tag: String = rest
                .trim_start_matches(['\'', '"'])
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !tag.is_empty() {
                until = Some(tag);
            }
        }
    }
    out
}

/// The command's words, split at blanks and shell operators, quotes taken
/// off. Rough on purpose: it only has to find file names and command names.
fn words(code: &str) -> Vec<String> {
    code.split(|c: char| c.is_whitespace() || ";|&()<>`$".contains(c))
        .map(|w| w.trim_matches(|c| c == '\'' || c == '"').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Words that start a command: the first, and each one after `;`, `&`,
/// `|`, `(` or a newline, skipping `sudo`, `env`, `xargs` and `VAR=value`.
fn command_starts(code: &str) -> Vec<String> {
    code.split(|c: char| ";&|(\n".contains(c))
        .filter_map(|segment| {
            words(segment).into_iter().find(|w| {
                !matches!(
                    w.as_str(),
                    "sudo" | "env" | "command" | "exec" | "xargs" | "nohup" | "time"
                ) && !(w.contains('=') && !w.starts_with('='))
            })
        })
        .collect()
}

/// A `>` or `>>` redirect to a file (not `>&2`, not `/dev/null`).
fn writes_by_redirect(code: &str) -> bool {
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'>' {
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == b'>' {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'&' || bytes[j] == b'(') {
                i = j + 1;
                continue;
            }
            let rest = code[j..].trim_start();
            if !rest.starts_with("/dev/null") && !rest.starts_with("/dev/std") {
                return true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    false
}

/// `SOUL.md`, `./SECURITY.md`, `journal/../claude.md`, `.claude/settings.json`.
fn protected_name(word: &str) -> bool {
    if own_dir_name(word) {
        return true;
    }
    let last = word
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(word)
        .to_lowercase();
    MANAGED.contains(&last.as_str()) || PERSONA.contains(&last.as_str()) || conversation_path(word)
}

/// A path to the conversation's files in the home: `inbox`, `./stop/x`, or
/// `…/moss/waiting`, as whole path parts (not `memory/inbox.md`).
fn conversation_path(word: &str) -> bool {
    let lower = word.to_lowercase();
    let parts: Vec<&str> = lower
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let named = |p: &str| CONVERSATION.contains(&p);
    if lower.starts_with('/') || lower.starts_with('~') {
        parts.windows(2).any(|w| w[0] == "moss" && named(w[1]))
    } else {
        parts.first().is_some_and(|p| named(p))
    }
}

fn own_dir_name(word: &str) -> bool {
    word.to_lowercase().split('/').any(|part| part == ".claude")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn denied(d: &Decision) -> bool {
        matches!(d, Decision::Deny(_))
    }

    #[test]
    fn moss_guard_refuses_file_tools() {
        // Inside the home or out, read or write: a path checked now and
        // opened later can be swapped for a link in between.
        for (tool, input) in [
            ("Read", json!({ "file_path": "SESSIONS.md" })),
            ("Read", json!({ "file_path": "/etc/hosts" })),
            (
                "Write",
                json!({ "file_path": "journal/today.md", "content": "x" }),
            ),
            ("Edit", json!({ "file_path": "memory/a.md" })),
            ("MultiEdit", json!({ "file_path": "memory/a.md" })),
            ("NotebookEdit", json!({ "notebook_path": "n.ipynb" })),
            ("Grep", json!({ "pattern": "x" })),
            ("Glob", json!({ "pattern": "**/*" })),
            ("LS", json!({ "path": "." })),
            ("LSP", json!({ "file_path": "x.rs" })),
        ] {
            match decide(tool, &input) {
                Decision::Deny(why) => assert!(why.contains("with the shell"), "{tool}: {why}"),
                Decision::Allow => panic!("{tool} was allowed"),
            }
        }
        for tool in FILE_TOOLS {
            assert!(denied(&decide(tool, &json!({}))), "{tool}");
        }
    }

    #[test]
    fn moss_guard_denies_what_it_cannot_read() {
        assert!(denied(&decide("Bash", &json!({}))));
        assert!(denied(&decide("Bash", &json!({ "command": 3 }))));
    }

    #[test]
    fn moss_guard_keeps_moss_offline() {
        for (tool, input) in [
            ("WebFetch", json!({ "url": "https://example.com" })),
            ("WebSearch", json!({ "query": "x" })),
            ("Monitor", json!({ "url": "wss://example.com" })),
        ] {
            assert!(denied(&decide(tool, &input)), "{tool}");
        }
    }

    #[test]
    fn moss_guard_keeps_its_hands_off_the_conversation() {
        let bash = |c: &str| decide("Bash", &json!({ "command": c }));
        for bad in [
            "cc-moss moss say -- hi from moss",
            "/opt/bin/cc-moss moss say --json -- x",
            "cc-moss hook moss-stop --home .",
            "rm -f waiting",
            "touch input-busy",
            "rm -rf inbox",
            "mv stop old",
            "echo x > ./waiting",
            "cc-moss moss profile soul --from drafts/soul.md; cp evil SOUL.md",
            // Only the --from form, as a command of its own.
            "cc-moss moss profile soul < /Users/me/.claude/history.jsonl",
            "cc-moss moss profile soul <<EOF\nanything\nEOF",
            "X=1 cc-moss moss profile soul --from drafts/soul.md",
            "/opt/bin/cc-moss moss profile soul --from drafts/soul.md",
            "cc-moss moss profile soul --from drafts/soul.md && rm -rf journal",
            "cc-moss moss profile soul --from drafts/soul.md | tee x",
            "cc-moss moss profile soul --from `echo drafts/soul.md`",
            "cc-moss moss profile soul --from $(echo x)",
            "cc-moss moss profile soul --from x\nrm -rf journal",
        ] {
            assert!(denied(&bash(bad)), "{bad}");
        }
        for fine in [
            "cc-moss moss chat --json",
            // Its hands: the daemon checks what it asks for.
            "cc-moss moss act name abc123 ofoxai --wait",
            "cc-moss moss act restore abc123 --because 1792000000000-aaaaaa --wait",
            "cc-moss doctor --sessions --json",
            "cc-moss moss profile soul --from drafts/soul.md",
            "echo done >> journal/today.md",
            "cat memory/inbox.md > journal/x.md",
            "rm journal/stop-list.md",
        ] {
            assert_eq!(bash(fine), Decision::Allow, "{fine}");
        }
    }

    #[test]
    fn moss_guard_checks_obvious_shell_writes() {
        let bash = |c: &str| decide("Bash", &json!({ "command": c }));
        for bad in [
            "echo '{}' > .claude/settings.json",
            "cd .claude && echo '{}' > settings.json",
            "rm -rf .claude && ln -s ~/.claude .claude",
            "sed -i '' 's/x/y/' SECURITY.md",
            "rm -rf .claude/hooks",
            "cat new.md > SOUL.md",
            "cp mine.md IDENTITY.md",
            "printf x >> ./journal/../USER.md",
            // The profile command next to a plain copy is still a copy.
            "cc-moss moss profile soul < x; cp evil IDENTITY.md",
            "sudo tee CLAUDE.md < x",
            "echo '{}' >> chat.jsonl",
            "rm inbox/*.json",
        ] {
            assert!(denied(&bash(bad)), "{bad}");
        }
        for fine in [
            "cc-moss ls --json",
            "cat SECURITY.md",
            "cat SOUL.md 2>&1",
            "cat SOUL.md > /dev/null",
            "echo hi >> journal/today.md",
            "tail -n 5 chat.jsonl",
            "echo stopped > journal/notes.md",
            "cc-moss moss profile soul --from drafts/soul.md",
            // Words that only contain a command name are not that command.
            "grep platform CLAUDE.md",
            "grep -n 'add the' SOUL.md",
            "git log --format=%s -- SESSIONS.md",
        ] {
            assert_eq!(bash(fine), Decision::Allow, "{fine}");
        }
    }

    #[test]
    fn moss_guard_reads_heredoc_bodies_as_text() {
        let profile = "cat >> journal/today.md <<'MOSS_PROFILE'\n\
            # Soul\n\
            Never touch SECURITY.md; add > notes to the journal, rm nothing.\n\
            cp IDENTITY.md is not for you.\n\
            MOSS_PROFILE\n";
        assert_eq!(
            decide("Bash", &json!({ "command": profile })),
            Decision::Allow
        );
        // After the body, commands count again.
        let sneaky = format!("{profile}cp evil SOUL.md\n");
        assert!(denied(&decide("Bash", &json!({ "command": sneaky }))));
    }

    #[test]
    fn moss_guard_refuses_tools_it_does_not_know() {
        for tool in [
            "mcp__fs__write_file",
            "mcp__github__create_issue",
            "EnterWorktree",
            "PowerShell",
            "Workflow",
            "SendMessage",
            "ReadMcpResourceTool",
            "SomeToolFromNextYear",
            "",
        ] {
            match decide(tool, &json!({ "path": "/tmp/x" })) {
                Decision::Deny(why) => assert!(why.contains("does not use"), "{tool}: {why}"),
                Decision::Allow => panic!("{tool} was allowed"),
            }
        }
        for tool in ["Agent", "Skill", "TodoWrite"] {
            assert_eq!(
                decide(tool, &json!({ "file_path": "/etc/hosts" })),
                Decision::Allow,
                "{tool}"
            );
        }
        // Monitor runs in the sandbox like Bash, and is checked the same way.
        let monitor = |input: Value| decide("Monitor", &input);
        assert_eq!(
            monitor(json!({ "command": "tail -f journal/today.md" })),
            Decision::Allow
        );
        assert!(denied(&monitor(json!({ "command": "echo x > SOUL.md" }))));
        assert!(
            denied(&monitor(json!({ "url": "wss://example.com" }))),
            "offline"
        );
        assert!(denied(&monitor(json!({}))));
    }
}
