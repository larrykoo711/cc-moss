use moss_core::worktree::{of, Worktree};

fn tree() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("acme");
    std::fs::create_dir_all(repo.join(".git/worktrees/97-t7.1")).unwrap();
    let wt = repo.join(".claude/worktrees/97-t7.1");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(
        wt.join(".git"),
        format!(
            "gitdir: {}\n",
            repo.join(".git/worktrees/97-t7.1").display()
        ),
    )
    .unwrap();
    (tmp, repo, wt)
}

#[test]
fn a_linked_worktree_names_its_repository() {
    let (_tmp, repo, wt) = tree();
    let want = Worktree {
        root: wt.to_string_lossy().into_owned(),
        repo: repo.to_string_lossy().into_owned(),
    };
    assert_eq!(of(&wt.to_string_lossy()), Some(want.clone()));
    assert_eq!(
        of(&wt.join("src").to_string_lossy()),
        Some(want),
        "from a subfolder"
    );
}

#[test]
fn the_main_checkout_and_plain_folders_are_not_worktrees() {
    let (_tmp, repo, _wt) = tree();
    assert_eq!(of(&repo.to_string_lossy()), None);
    assert_eq!(of("/nonexistent/folder"), None);
}

#[test]
fn a_submodule_is_not_a_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("app/vendor/lib");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join(".git"), "gitdir: ../../.git/modules/lib\n").unwrap();
    assert_eq!(of(&sub.to_string_lossy()), None);
}
