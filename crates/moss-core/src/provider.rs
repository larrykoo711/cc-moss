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

use std::collections::{BTreeMap, BTreeSet};
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

/// 启动时的精确来源；不能从归一化后的 settings 路径反推。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchSource {
    #[default]
    Unknown,
    Unset,
    Explicit(String),
}

impl LaunchSource {
    pub fn capture(value: Option<std::ffi::OsString>) -> Self {
        match value {
            None => Self::Unset,
            Some(value) => value.into_string().map(Self::Explicit).unwrap_or_default(),
        }
    }

    pub fn env_argv(&self) -> Result<Vec<String>, &'static str> {
        match self {
            Self::Unknown => Err("the session's config source is unknown; start it manually with the intended CLAUDE_CONFIG_DIR source first"),
            Self::Unset => Ok(vec!["/usr/bin/env".into(), "-u".into(), "CLAUDE_CONFIG_DIR".into()]),
            Self::Explicit(path) if Path::new(path).is_absolute() && !path.chars().any(char::is_control) => {
                Ok(vec!["/usr/bin/env".into(), format!("CLAUDE_CONFIG_DIR={path}")])
            }
            Self::Explicit(_) => Err("the session's config source is not safe to replay"),
        }
    }
}

/// 输入必须是已验证入口的选项，不含程序名或解释器前缀。
/// 仅记录固定参数名或不可解析标记，不保存参数值、内联 JSON 或凭据。
pub fn unreplayable_args(args: Option<&[String]>) -> Vec<String> {
    let Some(args) = args else {
        return vec!["argv-unavailable".into()];
    };
    let mut names = BTreeSet::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        i += 1;
        if arg == "--" {
            break;
        }
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(f, v)| (f, Some(v)));
        if matches!(
            name,
            "--mcp-config" | "--strict-mcp-config" | "--settings" | "--setting-sources"
        ) {
            names.insert(name.to_string());
        }
        match (crate::launch_args::option_arity(name), inline) {
            (Some(0), None) | (Some(1), Some(_)) => {}
            (Some(1), None) if i < args.len() => i += 1,
            _ => {
                names.insert("argv-unparseable".into());
                break;
            }
        }
    }
    names.into_iter().collect()
}

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
    /// Top-level `model`: the default for new sessions (`/model`), not the
    /// provider's; it never asks for a restart.
    pub model: Option<String>,
    /// Explicit fallback when neither fixed nor top-level model is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Only documented family mappings, never arbitrary env values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_aliases: BTreeMap<String, String>,
    /// The settings file this was read from, for a session: the one Claude
    /// Code itself reads for it (its own `CLAUDE_CONFIG_DIR`), which a
    /// switch has to reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub launch_source: LaunchSource,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreplayable_args: Vec<String>,
    /// false 表示只采集到启动来源，不把坏 settings 当成空配置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_valid: Option<bool>,
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
    default_model: Option<String>,
    model_aliases: BTreeMap<String, String>,
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
                        "ANTHROPIC_DEFAULT_MODEL" => {
                            env.default_model = map.next_value::<Option<String>>()?
                        }
                        "ANTHROPIC_DEFAULT_OPUS_MODEL"
                        | "ANTHROPIC_DEFAULT_SONNET_MODEL"
                        | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
                        | "ANTHROPIC_DEFAULT_FABLE_MODEL" => {
                            if let Some(value) = map.next_value::<Option<String>>()? {
                                let family = key
                                    .trim_start_matches("ANTHROPIC_DEFAULT_")
                                    .trim_end_matches("_MODEL")
                                    .to_ascii_lowercase();
                                env.model_aliases.insert(family, value);
                            }
                        }
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
        let mut s: Settings = serde_json::from_str(text).ok()?;
        let mut env = s.env.take().unwrap_or_default();
        env.model = env.model.filter(|m| !m.is_empty());
        s.model = s.model.filter(|m| !m.is_empty());
        env.model_aliases.retain(|_, model| !model.is_empty());
        env.default_model = env.default_model.filter(|m| !m.is_empty());
        // An unused fallback must neither invalidate a fixed choice nor leak
        // arbitrary values into the snapshot. Only retain a selected fallback.
        if env.model.is_some() || s.model.is_some() {
            env.default_model = None;
        }
        if env
            .model
            .iter()
            .chain(s.model.iter())
            .chain(env.model_aliases.values())
            .chain(env.default_model.iter())
            .any(|model| !safe_model(model))
        {
            return None;
        }
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
            model_aliases: env.model_aliases,
            default_model: env.default_model,
            source: None,
            launch_source: LaunchSource::Unknown,
            unreplayable_args: Vec::new(),
            settings_valid: None,
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
        self.env_model.is_some()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Option<Snapshot> {
        serde_json::from_str(text).ok()
    }
}

/// Model identifiers are safe command arguments, not arbitrary configuration text.
/// Reject credential-like values rather than serializing them into a plan.
pub fn safe_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    !model.is_empty()
        && !model.starts_with('-')
        && model.len() <= 256
        && model
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-/:[]+".contains(&c))
        && !model.contains("://")
        && !["sk-", "token", "secret", "password", "api_key"]
            .iter()
            .any(|part| lower.contains(part))
}

/// The safe target of an explicit channel application. No credentials or paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelPlan {
    pub model: Option<String>,
    pub source: &'static str,
    pub revision: String,
    pub uncertain: bool,
}

impl Snapshot {
    /// Fixed model first, then the explicit user choice and its family mapping.
    pub fn model_plan(&self, uncertain: bool) -> ModelPlan {
        let (selection, source) = if let Some(model) = &self.env_model {
            (model.as_str(), "env")
        } else if let Some(model) = &self.model {
            (model.as_str(), "settings")
        } else if let Some(model) = &self.default_model {
            (model.as_str(), "env")
        } else {
            ("default", "default")
        };
        let (model, source) = match self.model_aliases.get(selection) {
            Some(mapped) => (mapped.as_str(), "alias"),
            None => (selection, source),
        };
        let mut plan = ModelPlan::new(Some(model), source, uncertain);
        let safe_revision = format!(
            "{}|{}|{}",
            plan.revision,
            self.host.as_deref().unwrap_or(""),
            self.keys.iter().cloned().collect::<Vec<_>>().join(",")
        );
        let mut hash = 0xcbf29ce484222325u64;
        for byte in safe_revision.bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        plan.revision = format!("{hash:016x}");
        plan
    }
}

impl ModelPlan {
    pub fn unknown() -> Self {
        Self::new(None, "unknown", true)
    }

    fn new(model: Option<&str>, source: &'static str, uncertain: bool) -> Self {
        // Stable FNV-1a over only the public model plan, never the source settings.
        let mut hash = 0xcbf29ce484222325u64;
        for byte in source.bytes().chain([0]).chain(model.unwrap_or("").bytes()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Self {
            model: model.map(str::to_string),
            source,
            revision: format!("{hash:016x}"),
            uncertain,
        }
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

/// Moss's copy of the user's provider (F24, SM1). Moss starts without the
/// user's settings (`--setting-sources project,local`); its home's
/// `.claude/settings.local.json` carries what picks the provider and is no
/// secret: the base URL, the models, the context size. Claude Code puts a
/// settings `env` into its shell commands' environment too, so a token or
/// key never goes there: when the user's settings have one (or a key
/// helper of their own), Moss's `apiKeyHelper` is `helper` (`cc-moss moss
/// provider-key`), which Claude Code runs outside Moss's sandbox. `None`
/// when `user` (the text of the user's `settings.json`) is not JSON.
pub fn moss_settings(user: &str, helper: &str) -> Option<MossSettings> {
    let value: serde_json::Value = serde_json::from_str(user).ok()?;
    let mut env = serde_json::Map::new();
    let mut dropped_url = false;
    let mut secret = value.get(TOP_HELPER).is_some_and(|h| h.is_string());
    if let Some(all) = value.get("env").and_then(|e| e.as_object()) {
        for (key, v) in all {
            let Some(text) = v.as_str() else {
                continue;
            };
            match key.as_str() {
                "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY" => secret = true,
                BASE_URL if has_userinfo(text) => dropped_url = true,
                k if provider_key(k) || k == MAX_CONTEXT => {
                    env.insert(key.clone(), v.clone());
                }
                _ => {}
            }
        }
    }
    let mut out = serde_json::Map::new();
    if !env.is_empty() {
        out.insert("env".into(), env.into());
    }
    if secret {
        out.insert(TOP_HELPER.into(), helper.into());
    }
    let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(out)).ok()?;
    text.push('\n');
    Some(MossSettings { text, dropped_url })
}

/// What [`moss_settings`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MossSettings {
    pub text: String,
    /// The base URL had a user or password in it, so it was left out.
    pub dropped_url: bool,
}

/// Whether a URL carries `user:password@`.
fn has_userinfo(url: &str) -> bool {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    authority.contains('@')
}

/// The key `cc-moss moss provider-key` hands Claude Code from the user's
/// settings text: the auth token, else the API key. `None` when there is
/// none (the user's own `apiKeyHelper` is then run by the caller).
pub fn settings_key(user: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(user).ok()?;
    let env = value.get("env")?;
    ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .iter()
        .find_map(|k| env.get(*k)?.as_str().filter(|v| !v.is_empty()))
        .map(String::from)
}

/// The user's own `apiKeyHelper` command, if any.
pub fn settings_helper(user: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(user).ok()?;
    value
        .get(TOP_HELPER)?
        .as_str()
        .filter(|h| !h.trim().is_empty())
        .map(String::from)
}

/// Passed on to Moss with the provider keys.
const MAX_CONTEXT: &str = "CLAUDE_CODE_MAX_CONTEXT_TOKENS";

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
        .filter(|k| k.as_str() != TOP_MODEL)
        .any(|k| !now.keys.contains(k))
    {
        return Verdict::NeedsRestart(Reason::Removed);
    }
    // Only the provider's fixed model; top-level `model` (`/model`) is the
    // default for new sessions and a running one keeps its own.
    if now.env_model != started.env_model {
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
    if started.settings_valid == Some(false) {
        return None;
    }
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

    #[test]
    fn launch_source_roundtrip_distinguishes_same_settings_path() {
        let unset = Snapshot {
            source: Some("/tmp/home/.claude/settings.json".into()),
            launch_source: LaunchSource::Unset,
            ..Snapshot::default()
        };
        let mut explicit = unset.clone();
        explicit.launch_source = LaunchSource::Explicit("/tmp/home/.claude".into());
        assert_ne!(
            Snapshot::from_json(&unset.to_json()),
            Snapshot::from_json(&explicit.to_json())
        );
        assert_eq!(Snapshot::from_json(&explicit.to_json()), Some(explicit));
    }

    #[test]
    fn launch_source_invalid_settings_are_not_an_empty_provider() {
        let snapshot = Snapshot {
            launch_source: LaunchSource::Unset,
            settings_valid: Some(false),
            ..Snapshot::default()
        };
        assert!(recorded(Some(&snapshot.to_json()), Some("[]")).is_none());
        assert_eq!(
            Snapshot::from_json(&snapshot.to_json())
                .unwrap()
                .launch_source,
            LaunchSource::Unset
        );
    }

    #[test]
    fn launch_source_legacy_is_unknown() {
        let old = r#"{"keys":[],"host":null,"env_model":null,"model":null,"source":"/tmp/home/.claude/settings.json"}"#;
        assert_eq!(
            Snapshot::from_json(old).unwrap().launch_source,
            LaunchSource::Unknown
        );
    }

    #[test]
    fn launch_source_cli_markers_never_store_values() {
        let args: Vec<String> = [
            "--mcp-config={\"token\":\"secret\"}",
            "--settings",
            "credential-value",
            "--strict-mcp-config",
            "--setting-sources=user",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(
            unreplayable_args(Some(&args)),
            [
                "--mcp-config",
                "--setting-sources",
                "--settings",
                "--strict-mcp-config"
            ]
        );
        assert_eq!(unreplayable_args(None), ["argv-unavailable"]);
    }

    #[test]
    fn launch_source_arity_dash_value_does_not_hide_mcp_flag() {
        let args = [
            "--append-system-prompt",
            "--",
            "--mcp-config",
            "/tmp/mcp.json",
        ]
        .map(str::to_string);
        assert_eq!(unreplayable_args(Some(&args)), ["--mcp-config"]);
    }

    #[test]
    fn launch_source_arity_flag_shaped_prompt_is_not_a_flag() {
        let args = ["--append-system-prompt", "--mcp-config"].map(str::to_string);
        assert!(unreplayable_args(Some(&args)).is_empty());
    }

    #[test]
    fn launch_source_arity_unknown_or_missing_values_fail_closed() {
        for words in [
            vec!["--unknown=fixture-secret"],
            vec!["--append-system-prompt"],
            vec!["--verbose=fixture-secret"],
        ] {
            let args: Vec<String> = words.into_iter().map(str::to_string).collect();
            assert_eq!(unreplayable_args(Some(&args)), ["argv-unparseable"]);
        }
        let args = ["--", "--mcp-config", "fixture-secret"].map(str::to_string);
        assert!(unreplayable_args(Some(&args)).is_empty());
    }

    #[test]
    fn launch_source_invalid_paths_refuse_without_echoing_values() {
        for path in ["", "relative", "/tmp/path\nsecret"] {
            assert_eq!(
                LaunchSource::Explicit(path.into()).env_argv().unwrap_err(),
                "the session's config source is not safe to replay"
            );
        }
    }

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
    fn moss_settings_carry_no_secret() {
        let helper = "/bin/cc-moss moss provider-key";
        let made = moss_settings(
            &format!(
                r#"{{"env":{{"ANTHROPIC_BASE_URL":"https://a.io","ANTHROPIC_AUTH_TOKEN":"{SECRET}",
                "ANTHROPIC_DEFAULT_OPUS_MODEL":"glm","CLAUDE_CODE_MAX_CONTEXT_TOKENS":"200000",
                "HTTPS_PROXY":"http://p","OTHER":"x"}},
                "apiKeyHelper":"echo {SECRET}","model":"opus","hooks":{{}},"permissions":{{}}}}"#
            ),
            helper,
        )
        .unwrap();
        assert!(!made.text.contains(SECRET), "{}", made.text);
        let v: serde_json::Value = serde_json::from_str(&made.text).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://a.io",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm",
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "200000"
                },
                "apiKeyHelper": helper
            })
        );
        // A password in the URL: the URL stays out.
        let made = moss_settings(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://u:pw@a.io","ANTHROPIC_API_KEY":"k"}}"#,
            helper,
        )
        .unwrap();
        assert!(made.dropped_url);
        assert!(!made.text.contains("pw"), "{}", made.text);
        // The subscription: nothing at all.
        assert_eq!(
            moss_settings(r#"{"theme":"dark","model":"opus"}"#, helper)
                .unwrap()
                .text,
            "{}\n"
        );
        assert_eq!(moss_settings("nope", helper), None);
        // What the helper hands over: the token, else the key.
        assert_eq!(
            settings_key(r#"{"env":{"ANTHROPIC_API_KEY":"k","ANTHROPIC_AUTH_TOKEN":"t"}}"#)
                .as_deref(),
            Some("t")
        );
        assert_eq!(
            settings_key(r#"{"env":{"ANTHROPIC_API_KEY":"k"}}"#).as_deref(),
            Some("k")
        );
        assert_eq!(settings_key(r#"{"apiKeyHelper":"x"}"#), None);
        assert_eq!(
            settings_helper(r#"{"apiKeyHelper":"x"}"#).as_deref(),
            Some("x")
        );
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

    fn provider_keys(s: &Snapshot) -> BTreeSet<String> {
        s.keys.clone()
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
        // `/model` writes top-level `model`: no restart, set or removed.
        let top = snap(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://a.io","ANTHROPIC_AUTH_TOKEN":"x"},"model":"opus"}"#,
        );
        assert_eq!(verdict(&third, &seen, &top), Verdict::Live);
        assert_eq!(verdict(&top, &provider_keys(&top), &third), Verdict::Live);
        assert!(!top.picks_model() && fixed.picks_model());
        assert_eq!(verdict(&third, &seen, &third), Verdict::Live);
    }

    #[test]
    fn provider_model_aliases_are_safe_and_recorded() {
        let s = snap(
            r#"{"model":"opus","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"route-b","ANTHROPIC_AUTH_TOKEN":"private-auth-value"}}"#,
        );
        let v: serde_json::Value = serde_json::from_str(&s.to_json()).unwrap();
        assert_eq!(v["model_aliases"]["opus"], "route-b");
        assert!(!s.to_json().contains("private-auth-value"));
    }

    #[test]
    fn provider_model_rejects_unsafe_model_values() {
        for model in [
            "bad\nmodel",
            "sk-private-auth",
            "model with spaces",
            "https://user:password@host/model",
        ] {
            let text = serde_json::json!({"env":{"ANTHROPIC_MODEL":model}}).to_string();
            assert!(
                Snapshot::parse(&text).is_none(),
                "unsafe model was accepted"
            );
        }
    }

    #[test]
    fn provider_model_fable_and_fallback_boundaries() {
        for (text, expected) in [
            (
                r#"{"model":"fable","env":{"ANTHROPIC_DEFAULT_FABLE_MODEL":"fable-route"}}"#,
                "fable-route",
            ),
            (
                r#"{"env":{"ANTHROPIC_DEFAULT_MODEL":"fallback-route"}}"#,
                "fallback-route",
            ),
            (
                r#"{"model":"chosen","env":{"ANTHROPIC_DEFAULT_MODEL":"ignored-route"}}"#,
                "chosen",
            ),
            (
                r#"{"env":{"ANTHROPIC_MODEL":"","ANTHROPIC_DEFAULT_OPUS_MODEL":""},"model":""}"#,
                "default",
            ),
        ] {
            assert_eq!(snap(text).model_plan(true).model.as_deref(), Some(expected));
        }
        assert!(!safe_model("--dangerously-skip-permissions"));
    }

    #[test]
    fn provider_model_resolution_table() {
        for (text, model, source) in [
            (
                r#"{"env":{"ANTHROPIC_MODEL":"fixed","ANTHROPIC_DEFAULT_OPUS_MODEL":"alias"},"model":"opus"}"#,
                "fixed",
                "env",
            ),
            (
                r#"{"model":"opus","env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"alias"}}"#,
                "alias",
                "alias",
            ),
            (r#"{"model":"chosen"}"#, "chosen", "settings"),
            (r#"{}"#, "default", "default"),
        ] {
            let plan = snap(text).model_plan(true);
            assert_eq!((plan.model.as_deref(), plan.source), (Some(model), source));
        }
    }

    #[test]
    fn provider_model_revision_tracks_host_not_credentials() {
        let plan = |host, token| {
            snap(&serde_json::json!({"env":{"ANTHROPIC_BASE_URL":host,"ANTHROPIC_AUTH_TOKEN":token,"ANTHROPIC_MODEL":"route"}}).to_string()).model_plan(true)
        };
        assert_eq!(
            plan("https://a.example", "one").revision,
            plan("https://a.example", "two").revision
        );
        assert_ne!(
            plan("https://a.example", "one").revision,
            plan("https://b.example", "one").revision
        );
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
