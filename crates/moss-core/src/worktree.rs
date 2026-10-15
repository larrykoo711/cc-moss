//! Recognising git linked worktrees (`git worktree add`), whose `.git` is a
//! file pointing into the main repository's `.git/worktrees/<name>`.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// The worktree's top folder.
    pub root: String,
    /// The main repository it belongs to.
    pub repo: String,
}

/// The linked worktree `dir` is in, if any. Main checkouts, submodules and
/// folders outside git are not worktrees.
pub fn of(dir: &str) -> Option<Worktree> {
    let mut current = Path::new(dir);
    loop {
        let git = current.join(".git");
        if git.is_dir() {
            return None;
        }
        if git.is_file() {
            let text = std::fs::read_to_string(&git).ok()?;
            let gitdir = text.strip_prefix("gitdir:")?.trim();
            let (repo, _) = gitdir.rsplit_once("/.git/worktrees/")?;
            return Some(Worktree {
                root: current.to_string_lossy().into_owned(),
                repo: repo.to_string(),
            });
        }
        current = current.parent()?;
    }
}
