//! Reading Claude Code transcripts (JSON lines), without loading them whole:
//! they grow to tens of megabytes.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK: u64 = 64 * 1024;

/// The session name from the last `/rename` (a `custom-title` record),
/// searching backwards from the end of the file.
///
/// Claude Code does not always put the name in hook payloads (e.g. after
/// `--resume`), but it always records renames in the transcript.
pub fn last_custom_title(path: &Path) -> Option<String> {
    custom_title_since(path, 0).0
}

/// Like [`last_custom_title`], but only reads bytes from offset `from` on
/// (a size returned by an earlier call). Returns the title, if any, and the
/// size scanned up to, so a caller can resume where it stopped.
pub fn custom_title_since(path: &Path, from: u64) -> (Option<String>, u64) {
    let Ok(mut file) = File::open(path) else {
        return (None, from);
    };
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return (None, from);
    };
    // A file that shrank was replaced: read it all again.
    let from = if from > size { 0 } else { from };
    (scan_back(&mut file, from, size), size)
}

fn scan_back(file: &mut File, from: u64, size: u64) -> Option<String> {
    let mut pos = size;
    // Bytes of a line cut at the start of the previous (later) chunk.
    let mut carry: Vec<u8> = Vec::new();
    while pos > from {
        let start = pos.saturating_sub(CHUNK).max(from);
        let mut buf = vec![0; (pos - start) as usize];
        file.seek(SeekFrom::Start(start)).ok()?;
        file.read_exact(&mut buf).ok()?;
        buf.extend_from_slice(&carry);
        // The first piece may be cut: keep it for the next round, unless
        // this is where the scan starts (a line boundary).
        let (head, whole) = match (start == from, buf.iter().position(|b| *b == b'\n')) {
            (true, _) => (Vec::new(), &buf[..]),
            (false, Some(nl)) => (buf[..nl].to_vec(), &buf[nl + 1..]),
            (false, None) => (buf.clone(), &buf[buf.len()..]),
        };
        for line in whole.split(|b| *b == b'\n').rev() {
            if let Some(title) = custom_title(line) {
                return Some(title);
            }
        }
        carry = head;
        pos = start;
    }
    None
}

fn custom_title(line: &[u8]) -> Option<String> {
    let needle = b"custom-title";
    if !line.windows(needle.len()).any(|w| w == needle) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    if v.get("type")?.as_str()? != "custom-title" {
        return None;
    }
    v.get("customTitle")?
        .as_str()
        .filter(|t| !t.is_empty())
        .map(String::from)
}
