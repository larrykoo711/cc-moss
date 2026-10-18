//! The fence around Moss: its project-level PreToolUse hook asks
//! [`decide`] before every tool call (the matcher is `*`). Moss runs
//! without permission prompts, so this is what keeps its file tools to its
//! own home: nothing outside it, not its own settings or hooks, not
//! SECURITY.md, and its persona files only through `/moss-profile`.
//!
//! Shell commands are fenced by Claude Code's Bash sandbox (set in the same
//! settings file), which the operating system enforces; the check on the
//! command text here only names the files Moss must not touch, so it can be
//! told why. Tools this file does not know are refused: an MCP server's
//! write tool or a new built-in tool would otherwise walk around the fence.
//!
//! Whoever calls this denies on any error of their own (fail closed): a
//! guard that cannot tell must not let the write through.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

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

/// Moss's persona: changed only through `/moss-profile`.
const PERSONA: &[&str] = &["identity.md", "soul.md", "user.md"];

/// Built-in tools that change nothing on disk (or only the session's own
/// state), so they run as they are. Anything not here and not a file or
/// shell tool is refused.
const HARMLESS: &[&str] = &[
    "Read",
    "Glob",
    "Grep",
    "LS",
    "LSP",
    "WebFetch",
    "WebSearch",
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

/// Whether `tool` may run with `input`. `home` is Moss's home and `cwd` the
/// session's directory (relative paths are taken from there).
pub fn decide(home: &Path, cwd: &Path, tool: &str, input: &Value) -> Decision {
    let key = match tool {
        "Write" | "Edit" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        // Both run in the Bash sandbox.
        "Bash" | "Monitor" => {
            return match input.get("command").and_then(Value::as_str) {
                Some(command) => shell(command),
                // Monitor's other form watches a WebSocket: nothing written.
                None if tool == "Monitor" && input.get("url").is_some() => Decision::Allow,
                None => Decision::deny("cc-moss could not read the command; not running it"),
            };
        }
        t if HARMLESS.contains(&t) => return Decision::Allow,
        t => {
            return Decision::deny(format!(
                "Moss does not use {t}: cc-moss only lets it read, search, write inside its home \
                 and run shell commands in its sandbox"
            ))
        }
    };
    let Some(raw) = input
        .get(key)
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
    else {
        return Decision::deny(format!("cc-moss could not read {key}; not writing"));
    };
    write_to(home, cwd, Path::new(raw))
}

/// A write to `path`.
fn write_to(home: &Path, cwd: &Path, path: &Path) -> Decision {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let (Some(home), Some(target)) = (resolve(home), resolve(&absolute)) else {
        return Decision::deny("cc-moss could not work out where that path leads; not writing");
    };
    let Ok(rel) = target.strip_prefix(&home) else {
        return Decision::deny(format!(
            "Moss only writes inside its own home ({}); {} is outside it",
            home.display(),
            target.display()
        ));
    };
    // macOS folders ignore case: SOUL.md and soul.md are one file.
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    match parts.as_slice() {
        [] => Decision::deny("that is Moss's home folder itself, not a file"),
        [first, ..] if first == ".claude" => Decision::deny(
            "Moss's own settings, hooks and skills under .claude/ belong to cc-moss and stay as they are",
        ),
        [only] if MANAGED.contains(&only.as_str()) => Decision::deny(format!(
            "{only} belongs to cc-moss (it is put back on every install); leave it as it is"
        )),
        [only] if PERSONA.contains(&only.as_str()) => Decision::deny(format!(
            "{only} is Moss's persona: change it only through /moss-profile"
        )),
        _ => Decision::Allow,
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
    MANAGED.contains(&last.as_str()) || PERSONA.contains(&last.as_str())
}

fn own_dir_name(word: &str) -> bool {
    word.to_lowercase().split('/').any(|part| part == ".claude")
}

/// `path` (absolute) with every symlink followed, also dangling ones and
/// ones in the middle, and `..` taken the way the system takes it (after the
/// link before it). Parts that do not exist yet are kept as written. `None`
/// for a relative path or a symlink loop.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut todo: VecDeque<OsString> = parts(path).into();
    let mut current = PathBuf::from("/");
    let mut hops = 0;
    while let Some(part) = todo.pop_front() {
        if part == ".." {
            current.pop();
            continue;
        }
        let next = current.join(&part);
        match std::fs::symlink_metadata(&next) {
            Ok(meta) if meta.file_type().is_symlink() => {
                hops += 1;
                if hops > 40 {
                    return None;
                }
                let target = std::fs::read_link(&next).ok()?;
                if target.is_absolute() {
                    current = PathBuf::from("/");
                }
                for p in parts(&target).into_iter().rev() {
                    todo.push_front(p);
                }
            }
            _ => current = next,
        }
    }
    Some(current)
}

/// Names and `..`, without `/` and `.`.
fn parts(path: &Path) -> Vec<OsString> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_os_string()),
            Component::ParentDir => Some("..".into()),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::symlink;

    struct Place {
        _dir: tempfile::TempDir,
        root: PathBuf,
        home: PathBuf,
    }

    fn place() -> Place {
        let dir = tempfile::tempdir().unwrap();
        // The temporary folder itself may sit behind a symlink (/var on macOS).
        let root = resolve(dir.path()).unwrap();
        let home = root.join("state/moss");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join("journal")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        Place {
            _dir: dir,
            root,
            home,
        }
    }

    fn write(p: &Place, path: &str) -> Decision {
        decide(&p.home, &p.home, "Write", &json!({ "file_path": path }))
    }

    fn denied(d: &Decision) -> bool {
        matches!(d, Decision::Deny(_))
    }

    #[test]
    fn moss_guard_rejects_outside_writes() {
        let p = place();
        let outside = p.root.join("elsewhere/x.md");
        assert!(denied(&write(&p, outside.to_str().unwrap())));
        assert!(denied(&write(&p, "/etc/hosts")));
        assert!(denied(&write(&p, "../outside.md")), "relative, up and out");
        assert!(denied(&write(&p, "journal/../../escape.md")));
        let sneaky = format!("{}/journal/../../../elsewhere/x.md", p.home.display());
        assert!(denied(&write(&p, &sneaky)));
        // A sibling whose name starts like the home is still outside it.
        let sibling = format!("{}-other/x.md", p.home.display());
        assert!(denied(&write(&p, &sibling)));
        assert!(denied(&decide(
            &p.home,
            &p.root,
            "Edit",
            &json!({ "file_path": "elsewhere/x.md" })
        )));
        assert!(denied(&decide(
            &p.home,
            &p.home,
            "NotebookEdit",
            &json!({ "notebook_path": "/tmp/n.ipynb" })
        )));
    }

    #[test]
    fn moss_guard_allows_its_own_files() {
        let p = place();
        assert_eq!(write(&p, "journal/2026-10-18.md"), Decision::Allow);
        assert_eq!(write(&p, "memory/people.md"), Decision::Allow);
        assert_eq!(write(&p, "notes.md"), Decision::Allow);
        let abs = p.home.join("journal/today.md");
        assert_eq!(write(&p, abs.to_str().unwrap()), Decision::Allow);
        assert_eq!(write(&p, "journal/./a/../b.md"), Decision::Allow);
        assert_eq!(
            decide(
                &p.home,
                &p.home,
                "Read",
                &json!({ "file_path": "/etc/hosts" })
            ),
            Decision::Allow,
            "reading is not guarded"
        );
    }

    #[test]
    fn moss_guard_rejects_own_settings() {
        let p = place();
        for path in [
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".claude/hooks/guard.sh",
            ".claude/skills/moss-profile/SKILL.md",
            ".CLAUDE/Settings.json",
            "SECURITY.md",
            "security.md",
            "CLAUDE.md",
            "SESSIONS.md",
            "journal/../SECURITY.md",
        ] {
            assert!(denied(&write(&p, path)), "{path}");
        }
        for persona in ["IDENTITY.md", "SOUL.md", "USER.md", "soul.md"] {
            match write(&p, persona) {
                Decision::Deny(why) => assert!(why.contains("/moss-profile"), "{why}"),
                Decision::Allow => panic!("{persona} was allowed"),
            }
        }
        // Files of the same name further down are Moss's own.
        assert_eq!(write(&p, "memory/SOUL.md"), Decision::Allow);
    }

    #[test]
    fn moss_guard_follows_symlinks() {
        let p = place();
        let out = p.root.join("elsewhere");
        // A folder link inside the home that leads out.
        symlink(&out, p.home.join("journal/out")).unwrap();
        assert!(denied(&write(&p, "journal/out/x.md")));
        // A file link inside the home that points out, also a dangling one.
        symlink(out.join("target.md"), p.home.join("notes.md")).unwrap();
        assert!(denied(&write(&p, "notes.md")), "dangling link to outside");
        std::fs::write(out.join("target.md"), "x").unwrap();
        assert!(denied(&write(&p, "notes.md")), "link to an outside file");
        // `..` after a link is taken from where the link leads.
        symlink(out.join("deep"), p.home.join("deep")).unwrap();
        std::fs::create_dir_all(out.join("deep")).unwrap();
        assert!(denied(&write(&p, "deep/../x.md")));
        // A link to a protected file inside the home.
        symlink(
            p.home.join(".claude/settings.json"),
            p.home.join("innocent.json"),
        )
        .unwrap();
        assert!(denied(&write(&p, "innocent.json")));
        // A link that stays inside is fine.
        symlink(p.home.join("journal"), p.home.join("j")).unwrap();
        assert_eq!(write(&p, "j/today.md"), Decision::Allow);
        // The home reached through a link to it.
        let alias = p.root.join("alias");
        symlink(&p.home, &alias).unwrap();
        assert_eq!(
            write(&p, alias.join("journal/x.md").to_str().unwrap()),
            Decision::Allow
        );
        assert!(denied(&write(
            &p,
            alias.join("SECURITY.md").to_str().unwrap()
        )));
        // A loop is denied, not followed forever.
        symlink(p.home.join("loop-b"), p.home.join("loop-a")).unwrap();
        symlink(p.home.join("loop-a"), p.home.join("loop-b")).unwrap();
        assert!(denied(&write(&p, "loop-a/x.md")));
    }

    #[test]
    fn moss_guard_denies_what_it_cannot_read() {
        let p = place();
        assert!(denied(&decide(&p.home, &p.home, "Write", &json!({}))));
        assert!(denied(&decide(
            &p.home,
            &p.home,
            "Write",
            &json!({ "file_path": 3 })
        )));
        assert!(denied(&decide(&p.home, &p.home, "Bash", &json!({}))));
    }

    #[test]
    fn moss_guard_checks_obvious_shell_writes() {
        let p = place();
        let bash = |c: &str| decide(&p.home, &p.home, "Bash", &json!({ "command": c }));
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
        ] {
            assert!(denied(&bash(bad)), "{bad}");
        }
        for fine in [
            "cc-moss ls --json",
            "cat SECURITY.md",
            "cat SOUL.md 2>&1",
            "cat SOUL.md > /dev/null",
            "echo hi >> journal/today.md",
            "cc-moss moss profile soul < /tmp/new-soul.md",
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
        let p = place();
        let profile = "cc-moss moss profile soul <<'MOSS_PROFILE'\n\
            # Soul\n\
            Never touch SECURITY.md; add > notes to the journal, rm nothing.\n\
            cp IDENTITY.md is not for you.\n\
            MOSS_PROFILE\n";
        assert_eq!(
            decide(&p.home, &p.home, "Bash", &json!({ "command": profile })),
            Decision::Allow
        );
        // After the body, commands count again.
        let sneaky = format!("{profile}cp evil SOUL.md\n");
        assert!(denied(&decide(
            &p.home,
            &p.home,
            "Bash",
            &json!({ "command": sneaky })
        )));
    }

    #[test]
    fn moss_guard_refuses_tools_it_does_not_know() {
        let p = place();
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
            match decide(&p.home, &p.home, tool, &json!({ "path": "/tmp/x" })) {
                Decision::Deny(why) => assert!(why.contains("does not use"), "{tool}: {why}"),
                Decision::Allow => panic!("{tool} was allowed"),
            }
        }
        for tool in [
            "Read",
            "Grep",
            "Glob",
            "WebFetch",
            "WebSearch",
            "Agent",
            "Skill",
        ] {
            assert_eq!(
                decide(
                    &p.home,
                    &p.home,
                    tool,
                    &json!({ "file_path": "/etc/hosts" })
                ),
                Decision::Allow,
                "{tool}"
            );
        }
        // Monitor runs in the sandbox like Bash, and is checked the same way.
        let monitor = |input: Value| decide(&p.home, &p.home, "Monitor", &input);
        assert_eq!(
            monitor(json!({ "command": "tail -f journal/today.md" })),
            Decision::Allow
        );
        assert!(denied(&monitor(json!({ "command": "echo x > SOUL.md" }))));
        assert_eq!(
            monitor(json!({ "url": "wss://example.com" })),
            Decision::Allow
        );
        assert!(denied(&monitor(json!({}))));
    }
}
