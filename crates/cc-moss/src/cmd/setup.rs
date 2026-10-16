//! `cc-moss setup`: register (or remove) the cc-moss hooks in Claude Code's
//! user settings.
//!
//! The file belongs to the user, so this command is deliberately cautious:
//! it refuses to touch a file it cannot parse, shows the exact change first,
//! asks before writing, keeps a backup and replaces the file atomically.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use moss_core::managed_block;
use moss_core::settings_json::{self, to_claude_string};
use serde_json::{json, Value};

use super::files::{self, read_optional, Change, Removal};

pub struct Options {
    pub dry_run: bool,
    pub yes: bool,
    pub uninstall: bool,
    pub settings: Option<PathBuf>,
    pub zshrc: Option<PathBuf>,
    /// Say nothing when there is nothing to do (`install --dry-run --json`).
    pub quiet: bool,
}

/// The managed `.zshrc` block where versions before 0.2 kept `t` as a zsh
/// function. `t` is a native alias now; setup takes this block out.
pub(super) const ZSH_TAG: &str = "cc-moss";

pub fn run(opts: Options) -> Result<()> {
    let mut changes = Vec::new();
    let mut notes = Vec::new();
    if let Some(c) = settings_change(&opts)? {
        changes.push(c);
    }
    if let Some(c) = zshrc_change(&opts)? {
        changes.push(c);
    }
    let place = super::alias::Place::detect()?;
    let (alias_changes, alias_notes) = if opts.uninstall {
        super::alias::uninstall_changes(&place, false)?
    } else {
        super::alias::default_changes(&place)?
    };
    let linking = !opts.uninstall && !alias_changes.is_empty();
    changes.extend(alias_changes);
    notes.extend(alias_notes);
    // `t` is a link in ~/.local/bin now, so that directory must be on PATH.
    // For zsh, setup adds the same PATH-only block install uses (shown in the
    // diff like everything else); other shells get a one-line hint.
    let paths = super::install::Paths::detect()?;
    if super::install::zsh_in_use(&paths) {
        changes.extend(zshenv_change(&opts, &paths)?);
    } else if linking {
        notes.extend(super::alias::path_hint(&place));
    }
    if changes.is_empty() {
        for n in &notes {
            println!("{n}");
        }
    }
    files::apply(&changes, &notes, opts.dry_run, opts.yes, "setup").map(drop)
}

pub(super) fn settings_change(opts: &Options) -> Result<Option<Change>> {
    let path = match &opts.settings {
        Some(p) => p.clone(),
        None => default_settings_path()?,
    };
    // Dotfile managers often symlink settings.json; write through the link
    // instead of replacing it with a regular file.
    let path = fs::canonicalize(&path).unwrap_or(path);
    let original = read_optional(&path)?;
    let old_doc: Value = match &original {
        Some(text) => serde_json::from_str(text).with_context(|| {
            format!(
                "{} is not valid JSON; refusing to modify it. Fix the file and run setup again",
                path.display()
            )
        })?,
        None => json!({}),
    };
    let new_doc = if opts.uninstall {
        settings_json::uninstall(&old_doc)?
    } else {
        settings_json::install(&old_doc, &hook_exe()?)?
    };
    // Compare documents, not text: a file the user formatted differently
    // should not be rewritten when nothing changes semantically.
    if new_doc == old_doc && (original.is_some() || opts.uninstall) {
        if opts.quiet {
            return Ok(None);
        }
        if opts.uninstall {
            println!(
                "{}",
                tr!(
                    format!("No cc-moss hooks in {}; nothing to remove.", path.display()),
                    format!("{} 里没有 cc-moss 钩子，不用移除。", path.display())
                )
            );
        } else {
            println!(
                "{}",
                tr!(
                    format!(
                        "cc-moss hooks in {} are already up to date.",
                        path.display()
                    ),
                    format!("{} 里的 cc-moss 钩子已是最新。", path.display())
                )
            );
        }
        return Ok(None);
    }
    Ok(Some(Change {
        remove: Removal::No,
        link: None,
        original,
        new_text: to_claude_string(&new_doc),
        done: if opts.uninstall {
            tr!(
                format!("Removed the cc-moss hooks from {}.", path.display()),
                format!("已从 {} 移除 cc-moss 钩子。", path.display())
            )
        } else {
            tr!(
                format!(
                    "Installed the cc-moss hooks in {}. New Claude Code sessions will check in automatically.",
                    path.display()
                ),
                format!(
                    "已在 {} 装好 cc-moss 钩子，之后新开的 Claude Code 会话会自动登记。",
                    path.display()
                )
            )
        },
        path,
    }))
}

/// Add the `cc-moss: path` block to `~/.zshenv` (it puts `~/.local/bin`
/// on PATH), or on uninstall take it out again unless `install`'s shell
/// blocks are there too: then it is install's to remove.
fn zshenv_change(opts: &Options, paths: &super::install::Paths) -> Result<Option<Change>> {
    use super::install::{self, TAG_PATH, TAG_SSH};
    if opts.uninstall {
        let zshrc = opts.zshrc.clone().or_else(default_zshrc_path);
        let zshrc = match zshrc {
            Some(p) => read_optional(&p)?.unwrap_or_default(),
            None => String::new(),
        };
        if managed_block::contains(&zshrc, TAG_SSH) {
            return Ok(None);
        }
        return install::strip_blocks(&paths.zshenv, &[TAG_PATH]);
    }
    let original = read_optional(&paths.zshenv)?.unwrap_or_default();
    let text = managed_block::upsert(&original, TAG_PATH, install::PATH_ZSH)
        .with_context(|| format!("cannot update {}", paths.zshenv.display()))?;
    install::change(&paths.zshenv, text)
}

/// Take the old `function t` block out of `.zshrc`: `t` is a link now, and
/// a function of the same name would hide it. Only cc-moss's own block is
/// touched; a `t` the user wrote is reported by `doctor`, never edited.
fn zshrc_change(opts: &Options) -> Result<Option<Change>> {
    let Some(path) = opts.zshrc.clone().or_else(default_zshrc_path) else {
        return Ok(None);
    };
    let path = fs::canonicalize(&path).unwrap_or(path);
    let Some(original) = read_optional(&path)? else {
        return Ok(None);
    };
    let new_text = managed_block::remove(&original, ZSH_TAG)
        .with_context(|| format!("cannot update {}", path.display()))?;
    if new_text == original {
        return Ok(None);
    }
    Ok(Some(Change {
        remove: Removal::No,
        link: None,
        original: Some(original),
        new_text,
        done: tr!(
            format!(
                "Took the old `t` shell function out of {}: `t` is a native alias now.",
                path.display()
            ),
            format!(
                "已从 {} 移除旧的 `t` shell 函数：`t` 现在是原生别名。",
                path.display()
            )
        ),
        path,
    }))
}

/// `$ZDOTDIR/.zshrc`, else `~/.zshrc`.
pub fn default_zshrc_path() -> Option<PathBuf> {
    let dir = std::env::var_os("ZDOTDIR")
        .filter(|d| !d.is_empty())
        .or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(dir).join(".zshrc"))
}

/// Claude Code's user settings: `$CLAUDE_CONFIG_DIR/settings.json` when that
/// variable is set (Claude Code honours it), else `~/.claude/settings.json`.
pub(super) fn default_settings_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("settings.json"));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; pass --settings <path>")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

/// The cc-moss path the hooks run: the stable `PATH` entry for this binary
/// when there is one, else the running binary itself.
fn hook_exe() -> Result<String> {
    let exe = std::env::current_exe().context("cannot locate the cc-moss executable")?;
    let exe = moss_core::aliases::stable_entry(&exe, std::env::var_os("PATH").as_deref());
    exe.into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("the cc-moss executable path is not valid UTF-8"))
}

/// What `doctor` reports about the hooks in Claude Code's settings.
pub(super) enum HookStatus {
    Current,
    /// Missing, or written by another cc-moss install or version.
    Outdated,
    /// Installed, but without hooks a newer cc-moss added (Claude Code
    /// event names, e.g. `Notification`).
    Incomplete(Vec<&'static str>),
    /// A hook runs a cc-moss that is gone or cannot be executed.
    Broken {
        path: String,
        missing: bool,
    },
}

pub(super) fn hook_status() -> Result<HookStatus> {
    let path = default_settings_path()?;
    let path = fs::canonicalize(&path).unwrap_or(path);
    let Some(text) = read_optional(&path)? else {
        return Ok(HookStatus::Outdated);
    };
    let doc: Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    for bin in settings_json::hook_binaries(&doc) {
        if let Some(missing) = unusable(&bin) {
            return Ok(HookStatus::Broken { path: bin, missing });
        }
    }
    let missing = settings_json::missing_hooks(&doc);
    if !missing.is_empty() && missing.len() < moss_core::agent::HookKind::ALL.len() {
        return Ok(HookStatus::Incomplete(
            missing.into_iter().map(settings_json::event_name).collect(),
        ));
    }
    Ok(if settings_json::install(&doc, &hook_exe()?)? == doc {
        HookStatus::Current
    } else {
        HookStatus::Outdated
    })
}

/// `Some(true)` when `bin` does not exist, `Some(false)` when it exists but
/// is not an executable file, `None` when it can run. A bare `cc-moss` is
/// looked up on `PATH`, as the shell would.
fn unusable(bin: &str) -> Option<bool> {
    use std::os::unix::fs::PermissionsExt;
    let found = if bin.contains('/') {
        Some(PathBuf::from(bin))
    } else {
        std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(bin))
                .find(|c| c.exists())
        })
    };
    let Some(meta) = found.and_then(|p| fs::metadata(p).ok()) else {
        return Some(true);
    };
    (!(meta.is_file() && meta.permissions().mode() & 0o111 != 0)).then_some(false)
}
