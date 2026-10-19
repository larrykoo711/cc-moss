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

/// Folders Moss's read tools stay out of: the transcripts of the user's
/// other Claude Code sessions (`~/.claude/projects`, and under
/// `CLAUDE_CONFIG_DIR` when that is set). Moss hears about sessions only
/// through cc-moss, never by reading their conversations.
pub fn private_reads(home: Option<&Path>, config_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home.filter(|h| h.is_absolute()) {
        out.push(home.join(".claude/projects"));
    }
    if let Some(config) = config_dir.filter(|c| c.is_absolute()) {
        out.push(config.join("projects"));
    }
    out
}

/// Whether a read tool (Read, Grep, Glob) may look where it is pointed:
/// not into `private` (links and `..` followed). Other tools are left to
/// [`decide`].
pub fn decide_read(private: &[PathBuf], cwd: &Path, tool: &str, input: &Value) -> Decision {
    let mut paths: Vec<&str> = Vec::new();
    let field = |k: &str| {
        input
            .get(k)
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
    };
    match tool {
        "Read" => paths.extend(field("file_path")),
        "Grep" => paths.extend(field("path")),
        "Glob" => {
            paths.extend(field("path"));
            // An absolute pattern names its folder itself: up to the first
            // wildcard.
            if let Some(pattern) = field("pattern").filter(|p| p.starts_with('/')) {
                let cut = pattern.find(['*', '?', '[', '{']).unwrap_or(pattern.len());
                paths.push(&pattern[..cut]);
            }
        }
        _ => return Decision::Allow,
    }
    if paths.is_empty() {
        paths.push(".");
    }
    let fenced: Vec<PathBuf> = private.iter().filter_map(|p| resolve(p)).collect();
    for raw in paths {
        let path = Path::new(raw);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        let Some(target) = resolve(&absolute) else {
            return Decision::deny("cc-moss could not work out where that path leads; not reading");
        };
        if fenced.iter().any(|f| target.starts_with(f)) {
            return Decision::deny(
                "the transcripts of the user's other sessions are private; \
                 SESSIONS.md and cc-moss's updates are what Moss knows about them",
            );
        }
    }
    Decision::Allow
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
        [first, ..] if CONVERSATION.contains(&first.as_str()) => Decision::deny(format!(
            "{first} is cc-moss's record of your conversation with Moss; leave it as it is"
        )),
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
    // The profile command runs outside the sandbox: alone, never with
    // another command riding along.
    if profile_command(command) {
        let rest = code.trim();
        let compound = [";", "&", "|", "\n", "`", "$(", "<(", ">("]
            .iter()
            .any(|op| rest.contains(op));
        if compound {
            return Decision::deny(
                "run `cc-moss moss profile <name> --from <file>` as a command of its own",
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
    let first = it.next().unwrap_or("");
    first.rsplit('/').next() == Some("cc-moss")
        && it.next() == Some("moss")
        && it.next() == Some("profile")
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
        // The conversation is cc-moss's record.
        for path in [
            "chat.jsonl",
            "chat.jsonl.1",
            "inbox/0000000000001-aaaaaa.json",
            "stop/123",
            "waiting",
            "Inbox/x.json",
        ] {
            assert!(denied(&write(&p, path)), "{path}");
        }
        assert_eq!(write(&p, "memory/inbox.md"), Decision::Allow);
    }

    #[test]
    fn moss_guard_keeps_other_transcripts_unread() {
        let p = place();
        let projects = p.root.join("user/.claude/projects");
        std::fs::create_dir_all(projects.join("-work-api")).unwrap();
        let private = vec![projects.clone()];
        let read =
            |tool: &str, input: serde_json::Value| decide_read(&private, &p.home, tool, &input);
        let file = projects.join("-work-api/abc.jsonl");
        assert!(denied(&read("Read", json!({ "file_path": file }))));
        assert!(denied(&read(
            "Grep",
            json!({ "pattern": "x", "path": projects })
        )));
        assert!(denied(&read(
            "Glob",
            json!({ "pattern": "**/*.jsonl", "path": projects.parent().unwrap().join("projects/.") })
        )));
        assert!(denied(&read(
            "Glob",
            json!({ "pattern": format!("{}/**/*.jsonl", projects.display()) })
        )));
        // Through a link, and through `..`.
        symlink(&projects, p.home.join("peek")).unwrap();
        assert!(denied(&read(
            "Read",
            json!({ "file_path": "peek/-work-api/abc.jsonl" })
        )));
        assert!(denied(&read(
            "Read",
            json!({ "file_path": projects.join("-work-api/../-work-api/abc.jsonl") })
        )));
        // Its own files and the rest of the machine read as before.
        assert_eq!(
            read("Read", json!({ "file_path": "SESSIONS.md" })),
            Decision::Allow
        );
        assert_eq!(read("Grep", json!({ "pattern": "x" })), Decision::Allow);
        assert_eq!(
            read("Read", json!({ "file_path": p.root.join("user/notes.md") })),
            Decision::Allow
        );
        assert_eq!(
            read("Write", json!({ "file_path": file })),
            Decision::Allow,
            "writes are decide's"
        );
        // The list: the user's Claude Code projects, and CLAUDE_CONFIG_DIR's.
        let list = private_reads(Some("/u".as_ref()), Some("/c".as_ref()));
        assert_eq!(
            list,
            [
                PathBuf::from("/u/.claude/projects"),
                PathBuf::from("/c/projects")
            ]
        );
    }

    #[test]
    fn moss_guard_keeps_its_hands_off_the_conversation() {
        let p = place();
        let bash = |c: &str| decide(&p.home, &p.home, "Bash", &json!({ "command": c }));
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
        assert!(denied(&write(&p, "input-busy")));
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
