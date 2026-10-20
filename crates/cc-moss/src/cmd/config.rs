//! `cc-moss config get|set`: read and change the few settings that apps and
//! scripts may touch (today only `ui.language`). `set` edits `config.toml`
//! in place, keeping comments and everything else, and writes it atomically.

use std::process::ExitCode;

use moss_core::config::{self, config_path, Config, SettingError, KEYS};
use serde_json::{json, Value};

use super::files::{self, read_optional};

pub const JSON_SCHEMA: u32 = 1;

pub enum Action {
    Get {
        key: String,
        json: bool,
    },
    Set {
        key: String,
        value: String,
        json: bool,
    },
}

/// Why `get` or `set` said no, with a code the app can act on.
struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Failure {
        Failure {
            code,
            message: message.into(),
        }
    }

    fn other(e: anyhow::Error) -> Failure {
        Failure::new("error", format!("{e:#}"))
    }

    fn setting(e: SettingError, key: &str, value: &str) -> Failure {
        match e {
            SettingError::UnknownKey => {
                let known = KEYS.join(", ");
                Failure::new(
                    "unknown_key",
                    tr!(
                        format!(
                            "`{key}` is not a setting cc-moss config knows; known keys: {known}"
                        ),
                        format!("cc-moss config 不认识 `{key}`；目前能用的键：{known}")
                    ),
                )
            }
            SettingError::ReadOnly => Failure::new(
                "read_only",
                tr!(
                    format!("`{key}` can only be read here; change it in config.toml"),
                    format!("`{key}` 在这里只能查看；要改就编辑 config.toml")
                ),
            ),
            SettingError::BadValue(allowed) => {
                let allowed = allowed.join(", ");
                Failure::new(
                    "bad_value",
                    tr!(
                        format!("`{value}` is not a value for {key}; use one of: {allowed}"),
                        format!("{key} 不能设为 `{value}`；可以用：{allowed}")
                    ),
                )
            }
        }
    }
}

pub fn run(action: Action) -> ExitCode {
    let (json, result) = match action {
        Action::Get { key, json } => (json, get(&key, json)),
        Action::Set { key, value, json } => (json, set(&key, &value, json)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            if json {
                print_json(&json!({
                    "schema": JSON_SCHEMA,
                    "ok": false,
                    "code": f.code,
                    "error": f.message,
                }));
            } else {
                eprintln!("cc-moss: {}", f.message);
            }
            ExitCode::FAILURE
        }
    }
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn print_value(key: &str, value: &str) {
    print_json(&json!({
        "schema": JSON_SCHEMA,
        "ok": true,
        "key": key,
        "value": value,
    }));
}

fn get(key: &str, as_json: bool) -> Result<(), Failure> {
    // An unknown key is refused before the file is read.
    config::get_setting(&Config::default(), key).map_err(|e| Failure::setting(e, key, ""))?;
    let config = Config::load().map_err(|e| Failure::new("config", format!("{e:#}")))?;
    let value = config::get_setting(&config, key).map_err(|e| Failure::setting(e, key, ""))?;
    if as_json {
        print_value(key, &value);
    } else {
        println!("{value}");
    }
    Ok(())
}

fn set(key: &str, value: &str, as_json: bool) -> Result<(), Failure> {
    let value = config::check_setting(key, value).map_err(|e| Failure::setting(e, key, value))?;
    // Through any symlink a dotfile manager made, as `alias add` does.
    let path = config_path();
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let original = read_optional(&path).map_err(Failure::other)?;
    let text = config::set_in_text(original.as_deref().unwrap_or(""), key, &value)
        .map_err(|e| Failure::new("config", format!("{}: {e:#}", path.display())))?;
    if original.as_deref() != Some(text.as_str()) {
        files::write_atomically(&path, text.as_bytes()).map_err(Failure::other)?;
    }
    if as_json {
        print_value(key, &value);
    } else {
        println!(
            "{}",
            tr!(
                format!("{key} is now {value}. cc-moss uses it from the next run."),
                format!("{key} 已设为 {value}，cc-moss 下次运行时生效。")
            )
        );
    }
    Ok(())
}

/// Set `key` to `value` in the config file at `path` for another command
/// (`moss stop`, which writes the daemon's), the same way `config set` does.
pub fn store_at(path: &std::path::Path, key: &str, value: &str) -> anyhow::Result<()> {
    let value = config::check_setting(key, value)
        .map_err(|_| anyhow::anyhow!("bad setting {key}={value}"))?;
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let original = read_optional(&path)?;
    let text = config::set_in_text(original.as_deref().unwrap_or(""), key, &value)
        .map_err(|e| anyhow::anyhow!("{}: {e:#}", path.display()))?;
    if original.as_deref() != Some(text.as_str()) {
        files::write_atomically(&path, text.as_bytes())?;
    }
    Ok(())
}
