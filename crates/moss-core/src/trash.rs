//! Moving a file to the Trash instead of deleting it, so a mistake can be
//! undone from Finder (macOS, `~/.Trash`) or the desktop's trash (Linux,
//! the freedesktop.org layout under `~/.local/share/Trash`).

use std::io;
use std::path::{Path, PathBuf};

/// Where things are put.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trash {
    /// A plain folder (macOS's `~/.Trash`).
    Folder(PathBuf),
    /// freedesktop.org: `files/` and `info/<name>.trashinfo`.
    Xdg(PathBuf),
}

impl Trash {
    /// This system's trash under `home`.
    pub fn for_home(home: &Path) -> Trash {
        if cfg!(target_os = "macos") {
            Trash::Folder(home.join(".Trash"))
        } else {
            Trash::Xdg(home.join(".local/share/Trash"))
        }
    }

    /// The trash of the user running this (`$HOME`).
    pub fn current() -> Option<Trash> {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
        let home = PathBuf::from(home);
        home.is_absolute().then(|| Trash::for_home(&home))
    }

    /// Move `path` (a regular file) into the trash under a name no file
    /// there has yet; where it went. The file is renamed, never copied or
    /// deleted: a trash on another disk is an error and the file stays.
    pub fn put(&self, path: &Path) -> io::Result<PathBuf> {
        let meta = std::fs::symlink_metadata(path)?;
        if !meta.file_type().is_file() {
            return Err(io::Error::other("not a regular file"));
        }
        let files = match self {
            Trash::Folder(dir) => dir.clone(),
            Trash::Xdg(dir) => dir.join("files"),
        };
        create_private(&files)?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("no file name"))?
            .to_string_lossy()
            .into_owned();
        for n in 1..10_000 {
            let candidate = if n == 1 {
                name.clone()
            } else {
                numbered(&name, n)
            };
            let target = files.join(&candidate);
            if std::fs::symlink_metadata(&target).is_ok() {
                continue;
            }
            if let Trash::Xdg(dir) = self {
                let info_dir = dir.join("info");
                create_private(&info_dir)?;
                let info = info_dir.join(format!("{candidate}.trashinfo"));
                // Claim the name first, as the spec says: the info file is
                // created only when no other has it.
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&info)
                {
                    Ok(mut f) => {
                        use std::io::Write;
                        f.write_all(trash_info(path).as_bytes())?;
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                }
                if let Err(e) = std::fs::rename(path, &target) {
                    let _ = std::fs::remove_file(&info);
                    return Err(e);
                }
                return Ok(target);
            }
            std::fs::rename(path, &target)?;
            return Ok(target);
        }
        Err(io::Error::other(
            "the trash has too many files of that name",
        ))
    }
}

fn create_private(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// `name 2.jsonl` for the second `name.jsonl`, as Finder names them.
fn numbered(name: &str, n: usize) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} {n}.{ext}"),
        _ => format!("{name} {n}"),
    }
}

/// The `.trashinfo` for `path`: its original place, percent-encoded, and
/// when it was trashed (local time, as the spec asks).
fn trash_info(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut encoded = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!(
        "[Trash Info]\nPath={encoded}\nDeletionDate={}\n",
        local_now()
    )
}

fn local_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as libc::time_t;
    // SAFETY: localtime_r with a valid out-pointer.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_keeps_both_files_of_one_name() {
        let dir = tempfile::tempdir().unwrap();
        for trash in [
            Trash::Folder(dir.path().join("mac")),
            Trash::Xdg(dir.path().join("xdg")),
        ] {
            let mut put = Vec::new();
            for content in ["one", "two"] {
                let src = dir.path().join("s.jsonl");
                std::fs::write(&src, content).unwrap();
                put.push(trash.put(&src).unwrap());
                assert!(!src.exists());
            }
            assert_eq!(std::fs::read_to_string(&put[0]).unwrap(), "one");
            assert_eq!(std::fs::read_to_string(&put[1]).unwrap(), "two");
            assert!(put[1].ends_with("s 2.jsonl"), "{put:?}");
            if let Trash::Xdg(root) = &trash {
                let info = std::fs::read_to_string(root.join("info/s.jsonl.trashinfo")).unwrap();
                assert!(info.starts_with("[Trash Info]\nPath=/"), "{info}");
                assert!(info.contains("DeletionDate="));
                assert!(root.join("info/s 2.jsonl.trashinfo").exists());
            }
        }
    }

    #[test]
    fn trash_takes_only_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let trash = Trash::Folder(dir.path().join("t"));
        let target = dir.path().join("real.jsonl");
        std::fs::write(&target, "x").unwrap();
        let link = dir.path().join("link.jsonl");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(trash.put(&link).is_err());
        assert!(trash.put(dir.path()).is_err());
        assert!(trash.put(&dir.path().join("missing.jsonl")).is_err());
        assert!(target.exists());
    }
}
