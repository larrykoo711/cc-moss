//! Which API provider Claude Code talks to (T2.6), from its user settings,
//! and whether a running session took a switch in or needs a restart.
//!
//! Claude Code 2.1.303 reloads `settings.json` while it runs: a changed
//! base URL, token or model alias applies to the next message. Two things
//! don't: a key removed from `env` (its old value stays in the process) and
//! a different fixed model (`ANTHROPIC_MODEL`, top-level `model`).
//!
//! Only key names, the base URL's host and the two model names are read.
//! A token's or key's value is skipped by the parser without ever becoming
//! a string here, and nothing of it is kept, logged or printed.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// The `env` keys that decide the provider.
const ENV_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_MODEL",
];
const BASE_URL: &str = "ANTHROPIC_BASE_URL";
const MODEL: &str = "ANTHROPIC_MODEL";
/// Top-level keys, recorded as `model` and `apiKeyHelper`.
const TOP_MODEL: &str = "model";
const TOP_HELPER: &str = "apiKeyHelper";

/// The provider as the settings say: which keys are set, the base URL's
/// host, and the fixed models. Never a token's or key's value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Key names present: `env.ANTHROPIC_*` as `ANTHROPIC_*`, and `model`,
    /// `apiKeyHelper`.
    pub keys: BTreeSet<String>,
    /// The host of `ANTHROPIC_BASE_URL`, without user, port or path.
    pub host: Option<String>,
    /// `env.ANTHROPIC_MODEL`.
    pub env_model: Option<String>,
    /// Top-level `model`.
    pub model: Option<String>,
}

/// Whether a key name is one that decides the provider.
fn provider_key(name: &str) -> bool {
    ENV_KEYS.contains(&name) || (name.starts_with("ANTHROPIC_DEFAULT_") && name.ends_with("_MODEL"))
}

/// The `env` table: names of provider keys, and only the base URL and the
/// model as text. Every other value, tokens and keys included, is skipped.
#[derive(Default)]
struct Env {
    keys: BTreeSet<String>,
    base_url: Option<String>,
    model: Option<String>,
}

impl<'de> Deserialize<'de> for Env {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Env;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an env table")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Env, A::Error> {
                let mut env = Env::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        BASE_URL => env.base_url = map.next_value::<Option<String>>()?,
                        MODEL => env.model = map.next_value::<Option<String>>()?,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                    if provider_key(&key) {
                        env.keys.insert(key);
                    }
                }
                Ok(env)
            }
        }
        d.deserialize_map(V)
    }
}

/// The parts of `settings.json` read here.
#[derive(Deserialize, Default)]
struct Settings {
    #[serde(default)]
    env: Option<Env>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "apiKeyHelper")]
    api_key_helper: Option<IgnoredAny>,
}

impl Snapshot {
    /// From the text of a `settings.json`; `None` when it is not JSON with
    /// these keys in their usual shape.
    pub fn parse(text: &str) -> Option<Snapshot> {
        let s: Settings = serde_json::from_str(text).ok()?;
        let env = s.env.unwrap_or_default();
        let mut keys = env.keys;
        if s.model.is_some() {
            keys.insert(TOP_MODEL.into());
        }
        if s.api_key_helper.is_some() {
            keys.insert(TOP_HELPER.into());
        }
        Some(Snapshot {
            keys,
            host: env.base_url.as_deref().and_then(host_of),
            env_model: env.model.filter(|m| !m.is_empty()),
            model: s.model.filter(|m| !m.is_empty()),
        })
    }

    /// The settings file at `path`; an empty snapshot when there is none,
    /// `None` when it can't be read or parsed.
    pub fn read(path: &Path) -> Option<Snapshot> {
        match std::fs::read_to_string(path) {
            Ok(text) => Snapshot::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Snapshot::default()),
            Err(_) => None,
        }
    }

    /// Whether the settings pick a fixed model, so a replayed `--model`
    /// would override the provider's.
    pub fn picks_model(&self) -> bool {
        self.env_model.is_some() || self.model.is_some()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Option<Snapshot> {
        serde_json::from_str(text).ok()
    }
}

/// The host of a URL: no scheme, user, password, port or path.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Claude Code's user settings: `<config dir>/settings.json`, the config
/// dir being `configured` (cc-moss's `[claude] config_dir`), else
/// `CLAUDE_CONFIG_DIR` (`env`), else `~/.claude`.
pub fn settings_path(
    configured: Option<&Path>,
    env: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    configured
        .filter(|p| p.is_absolute())
        .or(env.filter(|p| p.is_absolute()))
        .map(Path::to_path_buf)
        .or_else(|| home.filter(|h| h.is_absolute()).map(|h| h.join(".claude")))
        .map(|dir| dir.join("settings.json"))
}

/// Why a running session has to be restarted for the provider it is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// A key it saw is gone (back to the subscription, typically): its old
    /// value stays in the process.
    Removed,
    /// The fixed model changed; Claude Code reads it only at start.
    Model,
}

impl Reason {
    pub fn code(self) -> &'static str {
        match self {
            Reason::Removed => "removed",
            Reason::Model => "model",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The next message uses the provider the settings name.
    Live,
    NeedsRestart(Reason),
}

/// Whether a session that started with `started`, and has seen the keys
/// `seen` while it ran, uses the provider the settings now name (`now`).
pub fn verdict(started: &Snapshot, seen: &BTreeSet<String>, now: &Snapshot) -> Verdict {
    if seen
        .iter()
        .chain(&started.keys)
        .any(|k| !now.keys.contains(k))
    {
        return Verdict::NeedsRestart(Reason::Removed);
    }
    if now.env_model != started.env_model || now.model != started.model {
        return Verdict::NeedsRestart(Reason::Model);
    }
    Verdict::Live
}

/// `seen` with the keys of `now` added.
pub fn seen_with(seen: &BTreeSet<String>, now: &Snapshot) -> BTreeSet<String> {
    seen.union(&now.keys).cloned().collect()
}

/// A session's recorded provider: what it started with and the keys it
/// has seen, from the registry's JSON columns.
pub fn recorded(started: Option<&str>, seen: Option<&str>) -> Option<(Snapshot, BTreeSet<String>)> {
    let started = Snapshot::from_json(started?)?;
    let seen: BTreeSet<String> = seen
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| started.keys.clone());
    Some((started, seen))
}

/// Keys by name, for the JSON column.
pub fn keys_json(keys: &BTreeSet<String>) -> String {
    serde_json::to_string(keys).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "sk-ant-THIS-IS-A-SECRET-VALUE";

    fn snap(text: &str) -> Snapshot {
        Snapshot::parse(text).expect("parses")
    }

    #[test]
    fn provider_snapshot_reads_names_host_and_models_only() {
        let s = snap(&format!(
            r#"{{"env":{{"ANTHROPIC_BASE_URL":"https://user:{SECRET}@API.Example.com:8443/v1",
                "ANTHROPIC_AUTH_TOKEN":"{SECRET}","ANTHROPIC_DEFAULT_OPUS_MODEL":"glm-5",
                "ANTHROPIC_MODEL":"kimi","OTHER":"x"}},
                "model":"opus","apiKeyHelper":"echo {SECRET}","theme":"dark"}}"#
        ));
        assert_eq!(s.host.as_deref(), Some("api.example.com"));
        assert_eq!(s.env_model.as_deref(), Some("kimi"));
        assert_eq!(s.model.as_deref(), Some("opus"));
        let keys: Vec<&str> = s.keys.iter().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_MODEL",
                "apiKeyHelper",
                "model"
            ]
        );
        let json = s.to_json();
        assert!(!json.contains(SECRET), "{json}");
        assert!(!format!("{s:?}").contains(SECRET));
        assert_eq!(Snapshot::from_json(&json), Some(s));
        // Nothing set: the subscription.
        assert_eq!(snap("{}"), Snapshot::default());
        assert!(Snapshot::parse("not json").is_none());
    }

    #[test]
    fn provider_host_drops_everything_but_the_host() {
        assert_eq!(host_of("https://a.b/x").as_deref(), Some("a.b"));
        assert_eq!(
            host_of("http://127.0.0.1:15721").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(host_of("https://u:p@h.io/").as_deref(), Some("h.io"));
        assert_eq!(host_of("https://[::1]:80/").as_deref(), Some("::1"));
        assert_eq!(host_of("h.io").as_deref(), Some("h.io"));
        assert_eq!(host_of(""), None);
    }

    #[test]
    fn provider_verdict() {
        let third =
            snap(r#"{"env":{"ANTHROPIC_BASE_URL":"https://a.io","ANTHROPIC_AUTH_TOKEN":"x"}}"#);
        let seen = third.keys.clone();
        // Another value, or a key added: the next message uses it.
        let other =
            snap(r#"{"env":{"ANTHROPIC_BASE_URL":"https://b.io","ANTHROPIC_AUTH_TOKEN":"y"}}"#);
        assert_eq!(verdict(&third, &seen, &other), Verdict::Live);
        let more = snap(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://b.io","ANTHROPIC_AUTH_TOKEN":"y",
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"m"}}"#,
        );
        assert_eq!(verdict(&third, &seen, &more), Verdict::Live);
        // Back to the subscription: the keys are gone, the process keeps them.
        assert_eq!(
            verdict(&third, &seen, &Snapshot::default()),
            Verdict::NeedsRestart(Reason::Removed)
        );
        // A key seen later and removed again counts too.
        let seen_more = seen_with(&seen, &more);
        assert_eq!(
            verdict(&third, &seen_more, &other),
            Verdict::NeedsRestart(Reason::Removed)
        );
        // A fixed model, set or changed.
        let fixed = snap(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://a.io","ANTHROPIC_AUTH_TOKEN":"x",
                "ANTHROPIC_MODEL":"kimi"}}"#,
        );
        assert_eq!(
            verdict(&third, &seen, &fixed),
            Verdict::NeedsRestart(Reason::Model)
        );
        let top = snap(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://a.io","ANTHROPIC_AUTH_TOKEN":"x"},"model":"opus"}"#,
        );
        assert_eq!(
            verdict(&third, &seen, &top),
            Verdict::NeedsRestart(Reason::Model)
        );
        assert_eq!(verdict(&third, &seen, &third), Verdict::Live);
    }

    #[test]
    fn provider_settings_path_prefers_the_config() {
        let p = |c: Option<&str>, e: Option<&str>, h: Option<&str>| {
            settings_path(c.map(Path::new), e.map(Path::new), h.map(Path::new))
        };
        assert_eq!(
            p(Some("/c"), Some("/e"), Some("/h")),
            Some(PathBuf::from("/c/settings.json"))
        );
        assert_eq!(
            p(None, Some("/e"), Some("/h")),
            Some(PathBuf::from("/e/settings.json"))
        );
        assert_eq!(
            p(None, None, Some("/h")),
            Some(PathBuf::from("/h/.claude/settings.json"))
        );
        assert_eq!(p(Some("rel"), None, None), None);
    }
}
