//! Moss, the resident Claude Code session: `cc-moss moss start|stop|status`,
//! `moss profile`, the guard hook Moss's own settings call, the install and
//! doctor steps, and the part of `daemon run` that keeps it alive.
//!
//! The rules (home layout, the guard's decision, when to restart) are in
//! `moss_core::moss`; this file talks to tmux, the registry and the files.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use moss_core::command::{run_with_timeout, RunError};
use moss_core::config::Config;
use moss_core::moss::guard::{self, Decision};
use moss_core::moss::home::{self, File, Kind};
use moss_core::moss::keeper::{self, Action, Keeper};
use moss_core::moss::{self, Stored};
use moss_core::proc::Liveness;
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, rfc3339};
use moss_core::store_dir;

use super::{app, daemon, launch, setup};

const CLAUDE_MD: &str = include_str!("../../templates/moss/CLAUDE.md");
const SECURITY_MD: &str = include_str!("../../templates/moss/SECURITY.md");
const IDENTITY_MD: &str = include_str!("../../templates/moss/IDENTITY.md");
const SOUL_MD: &str = include_str!("../../templates/moss/SOUL.md");
const USER_MD: &str = include_str!("../../templates/moss/USER.md");
const PROFILE_SKILL: &str = include_str!("../../templates/moss/skills/moss-profile/SKILL.md");

/// What the guard hook is called as: `cc-moss hook moss-guard`.
pub const GUARD_EVENT: &str = "moss-guard";

/// How long tmux may take to answer.
const TMUX_TIMEOUT: Duration = Duration::from_secs(5);

/// Every file of Moss's home; `exe` is the cc-moss its guard hook runs.
pub fn files(exe: &str) -> Vec<File> {
    let managed = |path, text: &str| File {
        path,
        text: text.to_string(),
        kind: Kind::Managed,
    };
    let user = |path, text: &str| File {
        path,
        text: text.to_string(),
        kind: Kind::User,
    };
    vec![
        managed("CLAUDE.md", CLAUDE_MD),
        managed("SECURITY.md", SECURITY_MD),
        managed(".claude/settings.json", &settings_json(exe)),
        managed(".claude/skills/moss-profile/SKILL.md", PROFILE_SKILL),
        user("IDENTITY.md", IDENTITY_MD),
        user("SOUL.md", SOUL_MD),
        user("USER.md", USER_MD),
    ]
}

/// Moss's project settings: the guard before every write and command. The
/// `|| exit 2` blocks the tool also when cc-moss cannot run at all (missing,
/// crashed): Claude Code lets a tool through on any other failure.
fn settings_json(exe: &str) -> String {
    let command = format!("{} hook {GUARD_EVENT} || exit 2", shell_quote(exe));
    let value = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "Write|Edit|MultiEdit|NotebookEdit|Bash",
                "hooks": [{ "type": "command", "command": command }]
            }]
        }
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}

/// `'text'` for a POSIX shell.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The state directory the daemon uses (the one its LaunchAgent pins).
fn state() -> PathBuf {
    daemon::state()
}

/// The tmux socket name for Moss. Debug builds take `CC_MOSS_MOSS_SOCKET`;
/// a test that did not set it (`CC_MOSS_FORBID_LAUNCHCTL` is there) gets
/// `None`, so no test ever reaches the real Moss.
fn socket() -> Option<String> {
    if let Some(name) = app::test_override("CC_MOSS_MOSS_SOCKET") {
        return Some(name.to_string_lossy().into_owned()).filter(|n| !n.is_empty());
    }
    if cfg!(debug_assertions) && std::env::var_os("CC_MOSS_FORBID_LAUNCHCTL").is_some() {
        return None;
    }
    Some(moss::SOCKET.to_string())
}

/// `tmux -L <socket> <args>`.
fn tmux(socket: &str, args: &[&str]) -> Result<Output, RunError> {
    run_with_timeout(
        Command::new("tmux").arg("-L").arg(socket).args(args),
        TMUX_TIMEOUT,
    )
}

/// Whether Moss's tmux session is there. Only tmux saying plainly that it
/// is not counts as dead.
fn liveness(socket: &str) -> Liveness {
    let target = format!("={}", moss::SESSION);
    match tmux(socket, &["has-session", "-t", &target]) {
        Ok(out) => keeper::classify_has_session(
            out.status.success(),
            &String::from_utf8_lossy(&out.stderr),
        ),
        Err(_) => Liveness::Unknown,
    }
}

/// What Moss is asked first after every start.
fn boot_prompt() -> &'static str {
    tr!(
        "You are Moss. Read SESSIONS.md and today's journal first, then carry on with what was left unfinished; if there is nothing, wait quietly.",
        "你是 Moss。先读 SESSIONS.md 和今天的 journal，接着上次没做完的事；没有就安静待命。"
    )
}

/// What `pick`, `close` and the app say instead of ending Moss.
pub fn protected_hint() -> String {
    tr!(
        "Moss is looked after by cc-moss. To turn it off, go to Settings › Moss (or run `cc-moss moss stop`).",
        "Moss 由 cc-moss 照看，要关掉请到设置 › Moss（或运行 `cc-moss moss stop`）。"
    )
    .to_string()
}

fn enabled() -> Result<bool> {
    Ok(Config::load()?.moss.enabled)
}

// ---------------------------------------------------------------- start / stop / status

/// `cc-moss moss start`: switch Moss on; the daemon starts it.
pub fn start() -> Result<()> {
    set_enabled(true)?;
    if daemon::report().running {
        println!(
            "{}",
            tr!(
                "Moss is on. The cc-moss daemon starts it within a few seconds.",
                "Moss 已打开，cc-moss 守护进程几秒内就会把它拉起来。"
            )
        );
    } else {
        println!(
            "{}",
            tr!(
                "Moss is on, but the cc-moss daemon that runs it is not running. Start it: cc-moss daemon start",
                "Moss 已打开，但负责运行它的 cc-moss 守护进程没在跑。启动它：cc-moss daemon start"
            )
        );
    }
    Ok(())
}

/// `cc-moss moss stop`: switch Moss off and end it now. It stays off until
/// `moss start`, across logins too.
pub fn stop() -> Result<()> {
    set_enabled(false)?;
    let ended = end_moss(&state(), "moss-stopped")?;
    println!(
        "{}",
        if ended {
            tr!(
                "Stopped Moss. It stays off until `cc-moss moss start`.",
                "Moss 已停下；在你运行 `cc-moss moss start` 之前不会再启动。"
            )
        } else {
            tr!(
                "Moss is off. It stays off until `cc-moss moss start`.",
                "Moss 已关闭；在你运行 `cc-moss moss start` 之前不会再启动。"
            )
        }
    );
    Ok(())
}

fn set_enabled(on: bool) -> Result<()> {
    super::config::store("moss.enabled", if on { "true" } else { "false" })
}

/// End Moss's tmux session and close its records. True when one was running.
fn end_moss(state: &Path, reason: &str) -> Result<bool> {
    let mut ended = false;
    if let Some(socket) = socket() {
        if liveness(&socket) == Liveness::Alive {
            let target = format!("={}", moss::SESSION);
            let out = tmux(&socket, &["kill-session", "-t", &target])
                .map_err(|e| anyhow::anyhow!("tmux kill-session: {e:?}"))?;
            if !out.status.success() && liveness(&socket) == Liveness::Alive {
                bail!(
                    "tmux could not end Moss: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            ended = true;
        }
    }
    let db = state.join("registry.db");
    if db.exists() {
        Registry::open(&db)?.close_role(moss::ROLE, reason, now_ms())?;
    }
    moss::remove(&moss::status_path(state))?;
    Ok(ended)
}

/// What `moss status` reports.
pub fn report() -> moss::Report {
    let state = state();
    let on = enabled().unwrap_or(true);
    let stored = moss::read(&moss::status_path(&state));
    let running = match socket().map(|s| liveness(&s)) {
        Some(Liveness::Alive) => true,
        Some(Liveness::Dead) | None => false,
        // tmux did not say: believe the daemon's record while it runs.
        Some(Liveness::Unknown) => stored.is_some() && daemon::report().running,
    };
    moss::Report::new(on, running, stored.as_ref())
}

/// `cc-moss moss status [--json]`.
pub fn print_status(json: bool) -> Result<()> {
    let r = report();
    if json {
        println!("{}", serde_json::to_string(&r)?);
    } else {
        println!("{}", describe(&r));
    }
    Ok(())
}

/// `Moss is running: up 2 hours, restarted once.`
pub fn describe(r: &moss::Report) -> String {
    if !r.enabled {
        return tr!(
            "Moss is off. To turn it on: cc-moss moss start",
            "Moss 已关闭。要打开：cc-moss moss start"
        )
        .to_string();
    }
    if !r.running {
        return tr!(
            "Moss is on but not running right now; the cc-moss daemon starts it (see `cc-moss doctor`).",
            "Moss 已打开，但现在没在运行；cc-moss 守护进程会把它拉起来（看看 `cc-moss doctor`）。"
        )
        .to_string();
    }
    let up = r
        .since_ms
        .map(|ms| daemon::span(now_ms() - ms))
        .unwrap_or_else(|| "?".into());
    match r.restarts {
        0 => tr!(
            format!("Moss is running: up {up}."),
            format!("Moss 运行中：已运行 {up}。")
        ),
        1 => tr!(
            format!("Moss is running: up {up}, restarted once."),
            format!("Moss 运行中：已运行 {up}，重启过 1 次。")
        ),
        n => tr!(
            format!("Moss is running: up {up}, restarted {n} times."),
            format!("Moss 运行中：已运行 {up}，重启过 {n} 次。")
        ),
    }
}

// ---------------------------------------------------------------- profile

/// `cc-moss moss profile <identity|soul|user>`: the new text on stdin. The
/// one way to change Moss's persona; the old file is kept in `.snapshots/`.
pub fn profile(name: &str) -> Result<()> {
    let file = match name.to_ascii_lowercase().as_str() {
        "identity" => "IDENTITY.md",
        "soul" => "SOUL.md",
        "user" => "USER.md",
        _ => bail!(tr!(
            format!("`{name}` is not part of Moss's profile; use identity, soul or user"),
            format!("`{name}` 不是 Moss 档案的一部分；可以用 identity、soul 或 user")
        )),
    };
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("cannot read the new text from standard input")?;
    if text.trim().is_empty() {
        bail!(tr!(
            format!("nothing came in on standard input; {file} is left as it was"),
            format!("标准输入里什么都没有；{file} 保持不变")
        ));
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let home = moss::home_dir(&state());
    let path = home.join(file);
    if let Ok(old) = std::fs::read_to_string(&path) {
        let snaps = home.join(".snapshots");
        store_dir::ensure_private_dir(&snaps)?;
        let stamp = rfc3339(now_ms()).replace(':', "-");
        let stem = file.trim_end_matches(".md");
        std::fs::write(snaps.join(format!("{stem}-{stamp}.md")), old)?;
    }
    store_dir::ensure_private_dir(&home)?;
    home::write_if_changed(&path, &text)?;
    println!(
        "{}",
        tr!(format!("Saved {file}."), format!("已保存 {file}。"))
    );
    Ok(())
}

// ---------------------------------------------------------------- guard hook

/// `cc-moss hook moss-guard`: Moss's PreToolUse fence. Exit 0 lets the tool
/// run; exit 2 with the reason on stderr stops it. Anything that goes wrong
/// stops it too (fail closed).
pub fn guard_hook() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    let decision = std::panic::catch_unwind(guard_decision)
        .unwrap_or_else(|_| Err(anyhow::anyhow!("the guard crashed")));
    match decision {
        Ok(Decision::Allow) => ExitCode::SUCCESS,
        Ok(Decision::Deny(reason)) => {
            eprintln!("cc-moss: {reason}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("cc-moss: the guard could not check this ({e:#}), so it is not allowed");
            ExitCode::from(2)
        }
    }
}

fn guard_decision() -> Result<Decision> {
    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    let input: serde_json::Value =
        serde_json::from_str(&payload).context("unreadable hook input")?;
    let tool = input
        .get("tool_name")
        .and_then(serde_json::Value::as_str)
        .context("no tool_name")?;
    let empty = serde_json::Value::Object(Default::default());
    let tool_input = input.get("tool_input").unwrap_or(&empty);
    let home = std::env::var_os("CLAUDE_PROJECT_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| moss::home_dir(&store_dir::state_dir()));
    let cwd = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| home.clone());
    Ok(guard::decide(&home, &cwd, tool, tool_input))
}

// ---------------------------------------------------------------- install / uninstall

/// `cc-moss install`'s Moss step (macOS): lay out its home. Problems are
/// told and never stop the rest of install.
pub fn install_step(dry_run: bool) -> Result<()> {
    // A relative state directory is refused (and told) by the daemon step.
    if !daemon::applies() || app::as_root() || !store_dir::state_dir().is_absolute() {
        return Ok(());
    }
    let home = moss::home_dir(&store_dir::state_dir());
    if dry_run {
        if !home.join("CLAUDE.md").exists() {
            println!(
                "\n{}",
                tr!(
                    format!("Would set up Moss's home: {}", home.display()),
                    format!("会布置 Moss 的家：{}", home.display())
                )
            );
        }
        return Ok(());
    }
    let fresh = !home.join("CLAUDE.md").exists();
    match setup::hook_exe().and_then(|exe| Ok(home::lay_out(&home, &files(&exe))?)) {
        Ok(_) if fresh => println!(
            "\n{}",
            tr!(
                format!("Set up Moss's home: {}", home.display()),
                format!("已布置 Moss 的家：{}", home.display())
            )
        ),
        Ok(_) => {}
        Err(e) => println!(
            "\n{}",
            tr!(
                format!("Could not set up Moss's home ({}): {e:#}.", home.display()),
                format!("没能布置 Moss 的家（{}）：{e:#}。", home.display())
            )
        ),
    }
    Ok(())
}

/// `cc-moss install --uninstall`'s Moss step: end Moss, and take out
/// cc-moss's files from its home. The persona, journal and memory stay.
pub fn uninstall_step(dry_run: bool) -> Result<()> {
    if !daemon::applies() || app::as_root() || !store_dir::state_dir().is_absolute() {
        return Ok(());
    }
    let home = moss::home_dir(&store_dir::state_dir());
    if !home.exists() {
        return Ok(());
    }
    if dry_run {
        println!(
            "\n{}",
            tr!(
                format!(
                    "Would stop Moss and remove cc-moss's files from {}.",
                    home.display()
                ),
                format!("会停掉 Moss，并从 {} 移除 cc-moss 的文件。", home.display())
            )
        );
        return Ok(());
    }
    if let Err(e) = end_moss(&store_dir::state_dir(), "uninstalled") {
        println!(
            "\n{}",
            tr!(
                format!("Could not stop Moss: {e:#}."),
                format!("没能停掉 Moss：{e:#}。")
            )
        );
    }
    let exe = setup::hook_exe().unwrap_or_default();
    home::remove_managed(&home, &files(&exe))?;
    if home.exists() {
        println!(
            "\n{}",
            tr!(
                format!(
                    "Removed cc-moss's files from Moss's home; its persona, journal and memory stay in {}.",
                    home.display()
                ),
                format!(
                    "已从 Moss 的家移除 cc-moss 的文件；它的人设、日志和记忆还留在 {}。",
                    home.display()
                )
            )
        );
    }
    Ok(())
}

// ---------------------------------------------------------------- doctor

/// What doctor needs to know about Moss.
pub struct Facts {
    pub enabled: bool,
    pub running: bool,
    /// On the PATH the daemon runs with (its LaunchAgent's).
    pub claude_found: bool,
    pub tmux_found: bool,
}

/// `None` where there is no daemon to run Moss (not macOS, root).
pub fn facts() -> Option<Facts> {
    if !daemon::applies() || app::as_root() {
        return None;
    }
    let r = report();
    let path = daemon::pinned_path();
    let found = |name: &str| match &path {
        Some(p) => std::env::split_paths(p).any(|d| is_executable(&d.join(name))),
        None => moss_core::platform::on_path(&[name]),
    };
    Some(Facts {
        enabled: r.enabled,
        running: r.running,
        claude_found: found("claude") || std::env::var_os("CC_MOSS_CLAUDE_BIN").is_some(),
        tmux_found: found("tmux"),
    })
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

// ---------------------------------------------------------------- daemon

/// The part of `daemon run` that looks after Moss. One `tick` per loop of
/// the daemon; it decides itself when to read the switch and ask tmux.
pub struct Looker {
    keeper: Keeper,
    socket: String,
    state: PathBuf,
    exe: String,
    enabled: bool,
    check_every: Duration,
    next_config: Instant,
    next_check: Instant,
    next_sessions: Instant,
    next_seq: Instant,
    last_seq: Option<i64>,
    config_error: bool,
}

/// How often the switch in config.toml is read.
const CONFIG_EVERY: Duration = Duration::from_secs(1);
/// How often SESSIONS.md is rewritten at the latest.
const SESSIONS_EVERY: Duration = Duration::from_secs(60);
/// How often the registry is asked whether anything changed.
const SEQ_EVERY: Duration = Duration::from_secs(2);

impl Looker {
    /// `None` when Moss is out of reach (a test that gave no socket).
    pub fn new(state: &Path, log: &dyn Fn(&str)) -> Option<Looker> {
        let Some(socket) = socket() else {
            log("moss: not looked after (a test without CC_MOSS_MOSS_SOCKET)");
            return None;
        };
        if !state.is_absolute() {
            log("moss: not looked after: the state directory is not an absolute path");
            return None;
        }
        let exe = setup::hook_exe().unwrap_or_else(|_| "cc-moss".into());
        let steps = test_millis_list("CC_MOSS_MOSS_STEPS_MS");
        let check_every = test_millis("CC_MOSS_MOSS_CHECK_MS").unwrap_or(keeper::CHECK_EVERY);
        let now = Instant::now();
        Some(Looker {
            keeper: Keeper::new(steps),
            socket,
            state: state.to_path_buf(),
            exe,
            enabled: true,
            check_every,
            next_config: now,
            next_check: now + check_every,
            next_sessions: now,
            next_seq: now,
            last_seq: None,
            config_error: false,
        })
    }

    pub fn tick(&mut self, now: Instant, log: &dyn Fn(&str)) {
        if now >= self.next_config {
            self.next_config = now + CONFIG_EVERY;
            match enabled() {
                Ok(on) => {
                    self.enabled = on;
                    self.config_error = false;
                }
                Err(e) if !self.config_error => {
                    self.config_error = true;
                    log(&format!(
                        "moss: cannot read config.toml ({e:#}); keeping Moss as it was"
                    ));
                }
                Err(_) => {}
            }
        }
        let probe = (now >= self.next_check).then(|| {
            self.next_check = now + self.check_every;
            liveness(&self.socket)
        });
        let was_up = self.keeper.is_up();
        match self.keeper.tick(now, self.enabled, probe) {
            Action::Nothing => {
                if was_up && !self.keeper.is_up() {
                    log("moss: it is gone; starting it again after a wait");
                }
            }
            Action::Start => self.start(now, log),
            Action::Stop => match end_moss(&self.state, "moss-stopped") {
                Ok(true) => log("moss: stopped (switched off)"),
                Ok(false) => {}
                Err(e) => log(&format!("moss: error: could not stop it: {e:#}")),
            },
        }
        if self.enabled && self.keeper.is_up() {
            self.refresh_sessions(now, log);
        }
    }

    fn start(&mut self, now: Instant, log: &dyn Fn(&str)) {
        // It may be back by itself, or still there from before the daemon
        // restarted: look after that one rather than start a second.
        if liveness(&self.socket) == Liveness::Alive {
            self.adopt(now, log);
            return;
        }
        match self.launch() {
            Ok(session_id) => {
                self.keeper.started(now);
                let restarts = self.keeper.restarts();
                let stored = Stored {
                    schema: 1,
                    started_ms: now_ms(),
                    restarts,
                    session_id: session_id.clone(),
                };
                if let Err(e) = moss::write(&moss::status_path(&self.state), &stored) {
                    log(&format!("moss: error: cannot write its status: {e}"));
                }
                self.next_check = now + self.check_every;
                self.next_sessions = now;
                log(&if restarts == 0 {
                    format!("moss: started (session {session_id})")
                } else {
                    format!("moss: started again (session {session_id}, restart {restarts})")
                });
            }
            Err(Launch::AlreadyThere) => self.adopt(now, log),
            Err(Launch::Failed(e)) => {
                let at = self.keeper.start_failed(now);
                let wait = at.saturating_duration_since(now).as_secs();
                log(&format!(
                    "moss: error: could not start it: {e:#}; trying again in {wait}s"
                ));
            }
        }
    }

    fn adopt(&mut self, now: Instant, log: &dyn Fn(&str)) {
        let path = moss::status_path(&self.state);
        let stored = moss::read(&path);
        self.keeper
            .adopted(now, stored.as_ref().map_or(0, |s| s.restarts));
        if stored.is_none() {
            let _ = moss::write(
                &path,
                &Stored {
                    schema: 1,
                    started_ms: now_ms(),
                    restarts: 0,
                    session_id: String::new(),
                },
            );
        }
        self.next_check = now + self.check_every;
        log("moss: already running; looking after it");
    }

    /// Lay out the home, close the old records and start a fresh Moss.
    fn launch(&self) -> std::result::Result<String, Launch> {
        let home = moss::home_dir(&self.state);
        home::lay_out(&home, &files(&self.exe))
            .with_context(|| format!("cannot lay out {}", home.display()))
            .map_err(Launch::Failed)?;
        let claude = launch::claude_bin().map_err(Launch::Failed)?;
        if !moss_core::platform::on_path(&["tmux"]) {
            return Err(Launch::Failed(anyhow::anyhow!("tmux is not on the PATH")));
        }
        let registry = Registry::open(&self.state.join("registry.db")).map_err(Launch::Failed)?;
        registry
            .close_role(moss::ROLE, "moss-restarted", now_ms())
            .map_err(Launch::Failed)?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let cwd = home.to_string_lossy().replace('#', "##");
        let role = format!("{}={}", moss::ROLE_ENV, moss::ROLE);
        // `=moss:`: exactly that session (set-option takes a pane target).
        let target = format!("={}:", moss::SESSION);
        let out = tmux(
            &self.socket,
            &[
                "new-session",
                "-d",
                "-s",
                moss::SESSION,
                "-c",
                &cwd,
                "-e",
                &role,
                &claude,
                "--dangerously-skip-permissions",
                "--session-id",
                &session_id,
                boot_prompt(),
                // Whatever the user's tmux.conf says, a detached Moss stays.
                ";",
                "set-option",
                "-t",
                &target,
                "destroy-unattached",
                "off",
            ],
        )
        .map_err(|e| Launch::Failed(anyhow::anyhow!("tmux did not answer: {e:?}")))?;
        let said = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
        if said.contains("duplicate session") {
            return Err(Launch::AlreadyThere);
        }
        if !out.status.success() && liveness(&self.socket) != Liveness::Alive {
            return Err(Launch::Failed(anyhow::anyhow!(
                "tmux new-session failed: {}",
                said.trim()
            )));
        }
        std::thread::sleep(startup_grace());
        if liveness(&self.socket) == Liveness::Dead {
            return Err(Launch::Failed(anyhow::anyhow!(
                "Claude Code exited right after it started; run `{claude}` in {} to see why",
                home.display()
            )));
        }
        Ok(session_id)
    }

    /// Rewrite SESSIONS.md every minute, or sooner when the registry moved.
    fn refresh_sessions(&mut self, now: Instant, log: &dyn Fn(&str)) {
        let mut due = now >= self.next_sessions;
        let seq = if due || now >= self.next_seq {
            self.next_seq = now + SEQ_EVERY;
            let registry = match Registry::open(&self.state.join("registry.db")) {
                Ok(r) => r,
                Err(e) => {
                    self.next_sessions = now + SESSIONS_EVERY;
                    log(&format!("moss: error: SESSIONS.md: {e:#}"));
                    return;
                }
            };
            let seq = registry.last_event_seq().ok();
            due |= seq != self.last_seq;
            if !due {
                return;
            }
            let rows: Vec<_> = match registry.slots() {
                Ok(slots) => slots
                    .into_iter()
                    .map(|s| (s.effective_status(moss_core::proc::is_alive), s))
                    .collect(),
                Err(e) => {
                    log(&format!("moss: error: SESSIONS.md: {e:#}"));
                    return;
                }
            };
            let path = moss::home_dir(&self.state).join(home::SESSIONS_MD);
            if let Err(e) = home::write_if_changed(&path, &home::sessions_md(&rows)) {
                log(&format!("moss: error: SESSIONS.md: {e}"));
            }
            self.next_sessions = now + SESSIONS_EVERY;
            seq
        } else {
            return;
        };
        self.last_seq = seq;
    }
}

enum Launch {
    /// tmux says a `moss` session exists already.
    AlreadyThere,
    Failed(anyhow::Error),
}

/// How long a fresh Moss has to stay up to count as started.
fn startup_grace() -> Duration {
    test_millis("CC_MOSS_STARTUP_GRACE_MS").unwrap_or(Duration::from_millis(500))
}

fn test_millis(var: &str) -> Option<Duration> {
    app::test_override(var)
        .and_then(|v| v.to_str()?.parse().ok())
        .map(Duration::from_millis)
}

/// `30,60` → those waits; empty when unset (the real ladder).
fn test_millis_list(var: &str) -> Vec<Duration> {
    app::test_override(var)
        .map(|v| {
            v.to_string_lossy()
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .map(Duration::from_millis)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_settings_run_this_cc_moss_and_fail_closed() {
        let text = settings_json("/Users/a b/it's/cc-moss");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let hook = &v["hooks"]["PreToolUse"][0];
        assert_eq!(hook["matcher"], "Write|Edit|MultiEdit|NotebookEdit|Bash");
        assert_eq!(
            hook["hooks"][0]["command"],
            r"'/Users/a b/it'\''s/cc-moss' hook moss-guard || exit 2"
        );
    }

    #[test]
    fn home_files_split_managed_from_users() {
        let all = files("/bin/cc-moss");
        let kind = |p: &str| all.iter().find(|f| f.path == p).map(|f| f.kind);
        for managed in [
            "CLAUDE.md",
            "SECURITY.md",
            ".claude/settings.json",
            ".claude/skills/moss-profile/SKILL.md",
        ] {
            assert_eq!(kind(managed), Some(Kind::Managed), "{managed}");
        }
        for user in ["IDENTITY.md", "SOUL.md", "USER.md"] {
            assert_eq!(kind(user), Some(Kind::User), "{user}");
        }
        let claude = &all[0].text;
        for want in [
            "@SECURITY.md",
            "@IDENTITY.md",
            "@SOUL.md",
            "@USER.md",
            "@SESSIONS.md",
        ] {
            assert!(claude.contains(want), "{want}");
        }
        assert!(all
            .iter()
            .any(|f| f.path == "IDENTITY.md" && f.text.contains("Name: Moss")));
    }
}
