use moss_core::config::Config;

#[test]
fn missing_file_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let c = Config::load_from(&dir.path().join("nope.toml")).unwrap();
    assert!(c.claude.default_args.is_empty());
    assert!(c.naming.host_prefix);
    assert_eq!(c.tmux.socket, None);
}

#[test]
fn partial_fields() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(
        &p,
        "[claude]\ndefault_args = [\"--dangerously-skip-permissions\"]\n",
    )
    .unwrap();
    let c = Config::load_from(&p).unwrap();
    assert_eq!(
        c.claude.default_args,
        vec!["--dangerously-skip-permissions"]
    );
    assert!(c.naming.host_prefix);
}

#[test]
fn invalid_toml_error_has_path() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(&p, "[claude\n").unwrap();
    let err = format!("{:#}", Config::load_from(&p).unwrap_err());
    assert!(err.contains("config.toml"), "{err}");
}

#[test]
fn permission_mode_defaults_to_bypass() {
    use moss_core::config::PermissionMode;
    let dir = tempfile::tempdir().unwrap();
    let c = Config::load_from(&dir.path().join("nope.toml")).unwrap();
    assert_eq!(c.claude.permission_mode, PermissionMode::Bypass);
    assert_eq!(
        moss_core::config::get_setting(&c, "claude.permission_mode"),
        Ok("bypass".into())
    );
}

#[test]
fn permission_mode_default_parses() {
    use moss_core::config::PermissionMode;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(&p, "[claude]\npermission_mode = \"default\"\n").unwrap();
    let c = Config::load_from(&p).unwrap();
    assert_eq!(c.claude.permission_mode, PermissionMode::Default);
}

#[test]
fn permission_mode_rejects_unknown_value_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(&p, "[claude]\npermission_mode = \"yolo\"\n").unwrap();
    let err = format!("{:#}", Config::load_from(&p).unwrap_err());
    assert!(err.contains("permission_mode"), "{err}");
    assert!(err.contains("bypass"), "{err}");
}

#[test]
fn icons_follow_starship_unless_set() {
    use moss_core::config::Icons;
    let dir = tempfile::tempdir().unwrap();
    let c = Config::load_from(&dir.path().join("nope.toml")).unwrap();
    assert_eq!(c.ui.icons, Icons::Auto);
    assert!(Icons::Auto.enabled(Some("zsh")));
    assert!(!Icons::Auto.enabled(None));
    assert!(!Icons::Auto.enabled(Some("")));
    let p = dir.path().join("config.toml");
    std::fs::write(&p, "[ui]\nicons = \"text\"\n").unwrap();
    let c = Config::load_from(&p).unwrap();
    assert!(!c.ui.icons.enabled(Some("zsh")));
    assert!(Icons::Nerd.enabled(None));
}

mod setting {
    use moss_core::config::{self, Config, SettingError};

    #[test]
    fn only_known_keys_and_values() {
        assert_eq!(
            config::check_setting("ui.language", " ZH "),
            Ok("zh".into())
        );
        assert_eq!(
            config::check_setting("ui.language", "auto"),
            Ok("auto".into())
        );
        assert_eq!(
            config::check_setting("ui.icons", "nerd"),
            Err(SettingError::UnknownKey)
        );
        assert_eq!(
            config::check_setting("ui.language", "fr"),
            Err(SettingError::BadValue(&["auto", "zh", "en"]))
        );
        assert_eq!(
            config::KEYS,
            &["ui.language", "claude.permission_mode", "ask.enabled"]
        );
        assert_eq!(
            config::check_setting("claude.permission_mode", " Default "),
            Ok("default".into())
        );
        assert_eq!(
            config::check_setting("claude.permission_mode", "yolo"),
            Err(SettingError::BadValue(&["bypass", "default"]))
        );
    }

    #[test]
    fn get_defaults_to_auto() {
        let mut c = Config::default();
        assert_eq!(config::get_setting(&c, "ui.language"), Ok("auto".into()));
        c.ui.language = Some("En".into());
        assert_eq!(config::get_setting(&c, "ui.language"), Ok("en".into()));
        c.ui.language = Some(String::new());
        assert_eq!(config::get_setting(&c, "ui.language"), Ok("auto".into()));
        assert_eq!(
            config::get_setting(&c, "nope"),
            Err(SettingError::UnknownKey)
        );
    }

    #[test]
    fn set_in_empty_text_adds_the_table() {
        let text = config::set_in_text("", "ui.language", "zh").unwrap();
        assert_eq!(text, "[ui]\nlanguage = \"zh\"\n");
    }

    #[test]
    fn set_keeps_comments_order_and_other_keys() {
        let before = "# top\n[ui]\nicons = \"text\" # plain\nlanguage = \"en\" # mine\n\n[aliases]\nt = \"pick\"\n";
        let after = config::set_in_text(before, "ui.language", "zh").unwrap();
        assert_eq!(after, before.replace("\"en\"", "\"zh\""));
        let added = config::set_in_text("[tmux]\nsocket = \"x\"\n", "ui.language", "en").unwrap();
        assert_eq!(added, "[tmux]\nsocket = \"x\"\n\n[ui]\nlanguage = \"en\"\n");
    }

    #[test]
    fn set_refuses_broken_text() {
        assert!(config::set_in_text("[ui\n", "ui.language", "zh").is_err());
        assert!(config::set_in_text("ui = 3\n", "ui.language", "zh").is_err());
    }

    #[test]
    fn ask_defaults_on_with_five_minutes() {
        let c = Config::default();
        assert!(c.ask.enabled);
        assert_eq!(c.ask.wait_secs, 300);
        assert_eq!(config::get_setting(&c, "ask.enabled"), Ok("true".into()));
    }

    #[test]
    fn ask_enabled_is_written_as_a_boolean() {
        assert_eq!(
            config::check_setting("ask.enabled", " FALSE "),
            Ok("false".into())
        );
        assert!(config::check_setting("ask.enabled", "no").is_err());
        let text = config::set_in_text("", "ask.enabled", "false").unwrap();
        assert_eq!(text, "[ask]\nenabled = false\n");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, format!("{text}wait_secs = 30\n")).unwrap();
        let c = Config::load_from(&path).unwrap();
        assert!(!c.ask.enabled);
        assert_eq!(c.ask.wait_secs, 30);
    }
}
