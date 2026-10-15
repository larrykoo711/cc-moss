//! Blocks of a shell rc file that cc-moss owns, between marker lines.
//!
//! The rest of the file belongs to the user: a block is inserted, replaced or
//! removed as a whole, and removing it restores the file byte for byte.

use anyhow::{bail, Result};

fn markers(tag: &str) -> (String, String) {
    (format!("# >>> {tag} >>>"), format!("# <<< {tag} <<<"))
}

/// Line index range `[start, end]` (inclusive) of the block, if present.
fn find(lines: &[&str], tag: &str) -> Result<Option<(usize, usize)>> {
    let (open, close) = markers(tag);
    let opens: Vec<usize> = (0..lines.len()).filter(|&i| lines[i] == open).collect();
    let closes: Vec<usize> = (0..lines.len()).filter(|&i| lines[i] == close).collect();
    match (opens.as_slice(), closes.as_slice()) {
        ([], []) => Ok(None),
        ([o], [c]) if o < c => Ok(Some((*o, *c))),
        _ => bail!("the `{tag}` block markers are duplicated or unbalanced; fix the file by hand"),
    }
}

/// Insert or replace the block. A new block goes right before the line that
/// runs `starship init` (anything after starship that touches the prompt
/// would override it), or before the managed block containing that line,
/// or at the end of the file.
pub fn upsert(text: &str, tag: &str, body: &str) -> Result<String> {
    let (open, close) = markers(tag);
    let body = if body.ends_with('\n') || body.is_empty() {
        body.to_string()
    } else {
        format!("{body}\n")
    };
    let block = format!("{open}\n{body}{close}\n");
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let bare: Vec<&str> = lines.iter().map(|l| l.trim_end_matches('\n')).collect();
    let (before, after) = match find(&bare, tag)? {
        Some((start, end)) => (lines[..start].concat(), lines[end + 1..].concat()),
        None => match bare.iter().position(|l| l.contains("starship init")) {
            Some(i) => {
                let at = enclosing_block_start(&bare, i).unwrap_or(i);
                (lines[..at].concat(), lines[at..].concat())
            }
            None => {
                // Keep the file's last line intact if it lacks a newline.
                let mut head = text.to_string();
                if !head.is_empty() && !head.ends_with('\n') {
                    head.push('\n');
                }
                (head, String::new())
            }
        },
    };
    Ok(format!("{before}{block}{after}"))
}

/// Remove the block, leaving everything else exactly as it was.
pub fn remove(text: &str, tag: &str) -> Result<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let bare: Vec<&str> = lines.iter().map(|l| l.trim_end_matches('\n')).collect();
    Ok(match find(&bare, tag)? {
        Some((start, end)) => format!("{}{}", lines[..start].concat(), lines[end + 1..].concat()),
        None => text.to_string(),
    })
}

/// If line `i` sits inside another tool's managed block (`# >>> … >>>` …
/// `# <<< … <<<`), the index of that block's opening line.
fn enclosing_block_start(lines: &[&str], i: usize) -> Option<usize> {
    let mut depth = 0usize;
    for j in (0..i).rev() {
        let l = lines[j].trim();
        if l.starts_with("# <<< ") && l.ends_with(" <<<") {
            depth += 1;
        } else if l.starts_with("# >>> ") && l.ends_with(" >>>") {
            if depth == 0 {
                return Some(j);
            }
            depth -= 1;
        }
    }
    None
}
