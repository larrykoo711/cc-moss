pub mod act;
pub mod adopt;
pub mod alias;
pub mod app;
pub mod ask;
pub mod ask_hook;
pub mod attach;
pub mod checkup;
pub mod close;
pub mod config;
pub mod confirm;
pub mod daemon;
pub mod doctor;
pub mod events;
pub mod files;
pub mod hook;
pub mod install;
pub mod launch;
pub mod ls;
pub mod moss;
pub mod moss_talk;
pub mod new;
pub mod note;
pub mod restore;
pub mod send;
pub mod setup;
pub mod suggest;
pub mod update;

/// `text` padded with spaces to `width` terminal columns (Chinese takes two).
pub fn pad(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}

/// One session by name, tmux name or id prefix, with the reason in the
/// user's language when there is not exactly one.
pub fn resolve<'a>(
    slots: &'a [moss_core::slot::Slot],
    name: &str,
) -> anyhow::Result<&'a moss_core::slot::Slot> {
    use moss_core::restore::ResolveError;
    moss_core::restore::resolve(slots, name).map_err(|e| {
        let text = match &e {
            ResolveError::NotFound(_) => tr!(
                e.to_string(),
                format!("没有叫 `{name}` 的会话（看看 `cc-moss ls --all`）")
            ),
            ResolveError::Ambiguous { matches, .. } => tr!(
                e.to_string(),
                format!(
                    "`{name}` 对上了好几个会话：{}（换成 id 前缀试试）",
                    matches.join("、")
                )
            ),
        };
        anyhow::anyhow!(text)
    })
}
