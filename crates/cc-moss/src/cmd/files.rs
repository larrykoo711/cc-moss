//! Changing files that belong to the user: show the diff, ask once, keep a
//! timestamped backup of each file and replace it atomically.

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use moss_core::settings_json;
use moss_core::slot::now_ms;

use super::confirm;

/// New files up to this many lines are shown in full; longer ones as a summary.
const NEW_FILE_PREVIEW: usize = 30;

/// One file a command wants to change.
pub struct Change {
    pub path: PathBuf,
    pub original: Option<String>,
    pub new_text: String,
    /// What to say once written.
    pub done: String,
}

/// Show every diff, ask once, then back up and write each file. `command`
/// names the command in the `+++` header ("after setup"). False when the
/// user said no or this is a dry run.
/// `notes` are printed after the diffs, right before the question.
pub fn apply(
    changes: &[Change],
    notes: &[String],
    dry_run: bool,
    yes: bool,
    command: &str,
) -> Result<bool> {
    if changes.is_empty() {
        return Ok(true);
    }
    for c in changes {
        let lines = c.new_text.lines().count();
        // A whole new file of ours is one line, not a wall of `+`.
        if c.original.is_none() && lines > NEW_FILE_PREVIEW {
            println!(
                "{}",
                tr!(
                    format!("+++ {} (new, {lines} lines)", c.path.display()),
                    format!("+++ {}（新建，{lines} 行）", c.path.display())
                )
            );
            continue;
        }
        println!("--- {}", c.path.display());
        println!(
            "+++ {} {}",
            c.path.display(),
            tr!(format!("(after {command})"), format!("（{command} 之后）"))
        );
        print!(
            "{}",
            unified_diff(c.original.as_deref().unwrap_or(""), &c.new_text, 3)
        );
    }
    for n in notes {
        println!("{n}");
    }
    if dry_run {
        println!(
            "{}",
            tr!("Dry run: nothing written.", "演练模式：什么都没写入。")
        );
        return Ok(false);
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
        yes,
        &question,
        tr!(
            "re-run with --yes to apply the diff above",
            "加上 --yes 重新运行即可应用上面的改动"
        ),
    )? {
        println!("{}", tr!("Okay, nothing written.", "好的，什么都没动。"));
        return Ok(false);
    }
    for c in changes {
        if let Some(text) = &c.original {
            let backup = write_backup(&c.path, text.as_bytes())?;
            println!("{} {}", tr!("Backup:", "备份："), backup.display());
        }
        write_atomically(&c.path, c.new_text.as_bytes())?;
        println!("{}", c.done);
    }
    Ok(true)
}

pub fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
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
        // The backup is as private as the file it copies: rc files and
        // settings.json can hold tokens.
        let mode = fs::metadata(path).map_or(0o600, |m| m.permissions().mode() & 0o777);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&candidate)
        {
            Ok(mut f) => {
                f.write_all(bytes)
                    .and_then(|()| f.set_permissions(fs::Permissions::from_mode(mode)))
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
/// so nothing (Claude Code, a starting shell) ever reads a half-written file.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
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

/// Line diff via a plain LCS table. The files are a few hundred lines at most,
/// so O(n*m) is fine and saves a dependency.
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
