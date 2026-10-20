pub mod act;
pub mod adopt;
pub mod alias;
pub mod app;
pub mod archive;
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
pub mod provider;
pub mod restart;
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

/// Why a name did not pick one session, for `--json` results: a reason
/// code (`not-found` or `ambiguous`, as `style` spells codes), the
/// message, and for `ambiguous` the candidates' id prefixes.
pub struct Unresolved {
    pub ambiguous: bool,
    pub message: String,
    pub candidates: Vec<String>,
}

impl Unresolved {
    /// The reason code in a command's own spelling (`not-found` or
    /// `not_found`).
    pub fn code(&self, snake: bool) -> &'static str {
        match (self.ambiguous, snake) {
            (true, _) => "ambiguous",
            (false, true) => "not_found",
            (false, false) => "not-found",
        }
    }
}

/// [`resolve`], keeping what the result of one session says when there is
/// not exactly one.
pub fn resolve_item<'a>(
    slots: &'a [moss_core::slot::Slot],
    name: &str,
) -> Result<&'a moss_core::slot::Slot, Unresolved> {
    match moss_core::restore::resolve(slots, name) {
        Ok(s) => Ok(s),
        Err(e) => {
            let candidates = match &e {
                moss_core::restore::ResolveError::Ambiguous { candidates, .. } => {
                    candidates.clone()
                }
                _ => Vec::new(),
            };
            let ambiguous = matches!(e, moss_core::restore::ResolveError::Ambiguous { .. });
            Err(Unresolved {
                ambiguous,
                message: resolve_message(&e, name),
                candidates,
            })
        }
    }
}

/// One session by name, tmux name or id prefix, with the reason in the
/// user's language when there is not exactly one.
pub fn resolve<'a>(
    slots: &'a [moss_core::slot::Slot],
    name: &str,
) -> anyhow::Result<&'a moss_core::slot::Slot> {
    moss_core::restore::resolve(slots, name).map_err(|e| anyhow::anyhow!(resolve_message(&e, name)))
}

fn resolve_message(e: &moss_core::restore::ResolveError, name: &str) -> String {
    use moss_core::restore::ResolveError;
    {
        let text = match e {
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
        text
    }
}
