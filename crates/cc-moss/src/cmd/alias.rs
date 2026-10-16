//! `cc-moss alias`: native aliases. An alias is a link in `~/.local/bin` to
//! the stable `cc-moss`; started under that name, cc-moss runs the command
//! from `[aliases]` in `config.toml` with the user's arguments after it.
//!
//! Safety rules, all enforced here or in `moss_core::aliases`:
//! - the command text is split by our own tokenizer and handed to clap as
//!   argv; no shell ever sees it, and it must start with a cc-moss command;
//! - names are `a-z0-9-_`, so they are safe as file names and shell words;
//! - only links of ours are ever replaced or removed in `~/.local/bin`;
//! - a name that is already a command on `PATH` needs `--force`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use moss_core::aliases::{self, CommandError, LinkState, NameError, Shadow};
use moss_core::config::{config_path, Config};
use serde_json::{json, Value};

use super::files::{self, read_optional, Change, Removal};

pub const JSON_SCHEMA: u32 = 1;

/// The alias `setup` and `install` add: `t` opens the picker.
pub const DEFAULT_NAME: &str = "t";
pub const DEFAULT_COMMAND: &str = "pick";

/// How long the user's shell gets to say what a name means to it.
const SHELL_CHECK: Duration = Duration::from_secs(2);

/// Where aliases live on this machine.
pub struct Place {
    /// `~/.local/bin`.
    pub dir: PathBuf,
    /// The `cc-moss` links point at.
    pub entry: PathBuf,
    /// `config.toml`, through any symlink a dotfile manager made.
    pub config: PathBuf,
}

impl Place {
    pub fn detect() -> Result<Place> {
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .context("HOME is not set")?;
        // The same spelling install uses for the other files (macOS's /var
        // is /private/var).
        let home = std::fs::canonicalize(&home).unwrap_or(home);
        let exe = std::env::current_exe().context("cannot locate the cc-moss executable")?;
        let config = config_path();
        Ok(Place {
            dir: aliases::link_dir(&home),
            entry: aliases::stable_entry(&exe, std::env::var_os("PATH").as_deref()),
            config: std::fs::canonicalize(&config).unwrap_or(config),
        })
    }

    pub fn link(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Whether this process's `PATH` holds `~/.local/bin`.
    pub fn dir_on_path(&self) -> bool {
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let dir = canon(&self.dir);
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d == self.dir || canon(&d) == dir))
    }
}

/// cc-moss's own commands, the only things an alias may run.
fn subcommands() -> Vec<String> {
    crate::subcommands()
}

/// The arguments an alias stands for, when cc-moss was started as `name`.
pub fn expand_invocation(name: &str) -> Result<Vec<String>> {
    let unknown = || {
        anyhow::anyhow!(tr!(
            format!("`{name}` is not one of your aliases (see `cc-moss alias ls`)"),
            format!("`{name}` 不是你设置过的别名（看看 `cc-moss alias ls`）")
        ))
    };
    if aliases::check_name(name).is_err() {
        return Err(unknown());
    }
    let config = Config::load()?;
    let command = config.aliases.get(name).ok_or_else(unknown)?;
    let subs = subcommands();
    let subs: Vec<&str> = subs.iter().map(String::as_str).collect();
    aliases::expand(command, &subs).map_err(|e| {
        anyhow::anyhow!(tr!(
            format!(
                "alias `{name}` in {}: {}; change it with `cc-moss alias add {name} <command>`",
                config_path().display(),
                command_error(&e)
            ),
            format!(
                "{} 里的别名 `{name}`：{}；用 `cc-moss alias add {name} <命令>` 改一下",
                config_path().display(),
                command_error(&e)
            )
        ))
    })
}

fn name_error(e: &NameError) -> String {
    match e {
        NameError::Empty => tr!("an alias needs a name", "别名得有个名字").to_string(),
        NameError::TooLong => tr!(
            "alias names are at most 64 characters",
            "别名最长 64 个字符"
        )
        .to_string(),
        NameError::Reserved => tr!(
            "that name belongs to cc-moss itself",
            "这个名字是 cc-moss 自己的"
        )
        .to_string(),
        NameError::BadChar(c) => tr!(
            format!("`{c}` is not allowed: use a-z, 0-9, - and _"),
            format!("不能用 `{c}`：只能用 a-z、0-9、- 和 _")
        ),
        NameError::LeadingDash => {
            tr!("an alias name cannot start with -", "别名不能以 - 开头").to_string()
        }
    }
}

fn command_error(e: &CommandError) -> String {
    match e {
        CommandError::Split(aliases::SplitError::Unclosed(q)) => tr!(
            format!("the {q} quote is never closed"),
            format!("{q} 引号没有配对")
        ),
        CommandError::Split(aliases::SplitError::TrailingBackslash) => tr!(
            "it ends with a lone \\".to_string(),
            "结尾多了一个 \\".to_string()
        ),
        CommandError::Empty => tr!("the command is empty", "命令是空的").to_string(),
        CommandError::Hook => tr!(
            "`hook` is only for Claude Code",
            "`hook` 只给 Claude Code 用"
        )
        .to_string(),
        CommandError::Unknown(w) => tr!(
            format!(
                "`{w}` is not a cc-moss command (aliases run cc-moss commands, not shell commands)"
            ),
            format!("`{w}` 不是 cc-moss 的命令（别名只能跑 cc-moss 的命令，不能跑 shell 命令）")
        ),
    }
}

/// Why `add` or `rm` said no, with a code the app can act on.
struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Failure {
        Failure {
            code,
            message: message.into(),
        }
    }

    fn other(e: anyhow::Error) -> Failure {
        Failure::new("error", format!("{e:#}"))
    }
}

pub enum Action {
    Ls {
        json: bool,
    },
    Add {
        name: String,
        command: Vec<String>,
        force: bool,
        json: bool,
    },
    Rm {
        name: String,
        json: bool,
    },
}

pub fn run(action: Action) -> ExitCode {
    let (json, result) = match action {
        Action::Ls { json } => (json, ls(json)),
        Action::Add {
            name,
            command,
            force,
            json,
        } => (json, add(&name, &command, force, json)),
        Action::Rm { name, json } => (json, rm(&name, json)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            if json {
                print_json(&json!({
                    "schema": JSON_SCHEMA,
                    "ok": false,
                    "code": f.code,
                    "error": f.message,
                }));
            } else {
                eprintln!("cc-moss: {}", f.message);
            }
            ExitCode::FAILURE
        }
    }
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

/// One alias, as `ls` and `doctor` see it.
pub struct Row {
    pub name: String,
    pub command: String,
    /// Why the command cannot run, if it cannot.
    pub invalid: Option<String>,
    pub link: LinkState,
    pub shadow: Shadow,
}

impl Row {
    /// The one word the app shows, most serious first.
    pub fn status(&self) -> &'static str {
        if self.invalid.is_some() {
            return "invalid";
        }
        match (self.link, &self.shadow) {
            (LinkState::Foreign, _) => "foreign",
            (LinkState::Broken, _) => "broken",
            (LinkState::Missing, _) => "missing",
            (LinkState::Stale, _) => "stale",
            (_, Shadow::Alias | Shadow::Function | Shadow::Builtin | Shadow::Command(_)) => {
                "shadowed"
            }
            _ => "ok",
        }
    }

    /// What is wrong, in a sentence, and how to fix it. `None` when all is well.
    pub fn problem(&self, place: &Place) -> Option<(String, String)> {
        let name = &self.name;
        let link = place.link(name);
        if let Some(why) = &self.invalid {
            return Some((
                why.clone(),
                format!("cc-moss alias add {name} <{}>", tr!("command", "命令")),
            ));
        }
        let fix = "cc-moss doctor --fix".to_string();
        match self.link {
            LinkState::Foreign => {
                return Some((
                    tr!(
                        format!(
                            "{} is not cc-moss's, so the alias has no link",
                            link.display()
                        ),
                        format!("{} 不是 cc-moss 的，别名没法建链接", link.display())
                    ),
                    tr!(
                        format!(
                            "move that file away, or pick another name: cc-moss alias rm {name}"
                        ),
                        format!("把那个文件挪走，或者换个名字：cc-moss alias rm {name}")
                    ),
                ))
            }
            LinkState::Broken => {
                return Some((
                    tr!(
                        "its link points at a cc-moss that is gone".to_string(),
                        "链接指向的 cc-moss 不见了".to_string()
                    ),
                    fix,
                ))
            }
            LinkState::Missing => {
                return Some((
                    tr!("no link yet".to_string(), "还没有链接".to_string()),
                    fix,
                ))
            }
            LinkState::Stale => {
                return Some((
                    tr!(
                        "its link points at a versioned cc-moss that the next upgrade removes"
                            .to_string(),
                        "链接指向带版本号的 cc-moss，下次升级就会失效".to_string()
                    ),
                    fix,
                ))
            }
            LinkState::Ok => {}
        }
        let remove_theirs = tr!(
            format!("remove that `{name}` from your shell's startup files"),
            format!("把 shell 启动文件里的那个 `{name}` 删掉")
        );
        match &self.shadow {
            Shadow::Alias => Some((
                tr!(
                    "hidden by a shell alias of the same name".to_string(),
                    "被同名的 shell alias 遮住了".to_string()
                ),
                remove_theirs,
            )),
            Shadow::Function => Some((
                tr!(
                    "hidden by a shell function of the same name".to_string(),
                    "被同名的 shell 函数遮住了".to_string()
                ),
                remove_theirs,
            )),
            Shadow::Builtin => Some((
                tr!(
                    "hidden by a shell builtin of the same name".to_string(),
                    "被同名的 shell 内建命令遮住了".to_string()
                ),
                tr!(
                    format!("pick another name: cc-moss alias rm {name}"),
                    format!("换个名字：cc-moss alias rm {name}")
                ),
            )),
            Shadow::Command(p) => Some((
                tr!(
                    format!("{} comes first on your PATH", p.display()),
                    format!("PATH 里排在前面的 {} 会先被执行", p.display())
                ),
                tr!(
                    format!(
                        "pick another name, or put {} earlier on PATH",
                        place.dir.display()
                    ),
                    format!("换个名字，或者把 {} 放到 PATH 更前面", place.dir.display())
                ),
            )),
            Shadow::None | Shadow::NotFound | Shadow::Unknown => None,
        }
    }

    fn to_json(&self, place: &Place) -> Value {
        let problem = self.problem(place);
        json!({
            "name": self.name,
            "command": self.command,
            "status": self.status(),
            "link": self.link.as_str(),
            "shadow": self.shadow.as_str(),
            "detail": problem.as_ref().map(|p| &p.0),
            "fix": problem.as_ref().map(|p| &p.1),
        })
    }
}

/// Every alias in the config, with its link and what the shell makes of it.
/// `check_shell` asks the user's shell (up to two seconds).
pub fn rows(place: &Place, config: &Config, check_shell: bool) -> Vec<Row> {
    let subs = subcommands();
    let subs: Vec<&str> = subs.iter().map(String::as_str).collect();
    let mut rows: Vec<Row> = config
        .aliases
        .iter()
        .map(|(name, command)| {
            let invalid = match aliases::check_name(name) {
                Err(e) => Some(name_error(&e)),
                Ok(()) => aliases::expand(command, &subs)
                    .err()
                    .map(|e| command_error(&e)),
            };
            let link = if invalid.is_some() && aliases::check_name(name).is_err() {
                LinkState::Missing
            } else {
                aliases::link_state(&place.link(name), &place.entry)
            };
            Row {
                name: name.clone(),
                command: command.clone(),
                invalid,
                link,
                shadow: Shadow::Unknown,
            }
        })
        .collect();
    if check_shell {
        let names: Vec<&str> = rows
            .iter()
            .filter(|r| r.link == LinkState::Ok)
            .map(|r| r.name.as_str())
            .collect();
        let answers = shadows(&names, &place.dir);
        for row in &mut rows {
            if let Some((_, s)) = answers.iter().find(|(n, _)| *n == row.name) {
                row.shadow = s.clone();
            }
        }
    }
    rows
}

/// Ask the user's login shell (`$SHELL -ic`) what each name means to it, in
/// one run with a two-second limit. Names reach the script only after
/// [`aliases::check_name`], so they are plain `a-z0-9-_` words.
pub fn shadows(names: &[&str], dir: &Path) -> Vec<(String, Shadow)> {
    let unknown = || {
        names
            .iter()
            .map(|n| (n.to_string(), Shadow::Unknown))
            .collect()
    };
    if names.is_empty() {
        return Vec::new();
    }
    let Some(shell) = std::env::var_os("SHELL").filter(|s| Path::new(s).is_absolute()) else {
        return unknown();
    };
    let mut cmd = std::process::Command::new(shell);
    cmd.arg("-ic").arg(aliases::type_script(names));
    match moss_core::command::run_with_timeout(&mut cmd, SHELL_CHECK) {
        Ok(out) => aliases::parse_type_output(&String::from_utf8_lossy(&out.stdout), names, dir),
        Err(_) => unknown(),
    }
}

fn ls(as_json: bool) -> Result<(), Failure> {
    let place = Place::detect().map_err(Failure::other)?;
    let config = Config::load().map_err(Failure::other)?;
    let rows = rows(&place, &config, true);
    if as_json {
        print_json(&json!({
            "schema": JSON_SCHEMA,
            "ok": true,
            "dir": place.dir,
            "entry": place.entry,
            "config": place.config,
            "dir_on_path": place.dir_on_path(),
            "aliases": rows.iter().map(|r| r.to_json(&place)).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if rows.is_empty() {
        println!(
            "{}",
            tr!(
                "No aliases yet. Add one: cc-moss alias add <name> <command>, e.g. cc-moss alias add mycc new --dangerously-skip-permissions",
                "还没有别名。加一个：cc-moss alias add <名字> <命令>，比如 cc-moss alias add mycc new --dangerously-skip-permissions"
            )
        );
        return Ok(());
    }
    let width = rows.iter().map(|r| r.name.len()).max().unwrap_or(0) + 2;
    let cmd_width = rows
        .iter()
        .map(|r| unicode_width::UnicodeWidthStr::width(r.command.as_str()))
        .max()
        .unwrap_or(0)
        + 2;
    for r in &rows {
        let state = match r.problem(&place) {
            None => "✓".to_string(),
            Some((what, fix)) => format!("! {what} → {fix}"),
        };
        println!(
            "{}{}{state}",
            super::pad(&r.name, width),
            super::pad(&r.command, cmd_width)
        );
    }
    println!(
        "\n{}",
        tr!(
            format!(
                "Links live in {} and point at {}.",
                place.dir.display(),
                place.entry.display()
            ),
            format!(
                "链接放在 {}，指向 {}。",
                place.dir.display(),
                place.entry.display()
            )
        )
    );
    if let Some(hint) = path_hint(&place) {
        println!("{hint}");
    }
    Ok(())
}

/// What to say when `~/.local/bin` is not on `PATH`.
pub fn path_hint(place: &Place) -> Option<String> {
    if place.dir_on_path() {
        return None;
    }
    Some(tr!(
        format!(
            "{dir} is not on your PATH yet, so your shell cannot find the aliases. `cc-moss install` adds it for zsh; for other shells add `export PATH=\"$HOME/.local/bin:$PATH\"` to their startup file.",
            dir = place.dir.display()
        ),
        format!(
            "{dir} 还不在 PATH 里，shell 找不到这些别名。zsh 用户运行 `cc-moss install` 会自动加上；其他 shell 在启动文件里加一行 `export PATH=\"$HOME/.local/bin:$PATH\"`。",
            dir = place.dir.display()
        )
    ))
}

/// The command text for `alias add NAME WORDS…`: one argument is command
/// text as written (`"new --x 'my name'"`, what the app sends); several
/// are words, quoted as needed so they come back the same.
fn command_text(words: &[String]) -> String {
    match words {
        [one] => one.trim().to_string(),
        many => aliases::join(many),
    }
}

/// The first `name` on `PATH` that is not the alias link itself or another
/// link of ours.
fn on_path_elsewhere(name: &str, link: &Path) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(name))
        .filter(|c| c != link && !aliases::is_ours(c))
        .find(|c| std::fs::symlink_metadata(c).is_ok())
}

fn add(name: &str, words: &[String], force: bool, as_json: bool) -> Result<(), Failure> {
    aliases::check_name(name).map_err(|e| Failure::new("bad_name", name_error(&e)))?;
    let command = command_text(words);
    let subs = subcommands();
    let subs: Vec<&str> = subs.iter().map(String::as_str).collect();
    aliases::expand(&command, &subs).map_err(|e| Failure::new("bad_command", command_error(&e)))?;
    let place = Place::detect().map_err(Failure::other)?;
    let link = place.link(name);
    if aliases::link_state(&link, &place.entry) == LinkState::Foreign {
        return Err(Failure::new(
            "foreign",
            tr!(
                format!(
                    "{} already exists and is not cc-moss's; cc-moss never replaces it. Move it away or pick another name.",
                    link.display()
                ),
                format!(
                    "{} 已经存在，而且不是 cc-moss 的；cc-moss 不会替换它。把它挪走，或者换个名字。",
                    link.display()
                )
            ),
        ));
    }
    if let Some(other) = on_path_elsewhere(name, &link) {
        if !force {
            return Err(Failure::new(
                "exists",
                tr!(
                    format!(
                        "`{name}` is already a command ({}). Pick another name, or add --force to use it anyway.",
                        other.display()
                    ),
                    format!(
                        "`{name}` 已经是一个命令了（{}）。换个名字，或者加上 --force 坚持用它。",
                        other.display()
                    )
                ),
            ));
        }
    }

    let original = read_optional(&place.config).map_err(Failure::other)?;
    let existed = Config::load()
        .map(|c| c.aliases.contains_key(name))
        .unwrap_or(false);
    let text = aliases::set_in_config(original.as_deref().unwrap_or(""), name, &command)
        .map_err(|e| Failure::new("config", format!("{}: {e:#}", place.config.display())))?;
    files::write_atomically(&place.config, text.as_bytes()).map_err(Failure::other)?;
    if let Err(e) = aliases::make_link(&link, &place.entry) {
        // Put the config back as it was: no alias without its link.
        let _ = match &original {
            Some(old) => files::write_atomically(&place.config, old.as_bytes()),
            None => std::fs::remove_file(&place.config).map_err(Into::into),
        };
        return Err(Failure::other(e));
    }

    let shadow = shadows(&[name], &place.dir)
        .pop()
        .map_or(Shadow::Unknown, |(_, s)| s);
    let row = Row {
        name: name.to_string(),
        command: command.clone(),
        invalid: None,
        link: LinkState::Ok,
        shadow,
    };
    if as_json {
        print_json(&json!({
            "schema": JSON_SCHEMA,
            "ok": true,
            "updated": existed,
            "alias": row.to_json(&place),
            "dir_on_path": place.dir_on_path(),
        }));
        return Ok(());
    }
    println!(
        "{}",
        if existed {
            tr!(
                format!("Updated `{name}`: it now runs cc-moss {command}."),
                format!("已更新 `{name}`：现在它会运行 cc-moss {command}。")
            )
        } else {
            tr!(
                format!(
                    "Added `{name}`: it runs cc-moss {command}. Works right away in any shell."
                ),
                format!("已添加 `{name}`：它会运行 cc-moss {command}。任何 shell 里马上就能用。")
            )
        }
    );
    if let Some((what, fix)) = row.problem(&place) {
        println!(
            "{}",
            tr!(
                format!("Heads-up: `{name}` is {what}. To use the alias, {fix}."),
                format!("注意：`{name}` {what}。想用这个别名的话，{fix}。")
            )
        );
    }
    if let Some(hint) = path_hint(&place) {
        println!("{hint}");
    }
    Ok(())
}

fn rm(name: &str, as_json: bool) -> Result<(), Failure> {
    // A name that could never be an alias is not looked up as a path either.
    let not_found = || {
        Failure::new(
            "not_found",
            tr!(
                format!("there is no alias `{name}` (see `cc-moss alias ls`)"),
                format!("没有叫 `{name}` 的别名（看看 `cc-moss alias ls`）")
            ),
        )
    };
    aliases::check_name(name).map_err(|_| not_found())?;
    let place = Place::detect().map_err(Failure::other)?;
    let original = read_optional(&place.config).map_err(Failure::other)?;
    let mut in_config = false;
    if let Some(text) = &original {
        let (new_text, found) = aliases::remove_from_config(text, name)
            .map_err(|e| Failure::new("config", format!("{}: {e:#}", place.config.display())))?;
        if found {
            files::write_atomically(&place.config, new_text.as_bytes()).map_err(Failure::other)?;
            in_config = true;
        }
    }
    let link = place.link(name);
    let unlinked = aliases::remove_link(&link)
        .with_context(|| format!("cannot remove {}", link.display()))
        .map_err(Failure::other)?;
    if !in_config && !unlinked {
        return Err(not_found());
    }
    let left_alone = std::fs::symlink_metadata(&link).is_ok();
    if as_json {
        print_json(&json!({
            "schema": JSON_SCHEMA,
            "ok": true,
            "name": name,
        }));
        return Ok(());
    }
    println!(
        "{}",
        tr!(format!("Removed `{name}`."), format!("已删除 `{name}`。"))
    );
    if left_alone {
        println!(
            "{}",
            tr!(
                format!("{} is not cc-moss's, so it stays.", link.display()),
                format!("{} 不是 cc-moss 的，原样保留。", link.display())
            )
        );
    }
    Ok(())
}

/// Recreate the links of every alias whose link is missing, broken or
/// stale. Never touches a file that is not ours. Returns what it did.
pub fn fix_links(place: &Place, config: &Config) -> Vec<String> {
    let mut done = Vec::new();
    for row in rows(place, config, false) {
        if row.invalid.is_some()
            || !matches!(
                row.link,
                LinkState::Missing | LinkState::Broken | LinkState::Stale
            )
        {
            continue;
        }
        let link = place.link(&row.name);
        done.push(match aliases::make_link(&link, &place.entry) {
            Ok(()) => tr!(
                format!("Fixed: {} → {}", link.display(), place.entry.display()),
                format!("已修复：{} → {}", link.display(), place.entry.display())
            ),
            Err(e) => tr!(
                format!("Could not fix {}: {e:#}", link.display()),
                format!("没能修复 {}：{e:#}", link.display())
            ),
        });
    }
    done
}

/// What `setup` and `install` change for the `t` alias: the config entry
/// (unless `t` is already there, maybe pointing elsewhere: the user's
/// choice wins) and its link. Notes explain what was left alone.
pub fn default_changes(place: &Place) -> Result<(Vec<Change>, Vec<String>)> {
    let mut changes = Vec::new();
    let mut notes = Vec::new();
    let original = read_optional(&place.config)?;
    let config = match &original {
        Some(_) => Config::load_from(&place.config)?,
        None => Config::default(),
    };
    let link = place.link(DEFAULT_NAME);
    let state = aliases::link_state(&link, &place.entry);
    if state == LinkState::Foreign {
        notes.push(tr!(
            format!(
                "{} already exists and is not cc-moss's, so there is no `t` shortcut. Pick another name: cc-moss alias add <name> pick",
                link.display()
            ),
            format!(
                "{} 已经存在，而且不是 cc-moss 的，所以没加 `t`。换个名字：cc-moss alias add <名字> pick",
                link.display()
            )
        ));
        return Ok((changes, notes));
    }
    if !config.aliases.contains_key(DEFAULT_NAME) {
        let text = aliases::set_in_config(
            original.as_deref().unwrap_or(""),
            DEFAULT_NAME,
            DEFAULT_COMMAND,
        )
        .with_context(|| format!("{}", place.config.display()))?;
        changes.push(Change {
            path: place.config.clone(),
            original,
            new_text: text,
            remove: Removal::No,
            link: None,
            done: tr!(
                format!("Added the `t` alias to {}.", place.config.display()),
                format!("已在 {} 加好 `t` 别名。", place.config.display())
            ),
        });
    }
    if state != LinkState::Ok {
        changes.push(Change {
            path: link.clone(),
            original: None,
            new_text: String::new(),
            remove: Removal::No,
            link: Some(place.entry.clone()),
            done: tr!(
                format!(
                    "Linked {} → {}: type `t` in any shell.",
                    link.display(),
                    place.entry.display()
                ),
                format!(
                    "已建好链接 {} → {}：在任何 shell 里敲 `t` 都行。",
                    link.display(),
                    place.entry.display()
                )
            ),
        });
    }
    if !changes.is_empty() {
        notes.extend(path_hint(place));
    }
    Ok((changes, notes))
}

/// What `install --uninstall` removes: every alias link of ours, and the
/// `t = "pick"` entry it added (the file too, if nothing else is left).
/// The user's own aliases stay in the config.
pub fn uninstall_changes(place: &Place) -> Result<(Vec<Change>, Vec<String>)> {
    let mut changes = Vec::new();
    let mut notes = Vec::new();
    let original = read_optional(&place.config)?;
    let config = match &original {
        Some(_) => Config::load_from(&place.config)?,
        None => Config::default(),
    };
    let mut names: Vec<&str> = config.aliases.keys().map(String::as_str).collect();
    if !names.contains(&DEFAULT_NAME) {
        names.push(DEFAULT_NAME);
    }
    for name in names {
        if aliases::check_name(name).is_err() {
            continue;
        }
        let link = place.link(name);
        if aliases::is_ours(&link) {
            changes.push(Change {
                path: link.clone(),
                original: None,
                new_text: String::new(),
                remove: Removal::Verbatim,
                link: std::fs::read_link(&link).ok(),
                done: tr!(
                    format!("Removed the link {}.", link.display()),
                    format!("已删除链接 {}。", link.display())
                ),
            });
        }
    }
    if let (Some(text), Some(DEFAULT_COMMAND)) = (
        &original,
        config.aliases.get(DEFAULT_NAME).map(String::as_str),
    ) {
        let (rest, _) = aliases::remove_from_config(text, DEFAULT_NAME)
            .with_context(|| format!("{}", place.config.display()))?;
        let only_ours = rest.trim() == "[aliases]" || rest.trim().is_empty();
        changes.push(if only_ours {
            Change {
                path: place.config.clone(),
                original: Some(text.clone()),
                new_text: String::new(),
                remove: if *text == aliases::set_in_config("", DEFAULT_NAME, DEFAULT_COMMAND)? {
                    Removal::Verbatim
                } else {
                    Removal::WithBackup
                },
                link: None,
                done: tr!(
                    format!("Deleted {}.", place.config.display()),
                    format!("已删除 {}。", place.config.display())
                ),
            }
        } else {
            Change {
                path: place.config.clone(),
                original: Some(text.clone()),
                new_text: rest,
                remove: Removal::No,
                link: None,
                done: tr!(
                    format!("Removed the `t` alias from {}.", place.config.display()),
                    format!("已从 {} 移除 `t` 别名。", place.config.display())
                ),
            }
        });
    }
    let kept: Vec<&str> = config
        .aliases
        .keys()
        .map(String::as_str)
        .filter(|n| *n != DEFAULT_NAME)
        .collect();
    if !kept.is_empty() {
        notes.push(tr!(
            format!(
                "Kept your aliases in {} ({}); their links go. `cc-moss doctor --fix` brings them back.",
                place.config.display(),
                kept.join(", ")
            ),
            format!(
                "保留了 {} 里你的别名（{}），只删掉它们的链接；`cc-moss doctor --fix` 可以把链接再建回来。",
                place.config.display(),
                kept.join("、")
            )
        ));
    }
    Ok((changes, notes))
}
