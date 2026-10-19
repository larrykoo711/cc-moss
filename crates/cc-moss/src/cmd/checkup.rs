//! The checkup (体检, T2.4): `cc-moss doctor --sessions --json` gives Moss
//! the facts about every session and about the install, never a verdict;
//! what cannot be told is `"unknown"`, never "broken". And the daily
//! checkup: the daemon asks Moss for one, the same as the panel's button,
//! the first time it sees the desktop each day.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, Slot, Status};
use moss_core::tmux::Socket;
use moss_core::{naming, store_dir};
use serde_json::{json, Value};

/// A fact that may not be knowable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fact {
    Yes,
    No,
    Unknown,
}

impl Fact {
    fn json(self) -> Value {
        match self {
            Fact::Yes => json!(true),
            Fact::No => json!(false),
            Fact::Unknown => json!("unknown"),
        }
    }
}

/// Whether a folder is there, from what `stat` said: only "no such file"
/// is gone; anything else (no permission, a timeout) is unknown.
pub fn folder_fact(stat: &io::Result<std::fs::Metadata>) -> Fact {
    match stat {
        Ok(m) if m.is_dir() => Fact::Yes,
        Ok(_) => Fact::No,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Fact::No,
        Err(_) => Fact::Unknown,
    }
}

/// A name that is no name: none at all, or the session id's first letters.
pub fn auto_named(slot: &Slot) -> bool {
    match slot.name.as_deref().map(str::trim) {
        None | Some("") => true,
        Some(name) => {
            naming::looks_like_id(name)
                || (name.len() >= 6
                    && name.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                    && slot.session_id.starts_with(name))
        }
    }
}

/// tmux sessions by id on one server, or `None` when tmux did not answer.
pub type TmuxNames = Option<HashMap<String, String>>;

/// Whether a session's tmux name is its name (`<prefix>_<name>`, or that
/// with a `-2` style suffix). `None` (shown as null) when it is not in
/// tmux or has no name to match; unknown when tmux did not say.
pub fn tmux_fact(slot: &Slot, prefix: Option<&str>, live: &TmuxNames) -> Option<Fact> {
    let loc = slot.tmux.as_ref()?;
    let name = slot.name.as_deref().filter(|n| !n.is_empty())?;
    let id = loc.session_id.as_deref()?;
    let Some(live) = live else {
        return Some(Fact::Unknown);
    };
    let Some(actual) = live.get(id) else {
        return Some(Fact::Unknown);
    };
    let want = naming::session_name(prefix, name);
    let suffixed = actual
        .strip_prefix(&want)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    Some(if *actual == want || suffixed {
        Fact::Yes
    } else {
        Fact::No
    })
}

/// Per slot id: the corrected name of a named session that is not
/// standard, and a name for one without (from its folder), each unique
/// among the names there are.
pub fn name_facts(slots: &[&Slot]) -> HashMap<String, (Option<String>, Option<String>)> {
    let mut taken: Vec<String> = slots.iter().filter_map(|s| s.name.clone()).collect();
    let mut out = HashMap::new();
    for s in slots {
        let (fix, suggestion) = if auto_named(s) {
            let want = naming::short_name(&s.cwd);
            (None, Some(naming::unique(&want, &taken, None)))
        } else {
            let fix = s
                .name
                .as_deref()
                .and_then(naming::standard_name)
                .map(|want| naming::unique(&want, &taken, s.name.as_deref()));
            (fix, None)
        };
        if let Some(n) = fix.as_ref().or(suggestion.as_ref()) {
            taken.push(n.clone());
        }
        out.insert(s.slot_id.clone(), (fix, suggestion));
    }
    out
}

/// The facts about one session.
pub fn session_facts(
    slot: &Slot,
    status: Status,
    now_ms: i64,
    folder: Fact,
    tmux_matches: Option<Fact>,
    names: (Option<String>, Option<String>),
) -> Value {
    let secs = |since: i64| ((now_ms - since).max(0)) / 1000;
    let dropped = (status == Status::Interrupted)
        .then(|| secs(slot.ended_at_ms.unwrap_or(slot.last_active_at_ms)));
    let idle = (status == Status::Running && !slot.busy && slot.attention.is_none())
        .then(|| secs(slot.last_active_at_ms));
    json!({
        "slot_id": slot.slot_id,
        "session_id": slot.session_id,
        "name": slot.name,
        "folder": slot.cwd,
        "status": status.as_str(),
        "auto_named": auto_named(slot),
        "folder_exists": folder.json(),
        "dropped_for_secs": dropped,
        "idle_for_secs": idle,
        "working": status == Status::Running && slot.busy,
        "waiting_on_you": slot.attention.as_deref().filter(|_| status == Status::Running),
        "tmux_name_matches": tmux_matches.map(Fact::json),
        "name_fix": names.0,
        "name_suggestion": names.1,
    })
}

pub(super) fn live_names(socket: &str) -> TmuxNames {
    let out = moss_core::command::run_with_timeout(
        Command::new("tmux")
            .args(Socket::Path(socket.to_string()).args())
            .args(["list-sessions", "-F", "#{session_id}|:|#{session_name}"]),
        std::time::Duration::from_secs(3),
    )
    .ok()?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
        // A server that is plainly not there holds no sessions.
        let gone = said.contains("no server running") || said.contains("no such file");
        return gone.then(HashMap::new);
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split_once("|:|"))
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect(),
    )
}

/// The machine's prefix in tmux names, when the config uses one.
pub(super) fn prefix() -> Option<String> {
    moss_core::config::Config::load()
        .map(|c| c.naming.host_prefix)
        .unwrap_or(true)
        .then(|| naming::cached_host_prefix(&store_dir::state_dir()))
        .flatten()
}

/// `cc-moss doctor --sessions --json`.
pub fn print_sessions_json() -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&report()?)?);
    Ok(())
}

pub fn report() -> Result<Value> {
    let now = now_ms();
    let path = store_dir::registry_path();
    let slots = if path.exists() {
        Registry::open(&path)?.slots()?
    } else {
        Vec::new()
    };
    let prefix = prefix();
    let mut servers: HashMap<String, TmuxNames> = HashMap::new();
    let mut sessions = Vec::new();
    let open: Vec<(&Slot, Status)> = slots
        .iter()
        .filter(|s| !s.is_moss())
        .map(|s| (s, s.effective_status(moss_core::proc::is_alive)))
        .filter(|(_, st)| *st != Status::Closed)
        .collect();
    let names = name_facts(&open.iter().map(|(s, _)| *s).collect::<Vec<_>>());
    for (slot, status) in open {
        let folder = folder_fact(&std::fs::metadata(&slot.cwd));
        let live = slot.tmux.as_ref().map(|loc| {
            servers
                .entry(loc.socket.clone())
                .or_insert_with(|| live_names(&loc.socket))
                .clone()
        });
        let tmux = match (status, live) {
            (Status::Running, Some(live)) => tmux_fact(slot, prefix.as_deref(), &live),
            _ => None,
        };
        let named = names.get(&slot.slot_id).cloned().unwrap_or_default();
        sessions.push(session_facts(slot, status, now, folder, tmux, named));
    }
    Ok(json!({
        "schema": 1,
        "sessions": sessions,
        "install": install_facts(),
    }))
}

/// Whether the hooks and the alias links are in place.
fn install_facts() -> Value {
    let hooks = match super::setup::hook_status() {
        Ok(super::setup::HookStatus::Current) => Fact::Yes,
        Ok(_) => Fact::No,
        Err(_) => Fact::Unknown,
    };
    let links = match (
        super::alias::Place::detect(),
        moss_core::config::Config::load(),
    ) {
        (Ok(place), Ok(config)) => {
            let rows = super::alias::rows(&place, &config, false);
            if rows
                .iter()
                .any(|r| matches!(r.status(), "missing" | "broken" | "stale"))
            {
                Fact::No
            } else {
                Fact::Yes
            }
        }
        _ => Fact::Unknown,
    };
    json!({ "hooks": hooks.json(), "alias_links": links.json() })
}

// ---------------------------------------------------------------- daily

/// What the panel's 体检 button sends, word for word.
pub fn request(zh: bool) -> &'static str {
    if zh {
        "体检一下：能修的直接修，修完给我一段简短的体检简报；要我定的给我选项。"
    } else {
        "Give my sessions a checkup: fix what you can, then a short brief; give me options for what I need to decide."
    }
}

/// The daily checkup's request: the button's, and with the brief switched
/// off, a note to keep it quiet unless something needs a decision.
pub fn daily_request(zh: bool, brief: bool) -> String {
    let base = request(zh);
    if brief {
        return base.to_string();
    }
    if zh {
        format!("{base}（这是每天的例行体检：简报加上 [[silent]]，除非有要我定的事。）")
    } else {
        format!(
            "{base} (This is the daily checkup: mark the brief [[silent]] unless something needs my decision.)"
        )
    }
}

/// Whether the daily checkup is due: switched on, the desktop is there,
/// and none was sent on `today` (a local date).
pub fn daily_due(enabled: bool, desktop: bool, last: Option<&str>, today: &str) -> bool {
    enabled && desktop && last != Some(today)
}

/// `<state>/daemon/moss-checkup.txt`: the local date of the last daily
/// checkup.
pub fn daily_path(state: &Path) -> std::path::PathBuf {
    moss_core::daemon::Paths::new(state)
        .dir
        .join("moss-checkup.txt")
}

/// Today's local date, `YYYY-MM-DD`.
pub fn today() -> String {
    // SAFETY: time(NULL) and localtime_r with a valid out-pointer.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::event::TmuxLoc;

    fn slot(name: Option<&str>) -> Slot {
        Slot {
            slot_id: "s1".into(),
            agent: "claude".into(),
            session_id: "0123abcd-4567".into(),
            session_ids: vec![],
            name: name.map(String::from),
            cwd: "/work/api".into(),
            pid: Some(1),
            pid_start: None,
            tmux: Some(TmuxLoc {
                socket: "/tmp/s".into(),
                session_id: Some("$1".into()),
                session_name: None,
                pane_id: "%1".into(),
            }),
            status: Status::Running,
            end_reason: None,
            notes: Default::default(),
            launch_args: vec![],
            transcript_path: None,
            busy: false,
            attention: None,
            last_prompt: None,
            restore_started_at_ms: None,
            role: None,
            created_at_ms: 0,
            last_active_at_ms: 1_000,
            ended_at_ms: None,
        }
    }

    #[test]
    fn checkup_facts() {
        let now = 3 * 86_400_000 + 1_000;
        // Open, idle three days, named, in its tmux under its name.
        let s = slot(Some("api"));
        let live: TmuxNames = Some([("$1".to_string(), "host_api-2".to_string())].into());
        let f = session_facts(
            &s,
            Status::Running,
            now,
            Fact::Yes,
            tmux_fact(&s, Some("host"), &live),
            (None, None),
        );
        assert_eq!(f["auto_named"], false);
        assert_eq!(f["idle_for_secs"], 3 * 86_400);
        assert_eq!(f["dropped_for_secs"], Value::Null);
        assert_eq!(f["tmux_name_matches"], true);
        assert_eq!(f["folder_exists"], true);
        // Renamed in tmux by hand: a mismatch.
        let other: TmuxNames = Some([("$1".to_string(), "scratch".to_string())].into());
        assert_eq!(tmux_fact(&s, Some("host"), &other), Some(Fact::No));
        // Nameless, or named after its id: auto-named.
        assert!(auto_named(&slot(None)));
        assert!(auto_named(&slot(Some("0123abcd"))));
        assert!(!auto_named(&slot(Some("deadbeef-notes"))));
        // Dropped two days ago, folder gone.
        let mut d = slot(Some("old"));
        d.ended_at_ms = Some(now - 2 * 86_400_000);
        let f = session_facts(&d, Status::Interrupted, now, Fact::No, None, (None, None));
        assert_eq!(f["dropped_for_secs"], 2 * 86_400);
        assert_eq!(f["idle_for_secs"], Value::Null);
        assert_eq!(f["folder_exists"], false);
        assert_eq!(f["tmux_name_matches"], Value::Null);
        // Waiting on you is not idle.
        let mut w = slot(Some("web"));
        w.attention = Some("permission".into());
        let f = session_facts(&w, Status::Running, now, Fact::Yes, None, (None, None));
        assert_eq!(f["waiting_on_you"], "permission");
        assert_eq!(f["idle_for_secs"], Value::Null);
        let missing = std::fs::metadata("/nonexistent/cc-moss/checkup");
        assert_eq!(folder_fact(&missing), Fact::No);
    }

    #[test]
    fn checkup_names() {
        let mut nameless = slot(None);
        nameless.cwd = "/work/ofoxai".into();
        let mut id_named = slot(Some("0123abcd"));
        id_named.slot_id = "s2".into();
        id_named.cwd = "/r/.claude/worktrees/97-t7.1".into();
        let mut taken = slot(Some("ofoxai"));
        taken.slot_id = "s3".into();
        let mut spaced = slot(Some("接口 评审 v1.2"));
        spaced.slot_id = "s4".into();
        let mut fine = slot(Some("接口评审"));
        fine.slot_id = "s5".into();
        let mut twin = slot(None);
        twin.slot_id = "s6".into();
        twin.cwd = "/elsewhere/ofoxai".into();
        let all = [nameless, id_named, taken, spaced, fine, twin];
        let refs: Vec<&Slot> = all.iter().collect();
        let names = name_facts(&refs);
        let get = |id: &str| names.get(id).cloned().unwrap();
        // Unnamed: from the folder, unique among the names there are.
        assert_eq!(get("s1"), (None, Some("ofoxai-2".to_string())));
        assert_eq!(get("s6"), (None, Some("ofoxai-3".to_string())));
        assert_eq!(get("s2"), (None, Some("97-t7-1".to_string())));
        // Named but not standard: corrected.
        assert_eq!(get("s4"), (Some("接口-评审-v1-2".to_string()), None));
        // Standard: nothing to do.
        assert_eq!(get("s5"), (None, None));
        assert_eq!(get("s3"), (None, None));
    }

    #[test]
    fn checkup_unknown_is_not_broken() {
        // A folder cc-moss may not look into.
        let denied: io::Result<std::fs::Metadata> =
            Err(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(folder_fact(&denied), Fact::Unknown);
        let f = session_facts(
            &slot(Some("api")),
            Status::Running,
            5_000,
            Fact::Unknown,
            None,
            (None, None),
        );
        assert_eq!(f["folder_exists"], "unknown");
        // tmux that does not answer, or a session it does not list.
        let s = slot(Some("api"));
        assert_eq!(tmux_fact(&s, None, &None), Some(Fact::Unknown));
        assert_eq!(
            tmux_fact(&s, None, &Some(HashMap::new())),
            Some(Fact::Unknown)
        );
        // A process cc-moss cannot check counts as running, not dropped.
        let mut s = slot(Some("api"));
        s.pid = Some(424_242);
        let status = s.effective_status_since(|_, _| true, None);
        assert_eq!(status, Status::Running);
    }

    #[test]
    fn daily_checkup_once_a_day() {
        assert!(daily_due(true, true, None, "2026-10-19"));
        assert!(!daily_due(true, true, Some("2026-10-19"), "2026-10-19"));
        assert!(daily_due(true, true, Some("2026-10-18"), "2026-10-19"));
        // Only once the desktop is there.
        assert!(!daily_due(true, false, Some("2026-10-18"), "2026-10-19"));
        // The same words as the button.
        assert_eq!(daily_request(true, true), request(true));
        assert_eq!(daily_request(false, true), request(false));
    }

    #[test]
    fn daily_checkup_off() {
        assert!(!daily_due(false, true, None, "2026-10-19"));
    }

    #[test]
    fn daily_brief_off_is_silent_unless_asking() {
        let zh = daily_request(true, false);
        assert!(zh.starts_with(request(true)));
        assert!(
            zh.contains("[[silent]]") && zh.contains("除非有要我定的事"),
            "{zh}"
        );
        let en = daily_request(false, false);
        assert!(
            en.contains("[[silent]] unless something needs my decision"),
            "{en}"
        );
    }
}
