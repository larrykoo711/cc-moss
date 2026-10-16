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
    /// Native aliases: `name = "subcommand args…"` (see [`crate::aliases`]).
    /// An entry that is not a string is skipped (doctor names it) instead of
    /// making the whole file unreadable for every command.
    #[serde(deserialize_with = "string_entries")]
    pub aliases: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    /// Extra arguments for every `cc-moss new`, e.g. `["--dangerously-skip-permissions"]`.
    pub default_args: Vec<String>,
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
pub const KEYS: &[&str] = &["ui.language"];

/// What `ui.language` may be set to.
pub const LANGUAGES: &[&str] = &["auto", "zh", "en"];

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
        _ => Err(SettingError::UnknownKey),
    }
}

/// `text` with `key` (`table.name`) set to the string `value`. Comments,
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
    let mut new = toml_edit::Value::from(value);
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
