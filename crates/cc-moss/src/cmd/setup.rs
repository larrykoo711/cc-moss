//! `cc-moss setup`: register (or remove) the cc-moss hooks in Claude Code's
//! user settings.
//!
//! The file belongs to the user, so this command is deliberately cautious:
//! it refuses to touch a file it cannot parse, shows the exact change first,
//! asks before writing, keeps a backup and replaces the file atomically.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use moss_core::managed_block;
use moss_core::settings_json::{self, to_claude_string};
use moss_core::slot::now_ms;
use serde_json::{json, Value};

use super::confirm;

pub struct Options {
    pub dry_run: bool,
    pub yes: bool,
    pub uninstall: bool,
    pub settings: Option<PathBuf>,
    pub zshrc: Option<PathBuf>,
}

/// The `t` shortcut, kept in a managed block of the user's `.zshrc`.
const ZSH_TAG: &str = "cc-moss";
const ZSH_BODY: &str = "\
# Managed by `cc-moss setup`; remove with `cc-moss setup --uninstall`.
unalias t 2>/dev/null
function t { cc-moss pick \"$@\"; }
";

/// One file setup wants to change.
struct Change {
    path: PathBuf,
    original: Option<String>,
    new_text: String,
    /// What to say once written.
    done: String,
}

pub fn run(opts: Options) -> Result<()> {
    let mut changes = Vec::new();
    if let Some(c) = settings_change(&opts)? {
        changes.push(c);
    }
    if let Some(c) = zshrc_change(&opts)? {
        changes.push(c);
    }
    if changes.is_empty() {
        return Ok(());
    }

    for c in &changes {
        println!("--- {}", c.path.display());
        println!(
            "+++ {} {}",
            c.path.display(),
            tr!("(after setup)", "（setup 之后）")
        );
        print!(
            "{}",
            unified_diff(c.original.as_deref().unwrap_or(""), &c.new_text, 3)
        );
    }
    if opts.dry_run {
        println!(
            "{}",
            tr!("Dry run: nothing written.", "演练模式：什么都没写入。")
        );
        return Ok(());
    }
    let question = if changes.len() == 1 {
        tr!(
            format!("Apply this change to {}?", changes[0].path.display()),
            format!("把上面的改动写入 {}？", changes[0].path.display())
        )
    } else {
        tr!(
            format!("Apply these changes to {} files?", changes.len()),
            format!("把上面的改动写入这 {} 个文件？", changes.len())
        )
    };
    if !confirm::ask(
        opts.yes,
        &question,
        tr!(
            "re-run with --yes to apply the diff above",
            "加上 --yes 重新运行即可应用上面的改动"
        ),
    )? {
        println!("{}", tr!("Okay, nothing written.", "好的，什么都没动。"));
        return Ok(());
    }
    for c in &changes {
        if let Some(text) = &c.original {
            let backup = write_backup(&c.path, text.as_bytes())?;
            println!("{} {}", tr!("Backup:", "备份："), backup.display());
        }
        write_atomically(&c.path, c.new_text.as_bytes())?;
        println!("{}", c.done);
    }
    Ok(())
}

fn settings_change(opts: &Options) -> Result<Option<Change>> {
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

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// `$ZDOTDIR/.zshrc`, else `~/.zshrc`.
fn default_zshrc_path() -> Option<PathBuf> {
    let dir = std::env::var_os("ZDOTDIR")
        .filter(|d| !d.is_empty())
        .or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(dir).join(".zshrc"))
}

/// Claude Code's user settings: `$CLAUDE_CONFIG_DIR/settings.json` when that
/// variable is set (Claude Code honours it), else `~/.claude/settings.json`.
fn default_settings_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("settings.json"));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; pass --settings <path>")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

/// Copy the current file aside. Never overwrites an older backup: two runs in
/// the same second (install, then uninstall) get distinct names.
fn write_backup(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let base = settings_json::backup_path(path, now_ms());
    let base_name = base
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = base_name.strip_suffix(".bak").unwrap_or(&base_name);
    for n in 0..1000 {
        let candidate = if n == 0 {
            base.clone()
        } else {
            base.with_file_name(format!("{stem}-{n}.bak"))
        };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut f) => {
                f.write_all(bytes)
                    .and_then(|()| f.sync_all())
                    .with_context(|| format!("cannot write backup {}", candidate.display()))?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("cannot create backup {}", candidate.display()))
            }
        }
    }
    bail!("too many backups next to {}", path.display())
}

/// Write to a temp file in the same directory, then rename over the target,
/// so Claude Code never reads a half-written settings.json.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".into());
    let tmp = dir.join(format!(".{name}.cc-moss-{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.with_context(|| format!("cannot write {}", path.display()))
}

#[derive(Debug, PartialEq, Eq)]
enum Op<'a> {
    Keep(&'a str),
    Del(&'a str),
    Add(&'a str),
}

/// Line diff via a plain LCS table. settings.json is small, so O(n*m) is fine
/// and saves a dependency.
fn line_diff<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Op<'a>> {
    let (n, m) = (a.len(), b.len());
    // lcs[i][j] = length of the LCS of a[i..] and b[j..].
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut ops = Vec::with_capacity(n + m);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push(Op::Keep(a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            // On a tie, delete first so a replaced line reads `-old` then `+new`.
            ops.push(Op::Del(a[i]));
            i += 1;
        } else {
            ops.push(Op::Add(b[j]));
            j += 1;
        }
    }
    ops
}

/// Unified-diff hunks (`@@ -a,b +c,d @@`) with `context` lines around changes.
fn unified_diff(old: &str, new: &str, context: usize) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let ops = line_diff(&a, &b);

    // 1-based line numbers in old/new at the start of each op.
    let mut pos = Vec::with_capacity(ops.len());
    let (mut oi, mut ni) = (1, 1);
    for op in &ops {
        pos.push((oi, ni));
        match op {
            Op::Keep(_) => {
                oi += 1;
                ni += 1;
            }
            Op::Del(_) => oi += 1,
            Op::Add(_) => ni += 1,
        }
    }

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        if matches!(op, Op::Keep(_)) {
            continue;
        }
        let start = k.saturating_sub(context);
        let end = (k + context + 1).min(ops.len());
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => ranges.push((start, end)),
        }
    }

    let mut out = String::new();
    for (start, end) in ranges {
        let slice = &ops[start..end];
        let old_len = slice.iter().filter(|o| !matches!(o, Op::Add(_))).count();
        let new_len = slice.iter().filter(|o| !matches!(o, Op::Del(_))).count();
        let (os, ns) = pos[start];
        // A zero-length side points at the line before, as diff(1) does.
        let os = if old_len == 0 { os - 1 } else { os };
        let ns = if new_len == 0 { ns - 1 } else { ns };
        out.push_str(&format!("@@ -{os},{old_len} +{ns},{new_len} @@\n"));
        for op in slice {
            let (sign, line) = match op {
                Op::Keep(l) => (' ', l),
                Op::Del(l) => ('-', l),
                Op::Add(l) => ('+', l),
            };
            out.push(sign);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_shows_insertion_with_context() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nb\nc\nd\nX\ne\nf\ng\nh\n";
        assert_eq!(
            unified_diff(old, new, 3),
            "@@ -2,6 +2,7 @@\n b\n c\n d\n+X\n e\n f\n g\n"
        );
    }

    #[test]
    fn diff_from_empty_and_identical() {
        assert_eq!(unified_diff("", "{}\n", 3), "@@ -0,0 +1,1 @@\n+{}\n");
        assert_eq!(unified_diff("same\n", "same\n", 3), "");
    }

    #[test]
    fn diff_separates_distant_changes() {
        let line = |i: u32| match i {
            2 => "two\n".to_string(),
            17 => "seventeen\n".to_string(),
            _ => format!("{i}\n"),
        };
        let old: String = (0..20).map(|i| format!("{i}\n")).collect();
        let new: String = (0..20).map(line).collect();
        let d = unified_diff(&old, &new, 1);
        assert_eq!(d.matches("@@ -").count(), 2, "{d}");
        assert!(d.contains("-2\n+two\n"), "{d}");
    }
}
