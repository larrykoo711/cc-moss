//! Moss's hands (T2.4): `cc-moss moss act <action> …` leaves a request in
//! `<home>/actions/<id>.json`; the daemon checks it, carries it out (Moss's
//! sandbox cannot reach tmux or the user's sessions), writes the result next
//! to it and a line to `<state>/audit.jsonl`. `--wait` reads the result.
//!
//! Actions that lose nothing run at once: `setup-fix`, `sync-name`, `name`,
//! `forget-gone`, `archive`, `archive-idle` (archiving can be undone). Those
//! that start or end work or forget a session the user may want (`restore`,
//! `forget`, `end`, and `purge`, `purge-archived`, which delete archived
//! sessions and put their transcripts in the Trash) need `--because <id>`: the user's
//! tap, under ten minutes old, on the yes button cc-moss put under the Moss
//! message that proposed exactly that (`[[confirm:]]`). Moss cannot write the
//! conversation file, so it cannot make one up, and it never words the button.
//! Nothing is ever done to Moss itself.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use moss_core::config::Config;
use moss_core::moss::act::{audit_path, dir, result_path, Outcome, Request};
use moss_core::moss::chat::{self, Line};
use moss_core::moss::dir::Dir;
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
    SyncNames,
    SyncName(String),
    Name(String, String),
    ForgetGone(String),
    Restore(String),
    Forget(String),
    End(String),
    Archive(String),
    ArchiveIdle,
    Purge(String),
    PurgeArchived,
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
            "sync-names" if args.is_empty() => Ok(Action::SyncNames),
            "sync-names" => Err("sync-names takes nothing: it looks at every session".into()),
            "sync-name" => one(Action::SyncName),
            "forget-gone" => one(Action::ForgetGone),
            "restore" => one(Action::Restore),
            "forget" => one(Action::Forget),
            "end" => one(Action::End),
            "archive" => one(Action::Archive),
            "purge" => one(Action::Purge),
            "archive-idle" if args.is_empty() => Ok(Action::ArchiveIdle),
            "archive-idle" => Err("archive-idle takes nothing: it looks at every session".into()),
            "purge-archived" if args.is_empty() => Ok(Action::PurgeArchived),
            "purge-archived" => Err(
                "purge-archived takes nothing: it covers the archived sessions the user agreed to"
                    .into(),
            ),
            "name" => match args {
                [s, given] => {
                    let name = given.trim();
                    if name.is_empty()
                        || name.chars().count() > 60
                        || name.chars().any(char::is_control)
                    {
                        Err("a name is one line of at most 60 characters".into())
                    } else if moss_core::naming::has_hidden_chars(given) {
                        Err("a name has no invisible characters (direction marks, \
                             zero-width spaces, line separators)"
                            .into())
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
            Action::Restore(_)
                | Action::Forget(_)
                | Action::End(_)
                | Action::Archive(_)
                | Action::Purge(_)
                | Action::PurgeArchived
        )
    }

    fn session(&self) -> Option<&str> {
        match self {
            Action::SetupFix | Action::SyncNames | Action::ArchiveIdle | Action::PurgeArchived => {
                None
            }
            Action::Archive(s) | Action::Purge(s) => Some(s),
            Action::SyncName(s)
            | Action::Name(s, _)
            | Action::ForgetGone(s)
            | Action::Restore(s)
            | Action::Forget(s)
            | Action::End(s) => Some(s),
        }
    }
}

/// The key a used go-ahead is remembered by.
pub fn consent_key(because: &str, action: &str, slot_id: &str) -> String {
    format!("{because} {action} {slot_id}")
}

/// Whether `because` is the user's go-ahead for `action` on `slot_id`: a
/// message of theirs, at most ten minutes old, answering the Moss message
/// right before it, which proposed exactly this action for exactly this
/// session (`[[confirm:]]`, recorded by the daemon), and whose text is that
/// proposal's yes button, worded by cc-moss. Each go-ahead works once per
/// action and session (`used`).
pub fn consent(
    lines: &[Line],
    because: Option<&str>,
    action: &str,
    slot_id: &str,
    now_ms: i64,
    used: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
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
    let reply = messages[at].text.as_deref().unwrap_or("").trim();
    let asked = messages[..at]
        .iter()
        .rev()
        .find(|l| l.from.as_deref() == Some(chat::FROM_MOSS));
    let agreed = asked.is_some_and(|m| {
        m.proposals
            .iter()
            .any(|p| p.action == action && p.slot_id == slot_id && p.yes.iter().any(|y| y == reply))
    });
    if !agreed {
        return Err(
            "the user's reply is not a yes to this action for this session; ask with \
             [[confirm: <action> <session>]] and let them tap the yes button cc-moss adds"
                .into(),
        );
    }
    if used(&consent_key(because, action, slot_id)) {
        return Err("that go-ahead was used already; ask again".into());
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
        Action::SetupFix | Action::SyncNames | Action::ArchiveIdle | Action::PurgeArchived => true,
        Action::Archive(_) => now != Status::Running && !slot.is_archived(),
        Action::Purge(_) => now != Status::Running && slot.is_archived(),
        Action::SyncName(_) => now != Status::Closed,
        Action::Name(..) | Action::End(_) => now == Status::Running,
        Action::Restore(_) | Action::Forget(_) => now == Status::Interrupted,
        Action::ForgetGone(_) => now == Status::Interrupted && folder_gone(slot),
    };
    if !fits {
        let why = match (action, now) {
            (Action::ForgetGone(_), Status::Interrupted) => "its folder is still there".to_string(),
            (Action::Archive(_), Status::Running) => {
                "it is running; it can't be archived".to_string()
            }
            (Action::Archive(_), _) => "it is archived already".to_string(),
            (Action::Purge(_), Status::Running) => "it is running again".to_string(),
            (Action::Purge(_), _) => {
                "it is not archived; only archived sessions can be deleted".to_string()
            }
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
    let actions = moss::home::ensure_real_dir(&home, "actions")
        .with_context(|| format!("{}", dir(&home).display()))?;
    let path = actions.join(format!("{}.json", request.id));
    // Whole or not at all: the daemon never sees half a request.
    inbox::put_whole(&path, &serde_json::to_string(&request)?)
        .with_context(|| format!("{}", path.display()))?;
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
            // Read: gone. The audit log is the record.
            let _ = std::fs::remove_file(&result);
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
    /// Where purged transcripts go; none without a home folder.
    trash: Option<super::archive::Disposal>,
}

impl Hands {
    pub fn new(state: &Path, exe: &str) -> Hands {
        Hands {
            home: moss::home_dir(state),
            state: state.to_path_buf(),
            exe: exe.to_string(),
            next: Instant::now(),
            trash: super::archive::Disposal::current(),
        }
    }

    /// Carry out what waits. `enabled` is `[moss] actions`.
    pub fn tick(&mut self, now: Instant, enabled: bool, log: &dyn Fn(&str)) {
        if now < self.next {
            return;
        }
        self.next = now + Duration::from_millis(300);
        if !self.home.exists() {
            return;
        }
        // A real folder, never a link Moss put there, held open: renaming
        // it and putting a link in its place afterwards changes nothing.
        let Ok(actions) = Dir::open(&self.home, "actions") else {
            return;
        };
        self.prune_results(&actions);
        let pending: Vec<String> = actions
            .files("json")
            .into_iter()
            .filter(|n| !n.ends_with(".result.json"))
            .collect();
        // One a go: a long restore never holds up Moss's conversation more
        // than once in a row.
        if let Some(name) = pending.into_iter().next() {
            let id = name.trim_end_matches(".json").to_string();
            let request = actions
                .read(&name)
                .ok()
                .and_then(|t| serde_json::from_str::<Request>(&t).ok());
            let _ = actions.remove(&name);
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
            let _ = actions.write(
                &format!("{id}.result.json"),
                &serde_json::to_string(&out).unwrap_or_default(),
            );
            self.audit(&id, request.as_ref(), outcome, &message);
            log(&format!(
                "moss: act {}: {outcome}: {message}",
                request.as_ref().map_or("?", |r| r.action.as_str())
            ));
        }
    }

    /// `<state>/daemon/consents.txt`: go-aheads already used, one key a
    /// line (the last 500). In the daemon's folder, out of Moss's reach.
    fn consents_path(&self) -> PathBuf {
        moss_core::daemon::Paths::new(&self.state)
            .dir
            .join("consents.txt")
    }

    fn used_consents(&self) -> Vec<String> {
        std::fs::read_to_string(self.consents_path())
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn use_consent(&self, key: &str) {
        let mut all = self.used_consents();
        all.push(key.to_string());
        let keep = all.len().saturating_sub(500);
        let text = all[keep..].join("\n") + "\n";
        let _ = moss::home::write_if_changed(&self.consents_path(), &text);
    }

    /// Results nobody read within an hour.
    fn prune_results(&self, actions: &Dir) {
        let hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
        for name in actions.files("json") {
            let result = name.ends_with(".result.json");
            let old = actions.modified(&name).is_ok_and(|t| t < hour_ago);
            if result && old {
                let _ = actions.remove(&name);
            }
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
        let _ = moss_core::moss::act::append_audit(&audit_path(&self.state), &line.to_string());
    }

    fn carry_out(&self, r: &Request) -> (&'static str, String) {
        let action = match Action::parse(&r.action, &r.args) {
            Ok(a) => a,
            Err(e) => return ("refused", e),
        };
        if action == Action::SetupFix {
            return self.setup_fix();
        }
        if action == Action::SyncNames {
            return match sync_names(&self.state) {
                Ok(n) => (
                    "done",
                    format!("renamed {n} tmux session(s) after their names"),
                ),
                Err(e) => ("failed", format!("{e:#}")),
            };
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
        if action == Action::ArchiveIdle {
            return archive_idle(&registry, Config::load());
        }
        if action == Action::PurgeArchived {
            return self.purge_archived(&registry, &slots, r);
        }
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
        if action.needs_consent() {
            let used = self.used_consents();
            if let Err(e) = consent(
                &chat::lines(&self.home),
                r.because.as_deref(),
                &r.action,
                &slot.slot_id,
                now_ms(),
                &|key| used.iter().any(|u| u == key),
            ) {
                return ("refused", e);
            }
        }
        let result = match &action {
            Action::SyncName(_) => sync_name(&registry, &slot),
            Action::Name(_, name) => name_it(&registry, &slot, name),
            Action::ForgetGone(_) | Action::Forget(_) => registry
                .close_by_user(&slot.slot_id, now_ms())
                .map(|()| "it won't be offered for restore again".to_string()),
            Action::End(_) => end(&registry, &slot),
            Action::Restore(_) => self.restore(&slot),
            Action::Archive(_) => registry
                .archive(
                    std::slice::from_ref(&slot.slot_id),
                    "moss",
                    &|s| s.effective_status(moss_core::proc::is_alive) == Status::Running,
                    now_ms(),
                )
                .and_then(|done| match done.is_empty() {
                    false => Ok("archived; `cc-moss unarchive` brings it back".to_string()),
                    true => Err(anyhow::anyhow!("it could not be archived")),
                }),
            Action::Purge(_) => self.purge_one(&registry, &slot),
            Action::SetupFix | Action::SyncNames | Action::ArchiveIdle | Action::PurgeArchived => {
                unreachable!()
            }
        };
        settle(result, || {
            if action.needs_consent() {
                self.use_consent(&consent_key(
                    r.because.as_deref().unwrap_or_default(),
                    &r.action,
                    &slot.slot_id,
                ));
            }
        })
    }

    /// Delete one archived session the user agreed to.
    fn purge_one(&self, registry: &Registry, slot: &Slot) -> Result<String> {
        let Some(trash) = &self.trash else {
            bail!("there is no Trash to move its transcript to (no home folder)");
        };
        let done = super::archive::purge(registry, std::slice::from_ref(&slot.slot_id), trash)?;
        if let Some((_, why)) = done.kept.first() {
            bail!("it was kept: {why}");
        }
        Ok(format!(
            "deleted; {} transcript(s) moved to the Trash",
            done.trashed.len()
        ))
    }

    /// Delete the archived sessions the user's yes covers: those the
    /// `[[confirm: purge-archived]]` they tapped named when Moss asked.
    /// One archived later is not touched, nor is anything not archived.
    fn purge_archived(
        &self,
        registry: &Registry,
        slots: &[Slot],
        r: &Request,
    ) -> (&'static str, String) {
        let archived: Vec<&Slot> = slots
            .iter()
            .filter(|s| s.is_archived() && !s.is_moss())
            .filter(|s| s.effective_status(moss_core::proc::is_alive) != Status::Running)
            .collect();
        if archived.is_empty() {
            return ("skipped", "there are no archived sessions".into());
        }
        let lines = chat::lines(&self.home);
        let used = self.used_consents();
        let now = now_ms();
        let mut agreed: Vec<String> = Vec::new();
        let mut refusal = None;
        for slot in archived {
            match consent(
                &lines,
                r.because.as_deref(),
                &r.action,
                &slot.slot_id,
                now,
                &|key| used.iter().any(|u| u == key),
            ) {
                Ok(()) => agreed.push(slot.slot_id.clone()),
                Err(e) => refusal = Some(e),
            }
        }
        if agreed.is_empty() {
            return (
                "refused",
                refusal.unwrap_or_else(|| "the user did not agree".into()),
            );
        }
        let Some(trash) = &self.trash else {
            return (
                "failed",
                "there is no Trash to move transcripts to (no home folder)".into(),
            );
        };
        let done = match super::archive::purge(registry, &agreed, trash) {
            Ok(d) => d,
            Err(e) => return ("failed", format!("{e:#}")),
        };
        for id in &done.removed {
            self.use_consent(&consent_key(
                r.because.as_deref().unwrap_or_default(),
                &r.action,
                id,
            ));
        }
        let mut said = format!(
            "deleted {} archived session(s); {} transcript(s) moved to the Trash",
            done.removed.len(),
            done.trashed.len()
        );
        if !done.kept.is_empty() {
            said.push_str(&format!("; kept {}", done.kept.len()));
            if let Some((_, why)) = done.kept.first() {
                said.push_str(&format!(" ({why})"));
            }
        }
        if done.removed.is_empty() {
            ("failed", said)
        } else {
            ("done", said)
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
            Command::new(&self.exe).args(["restore", "--yes", "--json", "--", &slot.session_id]),
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

/// `archive-idle`: what the daily tidy-up does, now. Never on a guess: an
/// unreadable config archives nothing.
fn archive_idle(registry: &Registry, config: Result<Config>) -> (&'static str, String) {
    let days = match config {
        Ok(c) => c.sessions.archive_after_days,
        Err(e) => {
            return (
                "refused",
                format!("config.toml can't be read, so nothing is archived: {e:#}"),
            )
        }
    };
    match super::archive::archive_idle(registry, days, now_ms()) {
        Ok(_) if days == 0 => (
            "skipped",
            "archiving is off ([sessions] archive_after_days = 0)".to_string(),
        ),
        Ok(done) => (
            "done",
            format!(
                "archived {} session(s) not used for over {days} days",
                done.len()
            ),
        ),
        Err(e) => ("failed", format!("{e:#}")),
    }
}

/// Every running session whose tmux name is not its derived name (a `-2`
/// style suffix counts as matching) gets it back, by session id. Moss keeps
/// its own. How many were renamed.
fn sync_names(state: &Path) -> Result<usize> {
    let registry = Registry::open(&state.join("registry.db"))?;
    let prefix = super::checkup::prefix();
    let mut servers = std::collections::HashMap::new();
    let mut renamed = 0;
    for slot in registry.slots()? {
        if slot.is_moss() || slot.effective_status(moss_core::proc::is_alive) != Status::Running {
            continue;
        }
        let Some(loc) = slot.tmux.as_ref() else {
            continue;
        };
        let live = servers
            .entry(loc.socket.clone())
            .or_insert_with(|| super::checkup::live_names(&loc.socket));
        if super::checkup::tmux_fact(&slot, prefix.as_deref(), live)
            == Some(super::checkup::Fact::No)
        {
            super::hook::sync_tmux_name_of(&registry, &slot)?;
            renamed += 1;
        }
    }
    Ok(renamed)
}

/// The outcome of an action. The user's go-ahead is spent (`spend`) only
/// when it was done: a busy session or a failure leaves it for a retry.
fn settle(result: Result<String>, spend: impl FnOnce()) -> (&'static str, String) {
    match result {
        Ok(said) => {
            spend();
            ("done", said)
        }
        Err(e) if e.downcast_ref::<Busy>().is_some() => ("skipped", format!("{e}")),
        Err(e) => ("failed", format!("{e:#}")),
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
    match inbox::type_exactly(&mut pane, &format!("/rename {name}")) {
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
            moss_core::tmux::command()
                .args(self.socket.args())
                .args(args),
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
    use moss_core::trash::Trash;

    #[test]
    fn act_spends_the_go_ahead_only_when_done() {
        let spent = std::cell::Cell::new(0);
        let busy: Result<String> = Err(Busy("busy").into());
        assert_eq!(settle(busy, || spent.set(spent.get() + 1)).0, "skipped");
        let failed: Result<String> = Err(anyhow::anyhow!("restore: no"));
        assert_eq!(settle(failed, || spent.set(spent.get() + 1)).0, "failed");
        assert_eq!(spent.get(), 0, "a busy or failed action keeps the go-ahead");
        assert_eq!(
            settle(Ok("ended".into()), || spent.set(spent.get() + 1)).0,
            "done"
        );
        assert_eq!(spent.get(), 1);
    }

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
            archived_at_ms: None,
            archived_by: None,
            provider_started: None,
            provider_seen_keys: None,
        }
    }

    fn as_recorded(s: &Slot) -> Status {
        s.status
    }

    #[test]
    fn act_never_follows_a_linked_actions_folder() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let home = moss::home_dir(&state);
        std::fs::create_dir_all(&home).unwrap();
        // Somewhere else, full of things that look like requests.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let request = r#"{"schema":1,"id":"x","at":"t","action":"setup-fix","args":[]}"#;
        std::fs::write(elsewhere.join("0000000000001-aaaaaa.json"), request).unwrap();
        std::fs::write(elsewhere.join("keep.json"), "{}").unwrap();
        symlink(&elsewhere, home.join("actions")).unwrap();
        let mut hands = Hands::new(&state, "/nonexistent/cc-moss");
        hands.tick(Instant::now(), true, &|_| {});
        assert!(elsewhere.join("0000000000001-aaaaaa.json").exists());
        assert!(elsewhere.join("keep.json").exists());
        assert!(!elsewhere.join("0000000000001-aaaaaa.result.json").exists());
        let meta = std::fs::symlink_metadata(home.join("actions")).unwrap();
        assert!(
            meta.file_type().is_dir(),
            "the link was replaced by a folder"
        );
        // A link inside the folder is not a request either.
        std::fs::write(dir.path().join("target.json"), request).unwrap();
        symlink(
            dir.path().join("target.json"),
            home.join("actions/0000000000002-bbbbbb.json"),
        )
        .unwrap();
        hands.next = Instant::now();
        hands.tick(Instant::now(), true, &|_| {});
        assert!(dir.path().join("target.json").exists());
        assert!(!home
            .join("actions/0000000000002-bbbbbb.result.json")
            .exists());
    }

    #[test]
    fn act_takes_only_whole_requests_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let actions = moss::home_dir(&state).join("actions");
        std::fs::create_dir_all(&actions).unwrap();
        // Half written: left alone.
        std::fs::write(actions.join("0000000000001-aaaaaa.tmp"), "{\"schema\":1,").unwrap();
        for id in ["0000000000002-aaaaaa", "0000000000003-aaaaaa"] {
            std::fs::write(
                actions.join(format!("{id}.json")),
                format!(
                    r#"{{"schema":1,"id":"{id}","at":"t","action":"forget","args":["nobody"]}}"#
                ),
            )
            .unwrap();
        }
        let mut hands = Hands::new(&state, "/nonexistent/cc-moss");
        hands.tick(Instant::now(), false, &|_| {});
        assert!(actions.join("0000000000001-aaaaaa.tmp").exists());
        // One per go: the daemon's loop is never held up by a queue.
        assert!(actions.join("0000000000002-aaaaaa.result.json").exists());
        assert!(!actions.join("0000000000003-aaaaaa.result.json").exists());
        // Old results go.
        let old = actions.join("0000000000002-aaaaaa.result.json");
        let hour_ago = std::time::SystemTime::now() - Duration::from_secs(2 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(hour_ago)
            .unwrap();
        hands.next = Instant::now();
        hands.tick(Instant::now(), false, &|_| {});
        assert!(!old.exists(), "results over an hour old are cleared");
        assert!(actions.join("0000000000003-aaaaaa.result.json").exists());
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

    fn proposed(id: &str, at: i64, buttons: &[&str], proposals: &[(&str, &str, &[&str])]) -> Line {
        Line {
            proposals: proposals
                .iter()
                .map(|(action, slot, yes)| chat::Proposal {
                    action: action.to_string(),
                    slot_id: slot.to_string(),
                    yes: yes.iter().map(|y| y.to_string()).collect(),
                })
                .collect(),
            ..said(id, chat::FROM_MOSS, at, buttons)
        }
    }

    fn you(id: &str, at: i64, text: &str) -> Line {
        Line::message(id, &rfc3339(at), chat::FROM_YOU, text)
    }

    #[test]
    fn act_consent_needs_the_users_yes_to_that_proposal() {
        let now = 1_800_000_000_000;
        let lines = vec![
            proposed(
                "q1",
                now - 60_000,
                &["全部找回", "不要"],
                &[
                    ("restore", "SLOT-A", &["全部找回"]),
                    ("restore", "SLOT-B", &["全部找回"]),
                ],
            ),
            you("y1", now - 30_000, "全部找回"),
            you("y2", now - 25_000, "不要"),
            said("q2", chat::FROM_MOSS, now - 20_000, &["Yes"]),
            you("y3", now - 10_000, "Yes"),
            proposed(
                "q3",
                now - 13 * 60_000,
                &["找回"],
                &[("restore", "SLOT-A", &["找回"])],
            ),
            you("old", now - 11 * 60_000, "找回"),
        ];
        let none = |_: &str| false;
        let ok = |because, action, slot| consent(&lines, Some(because), action, slot, now, &none);
        assert_eq!(ok("y1", "restore", "SLOT-A"), Ok(()));
        assert_eq!(
            ok("y1", "restore", "SLOT-B"),
            Ok(()),
            "one yes for each session it named"
        );
        assert!(ok("y2", "restore", "SLOT-A").is_err(), "不要 is no yes");
        assert!(
            ok("y1", "restore", "SLOT-C").is_err(),
            "a session the proposal didn't name"
        );
        assert!(
            ok("y1", "end", "SLOT-A").is_err(),
            "an action it didn't propose"
        );
        assert!(
            ok("y3", "restore", "SLOT-A").is_err(),
            "a reply to a message with no proposal"
        );
        assert!(ok("q1", "restore", "SLOT-A").is_err(), "Moss's own words");
        assert!(
            ok("forged", "restore", "SLOT-A").is_err(),
            "no such message"
        );
        assert!(ok("old", "restore", "SLOT-A").is_err(), "too old");
        assert!(consent(&lines, None, "restore", "SLOT-A", now, &none).is_err());
        // Used once.
        let used = |key: &str| key == consent_key("y1", "restore", "SLOT-A");
        assert!(consent(&lines, Some("y1"), "restore", "SLOT-A", now, &used).is_err());
        assert_eq!(
            consent(&lines, Some("y1"), "restore", "SLOT-B", now, &used),
            Ok(())
        );
    }

    #[test]
    fn archive_idle_refuses_on_an_unreadable_config() {
        let dir = tempfile::tempdir().unwrap();
        let r = Registry::open(&dir.path().join("registry.db")).unwrap();
        let (outcome, said) = archive_idle(&r, Err(anyhow::anyhow!("bad toml")));
        assert_eq!(outcome, "refused", "{said}");
        assert_eq!(archive_idle(&r, Ok(Config::default())).0, "done");
    }

    #[test]
    fn act_parses_its_actions() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(Action::parse("setup-fix", &[]), Ok(Action::SetupFix));
        assert_eq!(Action::parse("sync-names", &[]), Ok(Action::SyncNames));
        assert!(Action::parse("sync-names", &a(&["x"])).is_err());
        assert!(!Action::SyncNames.needs_consent());
        assert_eq!(
            Action::parse("name", &a(&["abc123", "webshop"])),
            Ok(Action::Name("abc123".into(), "webshop".into()))
        );
        assert!(Action::parse("name", &a(&["abc", "two\nlines"])).is_err());
        // Names that would look like another session's in a yes button.
        for hidden in ["ap\u{202E}i", "a\u{200B}pi", "api\u{2028}", "a\u{200D}pi"] {
            assert!(
                Action::parse("name", &a(&["abc", hidden])).is_err(),
                "{hidden:?}"
            );
        }
        assert!(Action::parse("name", &a(&["abc", "👩\u{200D}💻 notes"])).is_ok());
        assert!(Action::parse("dispatch", &a(&["x"])).is_err());
        assert!(Action::parse("restore", &[]).is_err());
        assert!(Action::Restore("x".into()).needs_consent());
        assert!(!Action::ForgetGone("x".into()).needs_consent());
        assert_eq!(
            Action::parse("purge-archived", &[]),
            Ok(Action::PurgeArchived)
        );
        assert!(Action::parse("purge-archived", &a(&["x"])).is_err());
        assert!(Action::parse("purge", &[]).is_err());
        assert!(Action::Purge("x".into()).needs_consent());
        assert!(Action::PurgeArchived.needs_consent());
        // Archiving one session is Moss's idea: the user says yes first
        // (purge-archived could otherwise follow right away).
        assert!(Action::Archive("x".into()).needs_consent());
        assert!(!Action::ArchiveIdle.needs_consent());
    }

    // ------------------------------------------------------------ purge

    /// A daemon's hands on a throwaway state dir, with a trash of its own.
    struct World {
        dir: tempfile::TempDir,
        hands: Hands,
    }

    impl World {
        fn new() -> World {
            let dir = tempfile::tempdir().unwrap();
            let state = dir.path().join("state");
            std::fs::create_dir_all(moss::home_dir(&state)).unwrap();
            let mut hands = Hands::new(&state, "/nonexistent/cc-moss");
            hands.trash = Some(super::super::archive::Disposal {
                trash: Trash::Folder(dir.path().join("Trash")),
                claude_dirs: vec![dir.path().join("claude")],
            });
            World { dir, hands }
        }

        fn registry(&self) -> Registry {
            Registry::open(&self.hands.state.join("registry.db")).unwrap()
        }

        /// A session that ended, with a transcript; archived when asked.
        fn session(&self, name: &str, archived: bool) -> String {
            use moss_core::event::{Context, Event, StartSource};
            let transcript = self.transcript(name);
            std::fs::write(&transcript, "{}\n").unwrap();
            let mut r = self.registry();
            let ctx = Context {
                pid: None,
                pid_start: None,
                tmux: None,
                launch_args: None,
                now_ms: 1_000,
            };
            r.apply(
                &moss_core::event::Event::Start {
                    source: StartSource::Startup,
                    session_id: format!("{name}-0000-1111"),
                    cwd: "/work".into(),
                    title: Some(name.into()),
                    transcript_path: Some(transcript.to_string_lossy().into_owned()),
                },
                &ctx,
            )
            .unwrap();
            r.apply(
                &Event::End {
                    session_id: format!("{name}-0000-1111"),
                    reason: moss_core::event::EndReason::Closed("prompt_input_exit".into()),
                },
                &ctx,
            )
            .unwrap();
            let id = self.id(name).unwrap();
            if archived {
                r.archive(std::slice::from_ref(&id), "you", &|_| false, 2_000)
                    .unwrap();
            }
            id
        }

        /// Where Claude Code keeps it: `<claude>/projects/<folder>/<id>.jsonl`.
        fn transcript(&self, name: &str) -> PathBuf {
            let dir = self.dir.path().join("claude/projects/-work");
            std::fs::create_dir_all(&dir).unwrap();
            dir.join(format!("{name}-0000-1111.jsonl"))
        }

        fn id(&self, name: &str) -> Option<String> {
            self.registry()
                .slots()
                .unwrap()
                .into_iter()
                .find(|s| s.name.as_deref() == Some(name))
                .map(|s| s.slot_id)
        }

        /// Moss proposed `action` for `slots` with `yes`, and the user
        /// answered `said`; the id of their answer.
        fn asked(&self, action: &str, slots: &[&str], yes: &str, said: &str) -> String {
            let home = moss::home_dir(&self.hands.state);
            let now = now_ms();
            let mut q = Line::message(
                &format!("q{now}"),
                &rfc3339(now - 2_000),
                chat::FROM_MOSS,
                "Delete them?",
            );
            q.buttons = vec![yes.to_string(), "Keep them".into()];
            q.proposals = slots
                .iter()
                .map(|slot| chat::Proposal {
                    action: action.into(),
                    slot_id: slot.to_string(),
                    yes: vec![yes.to_string()],
                })
                .collect();
            chat::append(&home, &q).unwrap();
            let id = format!("y{now}");
            chat::append(
                &home,
                &Line::message(&id, &rfc3339(now - 1_000), chat::FROM_YOU, said),
            )
            .unwrap();
            id
        }

        fn act(
            &self,
            action: &str,
            args: &[&str],
            because: Option<&str>,
        ) -> (&'static str, String) {
            self.hands.carry_out(&Request {
                schema: 1,
                id: "r".into(),
                at: rfc3339(now_ms()),
                action: action.into(),
                args: args.iter().map(|a| a.to_string()).collect(),
                because: because.map(String::from),
            })
        }

        fn in_trash(&self, name: &str) -> bool {
            self.dir
                .path()
                .join("Trash")
                .join(format!("{name}-0000-1111.jsonl"))
                .exists()
        }
    }

    #[test]
    fn purge_needs_your_yes() {
        let w = World::new();
        let api = w.session("api", true);
        let yes = "Delete 1 archived session";
        // No go-ahead, a "no", a yes to something else: nothing goes.
        assert_eq!(w.act("purge", &["api"], None).0, "refused");
        let no = w.asked("purge", &[&api], "Delete api", "Keep them");
        assert_eq!(w.act("purge", &["api"], Some(&no)).0, "refused");
        let no = w.asked("purge-archived", &[&api], yes, "Keep them");
        assert_eq!(w.act("purge-archived", &[], Some(&no)).0, "refused");
        let other = w.asked("forget", &[&api], "Stop offering api", "Stop offering api");
        assert_eq!(w.act("purge", &["api"], Some(&other)).0, "refused");
        assert_eq!(w.act("purge-archived", &[], Some(&other)).0, "refused");
        assert!(w.id("api").is_some(), "its records stay");
        assert!(w.transcript("api").exists(), "its transcript stays");
        // The user's tap on cc-moss's yes: done, once.
        let yes_id = w.asked("purge-archived", &[&api], yes, yes);
        let (outcome, said) = w.act("purge-archived", &[], Some(&yes_id));
        assert_eq!(outcome, "done", "{said}");
        assert!(w.id("api").is_none());
    }

    #[test]
    fn purge_moves_transcripts_to_trash() {
        let w = World::new();
        let api = w.session("api", true);
        let yes = w.asked("purge", &[&api], "Delete api", "Delete api");
        let (outcome, said) = w.act("purge", &["api"], Some(&yes));
        assert_eq!(outcome, "done", "{said}");
        assert!(w.id("api").is_none(), "its records are gone");
        assert!(!w.transcript("api").exists());
        assert!(w.in_trash("api"), "the transcript is in the Trash");
        assert!(
            w.registry()
                .events(Some(0), 100)
                .unwrap()
                .iter()
                .all(|e| e.slot_id != api),
            "its events are gone too"
        );
        // Once only.
        assert_ne!(w.act("purge", &["api"], Some(&yes)).0, "done");
    }

    #[test]
    fn purge_touches_only_archived() {
        let w = World::new();
        let old = w.session("old", true);
        let live = w.session("live", false);
        // Even a yes that (somehow) names a session in the list deletes
        // only what is archived.
        let yes = "Delete 2 archived sessions";
        let id = w.asked("purge-archived", &[&old, &live], yes, yes);
        // Archived after Moss asked: the yes never covered it.
        let late = w.session("late", true);
        let (outcome, said) = w.act("purge-archived", &[], Some(&id));
        assert_eq!(outcome, "done", "{said}");
        assert!(w.id("old").is_none());
        assert!(w.in_trash("old"));
        for name in ["live", "late"] {
            assert!(w.id(name).is_some(), "{name} stays");
            assert!(w.transcript(name).exists(), "{name}'s transcript stays");
        }
        // Asked by name, a session in the list is not deleted either.
        let id = w.asked("purge", &[&live], "Delete live", "Delete live");
        assert_eq!(w.act("purge", &["live"], Some(&id)).0, "skipped");
        // Each layer on its own refuses a session that is not archived.
        let bin = super::super::archive::Disposal {
            trash: Trash::Folder(w.dir.path().join("Trash")),
            claude_dirs: vec![w.dir.path().join("claude")],
        };
        let done =
            super::super::archive::purge(&w.registry(), std::slice::from_ref(&live), &bin).unwrap();
        assert!(done.removed.is_empty() && done.trashed.is_empty());
        assert!(w.registry().purge(&[live.clone(), late.clone()]).unwrap() == [late.clone()]);
        assert!(w.id("live").is_some());
        assert!(w.transcript("live").exists());
    }
}
