//! Native aliases: one program, many names (as BusyBox does it).
//!
//! An alias is a symlink in `~/.local/bin` to the stable `cc-moss` entry on
//! `PATH`. When cc-moss runs under that name it looks the name up in the
//! `[aliases]` table of `config.toml` and runs the subcommand written there,
//! with the user's own arguments after it.
//!
//! Nothing here goes through a shell. The command text is split by
//! [`split`], a small quote-aware tokenizer that knows whitespace, quotes and
//! backslashes and nothing else: `$(…)`, `;`, `|` and friends are plain text.

use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The program's own name; never an alias.
pub const PROGRAM: &str = "cc-moss";

/// Where alias links live, under `HOME`.
pub const LINK_DIR: &str = ".local/bin";

/// Longest alias name accepted.
const MAX_NAME: usize = 64;

/// `~/.local/bin`.
pub fn link_dir(home: &Path) -> PathBuf {
    home.join(LINK_DIR)
}

/// Whether the program was started under its own name rather than an alias:
/// `cc-moss`, or a release build's longer name such as `cc-moss-aarch64`.
pub fn is_own_name(name: &str) -> bool {
    name == PROGRAM || name.starts_with("cc-moss-") || name.starts_with("cc-moss.")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// `cc-moss` itself, or a name it would take for its own.
    Reserved,
    /// Something other than `a-z`, `0-9`, `-` and `_`.
    BadChar(char),
    /// Starts with `-`, which commands would read as an option.
    LeadingDash,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Empty => write!(f, "an alias needs a name"),
            NameError::TooLong => write!(f, "alias names are at most {MAX_NAME} characters"),
            NameError::Reserved => write!(f, "`cc-moss` names the program itself"),
            NameError::BadChar(c) => write!(f, "`{c}` is not allowed: use a-z, 0-9, - and _"),
            NameError::LeadingDash => write!(f, "an alias name cannot start with `-`"),
        }
    }
}

impl std::error::Error for NameError {}

/// Alias names are `a-z0-9-_`, not starting with `-`, and never `cc-moss`.
/// This is also what makes a name safe to put in a path or a shell word.
pub fn check_name(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.len() > MAX_NAME {
        return Err(NameError::TooLong);
    }
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_')))
    {
        return Err(NameError::BadChar(c));
    }
    if name.starts_with('-') {
        return Err(NameError::LeadingDash);
    }
    if is_own_name(name) {
        return Err(NameError::Reserved);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitError {
    /// A quote (`'` or `"`) that is never closed.
    Unclosed(char),
    /// A `\` with nothing after it.
    TrailingBackslash,
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplitError::Unclosed(q) => write!(f, "the {q} quote is never closed"),
            SplitError::TrailingBackslash => write!(f, "it ends with a lone \\"),
        }
    }
}

impl std::error::Error for SplitError {}

/// Split command text into words. Whitespace separates words; `'…'` keeps
/// everything literally; `"…"` too, except that `\"` and `\\` stand for `"`
/// and `\`; outside quotes `\` takes the next character literally. Quoted
/// parts next to each other form one word, and `''` is an empty word.
/// Nothing else is special: there is no expansion of any kind.
pub fn split(text: &str) -> Result<Vec<String>, SplitError> {
    let mut words = Vec::new();
    let mut word = String::new();
    // True once the current word has begun, even if it is still empty ('').
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(SplitError::Unclosed('\'')),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(SplitError::Unclosed('"')),
                        },
                        Some(c) => word.push(c),
                        None => return Err(SplitError::Unclosed('"')),
                    }
                }
            }
            '\\' => {
                started = true;
                match chars.next() {
                    Some(c) => word.push(c),
                    None => return Err(SplitError::TrailingBackslash),
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// Words back into command text that [`split`] turns into the same words.
pub fn join(words: &[String]) -> String {
    words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && !word
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\'));
    if plain {
        word.to_string()
    } else if !word.contains('\'') {
        format!("'{word}'")
    } else {
        format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    Split(SplitError),
    Empty,
    /// `hook` is for Claude Code, never for people.
    Hook,
    /// The first word is not a cc-moss command.
    Unknown(String),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Split(e) => write!(f, "cannot read the command: {e}"),
            CommandError::Empty => write!(f, "the command is empty"),
            CommandError::Hook => write!(f, "`hook` is only for Claude Code"),
            CommandError::Unknown(w) => write!(f, "`{w}` is not a cc-moss command"),
        }
    }
}

impl std::error::Error for CommandError {}

/// The words an alias's command stands for, checked: the first word must be
/// one of `subcommands` (cc-moss's own commands) and never `hook`.
pub fn expand(command: &str, subcommands: &[&str]) -> Result<Vec<String>, CommandError> {
    let words = split(command).map_err(CommandError::Split)?;
    let Some(first) = words.first() else {
        return Err(CommandError::Empty);
    };
    if first == "hook" {
        return Err(CommandError::Hook);
    }
    if !subcommands.contains(&first.as_str()) {
        return Err(CommandError::Unknown(first.clone()));
    }
    Ok(words)
}

/// The `cc-moss` that links and Claude Code hooks should run: one that
/// survives upgrades. A package manager's versioned path
/// (`…/Cellar/cc-moss/0.1.1/bin/cc-moss`) is deleted by the next upgrade; its
/// `bin/cc-moss` link is not. Looks on `PATH` first (an entry is used as
/// written), then next to a `Cellar`, and takes the first `cc-moss` that is
/// the same file as `current`. Falls back to the real file `current`
/// resolves to: on macOS `current_exe()` is the path the program was started
/// by, which under an alias is the alias's own link.
pub fn stable_entry(current: &Path, path_var: Option<&OsStr>) -> PathBuf {
    let Ok(real) = std::fs::canonicalize(current) else {
        return current.to_path_buf();
    };
    let on_path = path_var
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(PROGRAM));
    let by_cellar: Vec<PathBuf> = [current, real.as_path()]
        .into_iter()
        .filter_map(cellar_prefix)
        .map(|prefix| prefix.join("bin").join(PROGRAM))
        .collect();
    let found = on_path
        .chain(by_cellar)
        .filter(|c| cellar_prefix(c).is_none())
        .find(|c| std::fs::canonicalize(c).is_ok_and(|c| c == real));
    found.unwrap_or(real)
}

/// The directory holding a `Cellar` component of `path`, if any.
fn cellar_prefix(path: &Path) -> Option<PathBuf> {
    let mut prefix = PathBuf::new();
    for part in path.components() {
        if part == Component::Normal(OsStr::new("Cellar")) {
            return Some(prefix);
        }
        prefix.push(part);
    }
    None
}

/// What is at an alias's place in `~/.local/bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Our link, pointing at the stable entry.
    Ok,
    /// Nothing there.
    Missing,
    /// Our link, but what it points at is gone or cannot run.
    Broken,
    /// Our link, working, but pointing somewhere other than the stable entry
    /// (a versioned path the next upgrade removes, an older install).
    Stale,
    /// A file, directory or link that is not ours: never touched.
    Foreign,
}

impl LinkState {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkState::Ok => "ok",
            LinkState::Missing => "missing",
            LinkState::Broken => "broken",
            LinkState::Stale => "stale",
            LinkState::Foreign => "foreign",
        }
    }
}

/// Whether `path` is a link cc-moss made: a symlink to a file named
/// `cc-moss`, whether or not that file still exists.
pub fn is_ours(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
        && std::fs::read_link(path)
            .is_ok_and(|target| target.file_name() == Some(OsStr::new(PROGRAM)))
}

pub fn link_state(path: &Path, entry: &Path) -> LinkState {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return LinkState::Missing,
        Err(_) => return LinkState::Foreign,
        Ok(_) if !is_ours(path) => return LinkState::Foreign,
        Ok(_) => {}
    }
    let runs =
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if !runs {
        LinkState::Broken
    } else if std::fs::read_link(path).is_ok_and(|t| t == entry) {
        LinkState::Ok
    } else {
        LinkState::Stale
    }
}

/// Make `path` a link to `entry`. Creates the directory if needed and
/// replaces only a link of ours; anything else at `path` is an error and
/// stays exactly as it was. The link appears atomically (made aside, then
/// renamed into place), so there is never a moment without one.
pub fn make_link(path: &Path, entry: &Path) -> Result<()> {
    match link_state(path, entry) {
        LinkState::Ok => return Ok(()),
        LinkState::Foreign => bail!(
            "{} exists and is not a cc-moss link; leaving it alone",
            path.display()
        ),
        LinkState::Missing | LinkState::Broken | LinkState::Stale => {}
    }
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .context("an alias link needs a directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = (0..1000)
        .map(|n| dir.join(format!(".{name}.cc-moss-{}-{n}.tmp", std::process::id())))
        .find(|candidate| match symlink(entry, candidate) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => false,
            Err(_) => false,
        })
        .with_context(|| format!("cannot create a link in {}", dir.display()))?;
    // Look again right before replacing: only nothing, or a link of ours.
    let free = matches!(std::fs::symlink_metadata(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound)
        || is_ours(path);
    if !free {
        let _ = std::fs::remove_file(&tmp);
        bail!(
            "{} exists and is not a cc-moss link; leaving it alone",
            path.display()
        );
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        anyhow::Error::new(e).context(format!("cannot create {}", path.display()))
    })
}

/// Remove `path` if it is a link of ours. `Ok(false)` when there was nothing
/// to remove or it is not ours (then it is left alone).
pub fn remove_link(path: &Path) -> io::Result<bool> {
    if !is_ours(path) {
        return Ok(false);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// `text` (a `config.toml`) with `[aliases] name = command`, comments and
/// everything else kept as written. Refuses text that is not valid TOML.
pub fn set_in_config(text: &str, name: &str, command: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("config.toml is not valid TOML")?;
    let table = doc
        .entry("aliases")
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .context("`aliases` in config.toml is not a table")?;
    let mut value = toml_edit::Value::from(command);
    if let Some(old) = table.get(name).and_then(toml_edit::Item::as_value) {
        *value.decor_mut() = old.decor().clone();
    }
    table.insert(name, toml_edit::Item::Value(value));
    Ok(doc.to_string())
}

/// The `[aliases]` entries in `text` that are not strings, which loading
/// the config skips. Empty when the text does not parse.
pub fn ignored_entries(text: &str) -> Vec<String> {
    let Ok(doc) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    match doc.get("aliases") {
        Some(toml::Value::Table(t)) => t
            .iter()
            .filter(|(_, v)| !v.is_str())
            .map(|(k, _)| k.clone())
            .collect(),
        _ => Vec::new(),
    }
}

/// `text` without `[aliases] name`, and whether it was there.
pub fn remove_from_config(text: &str, name: &str) -> Result<(String, bool)> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("config.toml is not valid TOML")?;
    let Some(item) = doc.get_mut("aliases") else {
        return Ok((text.to_string(), false));
    };
    let table = item
        .as_table_like_mut()
        .context("`aliases` in config.toml is not a table")?;
    if table.remove(name).is_none() {
        return Ok((text.to_string(), false));
    }
    Ok((doc.to_string(), true))
}

/// What hides an alias in the user's shell: shells look up their own
/// aliases, functions and builtins before `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shadow {
    /// The shell runs our link.
    None,
    Alias,
    Function,
    Builtin,
    /// Another program with the same name comes first on `PATH`.
    Command(PathBuf),
    /// The shell cannot find it at all (`~/.local/bin` is not on its `PATH`).
    NotFound,
    /// The shell's answer could not be read.
    Unknown,
}

impl Shadow {
    pub fn as_str(&self) -> &'static str {
        match self {
            Shadow::None => "none",
            Shadow::Alias => "alias",
            Shadow::Function => "function",
            Shadow::Builtin => "builtin",
            Shadow::Command(_) => "command",
            Shadow::NotFound => "not_found",
            Shadow::Unknown => "unknown",
        }
    }
}

/// Separates the answers in [`type_script`]'s output.
const MARK: &str = "__cc_moss_type__";

/// A script asking the shell what each name is, one `type` per name. Names
/// that fail [`check_name`] are left out: only `a-z0-9-_` ever reaches it.
pub fn type_script(names: &[&str]) -> String {
    let mut script = format!("echo {MARK}");
    for name in names.iter().filter(|n| check_name(n).is_ok()) {
        script.push_str(&format!("; type {name} 2>&1; echo {MARK}"));
    }
    script
}

/// [`type_script`]'s output read back: one answer per name, in order. What
/// the shell's startup files print before the first mark is ignored.
pub fn parse_type_output(output: &str, names: &[&str], dir: &Path) -> Vec<(String, Shadow)> {
    let names: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| check_name(n).is_ok())
        .collect();
    let sections: Vec<&str> = output.split(MARK).skip(1).collect();
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let shadow = match sections.get(i) {
                // A section is complete only when a mark follows it.
                Some(text) if i + 1 < sections.len() => parse_type(text, &dir.join(name)),
                _ => Shadow::Unknown,
            };
            (name.to_string(), shadow)
        })
        .collect()
}

/// What one `type <name>` says, for bash, zsh, fish and sh. Only the words
/// after `<name> is ` are read: the name and the path can contain any of
/// the words the answer is made of (`myalias`, `/home/function/…`).
pub fn parse_type(text: &str, link: &Path) -> Shadow {
    let Some(first) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return Shadow::Unknown;
    };
    let name = link
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(answer) = first.strip_prefix(&format!("{name} is ")) else {
        // `t not found` (zsh), `bash: type: t: not found`, `type: Could not
        // find 't'` (fish).
        let line = first.to_lowercase();
        return if line.ends_with("not found") || line.contains("could not find") {
            Shadow::NotFound
        } else {
            Shadow::Unknown
        };
    };
    let lower = answer.to_lowercase();
    let starts = |words: &[&str]| words.iter().any(|w| lower.starts_with(w));
    if starts(&["aliased to", "an alias"]) {
        return Shadow::Alias;
    }
    if starts(&["a function", "a shell function"]) {
        return Shadow::Function;
    }
    if starts(&[
        "a shell builtin",
        "a builtin",
        "a reserved word",
        "a shell keyword",
        "a keyword",
    ]) {
        return Shadow::Builtin;
    }
    // `/path/t`, or bash's `hashed (/path/t)`.
    let path = answer
        .strip_prefix("hashed (")
        .and_then(|p| p.strip_suffix(')'))
        .unwrap_or(answer);
    if !path.starts_with('/') {
        return Shadow::Unknown;
    }
    let p = Path::new(path);
    // The same directory may be spelled differently (a symlinked home).
    let dir = |p: &Path| p.parent().and_then(|d| std::fs::canonicalize(d).ok());
    let same =
        p == link || (p.file_name() == link.file_name() && dir(p).is_some() && dir(p) == dir(link));
    if same {
        Shadow::None
    } else {
        Shadow::Command(p.to_path_buf())
    }
}
