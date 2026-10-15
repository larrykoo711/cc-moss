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

use super::files::{self, read_optional, Change};
use super::{confirm, setup};

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
/// starship, shell (ssh reset, PATH for scripts) and the optional aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Part {
    Hooks,
    Tmux,
    Starship,
    Shell,
    Aliases,
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
    if opts.wants(Part::Hooks) {
        text = managed_block::upsert(&text, setup::ZSH_TAG, setup::ZSH_BODY)?;
    }
    if opts.wants(Part::Shell) {
        text = managed_block::upsert(&text, TAG_SSH, SSH_TTY_ZSH)?;
    }
    if opts.wants(Part::Aliases) {
        text = managed_block::upsert(&text, TAG_ALIASES, ALIASES_ZSH)?;
    }
    Ok(text)
}

/// The managed blocks install writes into `.zshrc`.
pub fn is_ours(tag: &str) -> bool {
    [setup::ZSH_TAG, TAG_PROMPT, TAG_SSH, TAG_ALIASES].contains(&tag)
}

/// A change to `path`, if `new_text` differs from what is there.
fn change(path: &Path, new_text: String) -> Result<Option<Change>> {
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
        path: path.to_path_buf(),
        original,
        new_text,
        done,
    }))
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
    let mut notes = Vec::new();
    let mut changes = Vec::new();

    if opts.wants(Part::Hooks) {
        let setup_opts = setup::Options {
            dry_run: opts.dry_run,
            yes: opts.yes,
            uninstall: false,
            settings: None,
            zshrc: None,
        };
        changes.extend(setup::settings_change(&setup_opts)?);
    }

    let in_file = |p: &Path| format!("{}", p.display());
    let zshrc = read_optional(&paths.zshrc)?.unwrap_or_default();
    let text = zshrc_text(&zshrc, &opts).with_context(|| in_file(&paths.zshrc))?;
    changes.extend(change(&paths.zshrc, text)?);

    if opts.wants(Part::Shell) {
        let zshenv = read_optional(&paths.zshenv)?.unwrap_or_default();
        let text = managed_block::upsert(&zshenv, TAG_PATH, PATH_ZSH)
            .with_context(|| in_file(&paths.zshenv))?;
        changes.extend(change(&paths.zshenv, text)?);
    }

    if opts.wants(Part::Tmux) {
        changes.extend(change(&paths.moss_tmux_conf, rendered_tmux_conf(&paths))?);
        let own = read_optional(&paths.tmux_conf)?.unwrap_or_default();
        let text = managed_block::upsert_top(&own, TAG_TMUX, &tmux_block(&paths))
            .with_context(|| in_file(&paths.tmux_conf))?;
        changes.extend(change(&paths.tmux_conf, text)?);
    }

    if opts.wants(Part::Starship) {
        match read_optional(&paths.starship)? {
            Some(theirs) if theirs != STARSHIP_TOML && !opts.replace_starship => notes.push(
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
            ),
            _ => changes.extend(change(&paths.starship, STARSHIP_TOML.to_string())?),
        }
    }

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

    let tools = install_tools(&opts);
    if opts.dry_run {
        return tools;
    }
    if tools.is_ok() && missing_tools(&opts).iter().all(|t| !t.is_core()) {
        println!(
            "\n{}",
            tr!(
                "All set. Open a new terminal (or `source ~/.zshrc`) and type `t`. In tmux, prefix + t opens the picker too.",
                "搞定。开个新终端（或者 `source ~/.zshrc`），敲 `t` 试试；在 tmux 里按 prefix + t 也能打开选择器。"
            )
        );
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
            format!("could not install {}: {}", core_missing.join(", "), failed.join("; ")),
            format!("没能装上 {}：{}", core_missing.join("、"), failed.join("；"))
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

fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}
