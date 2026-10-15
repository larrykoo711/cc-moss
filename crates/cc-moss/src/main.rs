//! cc-moss: civilizations carry on, so do conversations.

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
        /// Extra arguments for claude, after `--`.
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
        /// Remove the cc-moss hooks, leaving everything else untouched.
        #[arg(long)]
        uninstall: bool,
        /// Settings file to edit [default: ~/.claude/settings.json].
        #[arg(long, value_name = "PATH")]
        settings: Option<std::path::PathBuf>,
        /// zsh startup file for the `t` shortcut [default: ~/.zshrc].
        #[arg(long, value_name = "PATH")]
        zshrc: Option<std::path::PathBuf>,
    },
    /// Set up the terminal: hooks, tmux, starship and shell blocks, then the tools they need.
    Install {
        /// Also install an optional part: aliases.
        #[arg(long, value_enum, value_name = "PART", hide_possible_values = true)]
        with: Vec<cmd::install::Part>,
        /// Leave a part out: hooks, tmux, starship, shell or aliases.
        #[arg(long, value_enum, value_name = "PART", hide_possible_values = true)]
        skip: Vec<cmd::install::Part>,
        /// Replace an existing starship config with cc-moss's.
        #[arg(long)]
        replace_starship: bool,
        /// Show what would change; change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Apply without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Check hooks, tmux, starship, shell blocks and tools.
    Doctor,
    /// Pick a session, folder or interrupted session (the `t` shortcut).
    Pick,
    /// Bring interrupted sessions back, each in its own tmux session.
    Restore {
        /// Restore without asking.
        #[arg(long)]
        yes: bool,
        /// List what would be restored; change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Only these sessions (Claude name, tmux name or id prefix).
        names: Vec<String>,
    },
    /// Jump to a session's tmux session.
    Attach { name: String },
    /// Stop offering sessions for restore.
    Close {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Show or set a session's notes: what it is doing, where it stopped.
    Note {
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
    /// Entry point for Claude Code hooks. Always exits 0.
    #[command(hide = true)]
    Hook {
        /// session-start | user-prompt-submit | stop | session-end
        event: String,
    },
}

fn main() -> std::process::ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => explain(e),
    };
    if cli.version {
        version::print();
        return std::process::ExitCode::SUCCESS;
    }
    let Some(command) = cli.command else {
        return report(pick::run());
    };
    match command {
        Command::Hook { event } => {
            cmd::hook::run(&event);
            std::process::ExitCode::SUCCESS
        }
        Command::New {
            name,
            dir,
            detach,
            yes,
            claude_args,
        } => report(cmd::new::run(cmd::new::NewArgs {
            name,
            dir,
            detach,
            yes,
            claude_args,
        })),
        Command::Ls { all, json } => report(cmd::ls::run(all, json)),
        Command::Pick => report(pick::run()),
        Command::Install {
            with,
            skip,
            replace_starship,
            dry_run,
            yes,
        } => report(cmd::install::run(cmd::install::Options {
            dry_run,
            yes,
            with,
            skip,
            replace_starship,
        })),
        Command::Doctor => match cmd::doctor::run() {
            Ok(true) => std::process::ExitCode::SUCCESS,
            Ok(false) => std::process::ExitCode::FAILURE,
            Err(e) => report(Err(e)),
        },
        Command::Restore {
            yes,
            dry_run,
            names,
        } => report(cmd::restore::run(cmd::restore::RestoreArgs {
            yes,
            dry_run,
            names,
        })),
        Command::Attach { name } => report(cmd::attach::run(&name)),
        Command::Close { names } => report(cmd::close::run(&names)),
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
        })),
    }
}

/// Help, version and usage errors, in the reader's language. Only reached
/// when something is shown, so the hooks never decide a language.
fn explain(error: clap::Error) -> ! {
    if i18n::lang() == i18n::Lang::Zh {
        if let Err(zh) = help_zh::localize(Cli::command()).try_get_matches() {
            zh.exit();
        }
    }
    error.exit()
}

fn report(result: anyhow::Result<()>) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cc-moss: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
