//! Picker state and key handling, independent of the terminal.
//!
//! Made for people, not for muscle memory: type to search, ↑↓ to move,
//! Enter to open, → for everything else (a small menu), Esc to go back.
//! Sessions are grouped by project (their folder): the folder is written
//! once, and each session takes one line.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::source::{Item, SessionRow, SlotRow};

/// What a session is doing, for its status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Working,
    Waiting,
    Interrupted,
    /// A tmux session without Claude Code in it.
    Plain,
}

impl State {
    pub fn of(item: &Item) -> Option<State> {
        Some(match item {
            Item::Session(s) => match &s.claude {
                Some(c) if c.busy => State::Working,
                Some(_) => State::Waiting,
                None => State::Plain,
            },
            Item::Interrupted(_) => State::Interrupted,
            Item::New => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            State::Working => "Working",
            State::Waiting => "Waiting for you",
            State::Interrupted => "Interrupted",
            State::Plain => "tmux session",
        }
    }
}

/// When the item was last active, for ordering.
pub fn activity(item: &Item) -> i64 {
    match item {
        Item::Session(s) => s.activity_ms,
        Item::Interrupted(r) => r.last_active_ms,
        Item::New => i64::MIN,
    }
}

/// Sessions sharing a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub path: String,
    pub items: Vec<Item>,
}

/// One entry of the list (a project title, or something you can select).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Header(usize),
    /// Project index, item index within it.
    Item(usize, usize),
    New,
}

impl Row {
    pub fn selectable(self) -> bool {
        !matches!(self, Row::Header(_))
    }
}

/// The choices in the → menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Open,
    Restore,
    Note,
    End,
    Remove,
}

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Choice::Open => "Open",
            Choice::Restore => "Restore",
            Choice::Note => "Write a note",
            Choice::End => "End session",
            Choice::Remove => "Remove from list",
        }
    }

    fn for_item(item: &Item) -> Vec<Choice> {
        match item {
            Item::New => vec![],
            Item::Session(s) if s.claude.is_some() => vec![Choice::Open, Choice::Note, Choice::End],
            Item::Session(_) => vec![Choice::Open, Choice::End],
            Item::Interrupted(_) => vec![Choice::Restore, Choice::Note, Choice::Remove],
        }
    }
}

/// What is on top of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Menu {
        choices: Vec<Choice>,
        cursor: usize,
    },
    /// "End <name>?" with Yes / Cancel; `yes` is the highlighted button.
    Confirm {
        yes: bool,
    },
    Note {
        text: String,
    },
}

/// What the caller should do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Attach(SessionRow),
    /// Start a session in the current directory, named by the query if any.
    New {
        name: Option<String>,
    },
    Restore(SlotRow),
    /// End a session (its tmux session and the Claude Code inside it).
    End(Item),
    /// Stop offering an interrupted session.
    Remove(SlotRow),
    SetNote {
        slot_id: String,
        text: String,
    },
}

pub struct App {
    pub query: String,
    pub mode: Mode,
    projects: Vec<Project>,
    rows: Vec<Row>,
    /// Index into `rows`; always a selectable row.
    pub cursor: usize,
    pub message: Option<String>,
    /// Matching items / all items, for the search line.
    pub matched: usize,
    pub total: usize,
    matcher: Matcher,
}

impl App {
    pub fn new(sessions: Vec<Item>, interrupted: Vec<Item>) -> Self {
        let mut app = App {
            query: String::new(),
            mode: Mode::Browse,
            projects: Vec::new(),
            rows: Vec::new(),
            cursor: 0,
            message: None,
            matched: 0,
            total: 0,
            matcher: Matcher::new(Config::DEFAULT),
        };
        app.set_items(sessions, interrupted);
        app
    }

    /// Replace the data, keeping the selection on the same item if it still exists.
    pub fn set_items(&mut self, sessions: Vec<Item>, interrupted: Vec<Item>) {
        let keep = self.selected().filter(|k| *k != Item::New);
        let mut projects: Vec<Project> = Vec::new();
        for item in sessions
            .into_iter()
            .chain(interrupted)
            .filter(|i| *i != Item::New)
        {
            let path = item.dir().unwrap_or_default().to_string();
            match projects.iter_mut().find(|p| p.path == path) {
                Some(p) => p.items.push(item),
                None => projects.push(Project {
                    path,
                    items: vec![item],
                }),
            }
        }
        // Most recently active first, for projects and for sessions in them.
        for p in &mut projects {
            p.items.sort_by_key(|i| std::cmp::Reverse(activity(i)));
        }
        projects.sort_by_key(|p| std::cmp::Reverse(p.items.first().map_or(i64::MIN, activity)));
        self.projects = projects;
        self.refilter();
        if let Some(keep) = keep {
            if let Some(i) = (0..self.rows.len()).find(|&i| self.item(self.rows[i]) == Some(&keep))
            {
                self.cursor = i;
            }
        }
    }

    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    /// How many sessions are in each state.
    pub fn count(&self, state: State) -> usize {
        self.projects
            .iter()
            .flat_map(|p| &p.items)
            .filter(|i| State::of(i) == Some(state))
            .count()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn item(&self, row: Row) -> Option<&Item> {
        match row {
            Row::Item(p, i) => self.projects.get(p)?.items.get(i),
            _ => None,
        }
    }

    pub fn selected(&self) -> Option<Item> {
        match self.rows.get(self.cursor)? {
            Row::New => Some(Item::New),
            row => self.item(*row).cloned(),
        }
    }

    /// Search runs over every project; projects keep their order, sessions in
    /// a project are ranked by score. Projects without matches disappear.
    fn refilter(&mut self) {
        let query = self.query.trim().to_string();
        let mut rows = Vec::new();
        let pattern = Pattern::parse(&query, CaseMatching::Ignore, Normalization::Smart);
        let mut buf = Vec::new();
        let (mut matched, mut total) = (0, 0);
        let mut groups: Vec<(usize, Vec<(usize, u32)>)> = Vec::new();
        for (p, project) in self.projects.iter().enumerate() {
            total += project.items.len();
            let mut hits: Vec<(usize, u32)> = project
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    if query.is_empty() {
                        return Some((i, 0));
                    }
                    let text = item.search_text();
                    pattern
                        .score(Utf32Str::new(&text, &mut buf), &mut self.matcher)
                        .map(|s| (i, s))
                })
                .collect();
            if hits.is_empty() {
                continue;
            }
            if !query.is_empty() {
                hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            }
            matched += hits.len();
            groups.push((p, hits));
        }
        // While searching, the project holding the best match comes first.
        if !query.is_empty() {
            groups.sort_by(|a, b| b.1[0].1.cmp(&a.1[0].1).then(a.0.cmp(&b.0)));
        }
        for (p, hits) in groups {
            rows.push(Row::Header(p));
            rows.extend(hits.into_iter().map(|(i, _)| Row::Item(p, i)));
        }
        // An exact name means the session exists: names are unique (T1.5).
        if query.is_empty() || !self.names_exactly(&query) {
            rows.push(Row::New);
        }
        self.rows = rows;
        self.matched = matched;
        self.total = total;
        // Land on the first (best) session, else on "new".
        self.cursor = self
            .rows
            .iter()
            .position(|r| matches!(r, Row::Item(..)))
            .or_else(|| self.rows.iter().position(|r| r.selectable()))
            .unwrap_or(0);
    }

    fn names_exactly(&self, query: &str) -> bool {
        self.projects
            .iter()
            .flat_map(|p| &p.items)
            .any(|i| match i {
                Item::Session(s) => {
                    s.claude
                        .as_ref()
                        .is_some_and(|c| c.name.as_deref() == Some(query))
                        || s.tmux_name == query
                }
                Item::Interrupted(r) => r.name.as_deref() == Some(query),
                Item::New => false,
            })
    }

    /// Move to the next selectable row, without wrapping (like a list should).
    fn move_cursor(&mut self, forward: bool) {
        let mut i = self.cursor;
        loop {
            let next = if forward {
                i + 1
            } else {
                match i.checked_sub(1) {
                    Some(n) => n,
                    None => return,
                }
            };
            match self.rows.get(next) {
                Some(r) if r.selectable() => {
                    self.cursor = next;
                    return;
                }
                Some(_) => i = next,
                None => return,
            }
        }
    }

    /// Select a row by index (mouse); true if it was already selected.
    pub fn click(&mut self, row: usize) -> bool {
        if self.mode != Mode::Browse || !self.rows.get(row).is_some_and(|r| r.selectable()) {
            return false;
        }
        let again = self.cursor == row;
        self.cursor = row;
        again
    }

    pub fn scroll(&mut self, down: bool) {
        if self.mode == Mode::Browse {
            self.move_cursor(down);
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        self.message = None;
        match self.mode.clone() {
            Mode::Browse => self.browse(key, ctrl),
            Mode::Menu { choices, cursor } => self.menu(key, choices, cursor),
            Mode::Confirm { yes } => self.confirm(key, yes),
            Mode::Note { text } => self.note(key, text),
        }
    }

    /// Enter (or a second click): open what is selected.
    pub fn open(&mut self) -> Action {
        match self.selected() {
            None => Action::None,
            Some(Item::New) => Action::New {
                name: Some(self.query.trim().to_string()).filter(|q| !q.is_empty()),
            },
            Some(Item::Session(s)) => Action::Attach(s),
            Some(Item::Interrupted(r)) => Action::Restore(r),
        }
    }

    fn browse(&mut self, key: KeyEvent, ctrl: bool) -> Action {
        match key.code {
            KeyCode::Esc if !self.query.is_empty() => {
                self.query.clear();
                self.refilter();
                Action::None
            }
            KeyCode::Esc => Action::Quit,
            KeyCode::Up => {
                self.move_cursor(false);
                Action::None
            }
            KeyCode::Down => {
                self.move_cursor(true);
                Action::None
            }
            // Not advertised; there for people who expect them.
            KeyCode::Char('p') if ctrl => {
                self.move_cursor(false);
                Action::None
            }
            KeyCode::Char('n') if ctrl => {
                self.move_cursor(true);
                Action::None
            }
            KeyCode::Right => {
                if let Some(item) = self.selected().filter(|i| *i != Item::New) {
                    self.mode = Mode::Menu {
                        choices: Choice::for_item(&item),
                        cursor: 0,
                    };
                }
                Action::None
            }
            KeyCode::Enter => self.open(),
            // Every printable character is search input.
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.query.push(c);
                self.refilter();
                Action::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter();
                Action::None
            }
            _ => Action::None,
        }
    }

    fn menu(&mut self, key: KeyEvent, choices: Vec<Choice>, cursor: usize) -> Action {
        let last = choices.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Left => {
                self.mode = Mode::Browse;
                Action::None
            }
            KeyCode::Up | KeyCode::Down => {
                let cursor = if key.code == KeyCode::Up {
                    cursor.saturating_sub(1)
                } else {
                    (cursor + 1).min(last)
                };
                self.mode = Mode::Menu { choices, cursor };
                Action::None
            }
            KeyCode::Enter => {
                self.mode = Mode::Browse;
                match (choices.get(cursor).copied(), self.selected()) {
                    (Some(Choice::Open | Choice::Restore), _) => self.open(),
                    (Some(Choice::Note), Some(item)) => {
                        let current = match &item {
                            Item::Session(s) => s.claude.as_ref().and_then(|c| c.note.clone()),
                            Item::Interrupted(r) => r.note.clone(),
                            Item::New => None,
                        };
                        self.mode = Mode::Note {
                            text: current.unwrap_or_default(),
                        };
                        Action::None
                    }
                    (Some(Choice::End), Some(_)) => {
                        // Cancel is the default: ending a session cannot be undone here.
                        self.mode = Mode::Confirm { yes: false };
                        Action::None
                    }
                    (Some(Choice::Remove), Some(Item::Interrupted(r))) => Action::Remove(r),
                    _ => Action::None,
                }
            }
            _ => Action::None,
        }
    }

    fn confirm(&mut self, key: KeyEvent, yes: bool) -> Action {
        let end = |app: &mut App| {
            app.mode = Mode::Browse;
            app.selected().map_or(Action::None, Action::End)
        };
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                self.mode = Mode::Confirm { yes: !yes };
                Action::None
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => end(self),
            KeyCode::Enter if yes => end(self),
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.mode = Mode::Browse;
                Action::None
            }
            _ => Action::None,
        }
    }

    fn note(&mut self, key: KeyEvent, mut text: String) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                Action::None
            }
            KeyCode::Enter => {
                self.mode = Mode::Browse;
                let slot_id = match self.selected() {
                    Some(Item::Session(s)) => s.claude.map(|c| c.slot_id),
                    Some(Item::Interrupted(r)) => Some(r.slot_id),
                    _ => None,
                };
                slot_id.map_or(Action::None, |slot_id| Action::SetNote {
                    slot_id,
                    text: text.trim().to_string(),
                })
            }
            KeyCode::Backspace => {
                text.pop();
                self.mode = Mode::Note { text };
                Action::None
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                text.push(c);
                self.mode = Mode::Note { text };
                Action::None
            }
            _ => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn slot(name: &str, dir: &str, at: i64) -> SlotRow {
        SlotRow {
            slot_id: format!("slot-{name}"),
            session_id: format!("{name}-id"),
            name: Some(name.into()),
            note: None,
            cwd: dir.into(),
            last_active_ms: at,
            busy: false,
            preview: None,
        }
    }

    fn claude(name: &str, dir: &str, at: i64, busy: bool) -> Item {
        let mut c = slot(name, dir, at);
        c.busy = busy;
        Item::Session(SessionRow {
            tmux_id: format!("${name}"),
            tmux_name: format!("h_{name}"),
            attached: false,
            activity_ms: at,
            path: dir.into(),
            claude: Some(c),
        })
    }

    fn tmux(name: &str, dir: &str, at: i64) -> Item {
        Item::Session(SessionRow {
            tmux_id: format!("${name}"),
            tmux_name: name.into(),
            attached: false,
            activity_ms: at,
            path: dir.into(),
            claude: None,
        })
    }

    /// acme: api (5), web (9, busy), crashed (3); billing: scratch (1)
    fn app() -> App {
        App::new(
            vec![
                claude("api", "/p/acme", 5, false),
                claude("web", "/p/acme", 9, true),
                tmux("scratch", "/p/billing", 1),
            ],
            vec![Item::Interrupted(slot("crashed", "/p/acme", 3))],
        )
    }

    fn type_str(a: &mut App, s: &str) {
        for c in s.chars() {
            assert_eq!(a.handle(key(KeyCode::Char(c))), Action::None);
        }
    }

    fn titles(a: &App) -> Vec<String> {
        a.rows()
            .iter()
            .map(|r| match r {
                Row::Header(p) => format!("# {}", a.projects()[*p].path),
                Row::New => "+".into(),
                row => a.item(*row).unwrap().title(),
            })
            .collect()
    }

    #[test]
    fn groups_by_project() {
        let a = app();
        assert_eq!(
            titles(&a),
            [
                "# /p/acme",
                "web",
                "api",
                "crashed",
                "# /p/billing",
                "scratch",
                "+"
            ]
        );
        assert_eq!(a.selected(), Some(claude("web", "/p/acme", 9, true)));
        assert_eq!(a.count(State::Working), 1);
        assert_eq!(a.count(State::Waiting), 1);
        assert_eq!(a.count(State::Interrupted), 1);
        // Search keeps only projects with matches.
        let mut b = app();
        type_str(&mut b, "scr");
        assert_eq!(titles(&b), ["# /p/billing", "scratch", "+"]);
    }

    #[test]
    fn search_ranks_the_best_match_first() {
        // "doom" must not pick a session whose long path happens to contain d…o…o…m.
        let mut a = App::new(
            vec![
                tmux("driver", "/Users/me/src/cc-moss", 9),
                claude("doomed", "/p/work", 1, false),
            ],
            vec![],
        );
        type_str(&mut a, "doom");
        assert_eq!(a.selected().unwrap().title(), "doomed");
        assert_eq!(titles(&a), ["# /p/work", "doomed", "+"]);
        // The folder's own name is still searchable.
        let mut b = app();
        type_str(&mut b, "billing");
        assert_eq!(b.selected().unwrap().title(), "scratch");
    }

    #[test]
    fn letters_go_to_the_query() {
        let mut a = app();
        type_str(&mut a, "1 2 3 d s x r yN?q");
        assert_eq!(a.query, "1 2 3 d s x r yN?q");
        assert_eq!(a.mode, Mode::Browse);
    }

    #[test]
    fn arrows_skip_headers_and_stop_at_the_ends() {
        let mut a = app();
        let mut seen = vec![];
        for _ in 0..6 {
            a.handle(key(KeyCode::Down));
            seen.push(a.selected().unwrap().title());
        }
        assert_eq!(
            seen,
            [
                "api",
                "crashed",
                "scratch",
                "New session",
                "New session",
                "New session"
            ]
        );
        for _ in 0..10 {
            a.handle(key(KeyCode::Up));
        }
        assert_eq!(a.selected().unwrap().title(), "web", "no wrap at the top");
    }

    #[test]
    fn enter_opens_what_is_selected() {
        let mut a = app();
        assert!(
            matches!(a.handle(key(KeyCode::Enter)), Action::Attach(s) if s.tmux_name == "h_web")
        );
        a.handle(key(KeyCode::Down));
        a.handle(key(KeyCode::Down));
        assert_eq!(
            a.handle(key(KeyCode::Enter)),
            Action::Restore(slot("crashed", "/p/acme", 3))
        );
        let mut b = app();
        type_str(&mut b, "brand-new");
        assert_eq!(b.selected(), Some(Item::New));
        assert_eq!(
            b.handle(key(KeyCode::Enter)),
            Action::New {
                name: Some("brand-new".into())
            }
        );
        // An exact existing name: no "new", the session itself is selected.
        let mut c = app();
        type_str(&mut c, "api");
        assert!(!c.rows().contains(&Row::New));
        assert_eq!(c.selected().unwrap().title(), "api");
    }

    #[test]
    fn esc_clears_the_search_then_quits() {
        let mut a = app();
        type_str(&mut a, "we");
        assert_eq!(a.handle(key(KeyCode::Esc)), Action::None);
        assert!(a.query.is_empty());
        assert_eq!(a.handle(key(KeyCode::Esc)), Action::Quit);
    }

    #[test]
    fn right_opens_a_menu_with_choices_per_kind() {
        let mut a = app();
        a.handle(key(KeyCode::Right));
        assert!(
            matches!(&a.mode, Mode::Menu { choices, .. } if choices == &[Choice::Open, Choice::Note, Choice::End])
        );
        a.handle(key(KeyCode::Left));
        assert_eq!(a.mode, Mode::Browse);
        a.handle(key(KeyCode::Down));
        a.handle(key(KeyCode::Down)); // crashed
        a.handle(key(KeyCode::Right));
        assert!(
            matches!(&a.mode, Mode::Menu { choices, .. } if choices == &[Choice::Restore, Choice::Note, Choice::Remove])
        );
    }

    #[test]
    fn ending_a_session_needs_an_explicit_yes() {
        let mut a = app();
        let to_end = |a: &mut App| {
            a.handle(key(KeyCode::Right));
            a.handle(key(KeyCode::Down));
            a.handle(key(KeyCode::Down));
            a.handle(key(KeyCode::Enter))
        };
        assert_eq!(to_end(&mut a), Action::None);
        assert_eq!(a.mode, Mode::Confirm { yes: false });
        assert_eq!(
            a.handle(key(KeyCode::Enter)),
            Action::None,
            "Cancel is the default"
        );
        assert_eq!(a.mode, Mode::Browse);
        to_end(&mut a);
        a.handle(key(KeyCode::Left));
        assert_eq!(
            a.handle(key(KeyCode::Enter)),
            Action::End(claude("web", "/p/acme", 9, true))
        );
    }

    #[test]
    fn notes_are_typed_in_place() {
        let mut a = app();
        a.handle(key(KeyCode::Right));
        a.handle(key(KeyCode::Down)); // Write a note
        a.handle(key(KeyCode::Enter));
        assert_eq!(
            a.mode,
            Mode::Note {
                text: String::new()
            }
        );
        type_str(&mut a, "等供应商 key");
        assert_eq!(
            a.handle(key(KeyCode::Enter)),
            Action::SetNote {
                slot_id: "slot-web".into(),
                text: "等供应商 key".into()
            }
        );
    }

    #[test]
    fn mouse_click_selects_then_opens() {
        let mut a = app();
        let api = a
            .rows()
            .iter()
            .position(|r| a.item(*r).is_some_and(|i| i.title() == "api"))
            .unwrap();
        assert!(!a.click(api));
        assert_eq!(a.selected().unwrap().title(), "api");
        assert!(a.click(api), "a second click on the selection opens it");
        assert!(!a.click(0), "project titles are not selectable");
        assert_eq!(a.selected().unwrap().title(), "api");
    }

    #[test]
    fn refresh_keeps_the_selection() {
        let mut a = app();
        a.handle(key(KeyCode::Down)); // api
        a.set_items(
            vec![
                claude("fresh", "/p/new", 99, true),
                claude("api", "/p/acme", 5, false),
            ],
            vec![],
        );
        assert_eq!(a.selected().unwrap().title(), "api");
    }
}
