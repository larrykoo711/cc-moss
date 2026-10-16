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
