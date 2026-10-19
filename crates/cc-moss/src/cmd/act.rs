//! Moss's hands (T2.4): `cc-moss moss act <action> …` leaves a request in
//! `<home>/actions/<id>.json`; the daemon checks it, carries it out (Moss's
//! sandbox cannot reach tmux or the user's sessions), writes the result next
//! to it and a line to `<state>/audit.jsonl`. `--wait` reads the result.
//!
//! Actions that lose nothing run at once: `setup-fix`, `sync-name`, `name`,
//! `forget-gone`. Those that start or end work or forget a session the user
//! may want (`restore`, `forget`, `end`) need `--because <id>`: a message of
//! the user's, under ten minutes old, answering a Moss message with buttons.
//! Moss cannot write the conversation file, so it cannot make one up.
//! Nothing is ever done to Moss itself.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use moss_core::moss::act::{audit_path, dir, result_path, Outcome, Request};
use moss_core::moss::chat::{self, Line};
use moss_core::moss::inbox::{self, Pane, Typed};
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, rfc3339, Slot, Status};
use moss_core::tmux::{Socket, Tmux};
use moss_core::{moss, store_dir};

/// How old the user's go-ahead may be.
const CONSENT_MS: i64 = 10 * 60 * 1000;

/// How long `--wait` waits for the daemon.
const WAIT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SetupFix,
    SyncName(String),
    Name(String, String),
    ForgetGone(String),
    Restore(String),
    Forget(String),
    End(String),
}

impl Action {
    pub fn parse(action: &str, args: &[String]) -> Result<Action, String> {
        let one = |make: fn(String) -> Action| match args {
            [s] => Ok(make(s.clone())),
            _ => Err(format!("{action} takes one session")),
        };
        match action {
            "setup-fix" if args.is_empty() => Ok(Action::SetupFix),
            "setup-fix" => Err("setup-fix takes nothing".into()),
            "sync-name" => one(Action::SyncName),
            "forget-gone" => one(Action::ForgetGone),
            "restore" => one(Action::Restore),
            "forget" => one(Action::Forget),
            "end" => one(Action::End),
            "name" => match args {
                [s, name] => {
                    let name = name.trim();
                    if name.is_empty()
                        || name.chars().count() > 60
                        || name.chars().any(char::is_control)
                    {
                        Err("a name is one line of at most 60 characters".into())
                    } else {
                        Ok(Action::Name(s.clone(), name.to_string()))
                    }
                }
                _ => Err("name takes a session and a name".into()),
            },
            other => Err(format!("`{other}` is not an action Moss can take")),
        }
    }

    /// Whether the user has to have said yes.
    pub fn needs_consent(&self) -> bool {
        matches!(
            self,
            Action::Restore(_) | Action::Forget(_) | Action::End(_)
        )
    }

    fn session(&self) -> Option<&str> {
        match self {
            Action::SetupFix => None,
            Action::SyncName(s)
            | Action::Name(s, _)
            | Action::ForgetGone(s)
            | Action::Restore(s)
            | Action::Forget(s)
            | Action::End(s) => Some(s),
        }
    }
}

/// Whether `because` is the user's go-ahead: a message of theirs in the
/// conversation, at most ten minutes old, answering a Moss message that
/// offered buttons.
pub fn consent(lines: &[Line], because: Option<&str>, now_ms: i64) -> Result<(), String> {
    let Some(because) = because else {
        return Err("this needs the user's go-ahead (--because <their message id>)".into());
    };
    let messages: Vec<&Line> = lines.iter().filter(|l| l.state.is_none()).collect();
    let Some(at) = messages
        .iter()
        .position(|l| l.id == because && l.from.as_deref() == Some(chat::FROM_YOU))
    else {
        return Err(format!("{because} is not a message from the user"));
    };
    let said = moss::reply::parse_ms(&messages[at].at).unwrap_or(i64::MIN);
    if now_ms - said > CONSENT_MS || said - now_ms > 60_000 {
        return Err("the user's reply is too old; ask again".into());
    }
    let asked = messages[..at]
        .iter()
        .rev()
        .find(|l| l.from.as_deref() == Some(chat::FROM_MOSS));
    if !asked.is_some_and(|m| !m.buttons.is_empty()) {
        return Err("that message does not answer a question with options".into());
    }
    Ok(())
}

/// What to do about the session an action names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Go(Box<Slot>),
    Refuse(String),
    Skip(String),
}

/// Find the session and make sure the action still fits it. `status` is
/// how it is now; `folder_gone` whether its folder is plainly not there.
pub fn check(
    action: &Action,
    slots: &[Slot],
    status: &dyn Fn(&Slot) -> Status,
    folder_gone: &dyn Fn(&Slot) -> bool,
) -> Check {
    let Some(query) = action.session() else {
        return Check::Refuse("no session named".into());
    };
    let slot = match moss_core::restore::resolve(slots, query) {
        Ok(s) => s,
        Err(e) => return Check::Refuse(e.to_string()),
    };
    if slot.is_moss() {
        return Check::Refuse("Moss does not act on itself".into());
    }
    let now = status(slot);
    let fits = match action {
        Action::SetupFix => true,
        Action::SyncName(_) => now != Status::Closed,
        Action::Name(..) | Action::End(_) => now == Status::Running,
        Action::Restore(_) | Action::Forget(_) => now == Status::Interrupted,
        Action::ForgetGone(_) => now == Status::Interrupted && folder_gone(slot),
    };
    if !fits {
        let why = match (action, now) {
            (Action::ForgetGone(_), Status::Interrupted) => "its folder is still there".to_string(),
            (_, s) => format!("it is {} now", s.as_str()),
        };
        return Check::Skip(why);
    }
    Check::Go(Box::new(slot.clone()))
}

// ---------------------------------------------------------------- the command

/// `cc-moss moss act <action> [args] [--because id] [--wait]`.
pub fn run(action: &str, args: &[String], because: Option<&str>, wait: bool) -> Result<bool> {
    let parsed = Action::parse(action, args).map_err(anyhow::Error::msg)?;
    if parsed.needs_consent() && because.is_none() {
        bail!("{action} needs the user's go-ahead: --because <their message id>");
    }
    let home = moss::home_dir(&super::daemon::state());
    let now = now_ms();
    let request = Request {
        schema: 1,
        id: inbox::new_id(now),
        at: rfc3339(now),
        action: action.to_string(),
        args: args.to_vec(),
        because: because.map(String::from),
    };
    let actions = dir(&home);
    store_dir::ensure_private_dir(&actions).with_context(|| format!("{}", actions.display()))?;
    let path = actions.join(format!("{}.json", request.id));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("{}", path.display()))?;
    file.write_all(serde_json::to_string(&request)?.as_bytes())?;
    drop(file);
    if !wait {
        println!("{}", serde_json::json!({ "schema": 1, "id": request.id }));
        return Ok(true);
    }
    let until = Instant::now() + WAIT;
    let result = result_path(&home, &request.id);
    loop {
        if let Some(out) = std::fs::read_to_string(&result)
            .ok()
            .and_then(|t| serde_json::from_str::<Outcome>(&t).ok())
        {
            println!("{}", serde_json::to_string(&out)?);
            return Ok(out.outcome == "done");
        }
        if Instant::now() >= until {
            println!(
                "{}",
                serde_json::json!({ "schema": 1, "id": request.id, "outcome": "pending",
                    "message": "the daemon has not answered yet; it may still be working on it" })
            );
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

// ---------------------------------------------------------------- the daemon

/// The daemon's side: one request at a time, in order.
pub struct Hands {
    home: PathBuf,
    state: PathBuf,
    exe: String,
    next: Instant,
}

impl Hands {
    pub fn new(state: &Path, exe: &str) -> Hands {
        Hands {
            home: moss::home_dir(state),
            state: state.to_path_buf(),
            exe: exe.to_string(),
            next: Instant::now(),
        }
    }

    /// Carry out what waits. `enabled` is `[moss] actions`.
    pub fn tick(&mut self, now: Instant, enabled: bool, log: &dyn Fn(&str)) {
        if now < self.next {
            return;
        }
        self.next = now + Duration::from_millis(300);
        let Ok(entries) = std::fs::read_dir(dir(&self.home)) else {
            return;
        };
        let mut pending: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|x| x == "json")
                    && !p.to_string_lossy().ends_with(".result.json")
            })
            .collect();
        pending.sort();
        for path in pending {
            let id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let request = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Request>(&t).ok());
            let _ = moss::remove(&path);
            let (outcome, message) = match &request {
                None => ("refused", "the request could not be read".to_string()),
                Some(_) if !enabled => (
                    "refused",
                    "the user switched Moss's actions off ([moss] actions = false)".to_string(),
                ),
                Some(r) => self.carry_out(r),
            };
            let out = Outcome {
                schema: 1,
                id: id.clone(),
                outcome: outcome.to_string(),
                message: message.clone(),
            };
            let _ = moss::home::write_if_changed(
                &result_path(&self.home, &id),
                &serde_json::to_string(&out).unwrap_or_default(),
            );
            self.audit(&id, request.as_ref(), outcome, &message);
            log(&format!(
                "moss: act {}: {outcome}: {message}",
                request.as_ref().map_or("?", |r| r.action.as_str())
            ));
        }
    }

    fn audit(&self, id: &str, request: Option<&Request>, outcome: &str, message: &str) {
        let line = serde_json::json!({
            "schema": 1,
            "at": rfc3339(now_ms()),
            "by": "moss",
            "id": id,
            "action": request.map(|r| r.action.clone()),
            "args": request.map(|r| r.args.clone()),
            "because": request.and_then(|r| r.because.clone()),
            "outcome": outcome,
            "message": message,
        });
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(audit_path(&self.state))
            .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    }

    fn carry_out(&self, r: &Request) -> (&'static str, String) {
        let action = match Action::parse(&r.action, &r.args) {
            Ok(a) => a,
            Err(e) => return ("refused", e),
        };
        if action.needs_consent() {
            if let Err(e) = consent(&chat::lines(&self.home), r.because.as_deref(), now_ms()) {
                return ("refused", e);
            }
        }
        if action == Action::SetupFix {
            return self.setup_fix();
        }
        let db = self.state.join("registry.db");
        let registry = match Registry::open(&db) {
            Ok(r) => r,
            Err(e) => return ("failed", format!("{e:#}")),
        };
        let slots = match registry.slots() {
            Ok(s) => s,
            Err(e) => return ("failed", format!("{e:#}")),
        };
        let gone = |s: &Slot| {
            super::checkup::folder_fact(&std::fs::metadata(&s.cwd)) == super::checkup::Fact::No
        };
        let slot = match check(
            &action,
            &slots,
            &|s| s.effective_status(moss_core::proc::is_alive),
            &gone,
        ) {
            Check::Go(slot) => slot,
            Check::Refuse(why) => return ("refused", why),
            Check::Skip(why) => return ("skipped", why),
        };
        let result = match &action {
            Action::SyncName(_) => sync_name(&registry, &slot),
            Action::Name(_, name) => name_it(&registry, &slot, name),
            Action::ForgetGone(_) | Action::Forget(_) => registry
                .close_by_user(&slot.slot_id, now_ms())
                .map(|()| "it won't be offered for restore again".to_string()),
            Action::End(_) => end(&registry, &slot),
            Action::Restore(_) => self.restore(&slot),
            Action::SetupFix => unreachable!(),
        };
        match result {
            Ok(said) => ("done", said),
            Err(e) if e.downcast_ref::<Busy>().is_some() => ("skipped", format!("{e}")),
            Err(e) => ("failed", format!("{e:#}")),
        }
    }

    /// `cc-moss setup --yes`, then `cc-moss doctor --fix`.
    fn setup_fix(&self) -> (&'static str, String) {
        let run = |args: &[&str]| {
            moss_core::command::run_with_timeout(
                Command::new(&self.exe).args(args),
                Duration::from_secs(60),
            )
        };
        match run(&["setup", "--yes"]) {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                return (
                    "failed",
                    format!("setup: {}", String::from_utf8_lossy(&out.stderr).trim()),
                )
            }
            Err(e) => return ("failed", format!("setup did not finish: {e:?}")),
        }
        // doctor says non-zero while something stays broken; the links are
        // fixed either way.
        let _ = run(&["doctor", "--fix"]);
        ("done", "hooks and alias links are set up".into())
    }

    fn restore(&self, slot: &Slot) -> Result<String> {
        let out = moss_core::command::run_with_timeout(
            Command::new(&self.exe).args(["restore", "--yes", "--json", &slot.session_id]),
            Duration::from_secs(90),
        )
        .map_err(|e| anyhow::anyhow!("restore did not finish: {e:?}"))?;
        if !out.status.success() {
            bail!("restore: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        let said: serde_json::Value =
            serde_json::from_slice(&out.stdout).context("restore said something unreadable")?;
        let results = said["results"].as_array().cloned().unwrap_or_default();
        match results.first() {
            Some(r) if r["ok"] == true => Ok("it is running again".into()),
            Some(r) => bail!(
                "restore: {}",
                r["message"]
                    .as_str()
                    .unwrap_or("it could not be brought back")
            ),
            None => bail!("there was nothing to restore (it left nothing to resume)"),
        }
    }
}

/// The session is in the middle of something; try again later.
#[derive(Debug)]
struct Busy(&'static str);

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Busy {}

/// The name from the session's transcript (its last `/rename`), and the
/// tmux name after it.
fn sync_name(registry: &Registry, slot: &Slot) -> Result<String> {
    let mut slot = slot.clone();
    if let Some(name) = slot
        .transcript_path
        .as_deref()
        .and_then(|p| moss_core::transcript::last_custom_title(Path::new(p)))
    {
        if slot.name.as_deref() != Some(name.as_str()) {
            registry.set_name(&slot.slot_id, &name)?;
            slot.name = Some(name);
        }
    }
    super::hook::sync_tmux_name_of(registry, &slot)?;
    Ok(match &slot.name {
        Some(n) => format!("named {n}, in tmux too"),
        None => "it has no name to sync".into(),
    })
}

/// Type `/rename <name>` into the session, only while it is idle with an
/// empty input; then record the name and rename its tmux session.
fn name_it(registry: &Registry, slot: &Slot, name: &str) -> Result<String> {
    let Some(loc) = slot.tmux.as_ref() else {
        bail!("it is not in tmux, so nothing can be typed into it");
    };
    let in_turn = slot.busy
        || slot
            .transcript_path
            .as_deref()
            .and_then(|p| moss_core::transcript::turn_open(Path::new(p)))
            == Some(true);
    let mut pane = SessionPane {
        socket: Socket::Path(loc.socket.clone()),
        pane: loc.pane_id.clone(),
        scratch: store_dir::state_dir().join(format!("act-paste-{}.txt", std::process::id())),
    };
    let ready = !in_turn && pane.screen().is_ok_and(|s| inbox::input_empty(&s));
    if !ready {
        return Err(
            Busy("it is busy or something is typed in it; try again when it is idle").into(),
        );
    }
    match inbox::type_in(&mut pane, &format!("/rename {name}")) {
        Typed::Sent { .. } => {}
        Typed::Failed(e) | Typed::Stuck(e) => bail!("/rename did not go in: {e}"),
    }
    registry.set_name(&slot.slot_id, name)?;
    let mut named = slot.clone();
    named.name = Some(name.to_string());
    super::hook::sync_tmux_name_of(registry, &named)?;
    Ok(format!("named {name}"))
}

/// End a running session: closed first (never offered for restore), then
/// its tmux session.
fn end(registry: &Registry, slot: &Slot) -> Result<String> {
    registry.close_by_user(&slot.slot_id, now_ms())?;
    if let Some(loc) = slot.tmux.as_ref() {
        if let Some(id) = loc.session_id.as_deref() {
            Tmux::with_socket(Socket::Path(loc.socket.clone())).kill_session(id)?;
        }
    }
    Ok("ended".into())
}

/// A user session's pane, for typing `/rename`.
struct SessionPane {
    socket: Socket,
    pane: String,
    scratch: PathBuf,
}

impl SessionPane {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = moss_core::command::run_with_timeout(
            Command::new("tmux").args(self.socket.args()).args(args),
            Duration::from_secs(5),
        )
        .map_err(|e| format!("{e:?}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

impl Pane for SessionPane {
    fn paste(&mut self, text: &str) -> Result<(), String> {
        moss::home::write_if_changed(&self.scratch, text).map_err(|e| e.to_string())?;
        let file = self.scratch.to_string_lossy().into_owned();
        let buffer = format!("cc-moss-act-{}", std::process::id());
        let loaded = self.run(&["load-buffer", "-b", &buffer, &file]);
        let _ = std::fs::remove_file(&self.scratch);
        loaded?;
        self.run(&["paste-buffer", "-p", "-d", "-b", &buffer, "-t", &self.pane])
            .map(|_| ())
    }
    fn screen(&mut self) -> Result<String, String> {
        self.run(&["capture-pane", "-p", "-J", "-e", "-t", &self.pane])
    }
    fn key(&mut self, key: &str) -> Result<(), String> {
        self.run(&["send-keys", "-t", &self.pane, key]).map(|_| ())
    }
    fn wait(&mut self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(id: &str, name: &str, role: Option<&str>) -> Slot {
        Slot {
            slot_id: id.into(),
            agent: "claude".into(),
            session_id: format!("{id}-0000-1111"),
            session_ids: vec![],
            name: Some(name.into()),
            cwd: "/work".into(),
            pid: None,
            pid_start: None,
            tmux: None,
            status: Status::Interrupted,
            end_reason: None,
            notes: Default::default(),
            launch_args: vec![],
            transcript_path: None,
            busy: false,
            attention: None,
            last_prompt: None,
            restore_started_at_ms: None,
            role: role.map(String::from),
            created_at_ms: 0,
            last_active_at_ms: 0,
            ended_at_ms: None,
        }
    }

    fn as_recorded(s: &Slot) -> Status {
        s.status
    }

    #[test]
    fn act_refuses_moss() {
        let mut moss = slot("m", "Moss", Some("moss"));
        moss.status = Status::Running;
        let slots = vec![moss];
        for action in [
            Action::End("Moss".into()),
            Action::Name("Moss".into(), "x".into()),
            Action::SyncName("Moss".into()),
            Action::Forget("Moss".into()),
            Action::Restore("Moss".into()),
        ] {
            assert!(
                matches!(
                    check(&action, &slots, &as_recorded, &|_| true),
                    Check::Refuse(_)
                ),
                "{action:?}"
            );
        }
    }

    #[test]
    fn act_skips_a_changed_session() {
        // Restored by the user meanwhile: no second restore, no forgetting.
        let mut api = slot("a", "api", None);
        api.status = Status::Running;
        let slots = vec![api];
        for action in [Action::Restore("api".into()), Action::Forget("api".into())] {
            assert!(
                matches!(
                    check(&action, &slots, &as_recorded, &|_| false),
                    Check::Skip(_)
                ),
                "{action:?}"
            );
        }
        // Its folder came back: not "gone".
        let slots = vec![slot("b", "web", None)];
        assert!(matches!(
            check(
                &Action::ForgetGone("web".into()),
                &slots,
                &as_recorded,
                &|_| false
            ),
            Check::Skip(_)
        ));
        assert!(matches!(
            check(
                &Action::ForgetGone("web".into()),
                &slots,
                &as_recorded,
                &|_| true
            ),
            Check::Go(_)
        ));
        // Ended already: nothing to end.
        let slots = vec![slot("c", "docs", None)];
        assert!(matches!(
            check(&Action::End("docs".into()), &slots, &as_recorded, &|_| {
                false
            }),
            Check::Skip(_)
        ));
        // Not there at all.
        assert!(matches!(
            check(&Action::End("nope".into()), &slots, &as_recorded, &|_| {
                false
            }),
            Check::Refuse(_)
        ));
    }

    fn said(id: &str, from: &str, at: i64, buttons: &[&str]) -> Line {
        Line {
            buttons: buttons.iter().map(|b| b.to_string()).collect(),
            ..Line::message(id, &rfc3339(at), from, "x")
        }
    }

    #[test]
    fn act_consent_reads_the_conversation() {
        let now = 1_800_000_000_000;
        let lines = vec![
            said(
                "q1",
                chat::FROM_MOSS,
                now - 60_000,
                &["Bring them back", "Leave them"],
            ),
            said("y1", chat::FROM_YOU, now - 30_000, &[]),
            said("q2", chat::FROM_MOSS, now - 20_000, &[]),
            said("y2", chat::FROM_YOU, now - 10_000, &[]),
            said("old", chat::FROM_YOU, now - 11 * 60_000, &[]),
        ];
        assert_eq!(consent(&lines, Some("y1"), now), Ok(()));
        assert!(consent(&lines, None, now).is_err(), "no reply");
        assert!(
            consent(&lines, Some("forged"), now).is_err(),
            "no such message"
        );
        assert!(
            consent(&lines, Some("q1"), now).is_err(),
            "Moss's own words"
        );
        assert!(
            consent(&lines, Some("y2"), now).is_err(),
            "answers no options"
        );
        let stale = vec![
            said("q", chat::FROM_MOSS, now - 12 * 60_000, &["Yes"]),
            said("old", chat::FROM_YOU, now - 11 * 60_000, &[]),
        ];
        assert!(consent(&stale, Some("old"), now).is_err(), "too old");
    }

    #[test]
    fn act_parses_its_actions() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(Action::parse("setup-fix", &[]), Ok(Action::SetupFix));
        assert_eq!(
            Action::parse("name", &a(&["abc123", "ofoxai"])),
            Ok(Action::Name("abc123".into(), "ofoxai".into()))
        );
        assert!(Action::parse("name", &a(&["abc", "two\nlines"])).is_err());
        assert!(Action::parse("dispatch", &a(&["x"])).is_err());
        assert!(Action::parse("restore", &[]).is_err());
        assert!(Action::Restore("x".into()).needs_consent());
        assert!(!Action::ForgetGone("x".into()).needs_consent());
    }
}
