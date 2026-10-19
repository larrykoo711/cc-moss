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
use moss_core::moss::dir::Dir;
use moss_core::moss::guard::{self, Decision};
use moss_core::moss::home::{self, File, Kind};
use moss_core::moss::keeper::{self, Action, Keeper};
use moss_core::moss::{self, Stored};
use moss_core::moss::{chat, inbox, reply};
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
const SESSIONS_SKILL: &str = include_str!("../../templates/moss/skills/moss-sessions/SKILL.md");
const DOCTOR_SKILL: &str = include_str!("../../templates/moss/skills/moss-doctor/SKILL.md");

/// What the guard hook is called as: `cc-moss hook moss-guard`.
pub const GUARD_EVENT: &str = "moss-guard";

/// What Moss's Stop hook is called as: `cc-moss hook moss-stop`.
pub const STOP_EVENT: &str = "moss-stop";

/// How long tmux may take to answer.
const TMUX_TIMEOUT: Duration = Duration::from_secs(5);

/// Every file of Moss's home; `exe` is the cc-moss its guard hook runs and
/// `state` the state directory the home is in.
pub fn files(exe: &str, state: &Path) -> Vec<File> {
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
        managed(
            ".claude/settings.json",
            &settings_json(exe, state, config_dir().as_deref()),
        ),
        managed(".claude/skills/moss-profile/SKILL.md", PROFILE_SKILL),
        managed(".claude/skills/moss-sessions/SKILL.md", SESSIONS_SKILL),
        managed(".claude/skills/moss-doctor/SKILL.md", DOCTOR_SKILL),
        user("IDENTITY.md", IDENTITY_MD),
        user("SOUL.md", SOUL_MD),
        user("USER.md", USER_MD),
    ]
}

/// The files of Moss's home the sandbox keeps shell commands from writing
/// (cc-moss's, and the persona, which changes only through
/// `cc-moss moss profile`).
const SHELL_READ_ONLY: &[&str] = &[
    "SECURITY.md",
    "CLAUDE.md",
    "SESSIONS.md",
    "IDENTITY.md",
    "SOUL.md",
    "USER.md",
];

/// Where Claude Code keeps every session's transcript, and its own record
/// of your projects and history with the copies it keeps of it
/// (`.claude.json.backup`, `.claude.json.bak.<time>`). The sandbox takes
/// the `*` as a pattern; the plain names stay in case it ever doesn't.
const PRIVATE_READS: &[&str] = &[
    "~/.claude",
    "~/.claude.json",
    "~/.claude.json.backup",
    "~/.claude.json*",
];

/// Claude Code's folder when it is not `~/.claude`: `[claude] config_dir`
/// in cc-moss's config, else `CLAUDE_CONFIG_DIR`. Claude Code keeps there
/// what it otherwise keeps in `~/.claude` and `~/.claude.json`.
pub(super) fn config_dir() -> Option<PathBuf> {
    let configured = moss_core::config::Config::load()
        .ok()
        .and_then(|c| c.claude.config_dir);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    resolve_config_dir(
        configured.as_deref(),
        std::env::var_os("CLAUDE_CONFIG_DIR")
            .as_deref()
            .map(Path::new),
        home.as_deref(),
    )
}

/// The config's folder (`~` read as `home`) over the environment's; only
/// an absolute one.
fn resolve_config_dir(
    configured: Option<&str>,
    env: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let configured = configured
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| match (c.strip_prefix('~'), home) {
            (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with('/') => {
                h.join(rest.trim_start_matches('/'))
            }
            _ => PathBuf::from(c),
        });
    configured
        .or_else(|| env.map(Path::to_path_buf))
        .filter(|p| p.is_absolute())
}

/// `CLAUDE_CONFIG_DIR` as this shell has it, when Moss's settings
/// (`settings`, the text of its `.claude/settings.json`) don't keep Moss
/// out of it: the daemon didn't see it.
pub fn fence_gap(settings: &str, env: Option<&Path>) -> Option<PathBuf> {
    let env = env.filter(|e| e.is_absolute())?;
    let value: serde_json::Value = serde_json::from_str(settings).ok()?;
    let covered = value["sandbox"]["filesystem"]["denyRead"]
        .as_array()
        .is_some_and(|all| {
            all.iter()
                .filter_map(serde_json::Value::as_str)
                .any(|d| env.starts_with(d))
        });
    (!covered).then(|| env.to_path_buf())
}

/// The one command Moss runs outside the sandbox: it writes the persona
/// files the sandbox keeps read-only, and nothing else.
const PROFILE_COMMAND: &str = "cc-moss moss profile *";

/// Moss's project settings.
///
/// - The Stop hook marks every ended turn, so the daemon takes the reply.
/// - The guard before every tool call (`*`). The `|| exit 2` blocks the
///   tool also when cc-moss cannot run at all (missing, crashed): Claude
///   Code lets a tool through on any other failure. The home is in the
///   command, not taken from the environment.
/// - The Bash sandbox, on and strict: shell commands write only in the home
///   (the working directory), the per-user temp folder and the registry
///   cc-moss's own commands update; no retry outside it; Claude Code
///   refuses to start rather than run without it.
fn settings_json(exe: &str, state: &Path, config_dir: Option<&Path>) -> String {
    let home = moss::home_dir(state);
    let command = format!(
        "{} hook {GUARD_EVENT} --home {} || exit 2",
        shell_quote(exe),
        shell_quote(&home.to_string_lossy())
    );
    let stop = format!(
        "{} hook {STOP_EVENT} --home {}",
        shell_quote(exe),
        shell_quote(&home.to_string_lossy())
    );
    // Nothing outside the home: not even the registry. Moss reads it (cc-moss
    // opens it read-only for Moss); what it changes goes through `moss act`.
    let allow_write: Vec<String> = Vec::new();
    let deny_write: Vec<String> = SHELL_READ_ONLY
        .iter()
        .chain(guard::CONVERSATION)
        .chain(&[".claude"])
        .map(|f| format!("./{f}"))
        .collect();
    let deny_read: Vec<String> = PRIVATE_READS
        .iter()
        .map(|p| p.to_string())
        .chain(config_dir.map(|c| c.to_string_lossy().into_owned()))
        .chain([state.to_string_lossy().into_owned()])
        .collect();
    let value = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "*",
                "hooks": [{ "type": "command", "command": command }]
            }],
            "Stop": [{
                "hooks": [{ "type": "command", "command": stop }]
            }]
        },
        // No file tools at all (a deny rule holds in bypass mode too): they
        // check a path and open it later, outside the sandbox, so a link
        // swapped in between leads them anywhere. Moss reads and writes
        // with the shell, where the sandbox decides at the moment of use.
        "permissions": {
            "deny": guard::FILE_TOOLS
        },
        "sandbox": {
            "enabled": true,
            "failIfUnavailable": true,
            "allowUnsandboxedCommands": false,
            "autoAllowBashIfSandboxed": true,
            "excludedCommands": [PROFILE_COMMAND],
            "filesystem": {
                "allowWrite": allow_write,
                "denyWrite": deny_write,
                // Other sessions' conversations are not Moss's to read: not
                // Claude Code's folder, not cc-moss's state (the registry, other sessions' questions),
                // except its own home (the working directory) inside it.
                "denyRead": deny_read,
                "allowRead": ["."]
            },
            // It looks after sessions on this Mac: no network.
            "network": {
                "allowedDomains": []
            }
        }
    });
    // Moss starts with project settings only (`--setting-sources project`):
    // cc-moss's own hooks, which record it, go here too, and the bypass
    // warning is answered as in the user's settings.
    let value = moss_core::settings_json::install(&value, exe)
        .and_then(|v| moss_core::settings_json::accept_bypass_prompt(&v))
        .unwrap_or(value);
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

/// The tmux socket name for Moss. Debug builds take `CC_MOSS_MOSS_SOCKET`.
/// A test that did not set it (`CC_MOSS_FORBID_LAUNCHCTL` is there) gets
/// `None`, and so does every test of a release build, where the override is
/// not read: no test ever reaches the real Moss.
fn socket() -> Option<String> {
    if let Some(name) = app::test_override("CC_MOSS_MOSS_SOCKET") {
        return Some(name.to_string_lossy().into_owned()).filter(|n| !n.is_empty());
    }
    if std::env::var_os("CC_MOSS_FORBID_LAUNCHCTL").is_some() {
        return None;
    }
    Some(moss::SOCKET.to_string())
}

/// `tmux -L <socket> -f /dev/null <args>`: Moss's server never reads the
/// user's tmux.conf (no plugins, no `remain-on-exit` or `destroy-unattached`
/// that would change how Moss lives and dies).
pub(super) fn tmux(socket: &str, args: &[&str]) -> Result<Output, RunError> {
    run_with_timeout(
        Command::new("tmux")
            .arg("-L")
            .arg(socket)
            .arg("-f")
            .arg("/dev/null")
            .args(args),
        TMUX_TIMEOUT,
    )
}

/// What Moss's tmux server holds. A dead Moss kept on screen by
/// `remain-on-exit` is cleared away here.
pub(super) fn look(socket: &str) -> keeper::Seen {
    let seen = match tmux(socket, &["list-panes", "-a", "-F", keeper::PANES_FORMAT]) {
        Ok(out) => keeper::classify_panes(
            out.status.success(),
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        ),
        Err(_) => keeper::Seen {
            liveness: Liveness::Unknown,
            live: vec![],
            dead: vec![],
        },
    };
    for id in &seen.dead {
        let _ = tmux(socket, &["kill-session", "-t", id]);
    }
    seen
}

/// Whether Moss runs. Only tmux saying plainly that it does not counts as
/// dead.
fn liveness(socket: &str) -> Liveness {
    look(socket).liveness
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

/// The config file the daemon reads (the one its LaunchAgent pins), so
/// `moss stop` from a shell with its own `XDG_CONFIG_HOME` still reaches it.
fn config_file() -> PathBuf {
    daemon::pinned_config().unwrap_or_else(moss_core::config::config_path)
}

fn enabled() -> Result<bool> {
    Ok(Config::load_from(&config_file())?.moss.enabled)
}

/// `[moss]` in the config file the daemon reads: on, and news on.
fn switches() -> Result<moss_core::config::MossConfig> {
    Ok(Config::load_from(&config_file())?.moss)
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
    let ended = end_moss(&state(), "moss-stopped").map_err(|e| {
        anyhow::anyhow!(tr!(
            format!(
                "Moss is switched off, but cc-moss could not make sure it stopped: {e:#}. \
                 The daemon keeps trying; see `cc-moss moss status`."
            ),
            format!(
                "Moss 已关闭，但 cc-moss 没能确认它停下了：{e:#}。守护进程会继续尝试；可以看看 `cc-moss moss status`。"
            )
        ))
    })?;
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

/// End Moss without touching the switch, for `daemon stop`: the daemon is
/// going, so Moss goes with it, and `daemon start` brings both back.
pub fn end_with_daemon() -> Result<bool> {
    end_moss(&state(), "daemon-stopped")
}

fn set_enabled(on: bool) -> Result<()> {
    super::config::store_at(
        &config_file(),
        "moss.enabled",
        if on { "true" } else { "false" },
    )
}

/// End Moss's tmux session and close its records. True when one was
/// running. When tmux cannot say that it is gone (it does not answer, or the
/// session survives), this is an error and the records stay as they are:
/// the caller must not report a stop that may not have happened.
fn end_moss(state: &Path, reason: &str) -> Result<bool> {
    let Some(socket) = socket() else {
        return Ok(false);
    };
    let before = look(&socket);
    if before.liveness != Liveness::Dead {
        for id in &before.live {
            let _ = tmux(&socket, &["kill-session", "-t", id]);
        }
        if before.liveness == Liveness::Unknown {
            // tmux did not say what runs: end it by the name it starts with.
            let target = format!("={}", moss::SESSION);
            let _ = tmux(&socket, &["kill-session", "-t", &target]);
        }
    }
    match look(&socket).liveness {
        Liveness::Dead => {}
        Liveness::Alive => bail!("tmux could not end Moss's session"),
        Liveness::Unknown => bail!("tmux did not say whether Moss's session is gone"),
    }
    let db = state.join("registry.db");
    if db.exists() {
        Registry::open(&db)?.close_role(moss::ROLE, &socket, None, reason, now_ms())?;
    }
    moss::remove(&moss::status_path(state))?;
    moss::clear_error(state);
    Ok(before.liveness == Liveness::Alive)
}

/// What `moss status` reports.
pub fn report() -> moss::Report {
    let state = state();
    let on = enabled().unwrap_or(true);
    let stored = moss::read(&moss::status_path(&state));
    let daemon_running = daemon::report().running;
    let running = match socket().map(|s| liveness(&s)) {
        Some(Liveness::Alive) => true,
        Some(Liveness::Dead) | None => false,
        // tmux did not say: believe the daemon's record while it runs.
        Some(Liveness::Unknown) => stored.is_some() && daemon_running,
    };
    let last_error = moss::read_error(&state);
    let mut r = moss::Report::new(on, running, stored.as_ref(), daemon_running, last_error);
    let home = moss::home_dir(&state);
    r.queued = inbox::count_yours(&home);
    r.input_busy = home.join(inbox::INPUT_BUSY).exists();
    r.last_reply_at = chat::last_reply_at(&home);
    let turn = if running {
        transcript_of_moss(&state).and_then(|t| moss_core::transcript::turn_open(&t))
    } else {
        None
    };
    // What the daemon (and so Moss) runs with; else this process's view.
    r.account = std::fs::read_to_string(moss::account_path(&state))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| moss::account::current().as_str().to_string());
    let waiting = reply::waiting_path(&home).exists();
    r.activity = moss::activity(on, running, daemon_running, waiting, turn);
    r
}

/// The transcript of the Moss that runs, from its record.
fn transcript_of_moss(state: &Path) -> Option<PathBuf> {
    let db = state.join("registry.db");
    if !db.exists() {
        return None;
    }
    Registry::open(&db)
        .ok()?
        .slots()
        .ok()?
        .into_iter()
        .filter(|s| s.is_moss() && s.status != moss_core::slot::Status::Closed)
        .max_by_key(|s| s.last_active_at_ms)?
        .transcript_path
        .map(PathBuf::from)
}

// ---------------------------------------------------------------- say / chat

/// `cc-moss moss say -- <text>`: leave a message for Moss. The daemon types
/// it in when Moss is free; the reply lands in chat.jsonl.
pub fn say(json: bool, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!(tr!(
            "There is nothing to say: the message is empty.",
            "没有内容：消息是空的。"
        ));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        bail!(tr!(
            "The message has control characters in it; send plain text.",
            "消息里有控制字符；请发普通文字。"
        ));
    }
    if !enabled()? {
        bail!(tr!(
            "Moss is off, so it can't hear you. Turn it on in Settings › Moss (or run `cc-moss moss start`).",
            "Moss 已关闭，听不到你说话。去设置 › Moss 打开它（或运行 `cc-moss moss start`）。"
        ));
    }
    if !daemon::report().running {
        bail!(tr!(
            "The cc-moss daemon that passes your messages to Moss is not running. Start it: cc-moss daemon start",
            "负责把消息转给 Moss 的 cc-moss 守护进程没在运行。启动它：cc-moss daemon start"
        ));
    }
    let home = moss::home_dir(&state());
    let now = now_ms();
    let message = inbox::Message {
        schema: 1,
        id: inbox::new_id(now),
        at: rfc3339(now),
        from: chat::FROM_YOU.to_string(),
        text: text.to_string(),
    };
    store_dir::ensure_private_dir(&home).with_context(|| format!("{}", home.display()))?;
    // The chat first: the daemon's "sent" for it never comes before it.
    chat::append(
        &home,
        &chat::Line::message(&message.id, &message.at, chat::FROM_YOU, text),
    )
    .context("cannot write chat.jsonl")?;
    if let Err(e) = inbox::put(&home, &message) {
        let error = format!("{e}");
        let _ = chat::append(
            &home,
            &chat::Line::state(&message.id, &message.at, "failed", Some(&error)),
        );
        return Err(e).context("cannot leave the message in Moss's inbox");
    }
    if json {
        println!("{}", serde_json::json!({ "schema": 1, "id": message.id }));
    }
    Ok(())
}

/// `cc-moss moss chat [--json] [--limit N]`: the conversation so far.
pub fn chat(json: bool, limit: usize) -> Result<()> {
    let limit = limit.clamp(1, 1000);
    let home = moss::home_dir(&state());
    if json {
        let db = state().join("registry.db");
        let registry = if db.exists() {
            Registry::open(&db).ok()
        } else {
            None
        };
        // What an update said of a session, from the registry's events.
        let told = |slot: &str, from: i64, to: i64| -> Option<String> {
            let events = registry.as_ref()?.events_between(slot, from, to).ok()?;
            if let Some(e) = events.iter().find(|e| e.kind == "needs_you") {
                let why = e
                    .detail
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .filter(|w| matches!(*w, "permission" | "question" | "input"))
                    .unwrap_or("input");
                return Some(format!("needs-you-{why}"));
            }
            events
                .iter()
                .any(|e| e.kind == "interrupted")
                .then(|| "dropped".to_string())
        };
        println!("{}", chat::chat_json_with(&home, limit, &told));
        return Ok(());
    }
    let messages = chat::fold(&home, limit);
    if messages.is_empty() {
        println!("{}", tr!("Nothing said yet.", "还没有对话。"));
    }
    for m in &messages {
        let who = match m.from.as_str() {
            chat::FROM_YOU => tr!("You", "你"),
            chat::FROM_MOSS => "Moss",
            _ => tr!("Update", "动态"),
        };
        let state = match m.state.as_deref() {
            Some("queued") => tr!(" (queued)", "（排队中）"),
            Some("failed") => tr!(" (not delivered)", "（没送到）"),
            _ => "",
        };
        println!("{} {who}{state}: {}", m.at, m.text);
    }
    Ok(())
}

// ---------------------------------------------------------------- stop hook

/// `cc-moss hook moss-stop --home <home>`: Moss's Stop hook marks the end
/// of a turn for the daemon. Never fails, prints nothing.
pub fn stop_hook(args: &[std::ffi::OsString]) -> ExitCode {
    if let Some(home) = home_arg(args) {
        let _ = reply::mark_stop(&home, now_ms());
    }
    ExitCode::SUCCESS
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
pub fn profile(name: &str, from: Option<&Path>) -> Result<()> {
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
    match from {
        Some(path) => {
            // It runs outside Moss's sandbox: only a plain file in Moss's
            // own home may become its profile, opened folder by folder from
            // the home and never through a link, so a link swapped in while
            // it runs leads nowhere.
            let home = moss::home_dir(&state());
            let read = in_home(&home, path).and_then(|rel| {
                Dir::home(&home)
                    .and_then(|d| d.read_at(&rel))
                    .with_context(|| format!("cannot read {}", path.display()))
            });
            text = match read {
                Ok(text) => text,
                Err(e) => {
                    return Err(e.context(tr!(
                        format!(
                            "{} is not in Moss's home; save the draft there (drafts/) first",
                            path.display()
                        ),
                        format!(
                            "{} 不在 Moss 的家里；先把草稿存到那里（drafts/）",
                            path.display()
                        )
                    )))
                }
            };
        }
        None => {
            std::io::stdin()
                .read_to_string(&mut text)
                .context("cannot read the new text from standard input")?;
        }
    }
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
    home::ensure_real_dir(&home, ".snapshots")?;
    let dir = Dir::home(&home).with_context(|| format!("{}", home.display()))?;
    if let Ok(old) = dir.read(file) {
        // Never through a link: `.snapshots` is opened as a real folder
        // and written through that, and the file is replaced, not followed.
        let stamp = rfc3339(now_ms()).replace(':', "-");
        let stem = file.trim_end_matches(".md");
        dir.sub(".snapshots")?
            .write(&format!("{stem}-{stamp}.md"), &old)?;
    }
    dir.write(file, &text)?;
    println!(
        "{}",
        tr!(format!("Saved {file}."), format!("已保存 {file}。"))
    );
    Ok(())
}

/// `path` as a path inside `home` (plain names only), or why not.
fn in_home(home: &Path, path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    // The home as named, or as the system names it (/var is /private/var).
    let real = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let rel = absolute
        .strip_prefix(home)
        .or_else(|_| absolute.strip_prefix(&real))
        .map_err(|_| anyhow::anyhow!("{} is outside the home", path.display()))?;
    if !rel
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        bail!("{} is not a plain path inside the home", path.display());
    }
    Ok(rel.to_path_buf())
}

// ---------------------------------------------------------------- guard hook

/// `cc-moss hook moss-guard`: Moss's PreToolUse fence. Exit 0 lets the tool
/// run; exit 2 with the reason on stderr stops it. Anything that goes wrong
/// stops it too (fail closed).
pub fn guard_hook(args: &[std::ffi::OsString]) -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    let home = home_arg(args);
    let decision = std::panic::catch_unwind(|| guard_decision(home))
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

/// `--home <dir>` from the hook's command line: the home the settings file
/// was written for.
fn home_arg(args: &[std::ffi::OsString]) -> Option<PathBuf> {
    let at = args.iter().position(|a| a == "--home")?;
    args.get(at + 1)
        .filter(|h| Path::new(h).is_absolute())
        .map(PathBuf::from)
}

fn guard_decision(home: Option<PathBuf>) -> Result<Decision> {
    // Settings without it are from an older cc-moss: refuse until they are
    // rewritten.
    home.context("no --home in the guard's command (settings from an older cc-moss?)")?;
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
    Ok(guard::decide(tool, tool_input))
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
    match setup::hook_exe()
        .and_then(|exe| Ok(home::lay_out(&home, &files(&exe, &store_dir::state_dir()))?))
    {
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
    home::remove_managed(&home, &files(&exe, &store_dir::state_dir()))?;
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
    /// `CLAUDE_CONFIG_DIR` here, when Moss's fence doesn't cover it.
    pub fence_gap: Option<PathBuf>,
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
        // Only what the daemon has: CC_MOSS_CLAUDE_BIN in this shell is not
        // in its environment.
        claude_found: found("claude"),
        tmux_found: found("tmux"),
        fence_gap: std::fs::read_to_string(moss::home_dir(&state()).join(".claude/settings.json"))
            .ok()
            .and_then(|s| {
                fence_gap(
                    &s,
                    std::env::var_os("CLAUDE_CONFIG_DIR")
                        .as_deref()
                        .map(Path::new),
                )
            }),
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
    talk: super::moss_talk::Talk,
    /// The Claude Code account kind and when it was read.
    account: Option<(moss::account::Account, Instant)>,
    /// `[moss] digest`.
    digest: bool,
    /// `[moss] actions`.
    actions: bool,
    /// `[moss] daily_checkup`, `[moss] daily_brief`.
    daily: (bool, bool),
    next_daily: Instant,
    hands: super::act::Hands,
    socket: String,
    state: PathBuf,
    exe: String,
    /// The switch; `None` until config.toml was read once (a file that cannot
    /// be read at start must not count as "on").
    enabled: Option<bool>,
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
/// How often the daemon looks whether the daily checkup is due.
const DAILY_EVERY: Duration = Duration::from_secs(30);

/// SESSIONS.md also lists the sessions closed within this.
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
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
        let talk = super::moss_talk::Talk::new(state);
        // Replies the daemon before this one was still waiting on are lost.
        talk.recover(log);
        Some(Looker {
            keeper: Keeper::new(steps),
            talk,
            account: None,
            digest: true,
            actions: true,
            daily: (true, true),
            next_daily: now,
            hands: super::act::Hands::new(state, &exe),
            socket,
            state: state.to_path_buf(),
            exe,
            enabled: None,
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
            match switches() {
                Ok(moss) => {
                    self.enabled = Some(moss.enabled);
                    self.digest = moss.digest;
                    self.actions = moss.actions;
                    self.daily = (moss.daily_checkup, moss.daily_brief);
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
        // Nothing is started or stopped before the switch was read once.
        let Some(enabled) = self.enabled else {
            return;
        };
        let probe = (now >= self.next_check).then(|| {
            self.next_check = now + self.check_every;
            liveness(&self.socket)
        });
        let was_up = self.keeper.is_up();
        match self.keeper.tick(now, enabled, probe) {
            Action::Nothing => {
                if was_up && !self.keeper.is_up() {
                    log("moss: it is gone; starting it again after a wait");
                }
            }
            // The switch, read again right before a start: `moss stop` turns
            // it off and then ends Moss, and a start on a switch read before
            // that would bring Moss straight back.
            Action::Start => match switches() {
                Ok(moss) if !moss.enabled => {
                    self.enabled = Some(false);
                    let _ = self.keeper.tick(now, false, None);
                }
                _ => self.start(now, log),
            },
            Action::Stop => match end_moss(&self.state, "moss-stopped") {
                Ok(true) => log("moss: stopped (switched off)"),
                Ok(false) => {}
                Err(e) => log(&format!(
                    "moss: error: could not stop it: {e:#}; trying again at the next check"
                )),
            },
        }
        if enabled {
            self.hands.tick(now, self.actions, log);
        }
        if enabled && self.keeper.is_up() {
            self.refresh_sessions(now, log);
            self.daily_checkup(now, log);
            self.talk.tick(&self.socket, now, self.digest, log);
        }
    }

    /// Once a day, the first time the user opens the panel that day (the
    /// app's `panel-opened` mark): the same request as its checkup button.
    fn daily_checkup(&mut self, now: Instant, log: &dyn Fn(&str)) {
        if now < self.next_daily {
            return;
        }
        self.next_daily = now + DAILY_EVERY;
        let (on, brief) = self.daily;
        let path = super::checkup::daily_path(&self.state);
        let last = std::fs::read_to_string(&path).ok();
        let today = super::checkup::today();
        // The first time the panel is opened each day (the app marks it).
        if !super::checkup::daily_due(
            on,
            super::checkup::panel_opened_today(&self.state),
            last.as_deref().map(str::trim),
            &today,
        ) {
            return;
        }
        let zh = crate::i18n::lang() == crate::i18n::Lang::Zh;
        // Marked done only once it went in; else the next look tries again.
        if !self
            .talk
            .ask(&super::checkup::daily_request(zh, brief), now_ms(), log)
        {
            return;
        }
        if let Err(e) = home::write_if_changed(&path, &format!("{today}\n")) {
            log(&format!("moss: error: noting today's checkup: {e}"));
        }
        log("moss: daily checkup asked for");
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
                // A fresh Moss: whatever the old one had in hand is lost.
                self.talk.moss_restarted(log);
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
                moss::clear_error(&self.state);
                // Only now that the new one runs: the old records are closed
                // (never offered for restore), the new one is kept.
                match Registry::open(&self.state.join("registry.db")).and_then(|r| {
                    r.close_role(
                        moss::ROLE,
                        &self.socket,
                        Some(&session_id),
                        "moss-restarted",
                        now_ms(),
                    )
                }) {
                    Ok(_) => {}
                    Err(e) => log(&format!("moss: error: cannot close the old Moss: {e:#}")),
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
                if let Err(io) = moss::write_error(&self.state, &format!("{e:#}")) {
                    log(&format!("moss: error: cannot note the failure: {io}"));
                }
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
        moss::clear_error(&self.state);
        self.next_check = now + self.check_every;
        log("moss: already running; looking after it");
    }

    /// Lay out the home and start a fresh Moss. The old records are left to
    /// the caller, once this one runs.
    fn launch(&self) -> std::result::Result<String, Launch> {
        let home = moss::home_dir(&self.state);
        home::lay_out(&home, &files(&self.exe, &self.state))
            .with_context(|| format!("cannot lay out {}", home.display()))
            .map_err(Launch::Failed)?;
        let claude = launch::claude_bin().map_err(Launch::Failed)?;
        if !moss_core::platform::on_path(&["tmux"]) {
            return Err(Launch::Failed(anyhow::anyhow!("tmux is not on the PATH")));
        }
        let session_id = uuid::Uuid::new_v4().to_string();
        let cwd = home.to_string_lossy().replace('#', "##");
        let env = session_env(&session_id);
        // `=moss:`: exactly that session (set-option takes a pane target).
        let target = format!("={}:", moss::SESSION);
        let args = claude_args(&session_id);
        // Big enough for Claude Code's panels (/usage, /status) to fit.
        let columns = inbox::COLUMNS.to_string();
        let rows = inbox::ROWS.to_string();
        let mut command: Vec<&str> = vec![
            "new-session",
            "-d",
            "-x",
            &columns,
            "-y",
            &rows,
            "-s",
            moss::SESSION,
            "-c",
            &cwd,
        ];
        for e in &env {
            command.extend(["-e", e.as_str()]);
        }
        command.push(&claude);
        command.extend(args.iter().map(String::as_str));
        let out = tmux(
            &self.socket,
            &[
                &command[..],
                &[
                    // How Moss is known, whatever its name becomes.
                    ";",
                    "set-option",
                    "-t",
                    &target,
                    moss::ROLE_OPTION,
                    moss::ROLE,
                    // A detached Moss stays; a Moss that exits is gone, not kept
                    // as a dead pane.
                    ";",
                    "set-option",
                    "-t",
                    &target,
                    "destroy-unattached",
                    "off",
                    ";",
                    "set-option",
                    "-w",
                    "-t",
                    &target,
                    "remain-on-exit",
                    "off",
                    // Nobody attaching shrinks it. On its window only: set
                    // for the whole server, tmux 3.5 and older crash when
                    // another detached session is made there.
                    ";",
                    "set-option",
                    "-w",
                    "-t",
                    &target,
                    "window-size",
                    "manual",
                    // Its terminal tab says Moss.
                    ";",
                    "set-option",
                    "-t",
                    &target,
                    moss_core::tmux::NAME_OPTION,
                    "Moss",
                    ";",
                    "set-option",
                    "-g",
                    "set-titles",
                    "on",
                    ";",
                    "set-option",
                    "-g",
                    "set-titles-string",
                    moss_core::tmux::TITLE_FORMAT,
                ],
            ]
            .concat(),
        )
        .map_err(|e| Launch::Failed(anyhow::anyhow!("tmux did not answer: {e:?}")))?;
        let said = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
        if said.contains("duplicate session") {
            // A session of that name runs: Moss from before the mark was set,
            // or one that is just starting. Mark it and look after it.
            let _ = tmux(
                &self.socket,
                &["set-option", "-t", &target, moss::ROLE_OPTION, moss::ROLE],
            );
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
            let home_dir = moss::home_dir(&self.state);
            // Read at most once a minute: ~/.claude.json can be large.
            let account = match self.account {
                Some((a, at)) if now < at + SESSIONS_EVERY => a,
                _ => {
                    let a = moss::account::current();
                    self.account = Some((a, now));
                    let _ = home::write_if_changed(
                        &moss::account_path(&self.state),
                        &format!("{}\n", a.as_str()),
                    );
                    a
                }
            }
            .as_str();
            if let Err(e) = home::write_generated(
                &home_dir,
                home::SESSIONS_MD,
                &home::sessions_md_since(&rows, now_ms() - DAY_MS, Some(account)),
            ) {
                log(&format!("moss: error: SESSIONS.md: {e}"));
            }
            // The checkup's facts, for Moss (its sandbox can't read the
            // registry): names, folders, states and times, no conversation.
            match super::checkup::report() {
                Ok(facts) => {
                    let text = serde_json::to_string_pretty(&facts).unwrap_or_default();
                    if let Err(e) = home::write_generated(&home_dir, super::checkup::FACTS, &text) {
                        log(&format!("moss: error: facts.json: {e}"));
                    }
                }
                Err(e) => log(&format!("moss: error: facts.json: {e:#}")),
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

/// Claude Code's arguments for Moss. Bypass mode (the guard and the sandbox
/// fence it), its own session id, the boot prompt, and no MCP servers: the
/// user's would load for minutes at every start and fill its context, and
/// the guard refuses their tools anyway. User settings still load, so
/// cc-moss's own hooks record Moss.
/// The environment Moss's Claude Code starts with (`new-session -e`): its
/// role and session id, for cc-moss's hooks, and room to keep a long
/// conversation before compacting: 500k tokens (a plain number; Claude Code
/// caps it to the model's own window when that is smaller).
fn session_env(session_id: &str) -> Vec<String> {
    vec![
        format!("{}={}", moss::ROLE_ENV, moss::ROLE),
        format!("{}={session_id}", moss::SESSION_ENV),
        format!("{AUTO_COMPACT_ENV}={AUTO_COMPACT_WINDOW}"),
    ]
}

/// Claude Code's context window before it compacts on its own.
const AUTO_COMPACT_ENV: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
const AUTO_COMPACT_WINDOW: u32 = 500_000;

fn claude_args(session_id: &str) -> Vec<String> {
    [
        "--dangerously-skip-permissions",
        "--strict-mcp-config",
        // The user's settings (their sandbox, hooks, permissions) never
        // widen Moss's fence; what it needs is in its own.
        "--setting-sources",
        "project",
        "--session-id",
        session_id,
        boot_prompt(),
    ]
    .iter()
    .map(|a| a.to_string())
    .collect()
}

/// How long a fresh Moss has to stay up to count as started.
fn startup_grace() -> Duration {
    test_millis("CC_MOSS_STARTUP_GRACE_MS").unwrap_or(Duration::from_millis(500))
}

pub(super) fn test_millis(var: &str) -> Option<Duration> {
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
        let text = settings_json("/Users/a b/it's/cc-moss", Path::new("/s t/state"), None);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let hook = &v["hooks"]["PreToolUse"][0];
        assert_eq!(hook["matcher"], "*", "every tool, MCP tools too");
        assert_eq!(
            hook["hooks"][0]["command"],
            r"'/Users/a b/it'\''s/cc-moss' hook moss-guard --home '/s t/state/moss' || exit 2"
        );
        // Every ended turn is marked for the daemon, which takes the reply.
        assert_eq!(
            v["hooks"]["Stop"][0]["hooks"][0]["command"],
            r"'/Users/a b/it'\''s/cc-moss' hook moss-stop --home '/s t/state/moss'"
        );
    }

    #[test]
    fn moss_settings_turn_the_strict_sandbox_on() {
        let v: serde_json::Value =
            serde_json::from_str(&settings_json("/bin/cc-moss", Path::new("/s"), None)).unwrap();
        let sandbox = &v["sandbox"];
        assert_eq!(sandbox["enabled"], true);
        assert_eq!(sandbox["failIfUnavailable"], true, "never unsandboxed");
        assert_eq!(
            sandbox["allowUnsandboxedCommands"], false,
            "no retry outside it"
        );
        assert_eq!(
            sandbox["filesystem"]["allowWrite"],
            serde_json::json!([]),
            "the home is the working directory, and nothing else: not even the \
             registry (what Moss changes goes through moss act)"
        );
        let deny = sandbox["filesystem"]["denyWrite"].as_array().unwrap();
        for f in [
            "./SECURITY.md",
            "./CLAUDE.md",
            "./SOUL.md",
            "./IDENTITY.md",
            "./USER.md",
            "./chat.jsonl",
            "./inbox",
            "./stop",
            "./waiting",
        ] {
            assert!(deny.contains(&f.into()), "{f}");
        }
        assert_eq!(
            sandbox["excludedCommands"],
            serde_json::json!(["cc-moss moss profile *"])
        );
        assert!(deny.contains(&"./input-busy".into()));
        // Claude Code's folder and cc-moss's state are not for its shell,
        // except its own home.
        assert_eq!(
            sandbox["filesystem"]["denyRead"],
            serde_json::json!([
                "~/.claude",
                "~/.claude.json",
                "~/.claude.json.backup",
                "~/.claude.json*",
                "/s"
            ])
        );
        assert_eq!(sandbox["filesystem"]["allowRead"], serde_json::json!(["."]));
        // No file tools: Moss reads and writes through the sandboxed shell.
        assert_eq!(
            v["permissions"]["deny"],
            serde_json::json!(["Read", "Write", "Edit", "NotebookEdit", "Grep", "Glob"])
        );
        // Offline: no network from its shell.
        assert_eq!(sandbox["network"]["allowedDomains"], serde_json::json!([]));
        // Its own .claude folder is not for its shell either.
        assert!(deny.contains(&"./.claude".into()), "{deny:?}");
        // Claude Code's folder where CLAUDE_CONFIG_DIR puts it, too.
        let moved: serde_json::Value = serde_json::from_str(&settings_json(
            "/bin/cc-moss",
            Path::new("/s"),
            Some(Path::new("/c")),
        ))
        .unwrap();
        assert_eq!(
            moved["sandbox"]["filesystem"]["denyRead"],
            serde_json::json!([
                "~/.claude",
                "~/.claude.json",
                "~/.claude.json.backup",
                "~/.claude.json*",
                "/c",
                "/s"
            ])
        );
        // What doctor warns about: a CLAUDE_CONFIG_DIR the settings miss.
        let text = serde_json::to_string(&moved).unwrap();
        assert_eq!(fence_gap(&text, Some(Path::new("/c"))), None);
        assert_eq!(fence_gap(&text, Some(Path::new("/c/sub"))), None);
        assert_eq!(fence_gap(&text, None), None);
        assert_eq!(
            fence_gap(&text, Some(Path::new("/other"))),
            Some(PathBuf::from("/other"))
        );
    }

    #[test]
    fn moss_fence_reads_the_config_dir_from_the_config_first() {
        let home = Some(Path::new("/Users/me"));
        assert_eq!(
            resolve_config_dir(Some("~/cc"), Some(Path::new("/env")), home),
            Some(PathBuf::from("/Users/me/cc"))
        );
        assert_eq!(
            resolve_config_dir(None, Some(Path::new("/env")), home),
            Some(PathBuf::from("/env"))
        );
        assert_eq!(resolve_config_dir(Some(" "), None, home), None);
        assert_eq!(resolve_config_dir(Some("relative"), None, home), None);
        assert_eq!(resolve_config_dir(None, Some(Path::new("rel")), home), None);
    }

    #[test]
    fn moss_settings_carry_cc_moss_hooks_of_their_own() {
        // Moss starts with project settings only: the user's settings (and
        // any sandbox widening in them) never reach it, so the hooks that
        // record it are in its own settings.
        let v: serde_json::Value =
            serde_json::from_str(&settings_json("/bin/cc-moss", Path::new("/s"), None)).unwrap();
        let commands = |event: &str| -> Vec<String> {
            v["hooks"][event]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .flat_map(|g| g["hooks"].as_array().cloned().unwrap_or_default())
                .filter_map(|h| h["command"].as_str().map(String::from))
                .collect()
        };
        assert!(commands("SessionStart")
            .iter()
            .any(|c| c.contains("/bin/cc-moss hook session-start")));
        let stop = commands("Stop");
        assert!(
            stop.iter().any(|c| c.contains("hook moss-stop")),
            "{stop:?}"
        );
        assert!(stop.iter().any(|c| c.contains("hook stop")), "{stop:?}");
        let pre = commands("PreToolUse");
        assert!(
            pre.iter().any(|c| c.contains("hook moss-guard")),
            "the guard stays: {pre:?}"
        );
        assert!(commands("SessionEnd")
            .iter()
            .any(|c| c.contains("hook session-end")));
        let args = claude_args("abc");
        let at = args
            .iter()
            .position(|a| a == "--setting-sources")
            .expect("project settings only");
        assert_eq!(args[at + 1], "project");
    }

    #[test]
    fn moss_starts_with_its_role_and_a_large_context() {
        let env = session_env("abc");
        assert!(env.contains(&"CC_MOSS_ROLE=moss".to_string()), "{env:?}");
        assert!(
            env.contains(&"CC_MOSS_MOSS_SESSION=abc".to_string()),
            "{env:?}"
        );
        // A plain number: "500k" does not take. Claude Code caps it to a
        // smaller model's window by itself.
        assert!(
            env.contains(&"CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000".to_string()),
            "{env:?}"
        );
    }

    #[test]
    fn moss_starts_without_mcp_servers() {
        let args = claude_args("abc");
        assert!(args.iter().any(|a| a == "--strict-mcp-config"), "{args:?}");
        assert!(
            !args.iter().any(|a| a.starts_with("--mcp-config")),
            "zero servers"
        );
        assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));
        let at = args.iter().position(|a| a == "--session-id").unwrap();
        assert_eq!(args[at + 1], "abc");
        assert_eq!(args.last().map(String::as_str), Some(boot_prompt()));
    }

    #[test]
    fn guard_takes_its_home_from_the_command_line() {
        let args = |a: &[&str]| a.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        assert_eq!(
            home_arg(&args(&[
                "cc-moss",
                "hook",
                "moss-guard",
                "--home",
                "/s/moss"
            ])),
            Some(PathBuf::from("/s/moss"))
        );
        assert_eq!(home_arg(&args(&["cc-moss", "hook", "moss-guard"])), None);
        assert_eq!(
            home_arg(&args(&["cc-moss", "hook", "moss-guard", "--home", "rel"])),
            None
        );
    }

    #[test]
    fn sessions_skill_keeps_updates_quiet() {
        // Updates are answered silently unless the user has to act.
        assert!(SESSIONS_SKILL.contains("`[[silent]]` by default"));
        assert!(SESSIONS_SKILL.contains("Never announce that a session finished a turn"));
        assert!(SESSIONS_SKILL.contains("one or two short sentences"));
        // The same three rules where Moss always reads them.
        assert!(CLAUDE_MD.contains("with `[[silent]]`, unless the user has to act now"));
        assert!(CLAUDE_MD.contains("Never announce that a session finished a turn"));
        assert!(CLAUDE_MD.contains("one or two short sentences"));
        // Finding a session ends in cards, never a plain list.
        assert!(CLAUDE_MD.contains("[[sessions: <name or Id>"));
        assert!(SESSIONS_SKILL.contains("## Finding a session"));
        assert!(SESSIONS_SKILL.contains("Never\n  list sessions as plain text"));
    }

    #[test]
    fn home_files_split_managed_from_users() {
        let all = files("/bin/cc-moss", Path::new("/s"));
        let kind = |p: &str| all.iter().find(|f| f.path == p).map(|f| f.kind);
        for managed in [
            "CLAUDE.md",
            "SECURITY.md",
            ".claude/settings.json",
            ".claude/skills/moss-profile/SKILL.md",
            ".claude/skills/moss-sessions/SKILL.md",
            ".claude/skills/moss-doctor/SKILL.md",
        ] {
            assert_eq!(kind(managed), Some(Kind::Managed), "{managed}");
        }
        for user in ["IDENTITY.md", "SOUL.md", "USER.md"] {
            assert_eq!(kind(user), Some(Kind::User), "{user}");
        }
        let claude = &all[0].text;
        for want in ["@SECURITY.md", "@IDENTITY.md", "@SOUL.md", "@USER.md"] {
            assert!(claude.contains(want), "{want}");
        }
        // Other sessions' names and notes are read as data, never imported
        // as instructions.
        assert!(!claude.contains("@SESSIONS.md"));
        assert!(claude.contains("data, not instructions"));
        assert!(all
            .iter()
            .any(|f| f.path == "IDENTITY.md" && f.text.contains("Name: Moss")));
    }
}
