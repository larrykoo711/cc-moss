use std::io::Write;

use moss_core::transcript::{custom_title_since, last_custom_title};

fn file_with(lines: &[String]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
    f
}

fn title(t: &str) -> String {
    serde_json::json!({"type": "custom-title", "customTitle": t, "sessionId": "x"}).to_string()
}

fn filler(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            serde_json::json!({"type": "user", "message": format!("line {i} {}", "x".repeat(200))})
                .to_string()
        })
        .collect()
}

#[test]
fn finds_the_last_rename() {
    let mut lines = vec![title("first")];
    lines.extend(filler(10));
    lines.push(title("em-final"));
    lines.extend(filler(10));
    let f = file_with(&lines);
    assert_eq!(last_custom_title(f.path()).as_deref(), Some("em-final"));
}

#[test]
fn finds_a_title_far_from_the_end() {
    // Bigger than one read chunk on both sides of the title.
    let mut lines = filler(2_000);
    lines.push(title("deep"));
    lines.extend(filler(2_000));
    let f = file_with(&lines);
    assert_eq!(last_custom_title(f.path()).as_deref(), Some("deep"));
}

#[test]
fn none_without_a_rename_or_a_file() {
    let f = file_with(&filler(5));
    assert_eq!(last_custom_title(f.path()), None);
    assert_eq!(
        last_custom_title(std::path::Path::new("/nonexistent/x.jsonl")),
        None
    );
}

#[test]
fn rescans_only_what_was_appended() {
    let mut f = file_with(&filler(50));
    let (none, scanned) = custom_title_since(f.path(), 0);
    assert_eq!(none, None);
    assert_eq!(scanned, f.as_file().metadata().unwrap().len());
    writeln!(f, "{}", title("renamed")).unwrap();
    let (found, _) = custom_title_since(f.path(), scanned);
    assert_eq!(found.as_deref(), Some("renamed"));
}

#[test]
fn a_title_before_the_scanned_mark_is_not_read_again() {
    let f = file_with(&[title("old")]);
    let size = f.as_file().metadata().unwrap().len();
    assert_eq!(custom_title_since(f.path(), size), (None, size));
}

#[test]
fn a_title_just_before_the_mark_in_the_same_chunk_is_skipped() {
    let mut f = file_with(&[filler(1)[0].clone(), title("old")]);
    let mark = f.as_file().metadata().unwrap().len();
    writeln!(f, "{}", filler(1)[0]).unwrap();
    assert_eq!(custom_title_since(f.path(), mark).0, None);
}
