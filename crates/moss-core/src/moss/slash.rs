//! Claude Code commands from the Moss page (AMEND-2): a message of yours
//! that is one line starting with `/` is typed into Moss's terminal as a
//! command. The ones that only show something (`/usage`, `/status`, …) come
//! back as a picture of the screen; `/clear` and `/compact` look after
//! Moss's conversation. Anything else needs the terminal itself.

use std::time::Duration;

use super::inbox::{self, Pane, Typed};

/// Commands that show a panel and change nothing.
pub const DISPLAY: &[&str] = &["/usage", "/status", "/context", "/cost", "/help", "/stats"];

/// What a message starting with `/` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slash {
    /// Shows a panel: the screen comes back. The command's first word.
    Display(String),
    Clear,
    Compact,
    /// Not run from the panel. The command's first word.
    Refused(String),
}

impl Slash {
    /// The command's first word, as shown with the reply.
    pub fn name(&self) -> &str {
        match self {
            Slash::Display(n) | Slash::Refused(n) => n,
            Slash::Clear => "/clear",
            Slash::Compact => "/compact",
        }
    }
}

/// `Some` for a single line starting with `/`; several lines are an
/// ordinary message whatever they start with.
pub fn classify(text: &str) -> Option<Slash> {
    let line = text.trim();
    if !line.starts_with('/') || line.contains('\n') {
        return None;
    }
    let first = line.split_whitespace().next().unwrap_or(line).to_string();
    Some(match first.as_str() {
        "/clear" => Slash::Clear,
        "/compact" => Slash::Compact,
        f if DISPLAY.contains(&f) => Slash::Display(first),
        _ => Slash::Refused(first),
    })
}

/// How long the steps of a command may take. Tests shorten nothing: the
/// fake terminal does not sleep.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll: Duration,
    /// The screen counts as settled once unchanged this long.
    pub settled: Duration,
    /// Give up waiting for a panel after this.
    pub show: Duration,
    /// How long the prompt may take to come back after Esc or /clear.
    pub back: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            poll: Duration::from_millis(250),
            settled: Duration::from_secs(1),
            show: Duration::from_secs(10),
            back: Duration::from_secs(3),
        }
    }
}

fn steps(total: Duration, poll: Duration) -> u32 {
    (total.as_millis() / poll.as_millis().max(1)).max(1) as u32
}

/// Type `command` in and press Enter (paste and read back, as for a
/// message).
pub fn run(pane: &mut dyn Pane, command: &str) -> Result<(), String> {
    match inbox::type_in(pane, command) {
        Typed::Sent { .. } => Ok(()),
        Typed::Failed(e) | Typed::Stuck(e) => Err(e),
    }
}

/// Run a display command and bring its screen back, cleaned; then close
/// the panel (Esc, and once more if the prompt does not come back).
pub fn show(pane: &mut dyn Pane, command: &str, t: Timing) -> Result<String, String> {
    let before = pane.screen().unwrap_or_default();
    run(pane, command)?;
    let mut last = String::new();
    let mut still = Duration::ZERO;
    for _ in 0..steps(t.show, t.poll) {
        pane.wait(t.poll);
        let now = pane.screen().unwrap_or_default();
        if now == last && now != before {
            still += t.poll;
            if still >= t.settled {
                break;
            }
        } else {
            still = Duration::ZERO;
        }
        last = now;
    }
    let text = clean(&before, &last, command);
    for _ in 0..2 {
        let _ = pane.key("Escape");
        if wait_ready(pane, t) {
            break;
        }
    }
    Ok(text)
}

/// Wait for Moss's empty input box, up to `t.back`.
pub fn wait_ready(pane: &mut dyn Pane, t: Timing) -> bool {
    for _ in 0..steps(t.back, t.poll) {
        if pane.screen().is_ok_and(|s| inbox::input_empty(&s)) {
            return true;
        }
        pane.wait(t.poll);
    }
    pane.screen().is_ok_and(|s| inbox::input_empty(&s))
}

/// The panel `command` drew, as plain text: what came after the line that
/// echoes the command (else what differs from the screen before), without
/// the input box and footer, the "Esc to cancel" hint and blank edges.
pub fn clean(before: &str, after: &str, command: &str) -> String {
    let lines: Vec<String> = inbox::plain(after)
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect();
    let old: Vec<String> = inbox::plain(before)
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect();
    // Where the panel starts.
    let echo = lines.iter().rposition(|l| {
        let t = l.trim_start_matches(|c: char| c.is_whitespace() || c == '❯' || c == '>');
        t.trim() == command.trim()
    });
    let start = match echo {
        Some(i) => i + 1,
        None => lines
            .iter()
            .zip(old.iter())
            .take_while(|(a, b)| a == b)
            .count(),
    };
    // Where it ends: an input box drawn below it.
    let end = inbox::input_box_top(&lines[start..]).map_or(lines.len(), |top| start + top);
    let kept: Vec<&String> = lines[start..end]
        .iter()
        .filter(|l| !l.trim().eq_ignore_ascii_case("esc to cancel"))
        .collect();
    let first = kept.iter().position(|l| !l.trim().is_empty());
    let last = kept.iter().rposition(|l| !l.trim().is_empty());
    match (first, last) {
        (Some(a), Some(b)) => kept[a..=b]
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moss::inbox::fake::FakePane;

    #[test]
    fn slash_allowlist_is_exact() {
        assert_eq!(classify("/usage"), Some(Slash::Display("/usage".into())));
        assert_eq!(
            classify(" /status "),
            Some(Slash::Display("/status".into()))
        );
        assert_eq!(classify("/clear"), Some(Slash::Clear));
        assert_eq!(
            classify("/compact keep the api notes"),
            Some(Slash::Compact)
        );
        for refused in ["/login", "/Usage", "/model opus", "/mcp", "/usages"] {
            let first = refused.split_whitespace().next().unwrap();
            assert_eq!(
                classify(refused),
                Some(Slash::Refused(first.into())),
                "{refused}"
            );
        }
        // Not commands: plain words, or several lines.
        assert_eq!(classify("what does /usage show?"), None);
        assert_eq!(classify("/usage\nand then this"), None);
        assert_eq!(classify("hello"), None);
    }

    const RULE: &str = "────────────────────────────────────────";

    fn usage_screen() -> String {
        "\
⏺ Earlier answer.

❯ /usage

  Usage
  Skills % of usage
  Subagents         12%
  MCP servers        0%
  Loops              3%

  d to day · w to week
  Usage credits: 40 left

  Esc to cancel
"
        .to_string()
    }

    #[test]
    fn slash_screen_is_cleaned() {
        let before =
            format!("⏺ Earlier answer.\n\n{RULE}\n❯ \n{RULE}\n  ⏵⏵ bypass permissions on\n");
        let text = clean(&before, &usage_screen(), "/usage");
        assert_eq!(
            text,
            "  Usage\n  Skills % of usage\n  Subagents         12%\n  MCP servers        0%\n  Loops              3%\n\n  d to day · w to week\n  Usage credits: 40 left"
        );
        // No echo line: what differs from before; an input box below goes.
        let after = format!(
            "⏺ Earlier answer.\n\n  Settings  Status   Config\n  Version: 2.1.303\n  Esc to cancel\n{RULE}\n❯ \n{RULE}\n  footer\n"
        );
        assert_eq!(
            clean(&before, &after, "/status"),
            "  Settings  Status   Config\n  Version: 2.1.303"
        );
        assert_eq!(clean(&before, "", "/usage"), "");
    }

    #[test]
    fn slash_show_brings_the_panel_back_and_closes_it() {
        let mut pane = FakePane::default();
        pane.panels.push(("/usage".into(), usage_screen()));
        let text = show(&mut pane, "/usage", Timing::default()).unwrap();
        assert!(text.starts_with("  Usage\n"), "{text}");
        assert!(!text.contains("Esc to cancel"));
        assert!(
            pane.calls
                .ends_with(&["key Enter".into(), "key Escape".into()]),
            "{:?}",
            pane.calls
        );
        assert!(pane.showing.is_none(), "the panel is closed");
    }
}
