//! Moss's home, `<state>/moss/`: laying it out, taking cc-moss's part of it
//! away again, and the session overview the daemon keeps there.
//!
//! cc-moss's files (managed) are written afresh every time, so an upgrade
//! brings new ones. The persona, journal and memory are the user's and
//! Moss's: created once when missing, never overwritten, never deleted.

use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::slot::{rfc3339, Slot, Status};

/// Whose a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// cc-moss's: replaced on every layout, removed on uninstall.
    Managed,
    /// The user's or Moss's: only created when missing.
    User,
}

/// One file of the home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Relative to the home.
    pub path: &'static str,
    pub text: String,
    pub kind: Kind,
}

/// Folders Moss keeps its own things in; created when missing.
pub const USER_DIRS: &[&str] = &["journal", "memory"];

/// The generated overview.
pub const SESSIONS_MD: &str = "SESSIONS.md";

/// What a layout did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Managed files that were missing or different.
    pub updated: Vec<PathBuf>,
    /// User files and folders made because they were missing.
    pub created: Vec<PathBuf>,
}

/// Lay out `home` with `files`. Missing folders are made (owner only).
pub fn lay_out(home: &Path, files: &[File]) -> io::Result<Outcome> {
    let mut out = Outcome::default();
    private_dir(home)?;
    for dir in USER_DIRS {
        let path = home.join(dir);
        if std::fs::symlink_metadata(&path).is_err() {
            private_dir(&path)?;
            out.created.push(path);
        }
    }
    for file in files {
        let path = home.join(file.path);
        if let Some(parent) = path.parent() {
            private_dir(parent)?;
        }
        match file.kind {
            Kind::Managed => {
                if std::fs::read_to_string(&path).ok().as_deref() != Some(file.text.as_str()) {
                    replace(&path, &file.text)?;
                    out.updated.push(path);
                }
            }
            Kind::User => {
                // Anything there already (an edited file, an empty one, a
                // link, even a dangling one) is left exactly as it is.
                if std::fs::symlink_metadata(&path).is_ok() {
                    continue;
                }
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
                {
                    Ok(mut f) => {
                        use std::io::Write;
                        f.write_all(file.text.as_bytes())?;
                        out.created.push(path);
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(out)
}

/// Remove the managed files (and folders left empty by that, the home too
/// if nothing else is in it). The persona, journal and memory stay.
pub fn remove_managed(home: &Path, files: &[File]) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    let mut generated: Vec<&str> = files
        .iter()
        .filter(|f| f.kind == Kind::Managed)
        .map(|f| f.path)
        .collect();
    generated.push(SESSIONS_MD);
    for rel in generated {
        let path = home.join(rel);
        match std::fs::remove_file(&path) {
            Ok(()) => removed.push(path.clone()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        // Folders this file was in, deepest first, while they are empty.
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(home) && *d != home) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    for dir in USER_DIRS {
        let _ = std::fs::remove_dir(home.join(dir));
    }
    let _ = std::fs::remove_dir(home);
    Ok(removed)
}

/// Write `text` to `path` if it differs. True when it changed.
pub fn write_if_changed(path: &Path, text: &str) -> io::Result<bool> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(false);
    }
    replace(path, text)?;
    Ok(true)
}

/// A temporary file renamed over `path`: readers see the old or the new.
fn replace(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(text.as_bytes())?;
    drop(f);
    std::fs::rename(&tmp, path)
}

fn private_dir(dir: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// `SESSIONS.md`: the sessions cc-moss knows that are not closed, most
/// urgent first, without Moss itself. `rows` carry each slot's status as
/// shown (a dead process is interrupted). No time of writing in it, so an
/// unchanged list writes nothing.
pub fn sessions_md(rows: &[(Status, Slot)]) -> String {
    let mut shown: Vec<&(Status, Slot)> = rows
        .iter()
        .filter(|(st, s)| *st != Status::Closed && s.role.as_deref() != Some(super::ROLE))
        .collect();
    let rank = |st: Status| match st {
        Status::Interrupted => 0,
        Status::Running => 1,
        Status::Closed => 2,
    };
    shown.sort_by_key(|(st, s)| (rank(*st), std::cmp::Reverse(s.last_active_at_ms)));
    let mut out = String::from(
        "# Sessions\n\n\
         Written by cc-moss from its session records; it is rewritten every minute, so changes here are lost.\n\
         `cc-moss ls --json` has the full details.\n\n",
    );
    if shown.is_empty() {
        out.push_str("No sessions right now.\n");
        return out;
    }
    out.push_str("| Name | State | Folder | Last active | Note |\n|---|---|---|---|---|\n");
    for (st, s) in shown {
        let state = match (st, s.attention.as_deref(), s.busy) {
            (Status::Running, Some(why), _) => format!("waiting for you ({why})"),
            (Status::Running, None, true) => "working".to_string(),
            (Status::Running, None, false) => "idle".to_string(),
            (Status::Interrupted, ..) => "interrupted".to_string(),
            (Status::Closed, ..) => "closed".to_string(),
        };
        let name = s
            .name
            .clone()
            .unwrap_or_else(|| s.session_id.chars().take(8).collect());
        let note = s
            .notes
            .state
            .clone()
            .or_else(|| s.notes.note.clone())
            .or_else(|| s.notes.goal.clone())
            .unwrap_or_default();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            cell(&name),
            state,
            cell(&s.cwd),
            rfc3339(s.last_active_at_ms),
            cell(&note)
        ));
    }
    out
}

/// One line, no table breaks.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(version: &str) -> Vec<File> {
        vec![
            File {
                path: "CLAUDE.md",
                text: format!("managed {version}\n"),
                kind: Kind::Managed,
            },
            File {
                path: ".claude/settings.json",
                text: format!("{{\"v\":\"{version}\"}}\n"),
                kind: Kind::Managed,
            },
            File {
                path: "SOUL.md",
                text: format!("default soul {version}\n"),
                kind: Kind::User,
            },
        ]
    }

    #[test]
    fn layout_overwrites_managed_and_keeps_user_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("state/moss");
        let first = lay_out(&home, &files("1")).unwrap();
        assert_eq!(first.updated.len(), 2);
        assert!(home.join("journal").is_dir() && home.join("memory").is_dir());
        assert_eq!(
            std::fs::read_to_string(home.join("SOUL.md")).unwrap(),
            "default soul 1\n"
        );

        std::fs::write(home.join("SOUL.md"), "mine\n").unwrap();
        std::fs::write(home.join("CLAUDE.md"), "edited by hand\n").unwrap();
        let second = lay_out(&home, &files("2")).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join("SOUL.md")).unwrap(),
            "mine\n"
        );
        assert_eq!(
            std::fs::read_to_string(home.join("CLAUDE.md")).unwrap(),
            "managed 2\n"
        );
        assert!(second.created.is_empty());

        // Unchanged: nothing written.
        assert_eq!(lay_out(&home, &files("2")).unwrap(), Outcome::default());
    }

    #[test]
    fn empty_and_linked_user_files_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("moss");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("SOUL.md"), "").unwrap();
        lay_out(&home, &files("1")).unwrap();
        assert_eq!(std::fs::read_to_string(home.join("SOUL.md")).unwrap(), "");
        std::fs::remove_file(home.join("SOUL.md")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("nowhere.md"), home.join("SOUL.md")).unwrap();
        lay_out(&home, &files("1")).unwrap();
        assert!(
            !dir.path().join("nowhere.md").exists(),
            "a dangling link is not written through"
        );
    }

    #[test]
    fn removing_takes_only_managed_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("moss");
        lay_out(&home, &files("1")).unwrap();
        std::fs::write(home.join(SESSIONS_MD), "x").unwrap();
        std::fs::write(home.join("journal/2026-10-18.md"), "did things").unwrap();
        let removed = remove_managed(&home, &files("1")).unwrap();
        assert_eq!(removed.len(), 3);
        assert!(!home.join(".claude").exists());
        assert!(home.join("SOUL.md").exists());
        assert!(home.join("journal/2026-10-18.md").exists());
        assert!(!home.join("memory").exists(), "an empty folder goes");

        // Nothing of the user's: the home goes too.
        let bare = dir.path().join("bare");
        lay_out(&bare, &files("1")[..2]).unwrap();
        remove_managed(&bare, &files("1")).unwrap();
        assert!(!bare.exists());
    }

    fn slot(name: &str, status: Status, role: Option<&str>, at: i64) -> Slot {
        Slot {
            slot_id: name.into(),
            agent: "claude".into(),
            session_id: format!("{name}-0000-id"),
            session_ids: vec![],
            name: Some(name.into()),
            cwd: format!("/p/{name}"),
            pid: None,
            pid_start: None,
            tmux: None,
            status,
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
            last_active_at_ms: at,
            ended_at_ms: None,
        }
    }

    #[test]
    fn sessions_overview_lists_open_sessions_without_moss() {
        let mut busy = slot("api", Status::Running, None, 2_000);
        busy.busy = true;
        let rows = vec![
            (Status::Running, busy),
            (
                Status::Interrupted,
                slot("web", Status::Running, None, 1_000),
            ),
            (Status::Closed, slot("old", Status::Closed, None, 3_000)),
            (
                Status::Running,
                slot("moss", Status::Running, Some("moss"), 4_000),
            ),
        ];
        let md = sessions_md(&rows);
        assert!(md.contains("| web | interrupted | /p/web |"), "{md}");
        assert!(md.contains("| api | working | /p/api |"), "{md}");
        assert!(md.find("| web").unwrap() < md.find("| api").unwrap());
        assert!(!md.contains("| old") && !md.contains("| moss"), "{md}");
        assert!(sessions_md(&[]).contains("No sessions right now."));
    }
}
