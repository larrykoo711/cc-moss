//! The interface language, English or Chinese, and the `tr!` macro that
//! picks between the two wordings of a message written side by side.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    /// A language tag such as `zh_CN.UTF-8`, `zh-Hans-US` or `en`. Empty,
    /// `auto` and the C locale say nothing.
    fn parse(tag: &str) -> Option<Lang> {
        let tag = tag.trim().to_ascii_lowercase();
        match tag.as_str() {
            "" | "auto" | "c" | "posix" => None,
            t if t.starts_with("c.") => None,
            t if t.starts_with("zh") => Some(Lang::Zh),
            _ => Some(Lang::En),
        }
    }
}

/// Pick the language, first match wins: `CC_MOSS_LANG`, the config file,
/// `LC_ALL`, `LC_MESSAGES`, the system's preferred language, `LANG`.
///
/// The system language comes before `LANG` because terminals on macOS set
/// `LANG=en_US.UTF-8` whatever language the user reads.
pub fn detect(
    config: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    system: &dyn Fn() -> Option<String>,
) -> Lang {
    let from_env = |key: &str| env(key).and_then(|v| Lang::parse(&v));
    from_env("CC_MOSS_LANG")
        .or_else(|| config.and_then(Lang::parse))
        .or_else(|| from_env("LC_ALL"))
        .or_else(|| from_env("LC_MESSAGES"))
        .or_else(|| system().and_then(|v| Lang::parse(&v)))
        .or_else(|| from_env("LANG"))
        .unwrap_or(Lang::En)
}

/// The language for this process, decided on first use.
#[cfg(not(test))]
pub fn lang() -> Lang {
    static LANG: std::sync::OnceLock<Lang> = std::sync::OnceLock::new();
    *LANG.get_or_init(|| {
        let config = moss_core::config::Config::load().ok();
        let language = config.as_ref().and_then(|c| c.ui.language.clone());
        detect(
            language.as_deref(),
            &|k| std::env::var(k).ok(),
            &system_language,
        )
    })
}

/// Tests speak English unless they ask otherwise.
#[cfg(test)]
pub fn lang() -> Lang {
    FORCED.with(|f| f.get())
}

#[cfg(test)]
thread_local! {
    static FORCED: std::cell::Cell<Lang> = const { std::cell::Cell::new(Lang::En) };
}

/// Switch the language for the current test thread.
#[cfg(test)]
pub fn set_for_test(lang: Lang) {
    FORCED.with(|f| f.set(lang));
}

/// The first of the user's preferred languages on macOS.
#[cfg_attr(test, allow(dead_code))]
fn system_language() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = moss_core::command::output_with_timeout(
        std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]),
        Duration::from_millis(500),
    )?;
    first_quoted(&String::from_utf8_lossy(&out.stdout))
}

/// `(\n    "zh-Hans-US",\n    "en-US"\n)` → `zh-Hans-US`.
fn first_quoted(text: &str) -> Option<String> {
    let start = text.find('"')? + 1;
    let len = text[start..].find('"')?;
    Some(text[start..start + len].to_string())
}

/// `tr!("Saved.", "已保存。")`: the wording for the current language. Both
/// arms must have the same type; only the chosen one is evaluated.
macro_rules! tr {
    ($en:expr, $zh:expr $(,)?) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::En => $en,
            $crate::i18n::Lang::Zh => $zh,
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            vars.iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn macos_language_beats_the_default_lang() {
        let vars = [("LANG", "en_US.UTF-8")];
        let zh = || Some("zh-Hans-US".to_string());
        assert_eq!(detect(None, &env(&vars), &zh), Lang::Zh);
    }

    #[test]
    fn explicit_choices_win_in_order() {
        let zh = || Some("zh-Hans".to_string());
        let vars = [("CC_MOSS_LANG", "en"), ("LC_ALL", "zh_CN.UTF-8")];
        assert_eq!(detect(Some("zh"), &env(&vars), &zh), Lang::En);
        assert_eq!(detect(Some("en"), &env(&[]), &zh), Lang::En);
        let vars = [("LC_ALL", "en_US.UTF-8")];
        assert_eq!(detect(None, &env(&vars), &zh), Lang::En);
        let vars = [("LC_MESSAGES", "zh_TW.UTF-8")];
        assert_eq!(detect(None, &env(&vars), &|| None), Lang::Zh);
    }

    #[test]
    fn auto_and_the_c_locale_fall_through() {
        let vars = [
            ("CC_MOSS_LANG", "auto"),
            ("LC_ALL", "C"),
            ("LANG", "zh_CN.UTF-8"),
        ];
        assert_eq!(detect(Some("auto"), &env(&vars), &|| None), Lang::Zh);
        assert_eq!(detect(None, &env(&[]), &|| None), Lang::En);
    }

    #[test]
    fn reads_the_first_preferred_language() {
        let out = "(\n    \"zh-Hans-US\",\n    \"en-US\"\n)\n";
        assert_eq!(first_quoted(out).as_deref(), Some("zh-Hans-US"));
        assert_eq!(first_quoted(""), None);
    }
}
