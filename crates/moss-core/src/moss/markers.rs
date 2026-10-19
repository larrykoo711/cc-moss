//! Control markers in Moss's replies. `[[silent]]` keeps a reply out of the
//! panel and quiet; `[[buttons: yes | not now]]` turns into buttons that send
//! that text back; `[[quick: /usage | stuck]]` pins up to three things on the
//! panel's quick bar. All are taken out of the text people read.

/// A reply with its markers read and taken out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub text: String,
    pub silent: bool,
    pub buttons: Vec<String>,
    /// What to pin on the panel's quick bar: built-in action ids and
    /// Claude Code commands the panel may run.
    pub quick: Vec<String>,
    /// Sessions Moss points at (`[[sessions:]]`), as written: names, slot
    /// ids or id prefixes; [`resolve_sessions`] makes them slot ids.
    pub sessions: Vec<String>,
    /// What Moss asks the user to agree to (`[[confirm: restore api]]`).
    pub confirms: Vec<Confirm>,
}

/// One action Moss asks the user to agree to: which session, and which of
/// the reply's buttons mean yes to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Confirm {
    pub action: String,
    /// As written: a name, slot id or id prefix.
    pub session: String,
}

/// What a session was doing when Moss pointed at it, so a card does not
/// keep looking urgent after the fact: from its status as shown (a gone
/// process is dropped), what it waits on, and whether it is in a turn.
pub fn session_state(slot: &crate::slot::Slot, status: crate::slot::Status) -> &'static str {
    use crate::slot::Status;
    match status {
        Status::Closed => "closed",
        Status::Interrupted => "dropped",
        Status::Running => match slot.attention.as_deref() {
            Some("permission") => "needs-you-permission",
            Some("question") => "needs-you-question",
            Some(_) => "needs-you-input",
            None if slot.busy => "working",
            None => "idle",
        },
    }
}

/// The cards' states for a reply to session news: what the news said for
/// the sessions it named (`told`, by slot id), the state now for the rest.
pub fn states_as_told(
    now: Vec<(String, String)>,
    told: &std::collections::BTreeMap<String, String>,
) -> Vec<(String, String)> {
    now.into_iter()
        .map(|(id, state)| {
            let state = told.get(&id).cloned().unwrap_or(state);
            (id, state)
        })
        .collect()
}

/// At most this many sessions in one reply.
const MAX_SESSIONS: usize = 6;

/// Closed sessions this recent can still be pointed at.
const CLOSED_WITHIN_MS: i64 = 30 * 86_400_000;

/// An id prefix this long or longer counts.
const MIN_PREFIX: usize = 6;

/// The slot ids of the sessions `items` name, in order, each once. An item
/// is a session's name, its tmux name, its slot id, or a prefix (six or
/// more characters) of its slot id or session id. Open sessions and those
/// closed in the last 30 days count, Moss never; an item that names none or
/// several is dropped.
pub fn resolve_sessions(items: &[String], slots: &[crate::slot::Slot], now_ms: i64) -> Vec<String> {
    use crate::slot::Status;
    let known: Vec<&crate::slot::Slot> = slots
        .iter()
        .filter(|s| !s.is_moss())
        .filter(|s| {
            s.status != Status::Closed
                || s.ended_at_ms.unwrap_or(s.last_active_at_ms) >= now_ms - CLOSED_WITHIN_MS
        })
        .collect();
    resolve_among(items, &known)
}

/// [`resolve_sessions`] among archived sessions only, however long ago
/// they closed: what `purge` may name.
pub fn resolve_archived(items: &[String], slots: &[crate::slot::Slot]) -> Vec<String> {
    let known: Vec<&crate::slot::Slot> = slots
        .iter()
        .filter(|s| !s.is_moss() && s.is_archived())
        .collect();
    resolve_among(items, &known)
}

fn resolve_among(items: &[String], known: &[&crate::slot::Slot]) -> Vec<String> {
    use crate::slot::{Slot, Status};
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let q = item.as_str();
        let long = q.chars().count() >= MIN_PREFIX;
        let tiers: [&dyn Fn(&Slot) -> bool; 5] = [
            &|s| s.name.as_deref() == Some(q),
            &|s| s.tmux.as_ref().and_then(|t| t.session_name.as_deref()) == Some(q),
            &|s| s.slot_id == q,
            &|s| long && s.slot_id.starts_with(q),
            &|s| {
                long && std::iter::once(&s.session_id)
                    .chain(&s.session_ids)
                    .any(|id| id.starts_with(q))
            },
        ];
        for tier in tiers {
            let found: Vec<&&Slot> = known.iter().filter(|s| tier(s)).collect();
            let open: Vec<&&Slot> = found
                .iter()
                .copied()
                .filter(|s| s.status != Status::Closed)
                .collect();
            let pick = if open.is_empty() { found } else { open };
            match pick.as_slice() {
                [] => continue,
                [one] if !out.contains(&one.slot_id) => out.push(one.slot_id.clone()),
                // Named already, or several: no telling which.
                _ => {}
            }
            break;
        }
    }
    out
}

/// At most this many buttons; more would not fit a chat bubble.
const MAX_BUTTONS: usize = 6;

/// The actions that need the user's go-ahead (`cc-moss moss act`).
pub const CONSENT_ACTIONS: &[&str] = &["restore", "forget", "end", "purge", "purge-archived"];

/// The one consent action that names no session: it covers every archived
/// session there is when Moss asks.
pub const PURGE_ARCHIVED: &str = "purge-archived";

/// `cwd` as a yes button shows it: the last one or two folders, from `~`
/// when it is in `home` (`~/code/foo`, `…/srv/app`).
pub fn short_folder(cwd: &str, home: Option<&std::path::Path>) -> String {
    let path = std::path::Path::new(cwd);
    let parts = |p: &std::path::Path| -> Vec<String> {
        p.components()
            .filter_map(|c| match c {
                std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    if let Some(rel) = home
        .filter(|h| h.is_absolute())
        .and_then(|h| path.strip_prefix(h).ok())
    {
        let rel = parts(rel);
        return match rel.len() {
            0 => "~".to_string(),
            1 | 2 => format!("~/{}", rel.join("/")),
            n => format!("…/{}", rel[n - 2..].join("/")),
        };
    }
    let all = parts(path);
    match all.len() {
        0..=2 => cwd.to_string(),
        n => format!("…/{}", all[n - 2..].join("/")),
    }
}

/// At most this many `[[confirm:]]` in one message.
const MAX_CONFIRMS: usize = 12;

/// The yes button cc-moss puts under a question for `action` on the
/// sessions `names`: it says what the tap agrees to, in the user's words,
/// and only a tap on it is a go-ahead (Moss never words it).
pub fn yes_label(action: &str, names: &[String], chinese: bool) -> String {
    let (verb, sep) = match (action, chinese) {
        ("restore", true) => ("找回", "、"),
        ("forget", true) => ("不再提示", "、"),
        ("end", true) => ("结束", "、"),
        ("purge", true) => ("删除", "、"),
        ("restore", false) => ("Bring back", ", "),
        ("forget", false) => ("Stop offering", ", "),
        ("end", false) => ("End", ", "),
        ("purge", false) => ("Delete", ", "),
        (other, _) => (other, ", "),
    };
    format!("{verb} {}", names.join(sep))
}

/// The yes button for `purge-archived`, from the real number of archived
/// sessions it covers («删除 12 个归档会话», "Delete 12 archived sessions").
pub fn purge_archived_label(count: usize, chinese: bool) -> String {
    match (chinese, count) {
        (true, n) => format!("删除 {n} 个归档会话"),
        (false, 1) => "Delete 1 archived session".to_string(),
        (false, n) => format!("Delete {n} archived sessions"),
    }
}

/// At most this many pinned on the quick bar.
const MAX_QUICK: usize = 3;

/// The panel's built-in actions (`MossAction` ids in the app).
pub const ACTIONS: &[&str] = &["needs-you", "stuck", "today", "checkup", "tidy"];

/// Whether `item` can sit on the quick bar: a built-in action, or a Claude
/// Code command the panel runs.
fn quick_item(item: &str) -> bool {
    ACTIONS.contains(&item)
        || (!item.contains(char::is_whitespace)
            && !matches!(
                super::slash::classify(item),
                None | Some(super::slash::Slash::Refused(_))
            ))
}

/// Read the markers in `reply`. A marker is `[[` … `]]` on one line; ones
/// cc-moss does not know stay in the text as written.
pub fn parse(reply: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut text = String::new();
    let mut rest = reply;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]").filter(|c| !after[..*c].contains('\n')) else {
            text.push_str(&rest[..open + 2]);
            rest = after;
            continue;
        };
        let inner = after[..close].trim();
        let known = if inner.eq_ignore_ascii_case("silent") {
            out.silent = true;
            true
        } else if let Some(list) = strip_prefix_ci(inner, "buttons:") {
            for label in list.split('|').map(str::trim).filter(|l| !l.is_empty()) {
                if out.buttons.len() < MAX_BUTTONS && !out.buttons.iter().any(|b| b == label) {
                    out.buttons.push(label.to_string());
                }
            }
            true
        } else if let Some(list) = strip_prefix_ci(inner, "sessions:") {
            for item in list.split('|').map(str::trim).filter(|i| !i.is_empty()) {
                if out.sessions.len() < MAX_SESSIONS && !out.sessions.iter().any(|s| s == item) {
                    out.sessions.push(item.to_string());
                }
            }
            true
        } else if let Some(spec) = strip_prefix_ci(inner, "confirm:") {
            // `restore <session>`: only the actions that need the user's
            // go-ahead, with a session. Button labels after `|` are Moss's
            // words and never count as a yes: cc-moss words the yes button
            // itself ([`yes_label`]).
            let head: Vec<&str> = spec
                .split('|')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            if let [action] = head.as_slice() {
                // `purge-archived`: every archived session, none named.
                let confirm = Confirm {
                    action: action.to_string(),
                    session: String::new(),
                };
                if *action == PURGE_ARCHIVED
                    && out.confirms.len() < MAX_CONFIRMS
                    && !out.confirms.contains(&confirm)
                {
                    out.confirms.push(confirm);
                }
            } else if let [action, session] = head.as_slice() {
                let confirm = Confirm {
                    action: action.to_string(),
                    session: session.to_string(),
                };
                if CONSENT_ACTIONS.contains(action)
                    && *action != PURGE_ARCHIVED
                    && out.confirms.len() < MAX_CONFIRMS
                    && !out.confirms.contains(&confirm)
                {
                    out.confirms.push(confirm);
                }
            }
            true
        } else if let Some(list) = strip_prefix_ci(inner, "quick:") {
            for item in list.split('|').map(str::trim) {
                if out.quick.len() < MAX_QUICK
                    && quick_item(item)
                    && !out.quick.iter().any(|q| q == item)
                {
                    out.quick.push(item.to_string());
                }
            }
            true
        } else {
            false
        };
        if known {
            text.push_str(&rest[..open]);
        } else {
            text.push_str(&rest[..open + 2 + close + 2]);
        }
        rest = &after[close + 2..];
    }
    text.push_str(rest);
    out.text = tidy(&text);
    out
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// Lines left blank or trailing blanks by a removed marker are dropped.
fn tidy(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut out: Vec<&str> = Vec::new();
    for line in lines {
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_markers_silent_is_read_and_removed() {
        let p = parse("Noted. [[silent]]");
        assert!(p.silent);
        assert_eq!(p.text, "Noted.");
        assert!(p.buttons.is_empty());
        assert!(parse("[[SILENT]]\nok").silent);
    }

    #[test]
    fn reply_markers_buttons_become_a_list() {
        let p = parse("Restore it now?\n\n[[buttons: 好的 | 先不用 | 好的 ]]");
        assert_eq!(p.text, "Restore it now?");
        assert_eq!(p.buttons, ["好的", "先不用"]);
        assert!(!p.silent);
    }

    fn slot(id: &str, sid: &str, name: Option<&str>) -> crate::slot::Slot {
        crate::slot::Slot {
            slot_id: id.into(),
            agent: "claude".into(),
            session_id: sid.into(),
            session_ids: vec![sid.into()],
            name: name.map(String::from),
            cwd: "/w".into(),
            pid: None,
            pid_start: None,
            tmux: None,
            status: crate::slot::Status::Running,
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
            last_active_at_ms: 0,
            ended_at_ms: None,
            archived_at_ms: None,
        }
    }

    #[test]
    fn reply_markers_sessions() {
        const DAY: i64 = 86_400_000;
        let now = 100 * DAY;
        let mut moss = slot("SLOTMOSS", "99999999-moss", Some("Moss"));
        moss.role = Some("moss".into());
        let mut old = slot("SLOTOLD1", "77777777-old", Some("ancient"));
        old.status = crate::slot::Status::Closed;
        old.ended_at_ms = Some(now - 40 * DAY);
        let mut recent = slot("SLOTDONE", "66666666-done", Some("docs"));
        recent.status = crate::slot::Status::Closed;
        recent.ended_at_ms = Some(now - 2 * DAY);
        let slots = vec![
            slot("SLOTAPI1", "a1b2c3d4-0000", Some("api")),
            slot("SLOTWEB1", "e5f6a7b8-0000", Some("web")),
            slot("SLOTTWIN", "abcabc11-0000", Some("twin")),
            slot("SLOTTWIN2", "abcabc22-0000", Some("twin2")),
            moss,
            old,
            recent,
        ];
        let p = parse(
            "Two match. [[sessions: api | e5f6a7b8 | SLOTDONE | ancient | Moss | api | abcabc | nobody]] [[buttons: Open | Later]] [[quick: today]]",
        );
        assert_eq!(p.text, "Two match.");
        assert_eq!(p.buttons, ["Open", "Later"]);
        assert_eq!(p.quick, ["today"]);
        // As written, repeats once, at most six.
        assert_eq!(
            p.sessions,
            ["api", "e5f6a7b8", "SLOTDONE", "ancient", "Moss", "abcabc"]
        );
        // By name, by session id prefix, by slot id; closed within 30 days.
        // Unknown, Moss, ambiguous (abcabc) and closed long ago are dropped.
        assert_eq!(
            resolve_sessions(&p.sessions, &slots, now),
            ["SLOTAPI1", "SLOTWEB1", "SLOTDONE"]
        );
        assert!(resolve_sessions(&["nobody".into()], &slots, now).is_empty());
        // Each card keeps what the session was doing when Moss said it.
        use crate::slot::Status;
        let mut s = slot("S", "s", Some("x"));
        let state = |s: &crate::slot::Slot, st: Status| session_state(s, st);
        assert_eq!(state(&s, Status::Running), "idle");
        s.busy = true;
        assert_eq!(state(&s, Status::Running), "working");
        for (why, want) in [
            ("permission", "needs-you-permission"),
            ("question", "needs-you-question"),
            ("input", "needs-you-input"),
        ] {
            s.attention = Some(why.into());
            assert_eq!(state(&s, Status::Running), want);
        }
        assert_eq!(state(&s, Status::Interrupted), "dropped");
        assert_eq!(state(&s, Status::Closed), "closed");
        // A late reply to news: the cards say what the news said, so the
        // app can show "handled, idle now" against the session as it is.
        let now = vec![
            ("A".to_string(), "idle".to_string()),
            ("B".to_string(), "working".to_string()),
        ];
        let told = [("A".to_string(), "needs-you-permission".to_string())].into();
        assert_eq!(
            states_as_told(now, &told),
            [
                ("A".to_string(), "needs-you-permission".to_string()),
                ("B".to_string(), "working".to_string())
            ]
        );
        // At most six, and with silent too.
        let p = parse("[[silent]][[sessions: a|b|c|d|e|f|g|h]]");
        assert!(p.silent);
        assert_eq!(p.sessions.len(), 6);
        assert!(parse("plain").sessions.is_empty());
    }

    #[test]
    fn reply_markers_short_folder() {
        let home = Some(std::path::Path::new("/Users/me"));
        assert_eq!(short_folder("/Users/me/code/foo", home), "~/code/foo");
        assert_eq!(short_folder("/Users/me/foo", home), "~/foo");
        assert_eq!(short_folder("/Users/me", home), "~");
        assert_eq!(short_folder("/Users/me/a/b/c", home), "…/b/c");
        assert_eq!(short_folder("/srv/www/app", home), "…/www/app");
        assert_eq!(short_folder("/srv/app", None), "/srv/app");
    }

    #[test]
    fn reply_markers_confirm() {
        let p = parse(
            "4 dropped. [[buttons: 都不要了]]\n[[confirm: restore api | 好的]][[confirm: forget api]][[confirm: dispatch x]][[confirm: restore]][[confirm: restore api]]",
        );
        assert_eq!(p.text, "4 dropped.");
        assert_eq!(
            p.confirms,
            [
                Confirm {
                    action: "restore".into(),
                    session: "api".into(),
                },
                Confirm {
                    action: "forget".into(),
                    session: "api".into(),
                },
            ],
            "only consent actions, each with a session, once"
        );
    }

    #[test]
    fn reply_markers_confirm_purge() {
        let p = parse(
            "Clean up? [[confirm: purge api]][[confirm: purge-archived]][[confirm: purge-archived api]][[confirm: purge]]",
        );
        assert_eq!(
            p.confirms,
            [
                Confirm {
                    action: "purge".into(),
                    session: "api".into(),
                },
                Confirm {
                    action: "purge-archived".into(),
                    session: String::new(),
                },
            ],
            "purge names one session, purge-archived none"
        );
        assert_eq!(purge_archived_label(12, true), "删除 12 个归档会话");
        assert_eq!(
            purge_archived_label(12, false),
            "Delete 12 archived sessions"
        );
        assert_eq!(purge_archived_label(1, false), "Delete 1 archived session");
        assert_eq!(
            yes_label("purge", &["api".into(), "db".into()], true),
            "删除 api、db"
        );
    }

    #[test]
    fn reply_markers_quick() {
        let p = parse("Limits are tight today.\n[[quick: /usage | stuck]]");
        assert_eq!(p.text, "Limits are tight today.");
        assert_eq!(p.quick, ["/usage", "stuck"]);
        // Unknown items dropped (other ids, refused commands, plain words),
        // repeats once, at most three.
        let p = parse("[[quick: checkup | tidy]]x");
        assert_eq!(
            p.quick,
            ["checkup", "tidy"],
            "the panel's checkup and tidy-up"
        );
        let p = parse("[[quick: needs-you | waiting | /login | open it | today | /usage | /status | needs-you]]ok");
        assert_eq!(p.quick, ["needs-you", "today", "/usage"]);
        assert_eq!(p.text, "ok");
        // Alongside the other markers.
        let p = parse("Noted. [[silent]] [[buttons: Yes | No]] [[QUICK: /cost]]");
        assert!(p.silent);
        assert_eq!(p.buttons, ["Yes", "No"]);
        assert_eq!(p.quick, ["/cost"]);
        assert_eq!(p.text, "Noted.");
        assert!(parse("[[quick: nothing known]] hi").quick.is_empty());
    }

    #[test]
    fn reply_markers_unknown_ones_stay() {
        let p = parse("a [[link]] b [[ unclosed\nc ]]");
        assert_eq!(p.text, "a [[link]] b [[ unclosed\nc ]]");
        assert!(!p.silent && p.buttons.is_empty());
    }

    #[test]
    fn reply_markers_plain_text_is_untouched() {
        let p = parse("line one\n\n```\ncode\n```");
        assert_eq!(p.text, "line one\n\n```\ncode\n```");
    }
}
