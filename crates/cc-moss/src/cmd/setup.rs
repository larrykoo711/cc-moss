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

use super::files::{self, read_optional, Change};

pub struct Options {
    pub dry_run: bool,
    pub yes: bool,
    pub uninstall: bool,
    pub settings: Option<PathBuf>,
    pub zshrc: Option<PathBuf>,
}

/// The `t` shortcut, kept in a managed block of the user's `.zshrc`.
pub(super) const ZSH_TAG: &str = "cc-moss";
pub(super) const ZSH_BODY: &str = "\
# Managed by `cc-moss setup`; remove with `cc-moss setup --uninstall`.
unalias t 2>/dev/null
function t { cc-moss pick \"$@\"; }
";

pub fn run(opts: Options) -> Result<()> {
    let mut changes = Vec::new();
    if let Some(c) = settings_change(&opts)? {
        changes.push(c);
    }
    if let Some(c) = zshrc_change(&opts)? {
        changes.push(c);
    }
    files::apply(&changes, &[], opts.dry_run, opts.yes, "setup").map(drop)
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
        let exe = std::env::current_exe().context("cannot locate the cc-moss executable")?;
        let exe = exe
            .to_str()
            .context("the cc-moss executable path is not valid UTF-8")?;
        settings_json::install(&old_doc, exe)?
    };
    // Compare documents, not text: a file the user formatted differently
    // should not be rewritten when nothing changes semantically.
    if new_doc == old_doc && (original.is_some() || opts.uninstall) {
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

fn zshrc_change(opts: &Options) -> Result<Option<Change>> {
    let Some(path) = opts.zshrc.clone().or_else(default_zshrc_path) else {
        return Ok(None);
    };
    let path = fs::canonicalize(&path).unwrap_or(path);
    let Some(original) = read_optional(&path)? else {
        if !opts.uninstall {
            println!(
                "{}",
                tr!(
                    format!(
                        "No {}, so no `t` shortcut. Add `function t {{ cc-moss pick \"$@\"; }}` to your shell yourself.",
                        path.display()
                    ),
                    format!(
                        "没找到 {}，先不加 `t` 快捷命令。可以自己把 `function t {{ cc-moss pick \"$@\"; }}` 加进 shell 配置。",
                        path.display()
                    )
                )
            );
        }
        return Ok(None);
    };
    let new_text = if opts.uninstall {
        managed_block::remove(&original, ZSH_TAG)
    } else {
        managed_block::upsert(&original, ZSH_TAG, ZSH_BODY)
    }
    .with_context(|| format!("cannot update {}", path.display()))?;
    if new_text == original {
        if !opts.uninstall {
            println!(
                "{}",
                tr!(
                    format!(
                        "The `t` shortcut in {} is already up to date.",
                        path.display()
                    ),
                    format!("{} 里的 `t` 快捷命令已是最新。", path.display())
                )
            );
        }
        return Ok(None);
    }
    Ok(Some(Change {
        original: Some(original),
        new_text,
        done: if opts.uninstall {
            tr!(
                format!("Removed the `t` shortcut from {}.", path.display()),
                format!("已从 {} 移除 `t` 快捷命令。", path.display())
            )
        } else {
            tr!(
                format!(
                    "Added the `t` shortcut to {}. Open a new terminal (or `source` it) and type `t`.",
                    path.display()
                ),
                format!(
                    "已在 {} 加好 `t` 快捷命令。开个新终端（或 `source` 一下），敲 `t` 试试。",
                    path.display()
                )
            )
        },
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

/// Whether Claude Code's settings already carry the current cc-moss hooks.
pub(super) fn hooks_current() -> Result<bool> {
    let path = default_settings_path()?;
    let path = fs::canonicalize(&path).unwrap_or(path);
    let Some(text) = read_optional(&path)? else {
        return Ok(false);
    };
    let doc: Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    let exe = std::env::current_exe().context("cannot locate the cc-moss executable")?;
    let exe = exe
        .to_str()
        .context("the cc-moss executable path is not valid UTF-8")?;
    Ok(settings_json::install(&doc, exe)? == doc)
}
