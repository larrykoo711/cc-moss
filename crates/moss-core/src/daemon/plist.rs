//! The daemon's launchd LaunchAgent property list.

use std::path::PathBuf;

/// Everything that differs from one install to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub label: String,
    /// The program first, then its arguments.
    pub program_arguments: Vec<String>,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
    /// Variables launchd sets for the daemon, in this order.
    pub environment: Vec<(String, String)>,
}

/// Seconds launchd waits before starting the daemon again after it exits.
pub const THROTTLE_SECONDS: u32 = 30;

/// The plist text: started at login, kept alive, throttled, low priority.
pub fn render(agent: &Agent) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    let key = |out: &mut String, k: &str| out.push_str(&format!("    <key>{}</key>\n", escape(k)));
    let string = |out: &mut String, indent: &str, v: &str| {
        out.push_str(&format!("{indent}<string>{}</string>\n", escape(v)))
    };
    key(&mut out, "Label");
    string(&mut out, "    ", &agent.label);
    key(&mut out, "ProgramArguments");
    out.push_str("    <array>\n");
    for arg in &agent.program_arguments {
        string(&mut out, "        ", arg);
    }
    out.push_str("    </array>\n");
    if !agent.environment.is_empty() {
        key(&mut out, "EnvironmentVariables");
        out.push_str("    <dict>\n");
        for (k, v) in &agent.environment {
            out.push_str(&format!("        <key>{}</key>\n", escape(k)));
            string(&mut out, "        ", v);
        }
        out.push_str("    </dict>\n");
    }
    key(&mut out, "RunAtLoad");
    out.push_str("    <true/>\n");
    key(&mut out, "KeepAlive");
    out.push_str("    <true/>\n");
    key(&mut out, "ThrottleInterval");
    out.push_str(&format!("    <integer>{THROTTLE_SECONDS}</integer>\n"));
    key(&mut out, "ProcessType");
    string(&mut out, "    ", "Background");
    key(&mut out, "StandardOutPath");
    string(&mut out, "    ", &agent.stdout.to_string_lossy());
    key(&mut out, "StandardErrorPath");
    string(&mut out, "    ", &agent.stderr.to_string_lossy());
    out.push_str("</dict>\n</plist>\n");
    out
}

/// The value a plist written by [`render`] gives the environment variable
/// `name`, or `None` when it sets none (or the text is not one of ours).
pub fn environment_value(text: &str, name: &str) -> Option<String> {
    let key = format!("<key>{}</key>", escape(name));
    let mut lines = text.lines().map(str::trim);
    lines.find(|l| *l == key)?;
    let value = lines
        .next()?
        .strip_prefix("<string>")?
        .strip_suffix("</string>")?;
    Some(unescape(value))
}

/// The reverse of [`escape`].
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// XML text escaping.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(program: &str) -> Agent {
        Agent {
            label: "io.example.daemon".into(),
            program_arguments: vec![program.into(), "daemon".into(), "run".into()],
            stdout: "/s/daemon/stdout.log".into(),
            stderr: "/s/daemon/stderr.log".into(),
            environment: vec![("CC_MOSS_STATE_DIR".into(), "/s".into())],
        }
    }

    #[test]
    fn keys_launchd_needs_are_there() {
        let text = render(&agent("/opt/homebrew/bin/cc-moss"));
        for want in [
            "<key>Label</key>\n    <string>io.example.daemon</string>",
            "<string>/opt/homebrew/bin/cc-moss</string>\n        <string>daemon</string>\n        <string>run</string>",
            "<key>RunAtLoad</key>\n    <true/>",
            "<key>KeepAlive</key>\n    <true/>",
            "<key>ThrottleInterval</key>\n    <integer>30</integer>",
            "<key>ProcessType</key>\n    <string>Background</string>",
            "<key>StandardOutPath</key>\n    <string>/s/daemon/stdout.log</string>",
            "<key>StandardErrorPath</key>\n    <string>/s/daemon/stderr.log</string>",
            "<key>CC_MOSS_STATE_DIR</key>\n        <string>/s</string>",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
    }

    #[test]
    fn special_characters_are_escaped_and_spaces_kept() {
        let text = render(&agent("/Users/a b/R&D <x>/cc-moss"));
        assert!(text.contains("<string>/Users/a b/R&amp;D &lt;x&gt;/cc-moss</string>"));
        assert!(!text.contains("R&D"));
    }

    #[test]
    fn environment_values_read_back() {
        let mut a = agent("/opt/homebrew/bin/cc-moss");
        a.environment = vec![(
            "CC_MOSS_STATE_DIR".into(),
            "/Users/a b/R&D <x>/state".into(),
        )];
        let text = render(&a);
        assert_eq!(
            environment_value(&text, "CC_MOSS_STATE_DIR").as_deref(),
            Some("/Users/a b/R&D <x>/state")
        );
        assert_eq!(environment_value(&text, "OTHER"), None);
        assert_eq!(environment_value("old", "CC_MOSS_STATE_DIR"), None);
    }
}
