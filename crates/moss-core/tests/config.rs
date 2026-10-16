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
        assert_eq!(config::KEYS, &["ui.language"]);
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
}
