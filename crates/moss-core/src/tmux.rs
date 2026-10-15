//! tmux adapter. All commands go through a [`Runner`] so they can be tested
//! without a tmux server.

use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};

use crate::command::output_with_timeout;
use crate::event::TmuxLoc;

const TMUX_TIMEOUT: Duration = Duration::from_millis(500);

pub trait Runner {
    /// Run `tmux <args>` and return stdout.
    fn run(&self, args: &[String]) -> Result<String>;
}

/// Which tmux server to talk to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Socket {
    Default,
    /// `tmux -L <name>`
    Name(String),
    /// `tmux -S <path>`
    Path(String),
}

impl Socket {
    /// A configured value: paths contain `/`, anything else is a name.
    pub fn from_config(value: Option<&str>) -> Self {
        match value {
            None | Some("") => Socket::Default,
            Some(v) if v.contains('/') => Socket::Path(v.to_string()),
            Some(v) => Socket::Name(v.to_string()),
        }
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            Socket::Default => vec![],
            Socket::Name(n) => vec!["-L".into(), n.clone()],
            Socket::Path(p) => vec!["-S".into(), p.clone()],
        }
    }
}

pub struct ProcessRunner {
    pub socket: Socket,
    pub timeout: Duration,
}

impl Runner for ProcessRunner {
    fn run(&self, args: &[String]) -> Result<String> {
        let mut all = self.socket.args();
        all.extend_from_slice(args);
        let Some(out) = output_with_timeout(Command::new("tmux").args(&all), self.timeout) else {
            bail!(
                "tmux {} did not finish",
                args.first().map_or("", |s| s.as_str())
            );
        };
        if !out.status.success() {
            bail!("tmux {} failed", args.first().map_or("", |s| s.as_str()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

pub struct Tmux<R: Runner> {
    runner: R,
}

impl Tmux<ProcessRunner> {
    /// For hooks: give up quickly rather than delay Claude Code.
    pub fn with_socket(socket: Socket) -> Self {
        Self::with_timeout(socket, TMUX_TIMEOUT)
    }

    /// For commands a person waits on, where giving up early is worse
    /// (a slow `new-session` that still succeeds must not count as failed).
    pub fn with_timeout(socket: Socket, timeout: Duration) -> Self {
        Tmux::new(ProcessRunner { socket, timeout })
    }
}

/// Where a new session's only pane runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    /// The pane's process: the program itself, since argv is exec'd directly.
    pub pid: u32,
    pub session_id: String,
    pub pane_id: String,
    /// What the pane was started with (tmux's own rendering of argv).
    pub start_command: String,
}

const PANE_FORMAT: &str = "#{pane_pid}\t#{session_id}\t#{pane_id}\t#{pane_start_command}";

fn parse_pane(out: &str) -> Option<Pane> {
    let mut f = out.lines().next()?.splitn(4, '\t');
    Some(Pane {
        pid: f.next()?.parse().ok()?,
        session_id: f.next()?.to_string(),
        pane_id: f.next()?.to_string(),
        start_command: f.next().unwrap_or_default().to_string(),
    })
}

/// tmux expands `#` formats in `-c` and treats a trailing `;` in any argument
/// as a command separator; neither must change what reaches the program.
fn escape_cwd(cwd: &str) -> String {
    escape_semicolon(&cwd.replace('#', "##"))
}

fn escape_semicolon(arg: &str) -> String {
    match arg.strip_suffix(';') {
        Some(head) => format!("{head}\\;"),
        None => arg.to_string(),
    }
}

impl<R: Runner> Tmux<R> {
    pub fn new(runner: R) -> Self {
        Tmux { runner }
    }

    pub fn runner(&self) -> &R {
        &self.runner
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        self.runner
            .run(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    /// A detached session with one window running `argv` directly (no shell),
    /// so the session ends when the program exits (R9). Returns its pane.
    pub fn new_session(&self, name: &str, cwd: &str, argv: &[&str]) -> Result<Pane> {
        let cwd = escape_cwd(cwd);
        let argv: Vec<String> = argv.iter().map(|a| escape_semicolon(a)).collect();
        let mut args = vec![
            "new-session",
            "-d",
            "-P",
            "-F",
            PANE_FORMAT,
            "-s",
            name,
            "-c",
            &cwd,
        ];
        args.extend(argv.iter().map(String::as_str));
        let out = self.run(&args)?;
        parse_pane(&out).ok_or_else(|| anyhow::anyhow!("tmux new-session printed no pane"))
    }

    /// The server's socket path.
    pub fn socket_path(&self) -> Option<String> {
        self.run(&["display-message", "-p", "#{socket_path}"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// The first pane of session `name`, if that session exists.
    pub fn pane_of(&self, name: &str) -> Option<Pane> {
        let target = format!("={name}");
        self.run(&["list-panes", "-t", &target, "-F", PANE_FORMAT])
            .ok()
            .and_then(|out| parse_pane(&out))
    }

    /// Rename by session id (`$N`), never by name, so we cannot hit another session.
    pub fn rename_session(&self, session_id: &str, new_name: &str) -> Result<()> {
        self.run(&["rename-session", "-t", session_id, new_name])
            .map(drop)
    }

    /// Names of all sessions; empty when no server is running.
    pub fn session_names(&self) -> Result<Vec<String>> {
        let out = match self.run(&["list-sessions", "-F", "#{session_name}"]) {
            Ok(out) => out,
            Err(_) => return Ok(Vec::new()),
        };
        Ok(out
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    /// `(session id, session name)` for every session; empty when no server runs.
    pub fn sessions(&self) -> Vec<(String, String)> {
        self.run(&["list-sessions", "-F", "#{session_id}\t#{session_name}"])
            .map(|out| {
                out.lines()
                    .filter_map(|l| l.split_once('\t'))
                    .map(|(id, name)| (id.to_string(), name.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The current name of session `$N`.
    pub fn session_name(&self, session_id: &str) -> Option<String> {
        self.run(&["display-message", "-p", "-t", session_id, "#{session_name}"])
            .ok()
            .map(|s| s.trim_end().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// Locate the current pane from the `TMUX` / `TMUX_PANE` values a hook inherits.
/// Session id and name are looked up best-effort; failure leaves them empty.
pub fn locate(tmux_env: Option<&str>, pane_env: Option<&str>) -> Option<TmuxLoc> {
    let socket = tmux_env?.split(',').next()?.to_string();
    let pane_id = pane_env?.to_string();
    if socket.is_empty() || pane_id.is_empty() {
        return None;
    }
    let t = Tmux::with_socket(Socket::Path(socket.clone()));
    let (session_id, session_name) = t
        .run(&[
            "display-message",
            "-p",
            "-t",
            &pane_id,
            "#{session_id}\t#{session_name}",
        ])
        .ok()
        .map(|text| {
            let mut parts = text.trim_end().splitn(2, '\t');
            let id = parts.next().filter(|s| !s.is_empty()).map(String::from);
            let name = parts.next().filter(|s| !s.is_empty()).map(String::from);
            (id, name)
        })
        .unwrap_or((None, None));
    Some(TmuxLoc {
        socket,
        session_id,
        session_name,
        pane_id,
    })
}
