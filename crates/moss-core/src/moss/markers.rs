//! Control markers in Moss's replies. `[[silent]]` keeps a reply out of the
//! panel and quiet; `[[buttons: yes | not now]]` turns into buttons that send
//! that text back. Both are taken out of the text people read.

/// A reply with its markers read and taken out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub text: String,
    pub silent: bool,
    pub buttons: Vec<String>,
}

/// At most this many buttons; more would not fit a chat bubble.
const MAX_BUTTONS: usize = 6;

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
