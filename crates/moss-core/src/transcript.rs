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
    (scan_back(&mut file, from, size, custom_title), size)
}

/// Whether Claude Code is in the middle of a turn, judged by the last record
/// that says so: `Some(false)` once the turn ended or the user interrupted
/// it, `Some(true)` while it waits on the model or a tool, `None` when the
/// transcript is missing or says nothing yet.
///
/// Unlike `claude agents`, background shells and agents still running after
/// the turn do not count.
pub fn turn_open(path: &Path) -> Option<bool> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    scan_back(&mut file, 0, size, turn_state)
}

fn turn_state(line: &[u8]) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    let message = v.get("message");
    match v.get("type")?.as_str()? {
        "system" => match v.get("subtype")?.as_str()? {
            "turn_duration" | "stop_hook_summary" => Some(false),
            _ => None,
        },
        "assistant" => {
            let stop = message?.get("stop_reason")?.as_str()?;
            Some(stop != "end_turn")
        }
        // Meta records and local slash commands (`/rename`) do not start a turn.
        "user" if v.get("isMeta").and_then(|m| m.as_bool()) == Some(true) => None,
        "user" => {
            let text = first_text(message?.get("content")?);
            match text {
                Some(t) if t.starts_with("[Request interrupted by user") => Some(false),
                Some(t) if LOCAL_COMMAND.iter().any(|tag| t.starts_with(tag)) => None,
                _ => Some(true),
            }
        }
        "api-request" => Some(true),
        _ => None,
    }
}

const LOCAL_COMMAND: [&str; 3] = ["<command-name>", "<command-message>", "<local-command-"];

fn first_text(content: &serde_json::Value) -> Option<&str> {
    match content {
        serde_json::Value::String(s) => Some(s.as_str()),
        serde_json::Value::Array(parts) => parts
            .iter()
            .find_map(|p| p.get("text").and_then(|t| t.as_str())),
        _ => None,
    }
}

/// Feed lines to `pick` from the last one back to offset `from`, stopping at
/// the first answer.
fn scan_back<T>(
    file: &mut File,
    from: u64,
    size: u64,
    mut pick: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
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
            if let Some(answer) = pick(line) {
                return Some(answer);
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
