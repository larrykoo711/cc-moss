//! Native aliases: names, the quote-aware splitter, the config table and
//! where the links point. Every file lives in a temp dir.

use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use moss_core::aliases::{self, CommandError, LinkState, NameError, Shadow, SplitError};
use moss_core::config::Config;

const SUBCOMMANDS: &[&str] = &["ls", "new", "pick", "attach", "alias"];

fn words(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parses_table() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(
        &p,
        "[ui]\nlanguage = \"en\"\n\n[aliases]\nt = \"pick\"\nmycc = \"new --dangerously-skip-permissions\"\n",
    )
    .unwrap();
    let c = Config::load_from(&p).unwrap();
    assert_eq!(c.aliases.len(), 2);
    assert_eq!(c.aliases["t"], "pick");
    assert_eq!(c.aliases["mycc"], "new --dangerously-skip-permissions");
    // No table at all is no aliases, not an error.
    std::fs::write(&p, "[ui]\nlanguage = \"en\"\n").unwrap();
    assert!(Config::load_from(&p).unwrap().aliases.is_empty());
}

#[test]
fn rejects_bad_names() {
    for good in ["t", "mycc", "cc2", "my-cc", "my_cc", "0x"] {
        assert_eq!(aliases::check_name(good), Ok(()), "{good}");
    }
    for bad in [
        "",
        "T",
        "my cc",
        "-x",
        "a/b",
        "../t",
        "a;b",
        "$(x)",
        "名字",
        "a.b",
        "cc-moss",
        "cc-moss-dev",
    ] {
        assert!(aliases::check_name(bad).is_err(), "{bad:?} must be refused");
    }
    assert_eq!(aliases::check_name("cc-moss"), Err(NameError::Reserved));
    assert_eq!(
        aliases::check_name(&"a".repeat(65)),
        Err(NameError::TooLong)
    );
}

#[test]
fn splits_with_quotes() {
    let split = |s: &str| aliases::split(s).unwrap();
    assert_eq!(split("pick"), words(&["pick"]));
    assert_eq!(
        split("  new   --dangerously-skip-permissions\t"),
        words(&["new", "--dangerously-skip-permissions"])
    );
    assert_eq!(
        split("new 'my name' \"two words\""),
        words(&["new", "my name", "two words"])
    );
    assert_eq!(split("a'b c'd"), words(&["ab cd"]), "adjacent parts join");
    assert_eq!(
        split("x '' \"\""),
        words(&["x", "", ""]),
        "empty quotes count"
    );
    assert_eq!(split(r#""say \"hi\" \\ \n""#), words(&[r#"say "hi" \ \n"#]));
    assert_eq!(split(r"a\ b"), words(&["a b"]));
    assert_eq!(split("'it\"s'"), words(&["it\"s"]));
    assert_eq!(split("登录页 \"新 会话\""), words(&["登录页", "新 会话"]));
    // Shell syntax is just text: nothing is expanded, nothing is special.
    assert_eq!(
        split("pick $(touch x) `id` ; | & > y $HOME ~"),
        words(&["pick", "$(touch", "x)", "`id`", ";", "|", "&", ">", "y", "$HOME", "~"])
    );
    assert!(split("").is_empty());
    assert_eq!(aliases::split("'open"), Err(SplitError::Unclosed('\'')));
    assert_eq!(aliases::split("\"open"), Err(SplitError::Unclosed('"')));
    assert_eq!(aliases::split("x\\"), Err(SplitError::TrailingBackslash));
}

#[test]
fn join_round_trips_through_split() {
    for case in [
        words(&["pick"]),
        words(&["new", "--dangerously-skip-permissions"]),
        words(&["new", "my name"]),
        words(&["note", "it's", "say \"hi\"", "back\\slash", ""]),
        words(&["new", "登录页", "$(touch x)", "a;b"]),
        words(&["x", "both ' and \""]),
    ] {
        let text = aliases::join(&case);
        assert_eq!(aliases::split(&text).unwrap(), case, "{text}");
    }
    assert_eq!(aliases::join(&words(&["new", "--x"])), "new --x");
}

#[test]
fn expands_only_known_subcommands() {
    assert_eq!(
        aliases::expand("new --dangerously-skip-permissions", SUBCOMMANDS),
        Ok(words(&["new", "--dangerously-skip-permissions"]))
    );
    assert_eq!(
        aliases::expand("hook stop", &["hook", "ls"]),
        Err(CommandError::Hook)
    );
    assert_eq!(
        aliases::expand("rm -rf /", SUBCOMMANDS),
        Err(CommandError::Unknown("rm".into()))
    );
    assert_eq!(
        aliases::expand("--version", SUBCOMMANDS),
        Err(CommandError::Unknown("--version".into()))
    );
    assert_eq!(
        aliases::expand("   ", SUBCOMMANDS),
        Err(CommandError::Empty)
    );
    assert_eq!(
        aliases::expand("new 'x", SUBCOMMANDS),
        Err(CommandError::Split(SplitError::Unclosed('\'')))
    );
}

/// `dir/Cellar/cc-moss/0.1.1/bin/cc-moss` (a real file) and `dir/bin/cc-moss`
/// linking to it, as Homebrew lays them out.
fn brew_layout(dir: &Path) -> (PathBuf, PathBuf) {
    let cellar = dir.join("Cellar/cc-moss/0.1.1/bin");
    std::fs::create_dir_all(&cellar).unwrap();
    let real = cellar.join("cc-moss");
    std::fs::write(&real, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let link = bin.join("cc-moss");
    symlink("../Cellar/cc-moss/0.1.1/bin/cc-moss", &link).unwrap();
    (real, link)
}

#[test]
fn picks_stable_entry_not_cellar() {
    let dir = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(dir.path()).unwrap();
    let (real, link) = brew_layout(&dir);
    // From PATH: the entry as written there.
    let path = std::env::join_paths([dir.join("bin")]).unwrap();
    assert_eq!(aliases::stable_entry(&real, Some(&path)), link);
    // Not on PATH (an app's bare PATH): the prefix's bin next to the Cellar.
    assert_eq!(aliases::stable_entry(&real, None), link);
    let empty = std::env::join_paths(["/nonexistent"]).unwrap();
    assert_eq!(aliases::stable_entry(&real, Some(&empty)), link);
    // A PATH `cc-moss` that is some other program is never chosen.
    let other = dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("cc-moss"), "#!/bin/sh\n").unwrap();
    let path = std::env::join_paths([other.clone(), dir.join("bin")]).unwrap();
    assert_eq!(aliases::stable_entry(&real, Some(&path)), link);
    // Nothing stable anywhere: the binary itself.
    let lone = other.join("cc-moss");
    assert_eq!(aliases::stable_entry(&lone, None), lone);
}

#[test]
fn link_states() {
    let dir = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(dir.path()).unwrap();
    let (_real, entry) = brew_layout(&dir);
    let bin = dir.join(".local/bin");
    let t = bin.join("t");
    assert_eq!(aliases::link_state(&t, &entry), LinkState::Missing);

    aliases::make_link(&t, &entry).unwrap();
    assert_eq!(std::fs::read_link(&t).unwrap(), entry);
    assert_eq!(aliases::link_state(&t, &entry), LinkState::Ok);
    assert!(aliases::is_ours(&t));
    // Making it again is a no-op, not an error.
    aliases::make_link(&t, &entry).unwrap();

    // Pointing at the Cellar file works today and breaks on the next upgrade.
    let cellar = dir.join("Cellar/cc-moss/0.1.1/bin/cc-moss");
    std::fs::remove_file(&t).unwrap();
    symlink(&cellar, &t).unwrap();
    assert_eq!(aliases::link_state(&t, &entry), LinkState::Stale);
    std::fs::remove_dir_all(dir.join("Cellar")).unwrap();
    assert_eq!(aliases::link_state(&t, &entry), LinkState::Broken);
    // Repairing replaces our own link.
    aliases::make_link(&t, &entry).unwrap();
    assert_eq!(std::fs::read_link(&t).unwrap(), entry);
}

#[test]
fn never_replaces_or_removes_what_is_not_ours() {
    let dir = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(dir.path()).unwrap();
    let (_real, entry) = brew_layout(&dir);
    let bin = dir.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();

    let script = bin.join("mine");
    std::fs::write(&script, "#!/bin/sh\necho mine\n").unwrap();
    let other_link = bin.join("vim");
    symlink("/usr/bin/vi", &other_link).unwrap();
    let a_dir = bin.join("sub");
    std::fs::create_dir(&a_dir).unwrap();

    for p in [&script, &other_link, &a_dir] {
        assert_eq!(aliases::link_state(p, &entry), LinkState::Foreign, "{p:?}");
        assert!(!aliases::is_ours(p));
        assert!(aliases::make_link(p, &entry).is_err(), "{p:?}");
        assert!(!aliases::remove_link(p).unwrap(), "{p:?}");
    }
    assert_eq!(
        std::fs::read_to_string(&script).unwrap(),
        "#!/bin/sh\necho mine\n"
    );
    assert_eq!(
        std::fs::read_link(&other_link).unwrap(),
        Path::new("/usr/bin/vi")
    );
    assert!(a_dir.is_dir());
    // No temp files left behind by the refused attempts.
    let mut names: Vec<String> = std::fs::read_dir(&bin)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["mine", "sub", "vim"]);

    let t = bin.join("t");
    aliases::make_link(&t, &entry).unwrap();
    assert!(aliases::remove_link(&t).unwrap());
    assert!(std::fs::symlink_metadata(&t).is_err());
    assert!(!aliases::remove_link(&t).unwrap(), "already gone");
}

#[test]
fn edits_keep_comments_and_other_tables() {
    let original = "# my settings\n[ui]\nlanguage = \"zh\" # chinese please\n\n\
                    [aliases]\n# the picker\nt = \"pick\" # short\n";
    let added = aliases::set_in_config(original, "mycc", "new --x").unwrap();
    assert!(added.starts_with("# my settings\n[ui]\nlanguage = \"zh\" # chinese please\n"));
    assert!(
        added.contains("# the picker\nt = \"pick\" # short\n"),
        "{added}"
    );
    assert!(added.contains("mycc = \"new --x\""), "{added}");

    let changed = aliases::set_in_config(&added, "t", "pick api").unwrap();
    assert!(changed.contains("t = \"pick api\" # short"), "{changed}");

    let (removed, found) = aliases::remove_from_config(&added, "mycc").unwrap();
    assert!(found);
    assert_eq!(removed, original);
    let (same, found) = aliases::remove_from_config(original, "nope").unwrap();
    assert!(!found);
    assert_eq!(same, original);

    // A fresh file gets just the table.
    assert_eq!(
        aliases::set_in_config("", "t", "pick").unwrap(),
        "[aliases]\nt = \"pick\"\n"
    );
    // Quotes and backslashes survive as TOML strings.
    let tricky = "note --goal 'say \"hi\"' a\\b";
    let text = aliases::set_in_config("", "n", tricky).unwrap();
    let doc: toml::Table = toml::from_str(&text).unwrap();
    assert_eq!(doc["aliases"]["n"].as_str(), Some(tricky));

    // Broken TOML is never rewritten.
    assert!(aliases::set_in_config("[aliases\nt = 1", "x", "pick").is_err());
    assert!(aliases::set_in_config("aliases = 3\n", "x", "pick").is_err());
}

#[test]
fn reads_what_type_says() {
    let link = Path::new("/home/demo/.local/bin/t");
    let s = |text: &str| aliases::parse_type(text, link);
    assert_eq!(s("t is /home/demo/.local/bin/t"), Shadow::None);
    assert_eq!(s("t is hashed (/home/demo/.local/bin/t)"), Shadow::None);
    assert_eq!(
        s("t is a shell function from /home/demo/.zshrc"),
        Shadow::Function
    );
    assert_eq!(
        s("t is a function\nt () \n{ alias x=y\n}"),
        Shadow::Function
    );
    assert_eq!(s("t is aliased to `ls'"), Shadow::Alias);
    assert_eq!(s("t is an alias for cc-moss pick"), Shadow::Alias);
    assert_eq!(s("t is a shell builtin"), Shadow::Builtin);
    assert_eq!(s("t is a reserved word"), Shadow::Builtin);
    assert_eq!(s("t is /usr/bin/t"), Shadow::Command("/usr/bin/t".into()));
    assert_eq!(s("t not found"), Shadow::NotFound);
    assert_eq!(s("bash: type: t: not found"), Shadow::NotFound);
    assert_eq!(s(""), Shadow::Unknown);
}

#[test]
fn falls_back_to_the_real_binary_not_the_link_it_ran_through() {
    // Started as `~/.local/bin/ta` with no cc-moss on PATH: macOS reports
    // the link itself as the current executable.
    let dir = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(dir.path()).unwrap();
    let real = dir.join("opt/cc-moss");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bin = dir.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let ta = bin.join("ta");
    symlink(&real, &ta).unwrap();
    assert_eq!(aliases::stable_entry(&ta, None), real);
    let empty = std::env::join_paths(["/nonexistent"]).unwrap();
    assert_eq!(aliases::stable_entry(&ta, Some(&empty)), real);
}

#[test]
fn type_answers_are_read_after_the_name() {
    // Names and paths can hold the very words the answer is made of.
    let dir = Path::new("/home/function/alias/.local/bin");
    for name in ["myalias", "tbuiltin", "funcy", "keyword", "nfound"] {
        let link = dir.join(name);
        let answer = format!("{name} is {}", link.display());
        assert_eq!(
            aliases::parse_type(&answer, &link),
            Shadow::None,
            "{answer}"
        );
        let hashed = format!("{name} is hashed ({})", link.display());
        assert_eq!(
            aliases::parse_type(&hashed, &link),
            Shadow::None,
            "{hashed}"
        );
    }
    let link = dir.join("myalias");
    assert_eq!(
        aliases::parse_type("myalias is a shell function from /home/demo/.zshrc", &link),
        Shadow::Function
    );
    assert_eq!(
        aliases::parse_type("myalias is aliased to `ls'", &link),
        Shadow::Alias
    );
    assert_eq!(
        aliases::parse_type("myalias is /usr/local/bin/myalias", &link),
        Shadow::Command("/usr/local/bin/myalias".into())
    );
    assert_eq!(
        aliases::parse_type("myalias not found", &link),
        Shadow::NotFound
    );
}

#[test]
fn a_non_string_alias_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    let text = "[ui]\nlanguage = \"zh\"\n[aliases]\nt = \"pick\"\nn = 3\nl = [\"ls\"]\n";
    std::fs::write(&p, text).unwrap();
    let c = Config::load_from(&p).expect("the rest of the config still loads");
    assert_eq!(c.ui.language.as_deref(), Some("zh"));
    assert_eq!(c.aliases.keys().collect::<Vec<_>>(), ["t"]);
    assert_eq!(aliases::ignored_entries(text), ["l", "n"]);
    assert!(aliases::ignored_entries("[aliases]\nt = \"pick\"\n").is_empty());
}
