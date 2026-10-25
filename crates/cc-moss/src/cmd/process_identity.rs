//! Shared destructive-operation identity checks. Process origin is not chat history.
use moss_core::{launch_args, proc, registry::Registry, slot::Slot};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Selection {
    startup: Option<String>,
    resume: Option<String>,
    forked: bool,
}

fn selection(args: &[String]) -> Option<Selection> {
    let mut result = Selection {
        startup: None,
        resume: None,
        forked: false,
    };
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if arg == "--" {
            break;
        }
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(f, v)| (f, Some(v)));
        if launch_args::option_arity(flag) == Some(0) && inline.is_none() {
            if flag == "--fork-session" {
                result.forked = true;
            }
            continue;
        }
        if launch_args::option_arity(flag) != Some(1) {
            return None;
        }
        let value = match inline {
            Some(v) => v,
            None => {
                let value = args.get(i)?;
                i += 1;
                value.as_str()
            }
        };
        if !matches!(flag, "--session-id" | "--resume" | "-r") {
            continue;
        }
        let target = if flag == "--session-id" {
            &mut result.startup
        } else {
            &mut result.resume
        };
        if value.is_empty()
            || value.starts_with('-')
            || value.chars().any(char::is_control)
            || target.is_some()
        {
            return None;
        }
        *target = Some(value.into());
    }
    Some(result)
}

fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

#[derive(Debug, PartialEq, Eq)]
enum EntryIdentity {
    TrustedClaude(Vec<String>),
    NotClaude,
    Unknown,
}

pub(super) fn entry_arguments(pid: u32) -> Option<Vec<String>> {
    match entry_identity(pid) {
        EntryIdentity::TrustedClaude(args) => Some(args),
        EntryIdentity::NotClaude | EntryIdentity::Unknown => None,
    }
}

/// Trusted entry is the configured/discovered Claude binary, not any argv word
/// containing its name. Interpreters require that exact script as argv[1].
fn entry_identity(pid: u32) -> EntryIdentity {
    let exe = proc::executable(pid).and_then(|path| canonical(&path));
    let expected = super::launch::claude_bin()
        .ok()
        .and_then(|path| canonical(Path::new(&path)));
    #[cfg(debug_assertions)]
    let expected = std::env::var_os("CC_MOSS_TEST_CLAUDE_ENTRY")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .and_then(|path| canonical(&path))
        .or(expected);
    let supported = exe.as_deref().is_some_and(supported_interpreter);
    classify_entry(exe.as_deref(), expected.as_deref(), supported, || {
        launch_args::exact_argv(pid)
    })
}

fn supported_interpreter(exe: &Path) -> bool {
    let interpreter = exe.file_name().and_then(|name| name.to_str());
    let supported = interpreter == Some("node")
        && std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path)
                .find_map(|dir| canonical(&dir.join("node")))
                .as_deref()
                == Some(exe)
        });
    #[cfg(debug_assertions)]
    let supported = supported
        || matches!(
            interpreter,
            Some("bash" | "dash" | "sh" | "python3" | "python3.13" | "python3.14")
        );
    supported
}

fn classify_entry(
    exe: Option<&Path>,
    expected: Option<&Path>,
    supported: bool,
    read_argv: impl FnOnce() -> Option<Vec<String>>,
) -> EntryIdentity {
    let (Some(exe), Some(expected)) = (exe, expected) else {
        return EntryIdentity::Unknown;
    };
    let release = exe == expected || earlier_release(exe, expected);
    if !release && !supported {
        return EntryIdentity::NotClaude;
    }
    let Some(argv) = read_argv().filter(|args| !args.is_empty()) else {
        return EntryIdentity::Unknown;
    };
    if release {
        return EntryIdentity::TrustedClaude(argv.into_iter().skip(1).collect());
    }
    let Some(script) = argv.get(1).filter(|arg| !arg.starts_with('-')) else {
        return EntryIdentity::NotClaude;
    };
    let Some(script) = canonical(Path::new(script)) else {
        return EntryIdentity::Unknown;
    };
    if script != expected && !earlier_release(&script, expected) {
        return EntryIdentity::NotClaude;
    }
    EntryIdentity::TrustedClaude(argv.into_iter().skip(2).collect())
}

/// Claude's native updater installs each release beside the last
/// (`…/versions/<version>`) and repoints the launcher, so a session started before
/// an update keeps running an earlier sibling of the configured binary. A release
/// already removed has no path to resolve and stays unknown.
fn earlier_release(exe: &Path, expected: &Path) -> bool {
    let version = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            })
    };
    let versions = expected.parent();
    versions.is_some_and(|dir| dir.file_name() == Some("versions".as_ref()))
        && exe.parent() == versions
        && version(exe)
        && version(expected)
}

/// The hook must descend from the declared Claude, with no nearer Claude or
/// unknown entry. An inherited parent's CLAUDE_PID is not fresh provenance.
pub(super) fn hook_origin(pid: u32, start: &str) -> Option<String> {
    if hook_owner(std::process::id(), pid, &proc::parents(), entry_identity) {
        observed_origin(pid, start)
    } else {
        None
    }
}

fn hook_owner(
    hook: u32,
    declared: u32,
    parents: &std::collections::HashMap<u32, u32>,
    mut identity: impl FnMut(u32) -> EntryIdentity,
) -> bool {
    let mut current = hook;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..32 {
        let Some(parent) = parents.get(&current) else {
            return false;
        };
        current = *parent;
        if !seen.insert(current) {
            return false;
        }
        match identity(current) {
            EntryIdentity::TrustedClaude(_) => return current == declared,
            EntryIdentity::Unknown => return false,
            EntryIdentity::NotClaude if current == declared => return false,
            EntryIdentity::NotClaude => {}
        }
    }
    false
}

pub(super) fn observed_origin(pid: u32, start: &str) -> Option<String> {
    if start.is_empty() || proc::liveness(pid, Some(start)) != proc::Liveness::Alive {
        return None;
    }
    let selection = selection(&entry_arguments(pid)?)?;
    // A fork or simultaneous resume is not an unambiguous new root.
    if selection.forked || selection.resume.is_some() {
        return None;
    }
    let origin = selection.startup?;
    (proc::start_time(pid).as_deref() == Some(start)).then_some(origin)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OperationIdentity {
    binding: Option<moss_core::process_binding::ProcessBinding>,
    selection: Selection,
}
impl OperationIdentity {
    pub(super) fn recheck(&self, registry: &Registry, slot: &Slot) -> anyhow::Result<()> {
        if verify(registry, slot)? != *self {
            anyhow::bail!("the process binding changed; retry after checking the session");
        }
        Ok(())
    }
    pub(super) fn after_exit(&self, registry: &Registry, slot: &Slot, claimed_at: i64) -> bool {
        registry
            .stopped_operation_matches(slot, claimed_at, self.binding.as_ref())
            .unwrap_or(false)
    }
}

fn native_selection(selected: &Selection, slot: &Slot) -> bool {
    let accepts =
        |id: &str| id == slot.session_id || slot.session_ids.iter().any(|known| known == id);
    selected.startup.as_deref().is_none_or(accepts)
        && (selected.forked || selected.resume.as_deref().is_none_or(accepts))
}

pub(super) fn verify(registry: &Registry, slot: &Slot) -> anyhow::Result<OperationIdentity> {
    let refuse = || {
        anyhow::anyhow!("the process identity or its startup-session association cannot be verified; open the session instead")
    };
    let (pid, start) = slot
        .pid
        .zip(slot.pid_start.as_deref().filter(|s| !s.is_empty()))
        .ok_or_else(refuse)?;
    if proc::liveness(pid, Some(start)) != proc::Liveness::Alive {
        return Err(refuse());
    }
    let selected = selection(&entry_arguments(pid).ok_or_else(refuse)?).ok_or_else(refuse)?;
    let (binding, raw_binding) = registry.operation_identity(slot)?.ok_or_else(refuse)?;
    if !native_selection(&selected, slot) {
        let origin = selected
            .startup
            .as_deref()
            .filter(|_| !selected.forked && selected.resume.is_none())
            .ok_or_else(refuse)?;
        binding
            .filter(|b| b.state == "transferred" && b.hops == 1 && b.origin_session_id == origin)
            .ok_or_else(refuse)?;
    }
    if proc::start_time(pid).as_deref() != Some(start)
        || selection(&entry_arguments(pid).ok_or_else(refuse)?).as_ref() != Some(&selected)
    {
        return Err(refuse());
    }
    Ok(OperationIdentity {
        binding: raw_binding,
        selection: selected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn resume_identity_hook_ancestry_rejects_unknown_entry() {
        let parents = std::collections::HashMap::from([(40, 30), (30, 20), (20, 10)]);
        assert!(
            !hook_owner(40, 10, &parents, |pid| match pid {
                20 => EntryIdentity::Unknown,
                10 => EntryIdentity::TrustedClaude(vec![]),
                _ => EntryIdentity::NotClaude,
            }),
            "unknown nearer entry must not authorize the farther declared Claude"
        );
    }

    #[test]
    fn resume_identity_hook_ancestry_rejects_inherited_parent_and_cycles() {
        let parents = std::collections::HashMap::from([(40, 30), (30, 20), (20, 10)]);
        let identity = |pid| match pid {
            10 | 20 => EntryIdentity::TrustedClaude(vec![]),
            _ => EntryIdentity::NotClaude,
        };
        assert!(hook_owner(40, 20, &parents, identity));
        assert!(
            !hook_owner(40, 10, &parents, identity),
            "nearer Claude owns this hook, not inherited parent"
        );
        assert!(!hook_owner(40, 99, &parents, identity));
        let cycle = std::collections::HashMap::from([(40, 30), (30, 40)]);
        assert!(!hook_owner(40, 99, &cycle, |_| EntryIdentity::NotClaude));
        let deep = (1..=34).map(|pid| (pid, pid + 1)).collect();
        assert!(!hook_owner(1, 35, &deep, |_| EntryIdentity::NotClaude));
    }

    #[test]
    fn resume_identity_hook_ancestry_rejects_unreadable_exe_and_argv() {
        let parents = std::collections::HashMap::from([(40, 30), (30, 20), (20, 10)]);
        let claude = Path::new("/trusted/claude");
        let node = Path::new("/trusted/node");
        for exe in [None, Some(claude), Some(node)] {
            assert!(!hook_owner(40, 10, &parents, |pid| match pid {
                20 => classify_entry(exe, Some(claude), true, || None),
                10 => EntryIdentity::TrustedClaude(vec![]),
                _ => EntryIdentity::NotClaude,
            }));
        }
    }

    #[test]
    fn resume_identity_hook_ancestry_skips_known_shell() {
        let parents = std::collections::HashMap::from([(40, 30), (30, 20)]);
        assert!(hook_owner(40, 20, &parents, |pid| {
            if pid == 30 {
                classify_entry(
                    Some(Path::new("/bin/zsh")),
                    Some(Path::new("/trusted/claude")),
                    false,
                    || panic!("known non-Claude executable needs no argv"),
                )
            } else {
                EntryIdentity::TrustedClaude(vec![])
            }
        }));
    }

    #[test]
    fn resume_identity_hook_ancestry_validates_declared_entry() {
        let parents = std::collections::HashMap::from([(40, 20)]);
        assert!(!hook_owner(40, 20, &parents, |_| EntryIdentity::Unknown));
        assert!(!hook_owner(40, 20, &parents, |_| EntryIdentity::NotClaude));
    }

    #[test]
    fn resume_identity_entry_trusts_an_earlier_release_after_an_update() {
        let current = Path::new("/home/u/.local/share/claude/versions/2.1.310");
        let argv = || Some(args(&["claude", "--resume", "A"]));
        assert_eq!(
            classify_entry(
                Some(Path::new("/home/u/.local/share/claude/versions/2.1.308")),
                Some(current),
                false,
                argv
            ),
            EntryIdentity::TrustedClaude(args(&["--resume", "A"])),
            "a session started before the update still runs the previous release"
        );
        for exe in [
            "/home/u/.local/share/claude/versions/evil",
            "/home/u/.local/share/claude/versions/2.1.308/claude",
            "/home/u/.local/share/claude/other/2.1.308",
            "/tmp/versions/2.1.308",
            "/home/u/.local/share/claude/versions/2..308",
        ] {
            assert_eq!(
                classify_entry(Some(Path::new(exe)), Some(current), false, argv),
                EntryIdentity::NotClaude,
                "{exe} is not a release of the configured Claude"
            );
        }
        assert_eq!(
            classify_entry(
                Some(Path::new("/opt/bin/2.1.308")),
                Some(Path::new("/opt/bin/2.1.310")),
                false,
                argv
            ),
            EntryIdentity::NotClaude,
            "only Claude's own versions directory holds releases"
        );
        assert_eq!(
            classify_entry(
                Some(Path::new("/home/u/.local/share/claude/versions/2.1.308")),
                Some(Path::new("/home/u/.local/share/claude/versions/latest")),
                false,
                argv
            ),
            EntryIdentity::NotClaude,
            "a configured binary not named for a release has no earlier releases"
        );
        let parents = std::collections::HashMap::from([(40, 30), (30, 20)]);
        let earlier = Path::new("/home/u/.local/share/claude/versions/2.1.308");
        assert!(
            !hook_owner(40, 20, &parents, |pid| match pid {
                30 => classify_entry(Some(earlier), Some(current), false, argv),
                _ => EntryIdentity::TrustedClaude(vec![]),
            }),
            "a nearer Claude on an earlier release owns the hook, not the farther one"
        );
    }

    #[test]
    fn resume_identity_entry_preserves_exact_arguments() {
        let claude = Path::new("/trusted/claude");
        assert_eq!(
            classify_entry(Some(claude), Some(claude), false, || {
                Some(args(&["claude", "--session-id", "A"]))
            }),
            EntryIdentity::TrustedClaude(args(&["--session-id", "A"]))
        );
        assert_eq!(
            classify_entry(Some(claude), Some(claude), false, || Some(vec![])),
            EntryIdentity::Unknown
        );
    }

    #[test]
    fn resume_identity_entry_known_shell_command_is_not_claude_script() {
        assert_eq!(
            classify_entry(
                Some(Path::new("/bin/bash")),
                Some(Path::new("/trusted/claude")),
                true,
                || Some(args(&["bash", "-c", "cc-moss hook"])),
            ),
            EntryIdentity::NotClaude
        );
    }

    #[test]
    fn resume_identity_native_clear_and_fork_fast_paths_remain_scoped() {
        use moss_core::event::{Context, Event, StartSource};
        let mut registry = Registry::open_in_memory().unwrap();
        let ctx = Context {
            pid: Some(1),
            pid_start: Some("start".into()),
            tmux: None,
            launch_args: None,
            now_ms: 1,
        };
        for (id, source) in [
            ("old", StartSource::Startup),
            ("current", StartSource::Clear),
        ] {
            registry
                .apply(
                    &Event::Start {
                        source,
                        session_id: id.into(),
                        cwd: "/work".into(),
                        title: None,
                        transcript_path: None,
                    },
                    &ctx,
                )
                .unwrap();
        }
        let slot = registry.slots().unwrap().remove(0);
        assert!(native_selection(
            &selection(&args(&["--session-id", "old"])).unwrap(),
            &slot
        ));
        assert!(native_selection(
            &selection(&args(&["--resume", "parent", "--fork-session"])).unwrap(),
            &slot
        ));
        assert!(!native_selection(
            &selection(&args(&["--resume", "other"])).unwrap(),
            &slot
        ));
        assert!(!native_selection(
            &selection(&args(&["--session-id", "other", "--fork-session"])).unwrap(),
            &slot
        ));
    }

    #[test]
    fn resume_identity_option_values_cannot_hide_or_invent_session_flags() {
        let selected = selection(&args(&[
            "--append-system-prompt",
            "literal -- ignored",
            "--session-id",
            "A",
        ]))
        .unwrap();
        assert_eq!(selected.startup.as_deref(), Some("A"));
        let selected = selection(&args(&[
            "--append-system-prompt",
            "--session-id",
            "--session-id",
            "A",
        ]))
        .unwrap();
        assert_eq!(selected.startup.as_deref(), Some("A"));
        assert!(selection(&args(&["--unknown-option", "value", "--session-id", "A"])).is_none());
    }

    #[test]
    fn resume_identity_start_flags_are_unambiguous() {
        assert_eq!(
            selection(&args(&["--session-id=A"]))
                .unwrap()
                .startup
                .as_deref(),
            Some("A")
        );
        for words in [
            vec!["--session-id"],
            vec!["--session-id", "--other"],
            vec!["--session-id", "A", "--session-id=B"],
            vec!["-r", "--name", "x"],
        ] {
            assert!(selection(&args(&words)).is_none());
        }
        let fork = selection(&args(&["--resume", "parent", "--fork-session"])).unwrap();
        assert!(fork.forked);
    }
}
