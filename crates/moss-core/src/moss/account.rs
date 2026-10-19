//! Which kind of Claude Code account Moss runs on: a subscription (claude.ai
//! login, with the `/usage` panel), an API key or third-party gateway, a
//! cloud provider, or unknown. Only whether things are there is looked at,
//! never a secret's value.

use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Account {
    Subscription,
    /// An API key, auth token, key helper or custom base URL (a gateway).
    Api,
    /// Bedrock, Vertex or Foundry.
    Cloud,
    Unknown,
}

impl Account {
    pub fn as_str(self) -> &'static str {
        match self {
            Account::Subscription => "subscription",
            Account::Api => "api",
            Account::Cloud => "cloud",
            Account::Unknown => "unknown",
        }
    }
}

const CLOUD: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

const API: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
];

/// What the account is, from what is set. `env` says whether a variable is
/// set (and, for the cloud switches, whether it is on); `settings` is
/// Claude Code's user settings, `login` its `.claude.json`. An API key or
/// cloud switch wins over a claude.ai login, as in Claude Code itself.
pub fn detect(
    env: &dyn Fn(&str) -> Option<bool>,
    settings: Option<&Value>,
    login: Option<&Value>,
) -> Account {
    let settings_env = settings
        .and_then(|s| s.get("env"))
        .and_then(Value::as_object);
    let in_settings = |name: &str| settings_env.is_some_and(|e| e.contains_key(name));
    let switch_in_settings = |name: &str| {
        settings_env.and_then(|e| e.get(name)).is_some_and(|v| {
            on(&v
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| v.to_string()))
        })
    };
    if CLOUD
        .iter()
        .any(|n| env(n) == Some(true) || switch_in_settings(n))
    {
        return Account::Cloud;
    }
    let helper = settings.is_some_and(|s| s.get("apiKeyHelper").is_some_and(|h| !h.is_null()));
    if helper || API.iter().any(|n| env(n).is_some() || in_settings(n)) {
        return Account::Api;
    }
    if login.is_some_and(|l| l.get("oauthAccount").is_some_and(|a| !a.is_null())) {
        return Account::Subscription;
    }
    Account::Unknown
}

/// A switch's value read as on: anything but empty, `0`, `false`, `no`.
fn on(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

/// This process's environment, for [`detect`]: whether a variable is set
/// (not empty), and for the cloud switches whether it is on. Secrets are
/// only checked for being there.
pub fn process_env(name: &str) -> Option<bool> {
    let value = std::env::var_os(name).filter(|v| !v.is_empty())?;
    if CLOUD.contains(&name) {
        return Some(on(&value.to_string_lossy()));
    }
    Some(true)
}

/// Claude Code's user settings and `.claude.json`: under
/// `CLAUDE_CONFIG_DIR` when that is set, else `~/.claude/settings.json` and
/// `~/.claude.json`.
pub fn files(home: Option<&Path>, config_dir: Option<&Path>) -> (Option<PathBuf>, Option<PathBuf>) {
    match (config_dir, home) {
        (Some(dir), _) => (
            Some(dir.join("settings.json")),
            Some(dir.join(".claude.json")),
        ),
        (None, Some(home)) => (
            Some(home.join(".claude/settings.json")),
            Some(home.join(".claude.json")),
        ),
        (None, None) => (None, None),
    }
}

fn read_json(path: Option<&Path>) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path?).ok()?).ok()
}

/// The account of a Claude Code started with this process's environment.
pub fn current() -> Account {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let (settings, login) = files(home.as_deref(), config.as_deref());
    detect(
        &process_env,
        read_json(settings.as_deref()).as_ref(),
        read_json(login.as_deref()).as_ref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(set: &'static [(&'static str, bool)]) -> impl Fn(&str) -> Option<bool> {
        move |n| set.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
    }

    const LOGIN: fn() -> Value = || json!({ "oauthAccount": { "emailAddress": "x" } });

    #[test]
    fn account_subscription_from_the_login() {
        assert_eq!(
            detect(&env(&[]), None, Some(&LOGIN())),
            Account::Subscription
        );
        assert_eq!(
            detect(&env(&[]), None, Some(&json!({ "oauthAccount": null }))),
            Account::Unknown
        );
        assert_eq!(detect(&env(&[]), None, None), Account::Unknown);
    }

    #[test]
    fn account_api_from_env_settings_or_helper() {
        for name in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
        ] {
            let set: &'static [(&'static str, bool)] = match name {
                "ANTHROPIC_API_KEY" => &[("ANTHROPIC_API_KEY", true)],
                "ANTHROPIC_AUTH_TOKEN" => &[("ANTHROPIC_AUTH_TOKEN", true)],
                _ => &[("ANTHROPIC_BASE_URL", true)],
            };
            // A key in the environment beats a claude.ai login.
            assert_eq!(
                detect(&env(set), None, Some(&LOGIN())),
                Account::Api,
                "{name}"
            );
            let settings = json!({ "env": { name: "anything" } });
            assert_eq!(
                detect(&env(&[]), Some(&settings), Some(&LOGIN())),
                Account::Api,
                "{name}"
            );
        }
        let helper = json!({ "apiKeyHelper": "~/bin/key.sh" });
        assert_eq!(
            detect(&env(&[]), Some(&helper), Some(&LOGIN())),
            Account::Api
        );
    }

    #[test]
    fn account_cloud_wins_and_needs_the_switch_on() {
        let bedrock: &'static [(&'static str, bool)] = &[
            ("CLAUDE_CODE_USE_BEDROCK", true),
            ("ANTHROPIC_API_KEY", true),
        ];
        assert_eq!(detect(&env(bedrock), None, Some(&LOGIN())), Account::Cloud);
        let off: &'static [(&'static str, bool)] = &[("CLAUDE_CODE_USE_VERTEX", false)];
        assert_eq!(
            detect(&env(off), None, Some(&LOGIN())),
            Account::Subscription
        );
        let settings = json!({ "env": { "CLAUDE_CODE_USE_FOUNDRY": "1" } });
        assert_eq!(detect(&env(&[]), Some(&settings), None), Account::Cloud);
        let settings = json!({ "env": { "CLAUDE_CODE_USE_FOUNDRY": "0" } });
        assert_eq!(detect(&env(&[]), Some(&settings), None), Account::Unknown);
    }

    #[test]
    fn account_files_follow_the_config_dir() {
        assert_eq!(
            files(Some(Path::new("/u")), None),
            (
                Some(PathBuf::from("/u/.claude/settings.json")),
                Some(PathBuf::from("/u/.claude.json"))
            )
        );
        assert_eq!(
            files(Some(Path::new("/u")), Some(Path::new("/c"))),
            (
                Some(PathBuf::from("/c/settings.json")),
                Some(PathBuf::from("/c/.claude.json"))
            )
        );
    }
}
