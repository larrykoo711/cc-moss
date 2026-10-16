//! `cc-moss alias` and running cc-moss under an alias's name, in a throwaway
//! HOME. The fake Homebrew `bin/cc-moss` is a link to the test binary; no
//! test writes outside its temp dir or reads the real shell config.

mod common;

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A home, a fake Homebrew `bin` holding `cc-moss`, and a dir for other
/// programs on PATH.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let env = Env { dir };
        for d in ["home", "brew/bin", "sys"] {
            fs::create_dir_all(env.path(d)).unwrap();
        }
        symlink(env!("CARGO_BIN_EXE_cc-moss"), env.entry()).unwrap();
        env
    }

    fn path(&self, rel: &str) -> PathBuf {
        // Canonical, so paths compare equal to what cc-moss prints on macOS.
        fs::canonicalize(self.dir.path()).unwrap().join(rel)
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    fn entry(&self) -> PathBuf {
        self.path("brew/bin/cc-moss")
    }

    fn link(&self, name: &str) -> PathBuf {
        self.home().join(".local/bin").join(name)
    }

    fn config(&self) -> PathBuf {
        self.home().join(".config/cc-moss/config.toml")
    }

    fn read_config(&self) -> String {
        fs::read_to_string(self.config()).unwrap_or_default()
    }

    fn write_config(&self, text: &str) {
        fs::create_dir_all(self.config().parent().unwrap()).unwrap();
        fs::write(self.config(), text).unwrap();
    }

    /// An executable script at `rel`.
    fn program(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn default_path(&self) -> String {
        format!(
            "{}:{}:{}:/usr/bin:/bin",
            self.path("brew/bin").display(),
            self.home().join(".local/bin").display(),
            self.path("sys").display()
        )
    }

    /// A clean environment: nothing of the caller's shell, tmux or config.
    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env("HOME", self.home())
            .env("PATH", self.default_path())
            .env("SHELL", "/bin/bash")
            .env("TERM", "dumb")
            .env("CC_MOSS_LANG", "en")
            .env("CC_MOSS_STATE_DIR", self.path("state"))
            .env("CC_MOSS_CLAUDE_BIN", "/nonexistent/claude")
            .env("CC_MOSS_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::null());
        cmd
    }

    /// `cc-moss`, run as Homebrew users run it: from the PATH link.
    fn cc(&self, args: &[&str]) -> Output {
        self.command(self.entry()).args(args).output().unwrap()
    }

    /// The program run under an alias name, the way a shell runs it.
    fn run_alias(&self, name: &str, args: &[&str]) -> Output {
        self.command(self.link(name)).args(args).output().unwrap()
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let out = self.cc(args);
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e}): {}", text(&out)))
    }

    fn links(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.home().join(".local/bin"))
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn text(o: &Output) -> String {
    stdout(o) + &String::from_utf8_lossy(&o.stderr)
}

fn ok(o: Output) -> String {
    assert!(o.status.success(), "{}", text(&o));
    text(&o)
}

fn failed(o: Output) -> String {
    assert!(!o.status.success(), "should fail: {}", text(&o));
    text(&o)
}

#[test]
fn dispatches_on_argv0() {
    let env = Env::new();
    let out = ok(env.cc(&["alias", "add", "tl", "ls", "--json"]));
    assert!(out.contains("Added `tl`"), "{out}");
    assert_eq!(fs::read_link(env.link("tl")).unwrap(), env.entry());
    assert!(env.read_config().contains("tl = \"ls --json\""));

    // The alias's words come first, then the user's own arguments.
    let listed = env.run_alias("tl", &["--all"]);
    let doc: serde_json::Value =
        serde_json::from_slice(&listed.stdout).unwrap_or_else(|_| panic!("{}", text(&listed)));
    assert_eq!(doc["schema"], 1);

    // An argument with a space stays one argument.
    ok(env.cc(&["alias", "add", "ta", "attach"]));
    let out = failed(env.run_alias("ta", &["my name"]));
    assert!(out.contains("no session named `my name`"), "{out}");
    assert!(!out.contains("unexpected argument"), "{out}");

    // Help and usage errors read as cc-moss's own.
    let out = failed(env.run_alias("ta", &[]));
    assert!(out.contains("cc-moss attach <NAME>"), "{out}");
}

#[test]
fn unknown_alias_explains() {
    let env = Env::new();
    fs::create_dir_all(env.home().join(".local/bin")).unwrap();
    symlink(env.entry(), env.link("zz")).unwrap();
    let out = failed(env.run_alias("zz", &["pick"]));
    assert!(
        out.contains("`zz` is not one of your aliases") && out.contains("cc-moss alias ls"),
        "{out}"
    );

    // A hand-edited config cannot make an alias run anything but cc-moss.
    env.write_config("[aliases]\nzz = \"rm -rf /\"\nhk = \"hook stop\"\n");
    symlink(env.entry(), env.link("hk")).unwrap();
    let out = failed(env.run_alias("zz", &[]));
    assert!(out.contains("`rm` is not a cc-moss command"), "{out}");
    assert!(out.contains("cc-moss alias add zz"), "{out}");
    // `hook` always exits 0; refusing it means it never ran.
    let out = failed(env.run_alias("hk", &[]));
    assert!(out.contains("only for Claude Code"), "{out}");

    // Chinese too.
    let mut zh = env.command(env.link("zz"));
    zh.env("CC_MOSS_LANG", "zh");
    let out = failed(zh.output().unwrap());
    assert!(out.contains("不是 cc-moss 的命令"), "{out}");
}

#[test]
fn alias_never_goes_through_a_shell() {
    let env = Env::new();
    let marker = |n: u32| env.path(&format!("pwned{n}"));
    // Written by hand, so `alias add` checks do not filter anything out.
    env.write_config(&format!(
        "[aliases]\n\
         a = \"attach \\\"$(touch {})\\\"\"\n\
         b = \"attach x; touch {}\"\n\
         c = \"attach `touch {}`\"\n\
         d = \"attach\"\n\
         e = \"attach x | touch {} && touch {}\"\n",
        marker(1).display(),
        marker(2).display(),
        marker(3).display(),
        marker(4).display(),
        marker(5).display(),
    ));
    fs::create_dir_all(env.home().join(".local/bin")).unwrap();
    for name in ["a", "b", "c", "d", "e"] {
        symlink(env.entry(), env.link(name)).unwrap();
    }

    // `$(…)` arrives as text: attach looks for a session by that name.
    let out = failed(env.run_alias("a", &[]));
    assert!(
        out.contains(&format!(
            "no session named `$(touch {})`",
            marker(1).display()
        )),
        "{out}"
    );
    // `;`, backticks, `|` and `&&` are words like any other.
    for name in ["b", "c", "e"] {
        let out = failed(env.run_alias(name, &[]));
        assert!(out.contains("unexpected argument"), "{name}: {out}");
    }
    // The user's own arguments are passed on untouched, too.
    let evil = format!(
        "$(touch {}); `touch {}`",
        marker(6).display(),
        marker(7).display()
    );
    let out = failed(env.run_alias("d", &[&evil]));
    assert!(out.contains(&format!("no session named `{evil}`")), "{out}");

    for n in 1..=7 {
        assert!(!marker(n).exists(), "pwned{n} was created: a shell ran");
    }
}

#[test]
fn add_refuses_existing_command() {
    let env = Env::new();
    let theirs = env.program("sys/mytool", "echo theirs");
    let out = failed(env.cc(&["alias", "add", "mytool", "pick"]));
    assert!(
        out.contains("`mytool` is already a command")
            && out.contains(&theirs.display().to_string()),
        "{out}"
    );
    assert!(out.contains("--force"), "{out}");
    assert!(env.links().is_empty(), "{:?}", env.links());
    assert!(!env.config().exists());

    // System commands too, and the app gets a code it can act on.
    let doc = env.json(&["alias", "add", "--json", "ls", "pick"]);
    assert_eq!(doc["ok"], false);
    assert_eq!(doc["code"], "exists");
    assert!(!env.config().exists());
}

#[test]
fn force_overrides() {
    let env = Env::new();
    let theirs = env.program("sys/mytool", "echo theirs");
    let out = ok(env.cc(&["alias", "add", "--force", "mytool", "pick"]));
    assert!(out.contains("Added `mytool`"), "{out}");
    assert_eq!(fs::read_link(env.link("mytool")).unwrap(), env.entry());
    assert_eq!(
        fs::read_to_string(&theirs).unwrap(),
        "#!/bin/sh\necho theirs\n"
    );

    // When the other one comes first on PATH, the user is told it wins.
    let path = format!("{}:{}", env.path("sys").display(), env.default_path());
    let out = ok(env
        .command(env.entry())
        .env("PATH", &path)
        .args(["alias", "add", "--force", "mytool", "pick", "api"])
        .output()
        .unwrap());
    assert!(out.contains("Updated `mytool`"), "{out}");
    assert!(
        out.contains("Heads-up") && out.contains("comes first on your PATH"),
        "{out}"
    );
}

#[test]
fn add_never_replaces_a_file_that_is_not_ours() {
    let env = Env::new();
    let mine = env.program("home/.local/bin/mine", "echo mine");
    let other = env.link("vim");
    symlink("/usr/bin/vi", &other).unwrap();
    for name in ["mine", "vim"] {
        for force in [false, true] {
            let mut args = vec!["alias", "add", "--json"];
            if force {
                args.push("--force");
            }
            args.extend([name, "pick"]);
            let doc = env.json(&args);
            assert_eq!(doc["code"], "foreign", "{name} {force}: {doc}");
        }
    }
    assert_eq!(fs::read_to_string(&mine).unwrap(), "#!/bin/sh\necho mine\n");
    assert_eq!(fs::read_link(&other).unwrap(), Path::new("/usr/bin/vi"));
    assert_eq!(env.links(), ["mine", "vim"], "no temp files left");
    assert!(!env.config().exists());

    // setup leaves a `t` that is not ours alone, and says so.
    let t = env.program("home/.local/bin/t", "echo my t");
    let out = ok(env.cc(&[
        "setup",
        "--yes",
        "--settings",
        env.path("settings.json").to_str().unwrap(),
    ]));
    assert!(out.contains("is not cc-moss's"), "{out}");
    assert_eq!(fs::read_to_string(&t).unwrap(), "#!/bin/sh\necho my t\n");
}

#[test]
fn rejects_bad_names_and_commands() {
    let env = Env::new();
    for name in ["../x", "a/b", "X", "-x", "cc-moss", "a b", "$(id)"] {
        let doc = env.json(&["alias", "add", "--json", "--", name, "pick"]);
        assert_eq!(doc["code"], "bad_name", "{name}: {doc}");
    }
    for command in ["rm -rf /", "hook stop", "", "pick 'open", "--version"] {
        let doc = env.json(&["alias", "add", "--json", "x", command]);
        assert_eq!(doc["code"], "bad_command", "{command:?}: {doc}");
    }
    assert!(env.links().is_empty());
    assert!(!env.config().exists());
    // rm never treats a name as a path.
    let outside = env.program("home/x", "echo keep");
    let doc = env.json(&["alias", "rm", "--json", "../x"]);
    assert_eq!(doc["code"], "not_found");
    assert!(outside.exists());
}

#[test]
fn warns_when_shadowed() {
    let env = Env::new();
    fs::write(
        env.home().join(".bashrc"),
        "mycc() { echo theirs; }\nalias tt='echo theirs'\n",
    )
    .unwrap();
    let out = ok(env.cc(&["alias", "add", "mycc", "pick"]));
    assert!(out.contains("Added `mycc`"), "added anyway: {out}");
    assert!(out.contains("hidden by a shell function"), "{out}");
    let out = ok(env.cc(&["alias", "add", "tt", "pick"]));
    assert!(out.contains("hidden by a shell alias"), "{out}");

    let doc = env.json(&["alias", "ls", "--json"]);
    let rows = doc["aliases"].as_array().unwrap();
    let row = |n: &str| rows.iter().find(|r| r["name"] == n).unwrap().clone();
    assert_eq!(row("mycc")["status"], "shadowed");
    assert_eq!(row("mycc")["shadow"], "function");
    assert_eq!(row("tt")["shadow"], "alias");

    // Nothing in the user's shell config was changed.
    assert_eq!(
        fs::read_to_string(env.home().join(".bashrc")).unwrap(),
        "mycc() { echo theirs; }\nalias tt='echo theirs'\n"
    );
}

#[test]
fn keeps_config_comments() {
    let env = Env::new();
    let original = "# my cc-moss settings\n[ui]\nlanguage = \"en\" # keep English\n\n\
                    [aliases]\n# picker\nt = \"pick\" # mine\n";
    // A dotfile manager's link: written through, not replaced.
    let real = env.path("dotfiles/config.toml");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::write(&real, original).unwrap();
    fs::create_dir_all(env.config().parent().unwrap()).unwrap();
    symlink(&real, env.config()).unwrap();

    ok(env.cc(&["alias", "add", "tn", "new"]));
    let added = fs::read_to_string(&real).unwrap();
    assert!(added.starts_with(original), "{added}");
    assert!(added.ends_with("tn = \"new\"\n"), "{added}");
    assert!(fs::symlink_metadata(env.config())
        .unwrap()
        .file_type()
        .is_symlink());

    ok(env.cc(&["alias", "rm", "tn"]));
    assert_eq!(fs::read_to_string(&real).unwrap(), original);

    // A config that does not parse is never rewritten.
    let broken = "[aliases\nt = \"pick\"\n";
    fs::write(&real, broken).unwrap();
    failed(env.cc(&["alias", "add", "tn", "new"]));
    assert_eq!(fs::read_to_string(&real).unwrap(), broken);
    assert!(!env.links().contains(&"tn".to_string()));
}

#[test]
fn rm_removes_link() {
    let env = Env::new();
    ok(env.cc(&["alias", "add", "tn", "new"]));
    assert!(env.link("tn").exists());
    let doc = env.json(&["alias", "rm", "--json", "tn"]);
    assert_eq!(doc["ok"], true);
    assert!(fs::symlink_metadata(env.link("tn")).is_err());
    assert!(!env.read_config().contains("tn"));
    let doc = env.json(&["alias", "rm", "--json", "tn"]);
    assert_eq!(doc["code"], "not_found");

    // A file of the same name that is not ours stays.
    env.write_config("[aliases]\nmine = \"pick\"\n");
    let mine = env.program("home/.local/bin/mine", "echo mine");
    let out = ok(env.cc(&["alias", "rm", "mine"]));
    assert!(out.contains("is not cc-moss's, so it stays"), "{out}");
    assert_eq!(fs::read_to_string(&mine).unwrap(), "#!/bin/sh\necho mine\n");
    assert!(!env.read_config().contains("mine"));
}

#[test]
fn ls_reports_each_alias() {
    let env = Env::new();
    let doc = env.json(&["alias", "ls", "--json"]);
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["aliases"].as_array().unwrap().len(), 0);
    assert_eq!(doc["entry"], env.entry().to_str().unwrap());
    assert_eq!(doc["dir_on_path"], true);

    ok(env.cc(&["alias", "add", "tn", "new", "--detach"]));
    env.write_config(&(env.read_config() + "gone = \"pick\"\nbad = \"rm x\"\n"));
    let doc = env.json(&["alias", "ls", "--json"]);
    let rows = doc["aliases"].as_array().unwrap();
    let row = |n: &str| rows.iter().find(|r| r["name"] == n).unwrap().clone();
    assert_eq!(row("tn")["command"], "new --detach");
    assert_eq!(row("tn")["status"], "ok");
    assert_eq!(row("gone")["status"], "missing");
    assert_eq!(row("gone")["fix"], "cc-moss doctor --fix");
    assert_eq!(row("bad")["status"], "invalid");

    let out = ok(env.cc(&["alias", "ls"]));
    assert!(out.contains("tn") && out.contains("new --detach"), "{out}");
    assert!(out.contains("no link yet"), "{out}");
}

/// Which of bash and zsh this machine has.
fn shells() -> Vec<PathBuf> {
    ["bash", "zsh"]
        .iter()
        .filter_map(|s| {
            let found = ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"]
                .iter()
                .map(|d| Path::new(d).join(s))
                .find(|p| p.exists());
            if found.is_none() {
                eprintln!("{s} is not installed here; skipping it");
            }
            found
        })
        .collect()
}

#[test]
fn works_in_bash_and_zsh() {
    let shells = shells();
    assert!(!shells.is_empty(), "neither bash nor zsh found");
    for shell in shells {
        let env = Env::new();
        let settings = env.path("settings.json");
        ok(env.cc(&["setup", "--yes", "--settings", settings.to_str().unwrap()]));
        // One shell session: `t` from setup, then an alias that works as soon
        // as it is added, then changed and working the new way at once.
        let script = "t --help && \
                      cc-moss alias add mossls 'ls --json' && mossls && \
                      cc-moss alias add mossls new && mossls --help";
        let out = env
            .command(&shell)
            .env("SHELL", &shell)
            .args(["-c", script])
            .output()
            .unwrap();
        let out = ok(out);
        let name = shell.display();
        assert!(
            out.contains("Start with this search"),
            "{name}: t --help\n{out}"
        );
        assert!(out.contains("\"schema\": 1"), "{name}: mossls\n{out}");
        assert!(
            out.contains("Start a named Claude Code session"),
            "{name}: changed alias\n{out}"
        );
    }
}

/// PATH without the fake Homebrew `bin`: no stable cc-moss anywhere.
fn no_entry_path(env: &Env) -> String {
    format!("{}:/usr/bin:/bin", env.home().join(".local/bin").display())
}

#[test]
fn started_through_an_alias_it_links_the_real_binary() {
    let env = Env::new();
    let real = fs::canonicalize(env!("CARGO_BIN_EXE_cc-moss")).unwrap();
    env.write_config("[aliases]\nta = \"alias add\"\nts = \"setup\"\n");
    fs::create_dir_all(env.home().join(".local/bin")).unwrap();
    for name in ["ta", "ts"] {
        symlink(&real, env.link(name)).unwrap();
    }
    let run = |name: &str, args: &[&str]| {
        env.command(env.link(name))
            .env("PATH", no_entry_path(&env))
            .args(args)
            .output()
            .unwrap()
    };

    ok(run("ta", &["zz", "pick"]));
    assert_eq!(
        fs::read_link(env.link("zz")).unwrap(),
        real,
        "not a link to `ta`"
    );
    let doc = env.json(&["alias", "ls", "--json"]);
    let zz = doc["aliases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "zz")
        .unwrap()
        .clone();
    assert_ne!(zz["status"], "foreign", "{zz}");
    ok(env.cc(&["alias", "rm", "zz"]));
    assert!(fs::symlink_metadata(env.link("zz")).is_err());

    // Hooks written through an alias run the real binary, not `ts hook …`.
    let settings = env.path("settings.json");
    ok(run(
        "ts",
        &["--yes", "--settings", settings.to_str().unwrap()],
    ));
    let text = fs::read_to_string(&settings).unwrap();
    assert!(
        text.contains(&format!("{} hook stop", real.display())),
        "{text}"
    );
    assert!(!text.contains("/ts hook"), "{text}");
}
