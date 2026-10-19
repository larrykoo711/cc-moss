//! A folder in Moss's home that the daemon reads and writes (`actions/`,
//! `inbox/`, `stop/`), held open by its file descriptor.
//!
//! Moss's shell can rename folders in its home and make links that lead out
//! of it. A path checked first and used later could then lead anywhere by
//! the time it is used. So the folder is opened once, never through a link,
//! and everything in it is done relative to that descriptor (`openat`,
//! `unlinkat`, `renameat`, `linkat`), never following a link inside it.
//! Swapping the folder afterwards changes nothing: the descriptor still
//! names the folder that was opened.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// An open folder.
#[derive(Debug)]
pub struct Dir {
    fd: OwnedFd,
}

/// `-1` as an `io::Error`.
fn check(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

fn c_name(name: &str) -> io::Result<CString> {
    // One plain name in the folder: no path, no way up.
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(io::Error::other(format!(
            "{name:?} is not a plain file name"
        )));
    }
    CString::new(name).map_err(|_| io::Error::other("a file name with a NUL"))
}

const DIR_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

fn open_dir_at(at: RawFd, name: &CStr) -> io::Result<OwnedFd> {
    let fd = check(unsafe { libc::openat(at, name.as_ptr(), DIR_FLAGS) })?;
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

impl Dir {
    /// `home` itself, never through a link at its last part. Its own
    /// parents are cc-moss's state folder, out of Moss's reach.
    pub fn home(home: &Path) -> io::Result<Dir> {
        let path = CString::new(home.as_os_str().as_bytes())
            .map_err(|_| io::Error::other("a path with a NUL"))?;
        let fd = check(unsafe { libc::open(path.as_ptr(), DIR_FLAGS) })?;
        Ok(Dir {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    /// `<home>/<sub>`, a real folder: a link or file there is removed (the
    /// link, not what it leads to) and a folder made in its place.
    pub fn open(home: &Path, sub: &str) -> io::Result<Dir> {
        Dir::home(home)?.sub(sub)
    }

    /// The folder `name` in this one, made if missing; a link or file there
    /// is replaced by a folder.
    pub fn sub(&self, name: &str) -> io::Result<Dir> {
        let c = c_name(name)?;
        for _ in 0..3 {
            match open_dir_at(self.raw(), &c) {
                Ok(fd) => return Ok(Dir { fd }),
                Err(e) => match e.raw_os_error() {
                    Some(libc::ENOENT) => {}
                    // A link (O_NOFOLLOW) or something not a folder.
                    Some(libc::ELOOP) | Some(libc::ENOTDIR) => {
                        let _ = check(unsafe { libc::unlinkat(self.raw(), c.as_ptr(), 0) });
                    }
                    _ => return Err(e),
                },
            }
            match check(unsafe { libc::mkdirat(self.raw(), c.as_ptr(), 0o700) }) {
                Ok(_) => {}
                Err(e) if e.raw_os_error() == Some(libc::EEXIST) => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other(format!(
            "{name} keeps changing; not using it"
        )))
    }

    /// The folder `name` in this one as it is: never made, never through a
    /// link.
    pub fn child(&self, name: &str) -> io::Result<Dir> {
        Ok(Dir {
            fd: open_dir_at(self.raw(), &c_name(name)?)?,
        })
    }

    /// The plain file at `rel` (plain names only, no `..`), each folder on
    /// the way opened from the one before and never through a link.
    pub fn read_at(&self, rel: &Path) -> io::Result<String> {
        let mut names = Vec::new();
        for part in rel.components() {
            match part {
                std::path::Component::Normal(n) => names.push(
                    n.to_str()
                        .ok_or_else(|| io::Error::other("a name that is not UTF-8"))?,
                ),
                _ => {
                    return Err(io::Error::other(format!(
                        "{} is not a plain path",
                        rel.display()
                    )))
                }
            }
        }
        let Some((file, folders)) = names.split_last() else {
            return Err(io::Error::other("no file named"));
        };
        let mut at: Option<Dir> = None;
        for folder in folders {
            let next = at.as_ref().unwrap_or(self).child(folder)?;
            at = Some(next);
        }
        at.as_ref().unwrap_or(self).read(file)
    }

    fn raw(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    fn stat(&self, name: &CStr) -> io::Result<libc::stat> {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        check(unsafe {
            libc::fstatat(
                self.raw(),
                name.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        })?;
        Ok(st)
    }

    /// Names of what is in the folder that is not a folder (plain files and
    /// links), sorted.
    pub fn entries(&self) -> io::Result<Vec<String>> {
        let dup = check(unsafe { libc::dup(self.raw()) })?;
        let stream = unsafe { libc::fdopendir(dup) };
        if stream.is_null() {
            let e = io::Error::last_os_error();
            unsafe { libc::close(dup) };
            return Err(e);
        }
        // Read from the start whatever an earlier listing left.
        unsafe { libc::rewinddir(stream) };
        let mut names = Vec::new();
        loop {
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            let Ok(text) = name.to_str() else {
                continue;
            };
            if text == "." || text == ".." {
                continue;
            }
            match self.stat(name) {
                Ok(st) if st.st_mode & libc::S_IFMT == libc::S_IFDIR => {}
                Ok(_) => names.push(text.to_string()),
                Err(_) => {}
            }
        }
        unsafe { libc::closedir(stream) };
        names.sort();
        Ok(names)
    }

    /// Plain files (never links) ending in `.<ext>`, sorted.
    pub fn files(&self, ext: &str) -> Vec<String> {
        let suffix = format!(".{ext}");
        self.entries()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.ends_with(&suffix))
            .filter(|n| {
                c_name(n)
                    .and_then(|c| self.stat(&c))
                    .is_ok_and(|st| st.st_mode & libc::S_IFMT == libc::S_IFREG)
            })
            .collect()
    }

    /// The plain file `name`, read whole; a link is not followed.
    pub fn read(&self, name: &str) -> io::Result<String> {
        let c = c_name(name)?;
        let fd = check(unsafe {
            libc::openat(
                self.raw(),
                c.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        })?;
        let mut file = unsafe { File::from_raw_fd(fd) };
        if !file.metadata()?.file_type().is_file() {
            return Err(io::Error::other(format!("{name} is not a plain file")));
        }
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    /// When `name` (not followed) was last changed.
    pub fn modified(&self, name: &str) -> io::Result<SystemTime> {
        let st = self.stat(&c_name(name)?)?;
        let secs = u64::try_from(st.st_mtime).unwrap_or(0);
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }

    /// Remove `name` (a link itself, never what it leads to). Gone already
    /// is fine.
    pub fn remove(&self, name: &str) -> io::Result<()> {
        let c = c_name(name)?;
        match check(unsafe { libc::unlinkat(self.raw(), c.as_ptr(), 0) }) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// A fresh `<name>.tmp` holding `text`.
    fn fresh_tmp(&self, name: &str, text: &str) -> io::Result<CString> {
        let tmp = c_name(&format!("{name}.tmp"))?;
        let _ = unsafe { libc::unlinkat(self.raw(), tmp.as_ptr(), 0) };
        let fd = check(unsafe {
            libc::openat(
                self.raw(),
                tmp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        })?;
        let mut file = unsafe { File::from_raw_fd(fd) };
        if let Err(e) = file.write_all(text.as_bytes()) {
            let _ = unsafe { libc::unlinkat(self.raw(), tmp.as_ptr(), 0) };
            return Err(e);
        }
        Ok(tmp)
    }

    /// Write `name` whole: readers see the old or the new. A link at
    /// `name` is replaced, never written through.
    pub fn write(&self, name: &str, text: &str) -> io::Result<()> {
        let c = c_name(name)?;
        let tmp = self.fresh_tmp(name, text)?;
        let renamed =
            check(unsafe { libc::renameat(self.raw(), tmp.as_ptr(), self.raw(), c.as_ptr()) });
        if renamed.is_err() {
            let _ = unsafe { libc::unlinkat(self.raw(), tmp.as_ptr(), 0) };
        }
        renamed.map(drop)
    }

    /// Write the new file `name` whole, never replacing one already there.
    pub fn put_new(&self, name: &str, text: &str) -> io::Result<()> {
        let c = c_name(name)?;
        let tmp = self.fresh_tmp(name, text)?;
        let linked =
            check(unsafe { libc::linkat(self.raw(), tmp.as_ptr(), self.raw(), c.as_ptr(), 0) });
        let _ = unsafe { libc::unlinkat(self.raw(), tmp.as_ptr(), 0) };
        linked.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn moss_dir_stays_put_when_swapped_for_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("moss");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.json"), "secret").unwrap();
        std::fs::write(outside.join("keep.result.json"), "keep").unwrap();
        let dir = Dir::open(&home, "actions").unwrap();
        dir.write("a.json", "mine").unwrap();
        // The folder is renamed and a link out put in its place.
        std::fs::rename(home.join("actions"), home.join("old")).unwrap();
        symlink(&outside, home.join("actions")).unwrap();
        assert_eq!(dir.files("json"), ["a.json"]);
        assert_eq!(dir.read("a.json").unwrap(), "mine");
        assert!(dir.read("secret.json").is_err());
        dir.remove("keep.result.json").unwrap();
        dir.write("x.result.json", "answer").unwrap();
        assert_eq!(
            std::fs::read_to_string(outside.join("keep.result.json")).unwrap(),
            "keep"
        );
        assert!(!outside.join("x.result.json").exists());
        assert!(home.join("old/x.result.json").exists());
        // Opened again, the link is replaced by a folder; outside is untouched.
        let again = Dir::open(&home, "actions").unwrap();
        assert!(again.files("json").is_empty());
        assert!(std::fs::symlink_metadata(home.join("actions"))
            .unwrap()
            .is_dir());
        assert!(outside.join("secret.json").exists());
    }

    #[test]
    fn moss_dir_reads_a_path_without_links_on_the_way() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("moss");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(home.join("drafts/deep")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("soul.md"), "secret").unwrap();
        std::fs::write(home.join("drafts/deep/soul.md"), "mine").unwrap();
        let dir = Dir::home(&home).unwrap();
        assert_eq!(
            dir.read_at(Path::new("drafts/deep/soul.md")).unwrap(),
            "mine"
        );
        // A folder on the way that is a link, and a file that is one.
        symlink(&outside, home.join("drafts/out")).unwrap();
        assert!(dir.read_at(Path::new("drafts/out/soul.md")).is_err());
        symlink(outside.join("soul.md"), home.join("drafts/l.md")).unwrap();
        assert!(dir.read_at(Path::new("drafts/l.md")).is_err());
        assert!(dir
            .read_at(Path::new("drafts/../../outside/soul.md"))
            .is_err());
        assert!(dir.read_at(Path::new("/etc/hosts")).is_err());
    }

    #[test]
    fn moss_dir_never_follows_links_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("moss");
        std::fs::create_dir_all(&home).unwrap();
        let target = tmp.path().join("target.json");
        std::fs::write(&target, "outside").unwrap();
        let dir = Dir::open(&home, "inbox").unwrap();
        symlink(&target, home.join("inbox/l.json")).unwrap();
        assert!(dir.files("json").is_empty(), "a link is not a file");
        assert_eq!(dir.entries().unwrap(), ["l.json"]);
        assert!(dir.read("l.json").is_err());
        dir.write("l.json", "new").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside");
        assert!(dir.put_new("l.json", "again").is_err(), "never replaces");
        dir.remove("l.json").unwrap();
        assert!(target.exists());
        assert!(dir.read("../target.json").is_err());
        assert!(dir.remove("../target.json").is_err());
    }
}
