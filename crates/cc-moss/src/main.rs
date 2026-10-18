//! cc-moss: civilizations carry on, so do conversations.

#[cfg(not(unix))]
compile_error!("cc-moss supports macOS and Linux only");

#[macro_use]
mod i18n;
mod cmd;
mod help_zh;
mod pick;
mod reconcile;
mod version;

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cc-moss",
    about = "Recoverable Claude Code sessions in tmux",
    after_help = "Run `cc-moss` without a command to open the session picker (the `t` shortcut).",
    disable_version_flag = true
)]
struct Cli {
    /// Print version.
    #[arg(short = 'V', long)]
    version: bool,
    /// Without a command, open the session picker (same as `t`).
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List sessions: interrupted ones first, then running ones.
    Ls {
        /// Include sessions that were closed on purpose.
        #[arg(long)]
        all: bool,
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
    },
    /// What just happened to the sessions, oldest first (for apps and scripts).
    Events {
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
        /// Only events after this sequence number; without it, the latest 50.
        #[arg(long, value_name = "SEQ")]
        since: Option<i64>,
        /// At most this many events (1 or more).
        #[arg(long, default_value_t = 500, value_name = "N",
              value_parser = clap::value_parser!(u64).range(1..))]
        limit: u64,
    },
    /// Start a named Claude Code session in its own tmux session.
    New {
        /// Session name; defaults to the directory name.
        name: Option<String>,
        /// Working directory (default: current directory).
        #[arg(short = 'C', long = "dir")]
        dir: Option<std::path::PathBuf>,
        /// Do not attach to the new session.
        #[arg(long)]
        detach: bool,
        /// If the name belongs to an interrupted session, restore it without asking.
        #[arg(long)]
        yes: bool,
        /// Normal mode this time: no bypass, not even one added in the config's default_args.
        #[arg(long)]
        safe: bool,
        /// Extra arguments for claude, e.g. `--model opus`; `--` is optional.
        #[arg(last = true)]
        claude_args: Vec<String>,
    },
    /// Install the Claude Code hooks that register sessions with cc-moss.
    ///
    /// Shows the change to settings.json as a diff and asks before writing;
    /// a timestamped backup is kept next to the file.
    Setup {
        /// Show the diff only; write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Apply without asking (required when stdin is not a terminal).
        #[arg(long)]
        yes: bool,
        /// Remove the cc-moss hooks and the `t` alias; everything else stays.
        #[arg(long)]
        uninstall: bool,
        /// Settings file to edit [default: ~/.claude/settings.json].
        #[arg(long, value_name = "PATH")]
        settings: Option<std::path::PathBuf>,
        /// zsh startup file to take the old `t` function out of [default: ~/.zshrc].
        #[arg(long, value_name = "PATH")]
        zshrc: Option<std::path::PathBuf>,
    },
    /// Set up the terminal: hooks, tmux, starship and shell blocks, then the tools they need (and the menubar app on macOS).
    Install {
        /// Also install an optional part: aliases.
        #[arg(long, value_enum, value_name = "PART", hide_possible_values = true)]
        with: Vec<cmd::install::Part>,
        /// Leave a part out: hooks, tmux, starship, shell, app or aliases.
        #[arg(long, value_enum, value_name = "PART", hide_possible_values = true)]
        skip: Vec<cmd::install::Part>,
        /// Replace an existing starship config with cc-moss's.
        #[arg(long)]
        replace_starship: bool,
        /// Undo install: remove the hooks, blocks and files it added, and the menubar app (macOS).
        #[arg(long, conflicts_with_all = ["with", "skip", "replace_starship"])]
        uninstall: bool,
        /// Show what would change; change nothing.
        #[arg(long)]
        dry_run: bool,
        /// With --dry-run: each part's state as JSON (for the menubar app).
        #[arg(long, requires = "dry_run", conflicts_with = "uninstall")]
        json: bool,
        /// Apply without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Check hooks, aliases, tmux, starship, shell blocks and tools.
    Doctor {
        /// Recreate alias links that are missing or broken.
        #[arg(long)]
        fix: bool,
    },
    /// Your own names for cc-moss commands, usable in any shell (`t` is one).
    Alias {
        #[command(subcommand)]
        action: AliasCommand,
    },
    /// Read or change a setting in config.toml (today: ui.language).
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Look for a new version of cc-moss and install it.
    Update {
        /// Only look; change nothing.
        #[arg(long)]
        check: bool,
        /// With --check: print the answer as JSON (for the menubar app).
        #[arg(long, requires = "check")]
        json: bool,
        /// Update without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Pick a session, folder or interrupted session (the `t` shortcut).
    Pick {
        /// Start with this search, e.g. `cc-moss pick api`.
        #[arg(value_name = "QUERY")]
        query: Vec<String>,
    },
    /// Bring interrupted sessions back, each in its own tmux session.
    Restore {
        /// Restore without asking.
        #[arg(long)]
        yes: bool,
        /// List what would be restored; change nothing.
        #[arg(long)]
        dry_run: bool,
        /// With --yes: print each session's result as JSON (for the menubar app).
        #[arg(long, requires = "yes", conflicts_with = "dry_run")]
        json: bool,
        /// Only these sessions (Claude name, tmux name or id prefix).
        names: Vec<String>,
    },
    /// Jump to a session's tmux session.
    Attach {
        /// Session name (Claude name, tmux name or id prefix).
        name: String,
    },
    /// Stop offering sessions for restore.
    Close {
        /// Session names (Claude name, tmux name or id prefix).
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Show or set a session's notes: what it is doing, where it stopped.
    Note {
        /// Session name (Claude name, tmux name or id prefix).
        name: String,
        /// What the session is for.
        #[arg(long)]
        goal: Option<String>,
        /// Where it stopped / what it waits for.
        #[arg(long)]
        state: Option<String>,
        /// Free-form note.
        text: Option<String>,
        /// Remove all notes.
        #[arg(long, conflicts_with_all = ["goal", "state", "text"])]
        clear: bool,
    },
    /// Type a message into a running session, as if you typed it in its terminal.
    Send {
        /// Session name (Claude name, tmux name or id prefix).
        name: String,
        /// What to send; several lines arrive whole.
        #[arg(last = true, required = true)]
        text: Vec<String>,
    },
    /// The resident cc-moss daemon that launchd keeps running (macOS).
    Daemon {
        #[command(subcommand)]
        action: DaemonCommand,
    },
    /// Entry point for Claude Code hooks. Always exits 0.
    #[command(hide = true)]
    Hook {
        /// session-start | user-prompt-submit | stop | session-end | notification | post-tool-use
        event: String,
    },
}

#[derive(Subcommand)]
enum AliasCommand {
    /// List your aliases and whether each one works.
    Ls {
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
    },
    /// Add an alias, or change what it runs.
    ///
    /// `cc-moss alias add tn new` makes `tn login-page` run
    /// `cc-moss new login-page`, in any shell, right away. One quoted
    /// argument is read as command text: `cc-moss alias add tback "restore --yes"`.
    /// Put --force and --json before the name.
    Add {
        /// Use the name even if it is already a command on your PATH.
        #[arg(long)]
        force: bool,
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
        /// The alias: a-z, 0-9, - and _.
        name: String,
        /// The cc-moss command it runs, with arguments.
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Remove an alias and its link.
    Rm {
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
        /// The alias to remove.
        name: String,
    },
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Run in the foreground; this is what launchd starts.
    Run,
    /// Have launchd start the daemon, setting it up if needed.
    Start,
    /// Stop the daemon; it stays off, across logins too, until `start`.
    Stop,
    /// Whether the daemon is running, since when, and which version.
    Status {
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print a setting's value (the default when it is not set).
    Get {
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
        /// The setting: ui.language.
        key: String,
    },
    /// Change a setting; comments and everything else in the file stay.
    ///
    /// `cc-moss config set ui.language zh` switches cc-moss to Chinese;
    /// `auto` follows the system again. Known keys: ui.language (auto, zh, en).
    Set {
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
        /// The setting: ui.language.
        key: String,
        /// The new value; for ui.language: auto, zh or en.
        value: String,
    },
}

/// cc-moss's commands that people may run (and aliases may stand for).
pub(crate) fn subcommands() -> Vec<String> {
    Cli::command()
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .map(|c| c.get_name().to_string())
        .collect()
}

/// The arguments to parse: the real ones, or, when cc-moss was started under
/// an alias's name (a link in ~/.local/bin), `cc-moss` plus the alias's
/// words plus the user's arguments. No shell is involved: the words come
/// from the alias tokenizer and the user's arguments are passed as they are.
fn arguments() -> Result<Vec<std::ffi::OsString>, anyhow::Error> {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let name = args
        .first()
        .and_then(|a| std::path::Path::new(a).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.is_empty() || moss_core::aliases::is_own_name(&name) {
        return Ok(args);
    }
    let words = cmd::alias::expand_invocation(&name)?;
    let user = args.split_off(1);
    Ok(std::iter::once(std::ffi::OsString::from("cc-moss"))
        .chain(words.into_iter().map(Into::into))
        .chain(user)
        .collect())
}

fn main() -> std::process::ExitCode {
    let mut argv = match arguments() {
        Ok(argv) => argv,
        Err(e) => return report(Err(e)),
    };
    // `cc-moss new` takes Claude Code's own flags without a `--`, so an
    // alias such as `mycc = "new --dangerously-skip-permissions"` works.
    if argv.get(1).is_some_and(|a| a == "new") {
        let rest: Vec<String> = argv[2..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        argv.truncate(2);
        argv.extend(
            moss_core::launch_args::normalize_new(&rest)
                .into_iter()
                .map(Into::into),
        );
    }
    // Hooks never go through clap: a usage error exits 2, which Claude Code
    // treats as a blocking error on UserPromptSubmit. Arguments a newer
    // cc-moss might add are ignored; the event is the first non-flag word.
    // An alias can never expand to `hook`.
    let mut args = argv.iter().skip(1);
    if args.next().is_some_and(|a| a == "hook") {
        let event = args
            .map(|a| a.to_string_lossy().into_owned())
            .find(|a| !a.starts_with('-'))
            .unwrap_or_default();
        cmd::hook::run(&event);
        return std::process::ExitCode::SUCCESS;
    }
    let cli = match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(e) => explain(e, &argv),
    };
    if cli.version {
        version::print();
        return std::process::ExitCode::SUCCESS;
    }
    let Some(command) = cli.command else {
        return report(pick::run(""));
    };
    match command {
        // Handled before parsing; kept in `Command` for the help text.
        Command::Hook { event } => {
            cmd::hook::run(&event);
            std::process::ExitCode::SUCCESS
        }
        Command::New {
            name,
            dir,
            detach,
            yes,
            safe,
            claude_args,
        } => report(cmd::new::run(cmd::new::NewArgs {
            name,
            dir,
            detach,
            yes,
            safe,
            claude_args,
        })),
        Command::Ls { all, json } => report(cmd::ls::run(all, json)),
        Command::Events { json, since, limit } => report(cmd::events::run(
            since,
            usize::try_from(limit).unwrap_or(usize::MAX),
            json,
        )),
        Command::Pick { query } => report(pick::run(&query.join(" "))),
        Command::Install {
            with,
            skip,
            replace_starship,
            uninstall,
            dry_run,
            json,
            yes,
        } => {
            let opts = cmd::install::Options {
                dry_run,
                yes,
                with,
                skip,
                replace_starship,
            };
            report(if uninstall {
                cmd::install::uninstall(&opts)
            } else if json {
                cmd::install::plan_json(&opts)
            } else {
                cmd::install::run(opts)
            })
        }
        Command::Doctor { fix } => match cmd::doctor::run(fix) {
            Ok(true) => std::process::ExitCode::SUCCESS,
            Ok(false) => std::process::ExitCode::FAILURE,
            Err(e) => report(Err(e)),
        },
        Command::Update { check, json, yes } => report(cmd::update::run(cmd::update::UpdateArgs {
            check,
            json,
            yes,
        })),
        Command::Restore {
            yes,
            dry_run,
            json,
            names,
        } => report(cmd::restore::run(cmd::restore::RestoreArgs {
            yes,
            dry_run,
            json,
            names,
        })),
        Command::Attach { name } => report(cmd::attach::run(&name)),
        Command::Close { names } => report(cmd::close::run(&names)),
        Command::Send { name, text } => report(cmd::send::run(&name, &text.join(" "))),
        Command::Daemon { action } => match action {
            DaemonCommand::Run => match cmd::daemon::run() {
                Ok(true) => std::process::ExitCode::SUCCESS,
                Ok(false) => std::process::ExitCode::FAILURE,
                Err(e) => report(Err(e)),
            },
            DaemonCommand::Start => report(cmd::daemon::start()),
            DaemonCommand::Stop => report(cmd::daemon::stop()),
            DaemonCommand::Status { json } => report(cmd::daemon::print_status(json)),
        },
        Command::Note {
            name,
            goal,
            state,
            text,
            clear,
        } => report(cmd::note::run(cmd::note::NoteArgs {
            name,
            goal,
            state,
            text,
            clear,
        })),
        Command::Alias { action } => cmd::alias::run(match action {
            AliasCommand::Ls { json } => cmd::alias::Action::Ls { json },
            AliasCommand::Add {
                force,
                json,
                name,
                command,
            } => cmd::alias::Action::Add {
                name,
                command,
                force,
                json,
            },
            AliasCommand::Rm { json, name } => cmd::alias::Action::Rm { name, json },
        }),
        Command::Config { action } => cmd::config::run(match action {
            ConfigCommand::Get { json, key } => cmd::config::Action::Get { key, json },
            ConfigCommand::Set { json, key, value } => {
                cmd::config::Action::Set { key, value, json }
            }
        }),
        Command::Setup {
            dry_run,
            yes,
            uninstall,
            settings,
            zshrc,
        } => report(cmd::setup::run(cmd::setup::Options {
            dry_run,
            yes,
            uninstall,
            settings,
            zshrc,
            quiet: false,
        })),
    }
}

/// Help, version and usage errors, in the reader's language. Only reached
/// when something is shown, so the hooks never decide a language.
fn explain(error: clap::Error, argv: &[std::ffi::OsString]) -> ! {
    if i18n::lang() == i18n::Lang::Zh {
        if let Err(zh) = help_zh::localize(Cli::command()).try_get_matches_from(argv) {
            zh.exit();
        }
    }
    error.exit()
}

fn report(result: anyhow::Result<()>) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            let newer = e
                .chain()
                .find_map(|c| c.downcast_ref::<moss_core::registry::NewerSchema>());
            match newer {
                Some(_) => eprintln!(
                    "cc-moss: {}",
                    tr!(
                        "this registry was written by a newer cc-moss, so this version only reads it; upgrade cc-moss",
                        "这份会话记录是更新版本的 cc-moss 写的，当前版本只能读不能改，请升级 cc-moss"
                    )
                ),
                None => eprintln!("cc-moss: {e:#}"),
            }
            std::process::ExitCode::FAILURE
        }
    }
}
