//! One daemon at a time: an exclusive `flock` on `<state>/daemon/lock`.
//!
//! The kernel drops the lock when the holder dies, `kill -9` included, so a
//! crash never leaves a stale lock behind. Files are opened close-on-exec,
//! so processes the daemon starts later do not inherit it.

use std::fs::{File, TryLockError};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Held for as long as this value lives.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

/// Take the lock, or `None` when another process holds it. Never waits.
pub fn try_acquire(path: &Path) -> io::Result<Option<Lock>> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Lock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_holder_is_refused_until_the_first_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let first = try_acquire(&path).unwrap();
        assert!(first.is_some());
        assert!(try_acquire(&path).unwrap().is_none());
        drop(first);
        assert!(try_acquire(&path).unwrap().is_some());
    }
}
