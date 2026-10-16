//! `cc-moss install` and `doctor` in a throwaway HOME, with stub tools on
//! the PATH. Nothing here touches the real home directory or package managers.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::bin;

struct Home {
    dir: tempfile::TempDir,
    stubs: PathBuf,
}

impl Home {
    /// A home with stub `claude`, plus `tmux` and `starship` unless left out.
    fn new(tools: &[&str]) -> Home {
        let dir = tempfile::tempdir().unwrap();
        let stubs = dir.path().join("stub-bin");
        fs::create_dir_all(&stubs).unwrap();
        for tool in tools {
            let script = match *tool {
                "tmux" => "#!/bin/sh\necho 'tmux 3.5a'\n",
                _ => "#!/bin/sh\nexit 0\n",
            };
            let p = stubs.join(tool);
            fs::write(&p, script).unwrap();
            std::process::Command::new("chmod")
                .arg("+x")
                .arg(&p)
                .status()
                .unwrap();
        }
        Home { dir, stubs }
    }

    fn with_all() -> Home {
        Home::new(&["claude", "tmux", "starship"])
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_default()
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, &[])
    }

    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = bin();
        cmd.stdin(std::process::Stdio::null())
            .env("HOME", self.dir.path())
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs.display()))
            .env("CC_MOSS_STATE_DIR", self.path("state"))
            // Never generate package-manager commands, let alone run them.
            .env("CC_MOSS_PLATFORM", "other")
            // zsh unless a test says otherwise, whatever runs the tests.
            .env("SHELL", "/bin/zsh")
            .env_remove("STARSHIP_CONFIG")
            .env_remove("STARSHIP_SHELL");
        // The test's own variables last, so they win.
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.args(args).output().unwrap()
    }

    /// Like `run`, but the PATH holds only the stubs and a shell: nothing on
    /// this machine (an apt-installed tmux, say) can leak in.
    fn run_sealed(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let sys = self.path("sys-bin");
        fs::create_dir_all(&sys).unwrap();
        for tool in ["sh", "id"] {
            let real = ["/bin", "/usr/bin"]
                .iter()
                .map(|d| Path::new(d).join(tool))
                .find(|p| p.exists())
                .unwrap();
            let link = sys.join(tool);
            if !link.exists() {
                std::os::unix::fs::symlink(real, link).unwrap();
            }
        }
        let mut cmd = bin();
        cmd.stdin(std::process::Stdio::null())
            .env("HOME", self.dir.path())
            .env(
                "PATH",
                format!("{}:{}", self.stubs.display(), sys.display()),
            )
            .env("CC_MOSS_STATE_DIR", self.path("state"))
            .env("CC_MOSS_PLATFORM", "other")
            // zsh unless a test says otherwise, whatever runs the tests.
            .env("SHELL", "/bin/zsh")
            .env_remove("STARSHIP_CONFIG")
            .env_remove("STARSHIP_SHELL");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.args(args).output().unwrap()
    }

    /// Every file under the home, for before/after comparisons.
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for e in fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push((p.clone(), fs::read(&p).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(self.dir.path(), &mut out);
        out.sort();
        out
    }
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn ok(o: Output) -> String {
    assert!(o.status.success(), "{}", text(&o));
    text(&o)
}

#[test]
fn fresh_home_installs_and_doctor_is_green() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let zshrc = h.read(".zshrc");
    for tag in ["cc-moss: prompt", "cc-moss", "cc-moss: ssh-tty-restore"] {
        assert!(
            zshrc.contains(&format!("# >>> {tag} >>>")),
            "{tag}:\n{zshrc}"
        );
    }
    assert!(h.read(".zshenv").contains("# >>> cc-moss: path >>>"));
    assert!(h.read(".tmux.conf").starts_with("# >>> cc-moss >>>"));
    assert!(h
        .read(".config/cc-moss/tmux.conf")
        .contains("display-popup"));
    assert!(h.read(".config/starship.toml").contains("[custom.device]"));
    assert!(h.read(".claude/settings.json").contains("cc-moss"));
    let report = ok(h.run(&["doctor"]));
    assert!(report.contains("Everything looks healthy"), "{report}");
}

#[test]
fn install_is_idempotent() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let before = h.snapshot();
    let again = ok(h.run(&["install", "--yes"]));
    assert!(again.contains("already up to date"), "{again}");
    assert_eq!(h.snapshot(), before);
}

#[test]
fn existing_starship_config_is_kept() {
    let h = Home::with_all();
    h.write(".config/starship.toml", "format = \"$all\"\n");
    let out = ok(h.run(&["install", "--yes"]));
    assert!(out.contains("Kept your starship config"), "{out}");
    assert_eq!(h.read(".config/starship.toml"), "format = \"$all\"\n");
    let report = ok(h.run(&["doctor"]));
    assert!(report.contains("your own (kept as is)"), "{report}");
    ok(h.run(&["install", "--yes", "--replace-starship"]));
    assert!(h.read(".config/starship.toml").contains("[custom.device]"));
}

#[test]
fn existing_tmux_conf_is_kept_below_the_block() {
    let h = Home::with_all();
    h.write(".tmux.conf", "set -g mouse off\nbind x kill-pane\n");
    ok(h.run(&["install", "--yes"]));
    let conf = h.read(".tmux.conf");
    assert!(conf.starts_with("# >>> cc-moss >>>\n"), "{conf}");
    assert!(
        conf.ends_with("# <<< cc-moss <<<\nset -g mouse off\nbind x kill-pane\n"),
        "{conf}"
    );
}

#[test]
fn blocks_stay_before_starship_init() {
    let h = Home::with_all();
    h.write(
        ".zshrc",
        "export A=1\neval \"$(starship init zsh)\"\nexport B=2\n",
    );
    ok(h.run(&["install", "--yes"]));
    let zshrc = h.read(".zshrc");
    assert_eq!(
        zshrc.matches("starship init").count(),
        1,
        "no second init:\n{zshrc}"
    );
    let init = zshrc.find("starship init").unwrap();
    let last_block = zshrc.rfind("# <<< cc-moss").unwrap();
    assert!(last_block < init, "{zshrc}");
    assert!(zshrc.ends_with("export B=2\n"), "{zshrc}");
    let report = ok(h.run(&["doctor"]));
    assert!(report.contains("starship runs last"), "{report}");
}

#[test]
fn aliases_only_with_the_flag_and_twins_are_reported() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    assert!(!h.read(".zshrc").contains("cc-moss: aliases"));
    h.write(".zshrc", &(h.read(".zshrc") + "alias ll='ls -lh'\n"));
    ok(h.run(&["install", "--yes", "--with", "aliases"]));
    assert!(h.read(".zshrc").contains("# >>> cc-moss: aliases >>>"));
    let report = ok(h.run(&["doctor"]));
    assert!(report.contains("also defined by hand: ll"), "{report}");
}

#[test]
fn skipped_parts_are_left_alone() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes", "--skip", "tmux", "--skip", "starship"]));
    assert!(!h.path(".tmux.conf").exists());
    assert!(!h.path(".config/starship.toml").exists());
    assert!(!h.read(".zshrc").contains("cc-moss: prompt"));
}

#[test]
fn missing_dependencies_are_listed() {
    let h = Home::new(&["claude"]);
    let out = text(&h.run_sealed(&["install", "--dry-run"], &[("CC_MOSS_PLATFORM", "debian")]));
    assert!(out.contains("Still missing: tmux, starship"), "{out}");
    assert!(
        out.contains("$ sudo apt-get update && sudo apt-get install -y tmux"),
        "{out}"
    );
    assert!(out.contains("https://starship.rs/install.sh"), "{out}");
    assert!(!h.path(".zshrc").exists(), "dry run writes nothing");
}

#[test]
fn with_only_takes_optional_parts() {
    let h = Home::with_all();
    let out = h.run(&["install", "--yes", "--with", "hooks"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("--with only adds optional parts"),
        "{}",
        text(&out)
    );
}

#[test]
fn all_set_only_when_nothing_is_missing() {
    let h = Home::new(&["claude"]);
    let out = ok(h.run(&["install", "--yes"]));
    assert!(!out.contains("All set"), "{out}");
    assert!(out.contains("cc-moss doctor"), "{out}");
}

#[test]
fn nothing_to_write_creates_no_empty_files() {
    let h = Home::with_all();
    let skip = [
        "--skip", "hooks", "--skip", "tmux", "--skip", "starship", "--skip", "shell",
    ];
    ok(h.run(&[&["install", "--yes"][..], &skip[..]].concat()));
    assert!(!h.path(".zshrc").exists());
}

#[test]
fn paths_follow_zdotdir_xdg_and_starship_config() {
    let h = Home::with_all();
    let zdot = h.path("zdot");
    let xdg = h.path("xdg");
    fs::create_dir_all(&zdot).unwrap();
    h.write("xdg/tmux/tmux.conf", "set -g mouse off\n");
    let star = h.path("my-starship.toml");
    ok(h.run_with(
        &["install", "--yes"],
        &[
            ("ZDOTDIR", zdot.to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
            ("STARSHIP_CONFIG", star.to_str().unwrap()),
        ],
    ));
    assert!(h.read("zdot/.zshrc").contains("# >>> cc-moss: prompt >>>"));
    // ssh host cmd reads ~/.zshenv, never $ZDOTDIR/.zshenv.
    assert!(h.read(".zshenv").contains("# >>> cc-moss: path >>>"));
    assert!(!h.path("zdot/.zshenv").exists());
    assert!(h
        .read("xdg/tmux/tmux.conf")
        .starts_with("# >>> cc-moss >>>"));
    assert!(!h.path(".tmux.conf").exists());
    let moss = h.read("xdg/cc-moss/tmux.conf");
    assert!(
        moss.contains(&format!(
            "source-file \"{}\"",
            fs::canonicalize(h.path("xdg/tmux/tmux.conf"))
                .unwrap()
                .display()
        )),
        "{moss}"
    );
    assert!(fs::read_to_string(&star)
        .unwrap()
        .contains("[custom.device]"));
}

#[test]
fn dangling_symlinks_are_written_through() {
    let h = Home::with_all();
    fs::create_dir_all(h.path("dotfiles")).unwrap();
    std::os::unix::fs::symlink(h.path("dotfiles/zshrc"), h.path(".zshrc")).unwrap();
    ok(h.run(&["install", "--yes"]));
    assert!(fs::symlink_metadata(h.path(".zshrc"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(h
        .read("dotfiles/zshrc")
        .contains("# >>> cc-moss: prompt >>>"));
}

#[test]
fn backups_are_as_private_as_the_file() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::with_all();
    h.write(".zshenv", "export TOKEN=secret\n");
    fs::set_permissions(h.path(".zshenv"), fs::Permissions::from_mode(0o600)).unwrap();
    ok(h.run(&["install", "--yes"]));
    let backup = fs::read_dir(h.dir.path())
        .unwrap()
        .flatten()
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".zshenv.cc-moss-")
        })
        .expect("a backup");
    let mode = backup.metadata().unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "backup mode {mode:o}");
}

#[test]
fn crlf_zshrc_gets_each_block_once() {
    let h = Home::with_all();
    h.write(".zshrc", "export A=1\r\n");
    ok(h.run(&["install", "--yes"]));
    ok(h.run(&["install", "--yes"]));
    let zshrc = h.read(".zshrc");
    assert_eq!(
        zshrc.matches("# >>> cc-moss: ssh-tty-restore >>>").count(),
        1,
        "{zshrc:?}"
    );
}

#[test]
fn doctor_flags_blocks_after_starship_and_install_moves_them() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    // Someone moves the ssh block below the prompt.
    let zshrc = h.read(".zshrc");
    let start = zshrc.find("# >>> cc-moss: ssh-tty-restore >>>").unwrap();
    let end = zshrc.find("# <<< cc-moss: ssh-tty-restore <<<\n").unwrap() + 36;
    let block = zshrc[start..end].to_string();
    h.write(".zshrc", &(zshrc.replace(&block, "") + &block));
    let out = h.run(&["doctor"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("after starship init: cc-moss: ssh-tty-restore"),
        "{}",
        text(&out)
    );
    ok(h.run(&["install", "--yes"]));
    assert!(ok(h.run(&["doctor"])).contains("starship runs last"));
}

#[test]
fn doctor_flags_a_missing_tmux_config() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    fs::remove_file(h.path(".config/cc-moss/tmux.conf")).unwrap();
    let out = h.run(&["doctor"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("tmux.conf is missing"),
        "{}",
        text(&out)
    );
}

#[test]
fn doctor_on_starship_without_the_binary() {
    // cc-moss's own prompt block is guarded: a warning, not a failure.
    let h = Home::new(&["claude", "tmux"]);
    ok(h.run(&["install", "--yes"]));
    let report = text(&h.run(&["doctor"]));
    assert!(
        report.contains("not installed: plain prompt for now"),
        "{report}"
    );
    // A hand-written init errors on every shell start: a failure.
    let h = Home::new(&["claude", "tmux"]);
    h.write(".zshrc", "eval \"$(starship init zsh)\"\n");
    let out = h.run(&["doctor"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("started in .zshrc but not installed"),
        "{}",
        text(&out)
    );
}

#[test]
fn doctor_fails_when_something_is_missing() {
    let h = Home::new(&["claude"]);
    let out = h.run(&["doctor"]);
    assert!(!out.status.success());
    let report = text(&out);
    // The hooks are always missing here; whether tmux counts too depends on
    // the host (it is in /usr/bin on Linux), so match "needs" and "need".
    assert!(
        report.contains("✗ hooks") && report.contains("fixing"),
        "{report}"
    );
}

#[test]
fn doctor_speaks_chinese() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let out = bin()
        .env("HOME", h.dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", h.stubs.display()))
        .env("CC_MOSS_LANG", "zh")
        .env("CC_MOSS_PLATFORM", "other")
        .env("SHELL", "/bin/zsh")
        .env_remove("STARSHIP_CONFIG")
        .arg("doctor")
        .output()
        .unwrap();
    assert!(text(&out).contains("一切正常"), "{}", text(&out));
}

/// The snapshot without what install and uninstall may leave on purpose:
/// backups, the state dir and Claude Code's (now hook-free) settings.json.
fn user_files(h: &Home) -> Vec<(PathBuf, Vec<u8>)> {
    h.snapshot()
        .into_iter()
        .filter(|(p, _)| {
            let rel = p.strip_prefix(h.dir.path()).unwrap();
            !p.to_string_lossy().ends_with(".bak")
                && !rel.starts_with("state")
                && rel != Path::new(".claude/settings.json")
        })
        .collect()
}

#[test]
fn uninstall_puts_the_home_back() {
    let h = Home::with_all();
    h.write(".zshrc", "export A=1\n");
    h.write(".zshenv", "export B=2\n");
    h.write(".tmux.conf", "set -g mouse off\n");
    let before = user_files(&h);
    ok(h.run(&["install", "--yes", "--with", "aliases"]));
    assert_ne!(user_files(&h), before);
    let out = ok(h.run(&["install", "--uninstall", "--yes"]));
    assert_eq!(user_files(&h), before, "{out}");
    assert!(!h.path(".config/cc-moss").exists());
    assert!(!h.path(".config/starship.toml").exists());
    assert!(!h.read(".claude/settings.json").contains("cc-moss"));
    assert!(out.contains("rm -rf"), "{out}");
    assert!(out.contains(h.path("state").to_str().unwrap()), "{out}");
}

#[test]
fn uninstall_removes_files_it_created() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    ok(h.run(&["install", "--uninstall", "--yes"]));
    for f in [".zshrc", ".zshenv", ".tmux.conf", ".config/starship.toml"] {
        assert!(!h.path(f).exists(), "{f} is left");
    }
    let again = ok(h.run(&["install", "--uninstall", "--yes"]));
    assert!(again.contains("Nothing of cc-moss's is left"), "{again}");
}

#[test]
fn uninstall_keeps_a_starship_config_that_is_not_ours() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let mine = h.read(".config/starship.toml") + "# my tweak\n";
    h.write(".config/starship.toml", &mine);
    let out = ok(h.run(&["install", "--uninstall", "--yes"]));
    assert_eq!(h.read(".config/starship.toml"), mine);
    assert!(out.contains("Kept your starship config"), "{out}");
}

#[test]
fn uninstall_dry_run_changes_nothing() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let before = h.snapshot();
    let out = ok(h.run(&["install", "--uninstall", "--dry-run"]));
    assert!(out.contains("Dry run"), "{out}");
    assert!(out.contains("tmux.conf (delete"), "{out}");
    assert_eq!(h.snapshot(), before);
}

#[test]
fn uninstall_asks_first() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let before = h.snapshot();
    let out = h.run(&["install", "--uninstall"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("--yes"), "{}", text(&out));
    assert_eq!(h.snapshot(), before);
}

const BASH: &[(&str, &str)] = &[("SHELL", "/bin/bash")];

#[test]
fn bash_users_get_no_zsh_files() {
    let h = Home::with_all();
    let out = ok(h.run_with(&["install", "--yes"], BASH));
    assert!(!h.path(".zshrc").exists());
    assert!(!h.path(".zshenv").exists());
    assert!(h.read(".claude/settings.json").contains("cc-moss"));
    assert!(h.read(".tmux.conf").starts_with("# >>> cc-moss >>>"));
    assert!(h.read(".config/starship.toml").contains("[custom.device]"));
    assert!(out.contains("zsh-only"), "{out}");
    assert!(out.contains("cc-moss pick"), "{out}");
    assert!(!out.contains("type `t`"), "{out}");
}

#[test]
fn bash_users_get_a_green_doctor_with_zsh_parts_skipped() {
    let h = Home::with_all();
    ok(h.run_with(&["install", "--yes"], BASH));
    let report = ok(h.run_with(&["doctor"], BASH));
    assert!(!report.contains('✗'), "{report}");
    assert!(report.contains("skipped: zsh only"), "{report}");
    assert!(report.contains("Everything looks healthy"), "{report}");
}

#[test]
fn bash_users_with_a_zshrc_get_the_blocks() {
    let h = Home::with_all();
    h.write(".zshrc", "export A=1\n");
    let out = ok(h.run_with(&["install", "--yes"], BASH));
    assert!(h.read(".zshrc").contains("# >>> cc-moss >>>"));
    assert!(h.read(".zshenv").contains("# >>> cc-moss: path >>>"));
    assert!(!out.contains("zsh-only"), "{out}");
}

#[test]
fn bash_users_are_told_in_chinese_too() {
    let h = Home::with_all();
    let out = ok(h.run_with(
        &["install", "--yes"],
        &[("SHELL", "/bin/bash"), ("CC_MOSS_LANG", "zh")],
    ));
    assert!(out.contains("只支持 zsh"), "{out}");
}

#[test]
fn uninstall_backs_up_a_shell_file_before_deleting_it() {
    // install created .zshrc; the user then typed inside one of our blocks.
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let edited = h.read(".zshrc").replacen(
        "# <<< cc-moss: prompt <<<",
        "export MINE=1\n# <<< cc-moss: prompt <<<",
        1,
    );
    h.write(".zshrc", &edited);
    ok(h.run(&["install", "--uninstall", "--yes"]));
    assert!(!h.path(".zshrc").exists());
    let backup = std::fs::read_dir(h.path(""))
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(".zshrc.cc-moss-") && n.ends_with(".bak")
        })
        .expect("a backup of the deleted .zshrc");
    assert!(std::fs::read_to_string(backup.path())
        .unwrap()
        .contains("export MINE=1"));
}

#[test]
fn uninstall_keeps_an_edited_cc_moss_tmux_config() {
    let h = Home::with_all();
    ok(h.run(&["install", "--yes"]));
    let mine = h.read(".config/cc-moss/tmux.conf") + "set -g status off\n";
    h.write(".config/cc-moss/tmux.conf", &mine);
    let out = ok(h.run(&["install", "--uninstall", "--yes"]));
    assert_eq!(h.read(".config/cc-moss/tmux.conf"), mine);
    assert!(out.contains("Kept"), "{out}");
}

#[test]
fn uninstall_dry_run_leaves_an_empty_config_dir() {
    let h = Home::with_all();
    std::fs::create_dir_all(h.path(".config/cc-moss")).unwrap();
    ok(h.run(&["install", "--uninstall", "--dry-run"]));
    assert!(h.path(".config/cc-moss").is_dir());
}
