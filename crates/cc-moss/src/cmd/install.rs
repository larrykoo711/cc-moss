//! `cc-moss install`: a complete terminal setup in one go. The Claude Code
//! hooks and `t` (as `setup` does), a tmux configuration, the starship
//! prompt and a few shell blocks, then the tools they need.
//!
//! Nothing of the user's is overwritten: tmux loads cc-moss's file from a
//! block at the top of their own config, an existing starship config is
//! kept, and shell additions live in managed blocks.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use moss_core::managed_block;
use moss_core::platform::{install_commands, on_path, without_sudo, Platform, Tool};

use super::files::{self, read_optional, Change, Removal};
use super::{alias, app, confirm, setup};

pub const TMUX_CONF: &str = include_str!("../../assets/tmux.conf");
pub const STARSHIP_TOML: &str = include_str!("../../assets/starship.toml");
pub const PROMPT_ZSH: &str = include_str!("../../assets/zsh/prompt.zsh");
pub const SSH_TTY_ZSH: &str = include_str!("../../assets/zsh/ssh-tty-restore.zsh");
pub const PATH_ZSH: &str = include_str!("../../assets/zsh/path.zsh");
pub const ALIASES_ZSH: &str = include_str!("../../assets/zsh/aliases.zsh");

pub const TAG_TMUX: &str = "cc-moss";
pub const TAG_PROMPT: &str = "cc-moss: prompt";
pub const TAG_SSH: &str = "cc-moss: ssh-tty-restore";
pub const TAG_PATH: &str = "cc-moss: path";
pub const TAG_ALIASES: &str = "cc-moss: aliases";

/// What `install` can set up: hooks (Claude Code hooks and `t`), tmux,
/// starship, shell (ssh reset, PATH for scripts), the menubar app (macOS)
/// and the optional aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Part {
    Hooks,
    Tmux,
    Starship,
    Shell,
    Aliases,
    App,
}

pub struct Options {
    pub dry_run: bool,
    pub yes: bool,
    pub with: Vec<Part>,
    pub skip: Vec<Part>,
    pub replace_starship: bool,
}

impl Options {
    fn wants(&self, part: Part) -> bool {
        !self.skip.contains(&part) && (part != Part::Aliases || self.with.contains(&part))
    }
}

/// Where everything lives, from `HOME`, `ZDOTDIR`, `XDG_CONFIG_HOME` and
/// `STARSHIP_CONFIG`.
///
/// `.zshenv` is always `~/.zshenv`: ZDOTDIR is usually set *in* that file, and
/// `ssh host cmd` (the shell this file is for) never sees it otherwise.
pub struct Paths {
    pub zshrc: PathBuf,
    pub zshenv: PathBuf,
    /// The user's own tmux config.
    pub tmux_conf: PathBuf,
    /// cc-moss's tmux config, loaded from the user's.
    pub moss_tmux_conf: PathBuf,
    pub starship: PathBuf,
}

impl Paths {
    pub fn detect() -> Result<Paths> {
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = env("HOME").context("HOME is not set")?;
        let zdot = env("ZDOTDIR").unwrap_or_else(|| home.clone());
        let config = env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let classic = home.join(".tmux.conf");
        let xdg = config.join("tmux").join("tmux.conf");
        let tmux_conf = if !classic.exists() && xdg.exists() {
            xdg
        } else {
            classic
        };
        Ok(Paths {
            zshrc: real(zdot.join(".zshrc")),
            zshenv: real(home.join(".zshenv")),
            tmux_conf: real(tmux_conf),
            moss_tmux_conf: real(config.join("cc-moss").join("tmux.conf")),
            starship: real(env("STARSHIP_CONFIG").unwrap_or_else(|| config.join("starship.toml"))),
        })
    }
}

/// The login shell's name (`$SHELL`'s last part), if set.
pub fn login_shell() -> Option<String> {
    let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
    Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// Whether to manage the zsh files: zsh is the login shell (or `$SHELL` says
/// nothing), or the user already has a `.zshrc` or `.zshenv`.
pub fn zsh_in_use(paths: &Paths) -> bool {
    login_shell().as_deref().unwrap_or("zsh") == "zsh"
        || paths.zshrc.exists()
        || paths.zshenv.exists()
}

/// Write through symlinks (dotfile managers) instead of replacing them,
/// including links whose target does not exist yet.
fn real(path: PathBuf) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(&path) {
        return p;
    }
    match std::fs::read_link(&path) {
        Ok(target) if target.is_absolute() => target,
        Ok(target) => path.parent().map_or(target.clone(), |dir| dir.join(target)),
        // Not there yet: resolve the nearest existing directory instead, so
        // the path is the same before and after the file is created.
        Err(_) => {
            let mut rest = Vec::new();
            let mut dir = path.as_path();
            while let Some(parent) = dir.parent() {
                rest.push(dir.file_name().unwrap_or_default().to_owned());
                if let Ok(real_dir) = std::fs::canonicalize(parent) {
                    return rest.iter().rev().fold(real_dir, |p, part| p.join(part));
                }
                dir = parent;
            }
            path
        }
    }
}

/// cc-moss's tmux config, reloading from the user's own config.
pub fn rendered_tmux_conf(paths: &Paths) -> String {
    // Inside tmux double quotes, \ " and $ need a backslash.
    let conf = paths
        .tmux_conf
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");
    TMUX_CONF.replace("{{USER_CONF}}", &conf)
}

/// The block at the top of the user's tmux config.
pub fn tmux_block(paths: &Paths) -> String {
    format!(
        "# Managed by `cc-moss install`. Settings below this block win.\nsource-file \"{}\"\n",
        paths.moss_tmux_conf.display()
    )
}

/// Whether `zshrc` starts starship by hand (outside cc-moss's block).
pub fn starts_starship_by_hand(zshrc: &str) -> bool {
    managed_block::remove(zshrc, TAG_PROMPT)
        .unwrap_or_else(|_| zshrc.to_string())
        .lines()
        .any(|l| l.contains("starship init") && !l.trim_start().starts_with('#'))
}

/// `.zshrc` with every wanted block in place. The prompt block goes first so
/// that the others land before it: starship must run last. A cc-moss block
/// found after starship init (moved by hand, or from an older layout) is
/// taken out and put back before it.
fn zshrc_text(original: &str, opts: &Options) -> Result<String> {
    let mut text = original.to_string();
    if let Some(init) = managed_block::starship_init_line(&text) {
        for (line, tag) in managed_block::tags(&text) {
            if line > init && tag != TAG_PROMPT && is_ours(&tag) {
                text = managed_block::remove(&text, &tag)?;
            }
        }
    }
    if opts.wants(Part::Starship) && !starts_starship_by_hand(&text) {
        text = managed_block::upsert(&text, TAG_PROMPT, PROMPT_ZSH)?;
    }
    // `t` used to be a function in this block; it is a native alias now.
    if opts.wants(Part::Hooks) {
        text = managed_block::remove(&text, setup::ZSH_TAG)?;
    }
    if opts.wants(Part::Shell) {
        text = managed_block::upsert(&text, TAG_SSH, SSH_TTY_ZSH)?;
    }
    if opts.wants(Part::Aliases) {
        text = managed_block::upsert(&text, TAG_ALIASES, ALIASES_ZSH)?;
    }
    Ok(text)
}

/// What a user whose login shell is not zsh gets instead of the zsh blocks.
fn not_zsh_note(shell: &str) -> String {
    tr!(
        format!(
            "Your login shell is {shell}: the shell blocks are zsh-only for now, so no zsh files were created. `t` is a plain command in ~/.local/bin and works in {shell} too. For the starship prompt, add starship's init line for {shell} yourself (https://starship.rs)."
        ),
        format!(
            "你的登录 shell 是 {shell}：shell 配置块暂时只支持 zsh，所以没有创建 zsh 的文件。`t` 是 ~/.local/bin 里的普通命令，在 {shell} 里也能用。想用 starship 提示符的话，自己把 {shell} 对应的 starship init 那一行加上（https://starship.rs）。"
        )
    )
}

/// The managed blocks install writes into `.zshrc`.
pub fn is_ours(tag: &str) -> bool {
    [setup::ZSH_TAG, TAG_PROMPT, TAG_SSH, TAG_ALIASES].contains(&tag)
}

/// A change to `path`, if `new_text` differs from what is there.
pub(super) fn change(path: &Path, new_text: String) -> Result<Option<Change>> {
    let original = read_optional(path)?;
    if original.as_deref().unwrap_or_default() == new_text {
        return Ok(None);
    }
    let done = if original.is_some() {
        tr!(
            format!("Updated {}.", path.display()),
            format!("已更新 {}。", path.display())
        )
    } else {
        tr!(
            format!("Created {}.", path.display()),
            format!("已创建 {}。", path.display())
        )
    };
    Ok(Some(Change {
        remove: Removal::No,
        link: None,
        path: path.to_path_buf(),
        original,
        new_text,
        done,
    }))
}

/// Every change install would make, each with the part it belongs to,
/// and what to tell the user. Shared by `install` and `install --dry-run --json`.
struct Gathered {
    changes: Vec<(Part, Change)>,
    notes: Vec<String>,
    /// The user's own starship config stays (no --replace-starship).
    kept_starship: bool,
}

fn tag(part: Part, changes: impl IntoIterator<Item = Change>) -> Vec<(Part, Change)> {
    changes.into_iter().map(|c| (part, c)).collect()
}

fn gather(opts: &Options, paths: &Paths, quiet: bool) -> Result<Gathered> {
    let mut notes = Vec::new();
    let mut changes = Vec::new();
    let mut kept_starship = false;

    if opts.wants(Part::Hooks) {
        let setup_opts = setup::Options {
            dry_run: opts.dry_run,
            yes: opts.yes,
            uninstall: false,
            settings: None,
            zshrc: None,
            quiet,
        };
        changes.extend(tag(Part::Hooks, setup::settings_change(&setup_opts)?));
        let place = alias::Place::detect()?;
        let (alias_changes, alias_notes) = alias::default_changes(&place)?;
        // zsh users get ~/.local/bin on PATH from the shell part's .zshenv block.
        if !alias_changes.is_empty() && !(zsh_in_use(paths) && opts.wants(Part::Shell)) {
            notes.extend(alias::path_hint(&place));
        }
        changes.extend(tag(Part::Hooks, alias_changes));
        notes.extend(alias_notes);
    }

    let in_file = |p: &Path| format!("{}", p.display());
    let zsh = zsh_in_use(paths);
    if zsh {
        let zshrc = read_optional(&paths.zshrc)?.unwrap_or_default();
        let text = zshrc_text(&zshrc, opts).with_context(|| in_file(&paths.zshrc))?;
        changes.extend(tag(Part::Shell, change(&paths.zshrc, text)?));
    } else {
        notes.push(not_zsh_note(&login_shell().unwrap_or_default()));
    }

    if zsh && opts.wants(Part::Shell) {
        let zshenv = read_optional(&paths.zshenv)?.unwrap_or_default();
        let text = managed_block::upsert(&zshenv, TAG_PATH, PATH_ZSH)
            .with_context(|| in_file(&paths.zshenv))?;
        changes.extend(tag(Part::Shell, change(&paths.zshenv, text)?));
    }

    if opts.wants(Part::Tmux) {
        changes.extend(tag(
            Part::Tmux,
            change(&paths.moss_tmux_conf, rendered_tmux_conf(paths))?,
        ));
        let own = read_optional(&paths.tmux_conf)?.unwrap_or_default();
        let text = managed_block::upsert_top(&own, TAG_TMUX, &tmux_block(paths))
            .with_context(|| in_file(&paths.tmux_conf))?;
        changes.extend(tag(Part::Tmux, change(&paths.tmux_conf, text)?));
    }

    if opts.wants(Part::Starship) {
        match read_optional(&paths.starship)? {
            Some(theirs) if theirs != STARSHIP_TOML && !opts.replace_starship => {
                kept_starship = true;
                notes.push(
                tr!(
                    format!(
                        "Kept your starship config ({}). Add --replace-starship to use cc-moss's instead.",
                        paths.starship.display()
                    ),
                    format!(
                        "保留了你自己的 starship 配置（{}）。想换成 cc-moss 的，加上 --replace-starship。",
                        paths.starship.display()
                    )
                ),
            )
            }
            _ => changes.extend(tag(
                Part::Starship,
                change(&paths.starship, STARSHIP_TOML.to_string())?,
            )),
        }
    }

    Ok(Gathered {
        changes,
        notes,
        kept_starship,
    })
}

/// `install --dry-run --json`: each part ready, or what installing it would
/// change, for the app's welcome. Changes nothing.
pub fn plan_json(opts: &Options) -> Result<()> {
    let paths = Paths::detect()?;
    let g = gather(opts, &paths, true)?;
    let missing = missing_tools(opts);
    let zsh = zsh_in_use(&paths);
    let home = std::env::var("HOME").unwrap_or_default();
    let short = |p: &Path| {
        let s = p.display().to_string();
        match s.strip_prefix(&home) {
            Some(rest) if !home.is_empty() => format!("~{rest}"),
            _ => s,
        }
    };
    let mut parts = Vec::new();
    for part in [
        Part::Hooks,
        Part::Tmux,
        Part::Starship,
        Part::Shell,
        Part::Aliases,
        Part::App,
    ] {
        let files: Vec<String> = g
            .changes
            .iter()
            .filter(|(p, _)| *p == part)
            .map(|(_, c)| short(&c.path))
            .collect();
        let tools: Vec<&str> = missing
            .iter()
            .filter(|t| tool_part(**t) == part)
            .map(|t| t.name())
            .collect();
        let (state, summary) = if part == Part::App {
            app_plan_state(opts)
        } else if !opts.wants(part) {
            ("skipped", None)
        } else if part == Part::Shell && !zsh {
            (
                "unsupported",
                Some(not_zsh_note(&login_shell().unwrap_or_default())),
            )
        } else if part == Part::Starship && g.kept_starship && tools.is_empty() {
            (
                "skipped",
                Some(
                    tr!(
                        "Your own starship config stays.",
                        "保留你自己的 starship 配置。"
                    )
                    .to_string(),
                ),
            )
        } else if files.is_empty() && tools.is_empty() {
            ("ready", None)
        } else {
            let mut what = files;
            what.extend(
                tools
                    .iter()
                    .map(|t| tr!(format!("install {t}"), format!("安装 {t}"))),
            );
            ("changes", Some(what.join(", ")))
        };
        let mut entry = serde_json::json!({ "part": part_name(part), "state": state });
        if let Some(s) = summary {
            entry["summary"] = serde_json::Value::String(s);
        }
        parts.push(entry);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "schema": 1, "parts": parts }))?
    );
    Ok(())
}

fn part_name(part: Part) -> &'static str {
    match part {
        Part::Hooks => "hooks",
        Part::Tmux => "tmux",
        Part::Starship => "starship",
        Part::Shell => "shell",
        Part::Aliases => "aliases",
        Part::App => "app",
    }
}

/// Which part a missing tool holds up.
fn tool_part(tool: Tool) -> Part {
    match tool {
        Tool::Tmux => Part::Tmux,
        Tool::Starship => Part::Starship,
        _ => Part::Aliases,
    }
}

/// The menubar app: in place and current, or what install would do.
fn app_plan_state(opts: &Options) -> (&'static str, Option<String>) {
    if !opts.wants(Part::App) {
        return ("skipped", None);
    }
    if !app::applies() {
        return ("unsupported", None);
    }
    let current = env!("CARGO_PKG_VERSION");
    match app::installed().and_then(|p| app::version(&p)) {
        Some(have) if !app::is_older(&have, current) => ("ready", None),
        _ if app::bundled().is_some() => (
            "changes",
            Some(app::display(&app::user_app().unwrap_or_default())),
        ),
        _ => ("unsupported", None),
    }
}

pub fn run(opts: Options) -> Result<()> {
    if let Some(p) = opts.with.iter().find(|p| **p != Part::Aliases) {
        anyhow::bail!(tr!(
            format!("--with only adds optional parts (aliases); {p:?} is installed by default"),
            format!("--with 只用来加可选部分（aliases）；{p:?} 默认就会安装")
        )
        .to_lowercase());
    }
    let paths = Paths::detect()?;
    let Gathered { changes, notes, .. } = gather(&opts, &paths, false)?;
    let changes: Vec<Change> = changes.into_iter().map(|(_, c)| c).collect();
    let zsh = zsh_in_use(&paths);

    if changes.is_empty() {
        println!(
            "{}",
            tr!(
                "Your terminal setup is already up to date.",
                "终端配置已经是最新的了。"
            )
        );
        for n in &notes {
            println!("{n}");
        }
    } else if !files::apply(&changes, &notes, opts.dry_run, opts.yes, "install")? && !opts.dry_run {
        return Ok(());
    }

    // The app never stops the rest: a copy that cannot go in (or an app that
    // will not quit) is told as a warning, and the tools still get installed.
    // Only a refusal to ask (no terminal, no --yes) ends install, as above.
    if opts.wants(Part::App) {
        app::offer(opts.dry_run, opts.yes)?;
    }

    let tools = install_tools(&opts);
    if opts.dry_run {
        return tools;
    }
    if tools.is_ok() && missing_tools(&opts).iter().all(|t| !t.is_core()) {
        let done = if zsh {
            tr!(
                "All set. Open a new terminal (or `source ~/.zshrc`) and type `t`. In tmux, prefix + t opens the picker too.",
                "搞定。开个新终端（或者 `source ~/.zshrc`），敲 `t` 试试；在 tmux 里按 prefix + t 也能打开选择器。"
            )
        } else {
            tr!(
                "All set. Type `t` (or `cc-moss pick`) to open the picker. In tmux, prefix + t opens it too.",
                "搞定。敲 `t`（或者 `cc-moss pick`）打开选择器；在 tmux 里按 prefix + t 也行。"
            )
        };
        println!("\n{done}");
    } else {
        println!(
            "\n{}",
            tr!(
                "Almost there: `cc-moss doctor` shows what is still missing.",
                "差一点就好了：`cc-moss doctor` 会告诉你还缺什么。"
            )
        );
    }
    tools
}

/// `cc-moss install --uninstall`: take out everything install put in. Blocks
/// leave the user's files byte for byte as they were; files that held only
/// cc-moss's text are deleted. The state dir is kept.
pub fn uninstall(opts: &Options) -> Result<()> {
    let paths = Paths::detect()?;
    let mut changes = Vec::new();
    let mut notes = Vec::new();

    let setup_opts = setup::Options {
        dry_run: opts.dry_run,
        yes: opts.yes,
        uninstall: true,
        settings: None,
        zshrc: None,
        quiet: false,
    };
    changes.extend(setup::settings_change(&setup_opts)?);
    let (alias_changes, alias_notes) = alias::uninstall_changes(&alias::Place::detect()?, true)?;
    changes.extend(alias_changes);
    notes.extend(alias_notes);
    let zsh_tags = [setup::ZSH_TAG, TAG_PROMPT, TAG_SSH, TAG_ALIASES];
    changes.extend(strip_blocks(&paths.zshrc, &zsh_tags)?);
    changes.extend(strip_blocks(&paths.zshenv, &[TAG_PATH])?);
    changes.extend(strip_blocks(&paths.tmux_conf, &[TAG_TMUX])?);
    match read_optional(&paths.moss_tmux_conf)? {
        Some(text) if text == rendered_tmux_conf(&paths) => {
            changes.push(removal(&paths.moss_tmux_conf, text, Removal::Verbatim))
        }
        Some(_) => notes.push(tr!(
            format!(
                "Kept {}: you changed it, so it is yours now.",
                paths.moss_tmux_conf.display()
            ),
            format!(
                "保留了 {}：你改过它，现在归你了。",
                paths.moss_tmux_conf.display()
            )
        )),
        None => {}
    }
    match read_optional(&paths.starship)? {
        Some(text) if text == STARSHIP_TOML => {
            changes.push(removal(&paths.starship, text, Removal::Verbatim))
        }
        Some(_) => notes.push(tr!(
            format!(
                "Kept your starship config ({}): it is not the one cc-moss installed.",
                paths.starship.display()
            ),
            format!(
                "保留了 starship 配置（{}）：它不是 cc-moss 装的那份。",
                paths.starship.display()
            )
        )),
        None => {}
    }

    if changes.is_empty() {
        println!(
            "{}",
            tr!(
                "Nothing of cc-moss's is left to remove.",
                "cc-moss 加的东西都已经清理干净了。"
            )
        );
        for n in &notes {
            println!("{n}");
        }
    } else if !files::apply(&changes, &notes, opts.dry_run, opts.yes, "uninstall")? {
        return Ok(());
    }
    // Our config dir, if that file was all it held. Not on a dry run.
    if !opts.dry_run {
        if let Some(dir) = paths.moss_tmux_conf.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    app::remove(opts.dry_run, opts.yes)?;

    let state = moss_core::store_dir::state_dir();
    println!(
        "\n{}",
        tr!(
            format!(
                "Your session records stay in {dir}. To delete them as well: rm -rf {dir}\nLast step: remove the cc-moss binary the way you installed it.",
                dir = state.display()
            ),
            format!(
                "会话记录还留在 {dir}。想一起删掉：rm -rf {dir}\n最后一步：按你当初安装的方式卸载 cc-moss 程序本身。",
                dir = state.display()
            )
        )
    );
    Ok(())
}

/// `path` without cc-moss's blocks `tags`; deleted when nothing else is left.
pub(super) fn strip_blocks(path: &Path, tags: &[&str]) -> Result<Option<Change>> {
    let Some(original) = read_optional(path)? else {
        return Ok(None);
    };
    let mut text = original.clone();
    for tag in tags {
        text = managed_block::remove(&text, tag).with_context(|| format!("{}", path.display()))?;
    }
    if text == original {
        return Ok(None);
    }
    if text.is_empty() {
        return Ok(Some(removal(path, original, Removal::WithBackup)));
    }
    Ok(Some(Change {
        path: path.to_path_buf(),
        original: Some(original),
        new_text: text,
        remove: Removal::No,
        link: None,
        done: tr!(
            format!("Removed cc-moss's blocks from {}.", path.display()),
            format!("已从 {} 移除 cc-moss 的配置块。", path.display())
        ),
    }))
}

/// Delete `path`, which holds only cc-moss's text.
fn removal(path: &Path, original: String, how: Removal) -> Change {
    Change {
        path: path.to_path_buf(),
        original: Some(original),
        new_text: String::new(),
        remove: how,
        link: None,
        done: tr!(
            format!("Deleted {}.", path.display()),
            format!("已删除 {}。", path.display())
        ),
    }
}

/// The tools the chosen parts rely on.
pub fn wanted_tools(tmux: bool, starship: bool, aliases: bool) -> Vec<Tool> {
    let mut tools = Vec::new();
    if tmux {
        tools.push(Tool::Tmux);
    }
    if starship {
        tools.push(Tool::Starship);
    }
    if aliases {
        tools.extend([Tool::Eza, Tool::Bat, Tool::Fd]);
    }
    tools
}

/// `CC_MOSS_PLATFORM` (macos, debian, other) wins over detection.
pub fn platform() -> Platform {
    match std::env::var("CC_MOSS_PLATFORM").as_deref() {
        Ok("macos") => Platform::MacOs,
        Ok("debian") => Platform::Debian,
        Ok("other") => Platform::Other,
        _ => Platform::detect(),
    }
}

/// The wanted tools that are not installed.
fn missing_tools(opts: &Options) -> Vec<Tool> {
    wanted_tools(
        opts.wants(Part::Tmux),
        opts.wants(Part::Starship),
        opts.wants(Part::Aliases),
    )
    .into_iter()
    .filter(|t| !installed(*t))
    .collect()
}

/// On the PATH, or in ~/.local/bin, where starship's installer puts it and
/// which this process's PATH may not have yet (a new shell will).
pub fn installed(tool: Tool) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let local = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin"));
    on_path(tool.commands())
        || local.is_some_and(|dir| {
            tool.commands().iter().any(|c| {
                std::fs::metadata(dir.join(c))
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
        })
}

/// Install commands for `tools` on this machine (none without Homebrew on
/// macOS, without sudo for root).
pub fn commands_for(tools: &[Tool]) -> Vec<String> {
    let platform = platform();
    if platform == Platform::MacOs && !on_path(&["brew"]) {
        return Vec::new();
    }
    let commands = install_commands(platform, tools);
    if is_root() {
        without_sudo(commands)
    } else {
        commands
    }
}

/// List what is missing, and install it once the user agrees.
fn install_tools(opts: &Options) -> Result<()> {
    let missing = missing_tools(opts);
    if missing.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = missing.iter().map(|t| t.name()).collect();
    println!(
        "\n{}",
        tr!(
            format!("Still missing: {}.", names.join(", ")),
            format!("还缺这些工具：{}。", names.join("、"))
        )
    );
    let commands = commands_for(&missing);
    if commands.is_empty() {
        println!("{}", manual_install_hint());
        return Ok(());
    }
    for c in &commands {
        println!("  $ {c}");
    }
    if opts.dry_run {
        return Ok(());
    }
    let question = tr!("Install them now?", "现在就装上吗？");
    let refused = tr!(
        "run the commands above yourself, or re-run with --yes",
        "自己运行上面的命令，或者加上 --yes 重新运行"
    );
    if !confirm::ask(opts.yes, question, refused)? {
        println!(
            "{}",
            tr!(
                "Okay. The commands above are all it takes, whenever you like.",
                "好的。什么时候想装了，运行上面的命令就行。"
            )
        );
        return Ok(());
    }
    // Run them all: one package an older system lacks should not stop the rest.
    let mut failed = Vec::new();
    for c in &commands {
        let status = std::process::Command::new("sh").args(["-c", c]).status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => failed.push(format!("`{c}` ({s})")),
            Err(e) => failed.push(format!("`{c}` ({e})")),
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    // Only the tools cc-moss's own setup needs make this a failure; an
    // optional one an older system lacks (eza on Debian 12) is a note.
    let core_missing: Vec<&str> = missing_tools(opts)
        .into_iter()
        .filter(|t| t.is_core())
        .map(|t| t.name())
        .collect();
    if !core_missing.is_empty() {
        bail!(tr!(
            format!(
                "could not install {}: {}",
                core_missing.join(", "),
                failed.join("; ")
            ),
            format!(
                "没能装上 {}：{}",
                core_missing.join("、"),
                failed.join("；")
            )
        ));
    }
    println!(
        "{}",
        tr!(
            "Some optional tools are not available here; the aliases that need them stay off. (eza: `cargo install eza`)",
            "有些可选工具这里装不上，用到它们的别名先不生效。（eza 可以用 `cargo install eza` 安装）"
        )
    );
    Ok(())
}

/// What to say when cc-moss cannot install the tools itself.
pub fn manual_install_hint() -> &'static str {
    if platform() == Platform::MacOs {
        tr!(
            "Install Homebrew first (https://brew.sh), then run `cc-moss install` again.",
            "先装好 Homebrew（https://brew.sh），再运行一次 `cc-moss install`。"
        )
    } else {
        tr!(
            "Install them with your package manager, then run `cc-moss doctor`.",
            "用你系统的包管理器装上它们，再运行 `cc-moss doctor` 看看。"
        )
    }
}

pub(super) fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}
