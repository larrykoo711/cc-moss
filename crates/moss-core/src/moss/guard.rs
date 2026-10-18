//! The fence around Moss: its project-level PreToolUse hook asks
//! [`decide`] before every file write and shell command. Moss runs without
//! permission prompts, so this is what keeps it to its own home: nothing
//! outside it, not its own settings or hooks, not SECURITY.md, and its
//! persona files only through `/moss-profile`.
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

/// Whether `tool` may run with `input`. `home` is Moss's home and `cwd` the
/// session's directory (relative paths are taken from there).
pub fn decide(home: &Path, cwd: &Path, tool: &str, input: &Value) -> Decision {
    let key = match tool {
        "Write" | "Edit" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        "Bash" => {
            return match input.get("command").and_then(Value::as_str) {
                Some(command) => bash(command),
                None => Decision::deny("cc-moss could not read the command; not running it"),
            }
        }
        _ => return Decision::Allow,
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

/// A shell command: Bash is Moss's hands, so this cannot fence off every
/// write outside the home. It stops the obvious ways of changing what the
/// file tools may not: a protected file named next to something that writes.
fn bash(command: &str) -> Decision {
    let lower = command.to_lowercase();
    let mentions = |names: &[&str]| names.iter().any(|n| lower.contains(n));
    let own = [
        ".claude/",
        ".claude\\",
        "security.md",
        "claude.md",
        "sessions.md",
    ];
    let persona = PERSONA;
    if !mentions(&own) && !mentions(persona) {
        return Decision::Allow;
    }
    // Redirections that write nothing worth guarding.
    let mut quiet = lower.clone();
    for harmless in [
        "2>&1",
        "1>&2",
        ">&2",
        "2>/dev/null",
        ">/dev/null",
        "> /dev/null",
    ] {
        quiet = quiet.replace(harmless, " ");
    }
    let writes = [
        ">", "tee ", "sed -i", "perl -i", "mv ", "cp ", "rm ", "ln ", "truncate", "chmod", "dd ",
        "install ", "rsync", "touch ", "unlink",
    ];
    if !writes.iter().any(|w| quiet.contains(w)) {
        return Decision::Allow;
    }
    if !mentions(&own) && lower.contains("cc-moss moss profile") {
        return Decision::Allow;
    }
    Decision::deny(
        "that command would change a file that belongs to cc-moss or Moss's persona; \
         the persona changes only through /moss-profile",
    )
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
            "sed -i '' 's/x/y/' SECURITY.md",
            "rm -rf .claude/hooks",
            "cat new.md > SOUL.md",
            "cp mine.md IDENTITY.md",
        ] {
            assert!(denied(&bash(bad)), "{bad}");
        }
        for fine in [
            "cc-moss ls --json",
            "cat SECURITY.md",
            "cat SOUL.md 2>&1",
            "echo hi >> journal/today.md",
            "cc-moss moss profile soul < /tmp/new-soul.md",
        ] {
            assert_eq!(bash(fine), Decision::Allow, "{fine}");
        }
    }
}
