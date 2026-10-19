//! `~/.config/cc-moss/config.toml`. Every field is optional.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub claude: ClaudeConfig,
    pub naming: NamingConfig,
    pub tmux: TmuxConfig,
    pub ui: UiConfig,
    pub update: UpdateConfig,
    pub ask: AskConfig,
    pub moss: MossConfig,
    /// Native aliases: `name = "subcommand args…"` (see [`crate::aliases`]).
    /// An entry that is not a string is skipped (doctor names it) instead of
    /// making the whole file unreadable for every command.
    #[serde(deserialize_with = "string_entries")]
    pub aliases: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    /// Extra arguments for every `cc-moss new`, e.g. `["--model", "opus"]`.
    pub default_args: Vec<String>,
    /// How `cc-moss new` starts a session: in bypass mode unless set to `default`.
    pub permission_mode: PermissionMode,
}

/// `[claude] permission_mode`: whether new sessions ask before each action.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// `--dangerously-skip-permissions`: Claude Code does not stop to ask.
    #[default]
    Bypass,
    /// Claude Code's own default: it asks for each permission.
    Default,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Bypass => "bypass",
            PermissionMode::Default => "default",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct NamingConfig {
    /// Prefix tmux session names with the machine name.
    pub host_prefix: bool,
}

impl Default for NamingConfig {
    fn default() -> Self {
        NamingConfig { host_prefix: true }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TmuxConfig {
    /// A socket name (`tmux -L`) or path (`tmux -S`, contains `/`). Default server if unset.
    pub socket: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub icons: Icons,
    /// "auto" (or unset), "en" or "zh".
    pub language: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Look for a new cc-moss once a day (picker and `doctor` only).
    pub check: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        UpdateConfig { check: true }
    }
}

/// `[ask]`: questions a session waits on can be answered from outside
/// (the app, Slack, `cc-moss ask answer`).
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AskConfig {
    /// On by default; with nobody taking questions it changes nothing.
    pub enabled: bool,
    /// How long a question waits for an answer from outside, in seconds
    /// (1 to [`crate::ask::MAX_WAIT_SECS`]). The session's own dialog stays
    /// usable meanwhile.
    pub wait_secs: u64,
}

impl Default for AskConfig {
    fn default() -> Self {
        AskConfig {
            enabled: true,
            wait_secs: crate::ask::DEFAULT_WAIT_SECS,
        }
    }
}

/// `[moss]`: the resident session the daemon looks after.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct MossConfig {
    /// On by default; off means the daemon ends Moss and leaves it off.
    pub enabled: bool,
    /// Tell Moss when other sessions need you, drop or finish (on by default).
    pub digest: bool,
    /// Let Moss have the daemon carry out its fixes (`moss act`).
    pub actions: bool,
    /// A checkup once a day, the first time the desktop is seen.
    pub daily_checkup: bool,
    /// The daily checkup's brief is shown (off: kept quiet unless something
    /// needs a decision).
    pub daily_brief: bool,
}

impl Default for MossConfig {
    fn default() -> Self {
        MossConfig {
            enabled: true,
            digest: true,
            actions: true,
            daily_checkup: true,
            daily_brief: true,
        }
    }
}

/// Whether to draw Nerd Font icons.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Icons {
    /// Icons when starship is loaded (it needs a Nerd Font too).
    #[default]
    Auto,
    Nerd,
    Text,
}

impl Icons {
    pub fn enabled(self, starship_shell: Option<&str>) -> bool {
        match self {
            Icons::Nerd => true,
            Icons::Text => false,
            Icons::Auto => starship_shell.is_some_and(|s| !s.is_empty()),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("read config {}", path.display())),
        }
    }

    /// The tmux socket: `$CC_MOSS_TMUX_SOCKET` wins over the config file.
    pub fn tmux_socket(&self) -> Option<String> {
        std::env::var("CC_MOSS_TMUX_SOCKET")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| self.tmux.socket.clone())
    }
}

fn string_entries<'de, D>(d: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = BTreeMap::<String, toml::Value>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| match v {
            toml::Value::String(s) => Some((k, s)),
            _ => None,
        })
        .collect())
}

/// The keys `cc-moss config get|set` may touch. Everything else in the file
/// is the user's to edit by hand.
pub const KEYS: &[&str] = &[
    "ui.language",
    "claude.permission_mode",
    "ask.enabled",
    "moss.enabled",
    "moss.digest",
    "moss.actions",
    "moss.daily_checkup",
    "moss.daily_brief",
];

/// What `ui.language` may be set to.
pub const LANGUAGES: &[&str] = &["auto", "zh", "en"];

/// What `claude.permission_mode` may be set to.
pub const PERMISSION_MODES: &[&str] = &["bypass", "default"];

/// What a true/false setting may be set to.
pub const BOOLS: &[&str] = &["true", "false"];

/// Keys stored as TOML booleans rather than strings.
const BOOL_KEYS: &[&str] = &[
    "ask.enabled",
    "moss.enabled",
    "moss.digest",
    "moss.actions",
    "moss.daily_checkup",
    "moss.daily_brief",
];

/// Why a key or value is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingError {
    /// Not one of [`KEYS`].
    UnknownKey,
    /// Not one of these values.
    BadValue(&'static [&'static str]),
}

/// The value as it is stored (trimmed, lowercase), if `key` may hold it.
pub fn check_setting(key: &str, value: &str) -> Result<String, SettingError> {
    let allowed = match key {
        "ui.language" => LANGUAGES,
        "claude.permission_mode" => PERMISSION_MODES,
        "ask.enabled" | "moss.enabled" | "moss.digest" | "moss.actions" | "moss.daily_checkup"
        | "moss.daily_brief" => BOOLS,
        _ => return Err(SettingError::UnknownKey),
    };
    let value = value.trim().to_ascii_lowercase();
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(SettingError::BadValue(allowed))
    }
}

/// A key's value in `config`, with its default when unset.
pub fn get_setting(config: &Config, key: &str) -> Result<String, SettingError> {
    match key {
        "ui.language" => Ok(config
            .ui
            .language
            .as_deref()
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "auto".into())),
        "claude.permission_mode" => Ok(config.claude.permission_mode.as_str().into()),
        "ask.enabled" => Ok(config.ask.enabled.to_string()),
        "moss.enabled" => Ok(config.moss.enabled.to_string()),
        "moss.digest" => Ok(config.moss.digest.to_string()),
        "moss.actions" => Ok(config.moss.actions.to_string()),
        "moss.daily_checkup" => Ok(config.moss.daily_checkup.to_string()),
        "moss.daily_brief" => Ok(config.moss.daily_brief.to_string()),
        _ => Err(SettingError::UnknownKey),
    }
}

/// `text` with `key` (`table.name`) set to `value` (a string, or a boolean
/// for true/false keys). Comments,
/// order and every other table and key stay as they are; a missing table is
/// added at the end. Check the key and value with [`check_setting`] first.
pub fn set_in_text(text: &str, key: &str, value: &str) -> Result<String> {
    let (table_name, name) = key
        .split_once('.')
        .with_context(|| format!("`{key}` is not `table.key`"))?;
    let mut doc: toml_edit::DocumentMut = text.parse().context("config.toml is not valid TOML")?;
    let table = doc
        .entry(table_name)
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .with_context(|| format!("`{table_name}` in config.toml is not a table"))?;
    let mut new = match (BOOL_KEYS.contains(&key), value) {
        (true, "true") => toml_edit::Value::from(true),
        (true, "false") => toml_edit::Value::from(false),
        _ => toml_edit::Value::from(value),
    };
    if let Some(old) = table.get(name).and_then(toml_edit::Item::as_value) {
        *new.decor_mut() = old.decor().clone();
    }
    table.insert(name, toml_edit::Item::Value(new));
    Ok(doc.to_string())
}

/// `$CC_MOSS_CONFIG`, else `$XDG_CONFIG_HOME/cc-moss/config.toml`, else `~/.config/cc-moss/config.toml`.
pub fn config_path() -> PathBuf {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    if let Some(p) = env("CC_MOSS_CONFIG") {
        return PathBuf::from(p);
    }
    let base = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env("HOME").unwrap_or_else(|| ".".into())).join(".config")
        });
    base.join("cc-moss/config.toml")
}
