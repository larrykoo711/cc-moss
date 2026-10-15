use moss_core::managed_block::{remove, upsert};

const RC: &str = "export A=1\nsource ~/.zsh/t.zsh\neval \"$(starship init zsh)\"\nalias z=1\n";
const BODY: &str = "t() { cc-moss pick \"$@\"; }\n";

#[test]
fn inserts_before_starship_init() {
    let out = upsert(RC, "cc-moss", BODY).unwrap();
    let block = out.find("# >>> cc-moss >>>").unwrap();
    let starship = out.find("starship init").unwrap();
    let t_zsh = out.find("t.zsh").unwrap();
    assert!(t_zsh < block && block < starship, "{out}");
    assert!(out.contains(BODY));
}

#[test]
fn appends_without_starship() {
    let out = upsert("export A=1\n", "cc-moss", BODY).unwrap();
    assert!(out.starts_with("export A=1\n# >>> cc-moss >>>\n"), "{out}");
    assert!(out.ends_with("# <<< cc-moss <<<\n"), "{out}");
}

#[test]
fn upsert_is_idempotent_and_replaces() {
    let once = upsert(RC, "cc-moss", BODY).unwrap();
    assert_eq!(upsert(&once, "cc-moss", BODY).unwrap(), once);
    let changed = upsert(&once, "cc-moss", "t() { echo new; }\n").unwrap();
    assert!(
        changed.contains("echo new") && !changed.contains("cc-moss pick"),
        "{changed}"
    );
    assert_eq!(changed.matches("# >>> cc-moss >>>").count(), 1);
}

#[test]
fn remove_restores_the_original_bytes() {
    for original in [RC, "export A=1\n", ""] {
        let with = upsert(original, "cc-moss", BODY).unwrap();
        assert_eq!(
            remove(&with, "cc-moss").unwrap(),
            original,
            "original={original:?}"
        );
    }
    // Nothing to remove: unchanged.
    assert_eq!(remove(RC, "cc-moss").unwrap(), RC);
}

#[test]
fn duplicate_or_broken_markers_are_refused() {
    let with = upsert(RC, "cc-moss", BODY).unwrap();
    let twice = format!("{with}{with}");
    assert!(upsert(&twice, "cc-moss", BODY).is_err());
    let unterminated = "# >>> cc-moss >>>\nt() {}\n";
    assert!(upsert(unterminated, "cc-moss", BODY).is_err());
    assert!(remove(unterminated, "cc-moss").is_err());
}

#[test]
fn never_lands_inside_another_managed_block() {
    // The starship line often lives in another tool's managed block; a block
    // inserted inside it would be wiped when that tool rewrites its block.
    let rc = "export A=1\n# >>> shell-standard: prompt >>>\n# keep starship last\neval \"$(starship init zsh)\"\n# <<< shell-standard: prompt <<<\nalias z=1\n";
    let out = upsert(rc, "cc-moss", BODY).unwrap();
    let ours = out.find("# >>> cc-moss >>>").unwrap();
    let theirs = out.find("# >>> shell-standard: prompt >>>").unwrap();
    assert!(ours < theirs, "{out}");
    assert_eq!(remove(&out, "cc-moss").unwrap(), rc);
}
