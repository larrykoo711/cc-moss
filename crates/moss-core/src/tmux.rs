//! tmux adapter. All commands go through a [`Runner`] so they can be tested
//! without a tmux server.

use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};

use crate::command::output_with_timeout;
use crate::event::TmuxLoc;

const TMUX_TIMEOUT: Duration = Duration::from_millis(500);

/// 只有 tmux 明确确认同名冲突时允许换号；未知结果不能再创建。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewSessionFailure {
    DuplicateName,
    Unconfirmed,
}

impl std::fmt::Display for NewSessionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DuplicateName => "tmux new-session: duplicate session name",
            Self::Unconfirmed => {
                "tmux new-session result is unconfirmed; do not start another session"
            }
        })
    }
}
impl std::error::Error for NewSessionFailure {}

/// `tmux -u`, for every tmux cc-moss reads. Outside tmux and without a UTF-8
/// locale (the daemon, the menu bar app) tmux otherwise prints each non-ASCII
/// letter of a name or format as `_`. Interactive attaching keeps plain tmux.
pub fn command() -> Command {
    let mut cmd = Command::new("tmux");
    cmd.arg("-u");
    cmd
}

pub trait Runner {
    /// Run `tmux <args>` and return stdout.
    fn run(&self, args: &[String]) -> Result<String>;
}

/// The tmux user option holding a session's own name as people gave it
/// (spaces, Chinese and all), for terminal tab titles.
pub const NAME_OPTION: &str = "@cc-moss-name";

/// The terminal title: the session's own name, else tmux's.
pub const TITLE_FORMAT: &str = "#{?#{@cc-moss-name},#{@cc-moss-name},#{session_name}}";

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
        let Some(out) = output_with_timeout(command().args(&all), self.timeout) else {
            if args.first().is_some_and(|arg| arg == "new-session") {
                return Err(NewSessionFailure::Unconfirmed.into());
            }
            bail!(
                "tmux {} did not finish",
                args.first().map_or("", |s| s.as_str())
            );
        };
        if !out.status.success() {
            let what = args.first().map_or("", |s| s.as_str());
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why.trim();
            if what == "new-session"
                && args
                    .windows(2)
                    .any(|pair| pair[0] == "-s" && why == format!("duplicate session: {}", pair[1]))
            {
                return Err(NewSessionFailure::DuplicateName.into());
            }
            if why.is_empty() {
                bail!("tmux {what} failed");
            }
            bail!("tmux {what} failed: {why}");
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Readable,
    Denied,
    NoServer,
}

/// Fails with an actionable message when the tmux server cannot read `dir`
/// (macOS privacy). Never restarts the server: that would end every session.
pub fn ensure_access<R: Runner>(tmux: &Tmux<R>, dir: &str) -> Result<()> {
    if cfg!(target_os = "macos") && tmux.probe_access(dir) == Access::Denied {
        bail!(
            "the tmux server cannot read {dir} (macOS privacy settings). \
             Restart tmux to fix it: `tmux kill-server` (this ends all tmux sessions)"
        );
    }
    Ok(())
}

/// One row of `tmux list-sessions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub activity_ms: i64,
    pub attached: bool,
    /// The session's start directory.
    pub path: String,
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

/// Field separator in `-F` formats. Printable on purpose: tmux 3.5 replaces
/// control characters such as tabs in format output with `_`, and non-ASCII
/// may be mangled without a UTF-8 locale.
const SEP: &str = "|:|";

const PANE_FORMAT: &str = "#{pane_pid}|:|#{session_id}|:|#{pane_id}|:|#{pane_start_command}";

fn parse_pane(out: &str) -> Option<Pane> {
    let mut f = out.lines().next()?.splitn(4, SEP);
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
        parse_pane(&out).ok_or_else(|| NewSessionFailure::Unconfirmed.into())
    }

    /// Whether the running server can read `dir`. A long-running tmux server
    /// on macOS can lose access to protected folders (Documents, Desktop…)
    /// that a fresh terminal still has; sessions it starts there then land in
    /// the wrong directory. No server means a new one will be started by us,
    /// with our own access.
    pub fn probe_access(&self, dir: &str) -> Access {
        if self.run(&["list-sessions", "-F", "#{session_id}"]).is_err() {
            return Access::NoServer;
        }
        let q = format!("'{}'", dir.replace('\'', r"'\''"));
        match self.run(&["run-shell", &format!("test -r {q} && test -x {q}")]) {
            Ok(_) => Access::Readable,
            Err(_) => Access::Denied,
        }
    }

    /// The server's socket path.
    pub fn socket_path(&self) -> Option<String> {
        self.run(&["display-message", "-p", "#{socket_path}"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// 同时核对 socket 与 server PID；不因仅存在 TMUX 就切换另一服务器。
    pub fn matches_client(&self, client: &str) -> bool {
        let mut parts = client.split(',');
        let Some(socket) = parts.next().filter(|value| !value.is_empty()) else {
            return false;
        };
        let Some(pid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
            return false;
        };
        let Ok(server) = self.run(&["display-message", "-p", "#{socket_path}|:|#{pid}"]) else {
            return false;
        };
        server
            .trim()
            .split_once(SEP)
            .is_some_and(|(path, process)| {
                path == socket && process.parse::<u32>().ok() == Some(pid)
            })
    }

    /// The first pane of session `name`, if that session exists.
    pub fn pane_of(&self, name: &str) -> Option<Pane> {
        let target = format!("={name}");
        self.run(&["list-panes", "-t", &target, "-F", PANE_FORMAT])
            .ok()
            .and_then(|out| parse_pane(&out))
    }

    /// Each session's display name ([`NAME_OPTION`]) by session id, empty
    /// where none is set; `None` when tmux did not answer.
    pub fn display_names(&self) -> Option<std::collections::HashMap<String, String>> {
        let format = format!("#{{session_id}}{SEP}#{{{NAME_OPTION}}}");
        let out = self.run(&["list-sessions", "-F", &format]).ok()?;
        Some(
            out.lines()
                .filter_map(|l| l.split_once(SEP))
                .map(|(id, name)| (id.to_string(), name.to_string()))
                .collect(),
        )
    }

    /// Give session `session_id` (`$N`) its own display name, for terminal
    /// titles ([`NAME_OPTION`]).
    pub fn set_display_name(&self, session_id: &str, name: &str) -> Result<()> {
        self.run(&["set-option", "-t", session_id, NAME_OPTION, name])
            .map(drop)
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
        self.run(&["list-sessions", "-F", "#{session_id}|:|#{session_name}"])
            .map(|out| {
                out.lines()
                    .filter_map(|l| l.split_once(SEP))
                    .map(|(id, name)| (id.to_string(), name.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `(pane pid, session id, session name, pane id)` for every pane.
    pub fn panes(&self) -> Vec<(u32, String, String, String)> {
        let fmt = "#{pane_pid}|:|#{session_id}|:|#{session_name}|:|#{pane_id}";
        let Ok(out) = self.run(&["list-panes", "-a", "-F", fmt]) else {
            return Vec::new();
        };
        out.lines()
            .filter_map(|l| {
                let mut f = l.splitn(4, SEP);
                Some((
                    f.next()?.parse().ok()?,
                    f.next()?.to_string(),
                    f.next()?.to_string(),
                    f.next()?.to_string(),
                ))
            })
            .collect()
    }

    /// Every session with its last activity; empty when no server runs.
    pub fn session_infos(&self) -> Vec<SessionInfo> {
        let fmt = "#{session_id}|:|#{session_name}|:|#{session_activity}|:|#{session_attached}|:|#{session_path}";
        let Ok(out) = self.run(&["list-sessions", "-F", fmt]) else {
            return Vec::new();
        };
        out.lines()
            .filter_map(|l| {
                let mut f = l.splitn(5, SEP);
                Some(SessionInfo {
                    id: f.next()?.to_string(),
                    name: f.next()?.to_string(),
                    activity_ms: f.next()?.parse::<i64>().ok()? * 1000,
                    attached: f.next()? != "0",
                    path: f.next().unwrap_or_default().to_string(),
                })
            })
            .collect()
    }

    /// End session `$N` (its programs get SIGHUP).
    pub fn kill_session(&self, session_id: &str) -> Result<()> {
        self.run(&["kill-session", "-t", session_id]).map(drop)
    }

    /// The current name of session `$N`.
    pub fn session_name(&self, session_id: &str) -> Option<String> {
        self.run(&["display-message", "-p", "-t", session_id, "#{session_name}"])
            .ok()
            .map(|s| s.trim_end().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Pane `%N`'s process and whether it is dead (`remain-on-exit`);
    /// `None` when there is no such pane.
    pub fn pane_process(&self, pane_id: &str) -> Option<(u32, bool)> {
        let out = self
            .run(&[
                "display-message",
                "-p",
                "-t",
                pane_id,
                "#{pane_id}|:|#{pane_pid}|:|#{pane_dead}",
            ])
            .ok()?;
        let mut f = out.trim_end().splitn(3, SEP);
        // display-message falls back to the current pane for a gone target.
        if f.next()? != pane_id {
            return None;
        }
        Some((f.next()?.parse().ok()?, f.next()? == "1"))
    }

    /// A pane option as set on the pane itself; `None` when it isn't.
    pub fn pane_option(&self, pane_id: &str, name: &str) -> Option<String> {
        self.run(&["show-options", "-p", "-q", "-v", "-t", pane_id, name])
            .ok()
            .map(|s| s.trim_end().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Set a pane option on the pane only (never globally), or unset it.
    pub fn set_pane_option(&self, pane_id: &str, name: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.run(&["set-option", "-p", "-t", pane_id, name, v]),
            None => self.run(&["set-option", "-p", "-u", "-t", pane_id, name]),
        }
        .map(drop)
    }

    /// Run `argv` in pane `%N` again, in `cwd`: same pane, window and
    /// session. The pane's program must have exited (`remain-on-exit`).
    /// With `kill`, whatever is left in the pane is ended first (`-k`).
    pub fn respawn_pane(&self, pane_id: &str, cwd: &str, argv: &[&str], kill: bool) -> Result<()> {
        let cwd = escape_cwd(cwd);
        let argv: Vec<String> = argv.iter().map(|a| escape_semicolon(a)).collect();
        let mut args = vec!["respawn-pane", "-t", pane_id, "-c", &cwd];
        if kill {
            args.push("-k");
        }
        args.extend(argv.iter().map(String::as_str));
        self.run(&args).map(drop)
    }

    /// Type `text` into pane `%N` as it is, then Enter.
    pub fn type_line(&self, pane_id: &str, text: &str) -> Result<()> {
        self.run(&["send-keys", "-t", pane_id, "-l", text])?;
        self.run(&["send-keys", "-t", pane_id, "Enter"]).map(drop)
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
            "#{session_id}|:|#{session_name}",
        ])
        .ok()
        .map(|text| {
            let mut parts = text.trim_end().splitn(2, SEP);
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

/// The tmux pane a process runs under: walk up its parents until one of them
/// is a pane's process.
pub fn locate_in_panes(
    pid: u32,
    parents: &std::collections::HashMap<u32, u32>,
    panes: &[(u32, String, String, String)],
    socket: &str,
) -> Option<TmuxLoc> {
    let mut current = pid;
    for _ in 0..64 {
        if let Some((_, sid, name, pane)) = panes.iter().find(|p| p.0 == current) {
            return Some(TmuxLoc {
                socket: socket.to_string(),
                session_id: Some(sid.clone()),
                session_name: Some(name.clone()),
                pane_id: pane.clone(),
            });
        }
        current = *parents.get(&current)?;
        if current <= 1 {
            return None;
        }
    }
    None
}
