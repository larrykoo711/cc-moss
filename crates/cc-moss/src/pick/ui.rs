//! Drawing the picker: a quiet list on the left, the selected session's
//! details on the right.
//!
//! The list carries only what tells sessions apart (a status dot, a name, how
//! long ago); folders appear once, as group titles. Everything else about a
//! session (state, note, last message, folder, what you can do) is shown for
//! the selected one only, in the card on the right. Nothing that is zero or
//! repeated is drawn.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};

use crate::i18n::Lang;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::app::{App, Choice, Mode, Row, State};
use super::source::Item;

/// Secondary text.
const GREY: Color = Color::Rgb(150, 150, 150);
/// Quiet labels, rules and separators.
const FAINT: Color = Color::Rgb(95, 95, 95);
/// The selection band.
pub const BAND: Color = Color::Rgb(52, 52, 56);
/// MOSS's red eye; interrupted sessions.
const RED: Color = Color::Rgb(232, 86, 86);
/// Claude Code at work; highlights.
const ORANGE: Color = Color::Rgb(215, 119, 87);
/// Waiting for you; notes.
const AMBER: Color = Color::Rgb(240, 186, 70);

/// Working: four dots running along a figure eight (the cc-moss mark, ∞,
/// stood upright) inside one braille cell.
const SPINNER: [&str; 8] = ["⠛", "⠝", "⡜", "⣔", "⣤", "⣢", "⢣", "⠫"];
/// The cc-moss mark, ∞, drawn with braille dots across two cells so it is as
/// tall as the text. While sessions are being checked, five dots run along it.
const MARK: &str = "⢎⡱";
/// The figure eight through the 4×4 dots of two braille cells: around the
/// left loop, across the middle, around the right loop and back.
const MARK_PATH: [(u8, u8); 8] = [
    (1, 0),
    (0, 1),
    (0, 2),
    (1, 3),
    (2, 0),
    (3, 1),
    (3, 2),
    (2, 3),
];
/// Brand colour of the mark (#FF5A3C).
const BRAND: Color = Color::Rgb(255, 90, 60);
/// Nerd Font "file-tree", shown next to the word "worktree".
const WORKTREE_ICON: &str = "\u{f0645}";
/// Below this width the card goes under the list.
pub const SPLIT_MIN_WIDTH: u16 = 90;

/// Everything the view needs besides the app state.
pub struct View<'a> {
    pub now_ms: i64,
    pub home: Option<&'a str>,
    /// This machine's tmux name prefix, hidden in names.
    pub host_prefix: Option<&'a str>,
    /// Where a new session would start.
    pub cwd: &'a str,
    pub version: &'a str,
    /// The terminal font has Nerd Font icons.
    pub nerd_icons: bool,
    /// Running sessions are still being checked in the background.
    pub checking: bool,
}

impl View<'_> {
    /// "worktree", with its icon when the font has one.
    fn worktree_word(&self) -> String {
        if self.nerd_icons {
            format!("{WORKTREE_ICON} worktree")
        } else {
            "worktree".into()
        }
    }
}

/// Which list row each screen line shows, for mouse clicks.
#[derive(Default)]
pub struct HitMap {
    pub lines: Vec<(u16, usize)>,
}

impl HitMap {
    pub fn row_at(&self, y: u16) -> Option<usize> {
        self.lines
            .iter()
            .find(|(line, _)| *line == y)
            .map(|(_, r)| *r)
    }
}

pub fn draw(frame: &mut Frame, app: &App, view: &View) -> HitMap {
    let area = frame.area();
    let [header, search, rule1, body, rule2, hint] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(frame, header, app, view);
    draw_search(frame, search, app);
    rule(frame, rule1);
    let hits = if area.width >= SPLIT_MIN_WIDTH {
        let list_w = (area.width * 2 / 5).clamp(34, 48);
        let [list, gap, card] = Layout::horizontal([
            Constraint::Length(list_w),
            Constraint::Length(3),
            Constraint::Min(20),
        ])
        .areas(body);
        let divider: Vec<Line> = (0..gap.height)
            .map(|_| Line::styled(" │ ", Style::new().fg(FAINT)))
            .collect();
        frame.render_widget(Paragraph::new(divider), gap);
        let hits = draw_list(frame, list, app, view);
        draw_card(frame, card, app, view);
        hits
    } else {
        // Room for the whole card: title, state, a section, folder, actions.
        let card_h = 12.min(body.height * 3 / 5);
        let [list, sep, card] = Layout::vertical([
            Constraint::Min(2),
            Constraint::Length(1),
            Constraint::Length(card_h),
        ])
        .areas(body);
        let hits = draw_list(frame, list, app, view);
        rule(frame, sep);
        draw_card(frame, card, app, view);
        hits
    };
    rule(frame, rule2);
    draw_hint(frame, hint, app);
    hits
}

fn grey() -> Style {
    Style::new().fg(GREY)
}

fn rule(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Line::styled("─".repeat(area.width as usize), Style::new().fg(FAINT)),
        area,
    );
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, view: &View) {
    let left = Line::from(vec![
        Span::styled(
            format!(" {} ", mark(view.checking, view.now_ms)),
            Style::new().fg(BRAND).add_modifier(Modifier::BOLD),
        ),
        Span::styled("cc-moss", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {}", view.version), Style::new().fg(FAINT)),
    ]);
    // Only what is there: no "0 working".
    let mut parts: Vec<Span> = Vec::new();
    for (state, color) in [
        (State::Interrupted, RED),
        (State::Working, ORANGE),
        (State::Waiting, AMBER),
    ] {
        let n = app.count(state);
        if n > 0 {
            let words = match state {
                State::Interrupted => tr!(format!("{n} interrupted"), format!("{n} 个已中断")),
                State::Working => tr!(format!("{n} working"), format!("{n} 个在忙")),
                _ => tr!(format!("{n} waiting for you"), format!("{n} 个在等你")),
            };
            parts.push(Span::styled(words, Style::new().fg(color)));
            parts.push(Span::styled("  ·  ", Style::new().fg(FAINT)));
        }
    }
    let total = app.total;
    parts.push(Span::styled(
        tr!(
            format!("{total} session{} ", if total == 1 { "" } else { "s" }),
            format!("共 {total} 个会话 "),
        ),
        grey(),
    ));
    let [line, notice] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    frame.render_widget(left, line);
    frame.render_widget(Line::from(parts).right_aligned(), line);
    if let Some(v) = &app.update {
        frame.render_widget(
            Line::styled(
                tr!(
                    format!(" MOSS has grown a little: cc-moss {v} is out. Pick the first line to update."),
                    format!(" MOSS 又长大了一点：cc-moss {v} 发布了，选第一行就能升级。")
                ),
                Style::new().fg(AMBER),
            ),
            notice,
        );
    }
}

fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    let typing = !app.query.is_empty();
    let line = if typing {
        Line::from(vec![
            Span::styled(" ⌕ ", Style::new().fg(ORANGE)),
            Span::raw(app.query.clone()),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ⌕ ", grey()),
            Span::styled(
                tr!(
                    "Find a session, or type a name to start one",
                    "搜一个会话，或者输入名字开个新的"
                ),
                Style::new().fg(FAINT),
            ),
        ])
    };
    frame.render_widget(line, area);
    if typing {
        frame.render_widget(
            Line::styled(
                tr!(
                    format!("{} of {} ", app.matched, app.total),
                    format!("{} / {} ", app.matched, app.total)
                ),
                grey(),
            )
            .right_aligned(),
            area,
        );
    }
    if app.mode == Mode::Browse {
        let x = area.x.saturating_add(3).saturating_add(cols(&app.query));
        frame.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
    }
}

/// One rendered list line and the row it belongs to.
struct ListLine {
    line: Line<'static>,
    row: Option<usize>,
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App, view: &View) -> HitMap {
    let width = area.width as usize;
    let labels = project_labels(app, view);
    let mut lines: Vec<ListLine> = Vec::new();
    let mut cursor_line = 0;
    for (i, row) in app.rows().iter().enumerate() {
        match row {
            Row::Header(p) => {
                if !lines.is_empty() {
                    lines.push(ListLine {
                        line: Line::raw(""),
                        row: None,
                    });
                }
                let mut title = vec![Span::styled(
                    format!(" {}", labels[*p]),
                    Style::new().fg(GREY).add_modifier(Modifier::BOLD),
                )];
                if let Some(wt) = &app.projects()[*p].worktree {
                    title.push(Span::styled(
                        format!("  {} · {}", view.worktree_word(), basename(&wt.repo)),
                        Style::new().fg(FAINT),
                    ));
                }
                lines.push(ListLine {
                    line: Line::from(title),
                    row: None,
                });
            }
            Row::New => {
                let q = app.query.trim();
                let what = if q.is_empty() {
                    tr!("New session here", "在这里开个新会话").to_string()
                } else {
                    tr!(format!("New session \"{q}\""), format!("新会话「{q}」"))
                };
                lines.push(ListLine {
                    line: Line::from(vec![
                        Span::styled("  + ", Style::new().fg(ORANGE).add_modifier(Modifier::BOLD)),
                        Span::styled(what, grey()),
                    ]),
                    row: Some(i),
                });
            }
            Row::Update => {
                let v = app.update.as_deref().unwrap_or_default();
                lines.push(ListLine {
                    line: Line::from(vec![
                        Span::styled("  ↑ ", Style::new().fg(ORANGE).add_modifier(Modifier::BOLD)),
                        Span::styled(
                            tr!(format!("Update to {v}"), format!("升级到 {v}")),
                            Style::new().fg(ORANGE),
                        ),
                    ]),
                    row: Some(i),
                });
            }
            Row::Item(..) => {
                let item = app.item(*row).expect("item row");
                lines.push(ListLine {
                    line: list_row(item, app, view, width, i == app.cursor),
                    row: Some(i),
                });
            }
        }
        if i == app.cursor {
            cursor_line = lines.len() - 1;
            let last = lines.last_mut().expect("just pushed");
            last.line = band(std::mem::take(&mut last.line), width);
        }
    }

    let height = area.height as usize;
    let top = (cursor_line + 1).saturating_sub(height);
    let mut hits = HitMap::default();
    let shown: Vec<Line> = lines
        .into_iter()
        .skip(top)
        .take(height)
        .enumerate()
        .map(|(n, l)| {
            if let Some(r) = l.row {
                hits.lines.push((area.y + n as u16, r));
            }
            l.line
        })
        .collect();
    frame.render_widget(Paragraph::new(shown), area);
    hits
}

/// Folder names for the group titles; the full (short) path where two
/// projects share a folder name.
/// The header mark: whole when idle, five running dots while checking.
fn mark(checking: bool, now_ms: i64) -> String {
    if !checking {
        return MARK.to_string();
    }
    let start = (now_ms / 120).rem_euclid(MARK_PATH.len() as i64) as usize;
    let mut cells = [0u32; 2];
    for k in 0..5 {
        let (col, row) = MARK_PATH[(start + k) % MARK_PATH.len()];
        let bit = match (col % 2, row) {
            (0, 3) => 0x40,
            (1, 3) => 0x80,
            (0, r) => 1 << r,
            (_, r) => 8 << r,
        };
        cells[usize::from(col / 2)] |= bit;
    }
    cells
        .iter()
        .filter_map(|c| char::from_u32(0x2800 + c))
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path)
}

fn project_labels(app: &App, view: &View) -> Vec<String> {
    let base = |p: &str| {
        let t = tilde(p, view.home);
        t.rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&t)
            .to_string()
    };
    let bases: Vec<String> = app.projects().iter().map(|p| base(&p.path)).collect();
    app.projects()
        .iter()
        .zip(&bases)
        .map(|(p, b)| {
            if bases.iter().filter(|x| *x == b).count() > 1 {
                short_path(&p.path, view.home)
            } else {
                b.clone()
            }
        })
        .collect()
}

fn dot(state: State, now_ms: i64) -> Span<'static> {
    match state {
        State::Working => {
            let frame = (now_ms / 120).rem_euclid(SPINNER.len() as i64) as usize;
            Span::styled(SPINNER[frame], Style::new().fg(ORANGE))
        }
        State::Waiting => Span::styled("●", Style::new().fg(AMBER)),
        State::Interrupted => Span::styled("↺", Style::new().fg(RED)),
        State::Plain => Span::styled("○", Style::new().fg(FAINT)),
    }
}

/// The Claude Code name, else the tmux session name: never a bare id while
/// a readable name exists.
fn display_name(item: &Item, view: &View) -> String {
    match item {
        Item::Session(s) if s.claude.as_ref().is_none_or(|c| c.name.is_none()) => {
            strip_prefix(&s.tmux_name, view.host_prefix)
        }
        other => other.title(),
    }
}

/// `● name ……… 3m`: one line per session.
fn list_row(item: &Item, app: &App, view: &View, width: usize, selected: bool) -> Line<'static> {
    let state = State::of(item).unwrap_or(State::Plain);
    let name = display_name(item, view);
    let age = short_ago(view.now_ms - super::app::activity(item));
    let room = width.saturating_sub(5 + age.width() + 2);
    let mut spans = vec![Span::raw("  "), dot(state, view.now_ms), Span::raw(" ")];
    let name_style = if selected {
        Style::new().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    spans.extend(highlighted(&fit(&name, room), &app.query, name_style));
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    spans.push(Span::raw(
        " ".repeat(width.saturating_sub(used + age.width() + 1)),
    ));
    spans.push(Span::styled(age, Style::new().fg(FAINT)));
    Line::from(spans)
}

/// The name with the characters matching the search picked out.
fn highlighted(name: &str, query: &str, base: Style) -> Vec<Span<'static>> {
    let plain = || vec![Span::styled(name.to_string(), base)];
    if query.trim().is_empty() {
        return plain();
    }
    let pattern = Pattern::parse(query.trim(), CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut idx = Vec::new();
    if pattern
        .indices(Utf32Str::new(name, &mut buf), &mut matcher, &mut idx)
        .is_none()
    {
        return plain();
    }
    name.chars()
        .enumerate()
        .map(|(i, ch)| {
            let style = if idx.contains(&(i as u32)) {
                base.fg(ORANGE).add_modifier(Modifier::BOLD)
            } else {
                base
            };
            Span::styled(ch.to_string(), style)
        })
        .collect()
}

/// The selection: an accent bar and a soft band across the list column.
fn band(line: Line<'static>, width: usize) -> Line<'static> {
    let mut spans: Vec<Span> = line.spans;
    if let Some(first) = spans.first_mut() {
        if first.content.starts_with(' ') {
            let rest: String = first.content.chars().skip(1).collect();
            *first = Span::raw(rest);
            spans.insert(0, Span::styled("▌", Style::new().fg(ORANGE)));
        }
    }
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    Line::from(
        spans
            .into_iter()
            .map(|s| {
                let style = s.style.bg(BAND);
                s.style(style)
            })
            .collect::<Vec<_>>(),
    )
}

/// The detail card for the selected item.
fn draw_card(frame: &mut Frame, area: Rect, app: &App, view: &View) {
    if app.on_update() {
        draw_update_card(frame, area, app, view);
        return;
    }
    let Some(item) = app.selected() else {
        return;
    };
    let label = |t: &str| {
        Line::styled(
            t.to_uppercase(),
            Style::new().fg(FAINT).add_modifier(Modifier::BOLD),
        )
    };
    let key = |k: &str| Span::styled(k.to_string(), Style::new().add_modifier(Modifier::BOLD));
    let word = |w: &str| Span::styled(w.to_string(), grey());
    let mut lines: Vec<Line> = Vec::new();

    match &item {
        Item::New => {
            let q = app.query.trim();
            lines.push(Line::styled(
                if q.is_empty() {
                    tr!("New session", "新会话").to_string()
                } else {
                    tr!(format!("New session \"{q}\""), format!("新会话「{q}」"))
                },
                Style::new().add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::styled(
                tr!(
                    "A fresh Claude Code, in a tmux session of its own.",
                    "一个全新的 Claude Code，住在自己的 tmux 会话里。"
                ),
                grey(),
            ));
            lines.push(Line::raw(""));
            lines.push(label(tr!("Folder", "目录")));
            lines.push(Line::raw(tilde(view.cwd, view.home)));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![key("↵"), word(tr!(" Start", " 开始"))]));
        }
        _ => {
            let state = State::of(&item).unwrap_or(State::Plain);
            let color = match state {
                State::Working => ORANGE,
                State::Waiting => AMBER,
                State::Interrupted => RED,
                State::Plain => GREY,
            };
            lines.push(Line::styled(
                display_name(&item, view),
                Style::new().add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(vec![
                dot(state, view.now_ms),
                Span::styled(format!(" {}", state.label()), Style::new().fg(color)),
                Span::styled(
                    format!(
                        "  ·  {}",
                        long_ago(view.now_ms - super::app::activity(&item))
                    ),
                    grey(),
                ),
            ]));
            if state == State::Interrupted {
                lines.push(Line::styled(
                    tr!(
                        "The machine blinked. The conversation didn't.",
                        "机器打了个盹，对话还在。"
                    ),
                    grey(),
                ));
            }
            let (note, preview) = match &item {
                Item::Session(s) => (
                    s.claude.as_ref().and_then(|c| c.note.clone()),
                    s.claude.as_ref().and_then(|c| c.preview.clone()),
                ),
                Item::Interrupted(r) => (r.note.clone(), r.preview.clone()),
                Item::New => (None, None),
            };
            if let Some(n) = note {
                lines.push(Line::raw(""));
                lines.push(label(tr!("Note", "备注")));
                lines.push(Line::styled(n, Style::new().fg(AMBER)));
            }
            if let Some(p) = preview {
                lines.push(Line::raw(""));
                lines.push(label(tr!("Last message", "最后一句")));
                lines.push(Line::raw(format!("“{p}”")));
            }
            if state == State::Plain {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    tr!(
                        "Just a plain tmux session, no Claude inside.",
                        "普通的 tmux 会话，里面没有 Claude。"
                    ),
                    grey(),
                ));
            }
            lines.push(Line::raw(""));
            lines.push(label(tr!("Folder", "目录")));
            let dir = item.dir().unwrap_or_default();
            let worktree = app
                .projects()
                .iter()
                .find(|p| p.path == dir)
                .and_then(|p| p.worktree.clone());
            match worktree {
                Some(wt) => {
                    lines.push(Line::raw(tilde(&wt.repo, view.home)));
                    let inside = dir.strip_prefix(wt.root.as_str()).unwrap_or_default();
                    lines.push(Line::styled(
                        format!("{} {}{inside}", view.worktree_word(), basename(&wt.root)),
                        grey(),
                    ));
                }
                None => lines.push(Line::raw(tilde(dir, view.home))),
            }
            lines.push(Line::raw(""));
            // An open menu, question or note box takes this line's place.
            if matches!(app.mode, Mode::Browse) {
                let open = if state == State::Interrupted {
                    tr!(" Restore", " 恢复")
                } else {
                    tr!(" Open", " 打开")
                };
                lines.push(Line::from(vec![
                    key("↵"),
                    word(open),
                    Span::raw("     "),
                    key("→"),
                    word(tr!(" More", " 更多")),
                ]));
            }
        }
    }
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let card = Paragraph::new(lines).wrap(Wrap { trim: false });
    // Rows the card's text takes once wrapped: the box opens right below it.
    let used = u16::try_from(card.line_count(inner.width)).unwrap_or(u16::MAX);
    frame.render_widget(card, inner);
    draw_overlay(frame, inner, used, app, &item, view);
}

/// The card for "Update to X.Y.Z".
fn draw_update_card(frame: &mut Frame, area: Rect, app: &App, view: &View) {
    let v = app.update.as_deref().unwrap_or_default();
    let notes = moss_core::update::DEFAULT_RELEASES_URL.trim_start_matches("https://");
    let lines = vec![
        Line::styled(
            tr!(format!("Update to {v}"), format!("升级到 {v}")),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            tr!(
                format!("You have {}. Enter leaves the picker and asks once more before anything changes.", view.version),
                format!("你现在用的是 {}。回车会先退出选择器，动手前还会再问你一次。", view.version)
            ),
            grey(),
        ),
        Line::raw(""),
        Line::styled(
            tr!("What's new", "更新了什么").to_uppercase(),
            Style::new().fg(FAINT).add_modifier(Modifier::BOLD),
        ),
        Line::raw(format!("{notes}/tag/v{v}")),
        Line::raw(""),
        Line::from(vec![
            Span::styled("↵", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(tr!(" Update", " 升级"), grey()),
        ]),
        Line::raw(""),
        Line::styled(
            tr!(
                "Rather not hear about new versions? Put `[update] check = false` in config.toml.",
                "不想收到新版本提醒？在 config.toml 里写上 `[update] check = false`。"
            ),
            Style::new().fg(FAINT),
        ),
    ];
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Menu, confirmation and note boxes, right below the card's text (`used`
/// rows), where the "↵ Open  → More" line was; pushed up only when the
/// terminal is too short to fit them there.
fn draw_overlay(frame: &mut Frame, card: Rect, used: u16, app: &App, item: &Item, view: &View) {
    let title = display_name(item, view);
    let place = |h: u16| {
        let w = card.width.min(48);
        let h = h.min(card.height);
        let below = card.y.saturating_add(used);
        let y = below.min(card.bottom().saturating_sub(h));
        Rect::new(card.x, y, w, h)
    };
    let boxed = |t: String| {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ORANGE))
            .title(Span::styled(
                format!(" {t} "),
                Style::new().add_modifier(Modifier::BOLD),
            ))
    };
    match &app.mode {
        Mode::Browse => {}
        Mode::Menu { choices, cursor } => {
            let area = place(choices.len() as u16 + 2);
            let lines: Vec<Line> = choices
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let danger = matches!(c, Choice::End | Choice::Remove);
                    let base = if danger {
                        Style::new().fg(RED)
                    } else {
                        Style::new()
                    };
                    if i == *cursor {
                        Line::from(vec![
                            Span::styled(" ❯ ", Style::new().fg(ORANGE)),
                            Span::styled(c.label(), base.add_modifier(Modifier::BOLD)),
                        ])
                        .style(Style::new().bg(BAND))
                    } else {
                        Line::from(vec![Span::raw("   "), Span::styled(c.label(), base)])
                    }
                })
                .collect();
            frame.render_widget(Clear, area);
            frame.render_widget(Paragraph::new(lines).block(boxed(title)), area);
        }
        Mode::Confirm { yes } => {
            let area = place(5);
            let button = |text: &str, on: bool, danger: bool| {
                let style = match (on, danger) {
                    (true, true) => Style::new()
                        .fg(Color::White)
                        .bg(RED)
                        .add_modifier(Modifier::BOLD),
                    (true, false) => Style::new()
                        .fg(Color::White)
                        .bg(BAND)
                        .add_modifier(Modifier::BOLD),
                    (false, _) => grey(),
                };
                Span::styled(format!(" {text} "), style)
            };
            let lines = vec![
                Line::styled(
                    tr!(
                        " Closes its tmux session and the Claude inside. No undo.",
                        " 会关掉它的 tmux 会话和里面的 Claude，没法撤销。"
                    ),
                    grey(),
                ),
                Line::raw(""),
                Line::from(vec![
                    Span::raw(" "),
                    button(tr!("Yes, end it", "结束它"), *yes, true),
                    Span::raw("   "),
                    button(tr!("Keep it", "再想想"), !*yes, false),
                ]),
            ];
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(lines).block(boxed(tr!(
                    format!("End {title}?"),
                    format!("结束 {title}？")
                ))),
                area,
            );
        }
        Mode::Note { text } => {
            let area = place(3);
            let inner = Block::bordered().inner(area);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(Line::raw(format!(" {text}")))
                    .block(boxed(tr!("Note", "备注").into())),
                area,
            );
            let x = inner.x.saturating_add(1).saturating_add(cols(text));
            frame.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y));
        }
    }
}

/// Terminal columns of `text`, capped at what a cursor position can hold.
fn cols(text: &str) -> u16 {
    u16::try_from(text.width()).unwrap_or(u16::MAX)
}

fn draw_hint(frame: &mut Frame, area: Rect, app: &App) {
    let key = |k: &str| Span::styled(k.to_string(), Style::new().add_modifier(Modifier::BOLD));
    let word = |w: &str| Span::styled(w.to_string(), grey());
    let gap = || Span::raw("     ");
    let parts: Vec<Span> = if let Some(m) = &app.message {
        vec![Span::styled(format!(" {m}"), Style::new().fg(AMBER))]
    } else {
        match &app.mode {
            Mode::Browse => {
                let open = match app.selected() {
                    _ if app.on_update() => tr!(" update", " 升级"),
                    Some(Item::New) => tr!(" start", " 新建"),
                    Some(Item::Interrupted(_)) => tr!(" restore", " 恢复"),
                    _ => tr!(" open", " 打开"),
                };
                let esc = if app.query.is_empty() {
                    tr!(" quit", " 退出")
                } else {
                    tr!(" clear", " 清空")
                };
                vec![
                    Span::raw(" "),
                    key("↑↓"),
                    word(tr!(" move", " 移动")),
                    gap(),
                    key("↵"),
                    word(open),
                    gap(),
                    key("→"),
                    word(tr!(" more", " 更多")),
                    gap(),
                    key("esc"),
                    word(esc),
                ]
            }
            Mode::Menu { .. } => vec![
                Span::raw(" "),
                key("↑↓"),
                word(tr!(" choose", " 选择")),
                gap(),
                key("↵"),
                word(tr!(" confirm", " 确认")),
                gap(),
                key("←"),
                word(tr!(" back", " 返回")),
            ],
            Mode::Confirm { .. } => vec![
                Span::raw(" "),
                key("←→"),
                word(tr!(" choose", " 选择")),
                gap(),
                key("↵"),
                word(tr!(" confirm", " 确认")),
                gap(),
                key("esc"),
                word(tr!(" cancel", " 取消")),
            ],
            Mode::Note { .. } => vec![
                Span::raw(" "),
                key("↵"),
                word(tr!(" save", " 保存")),
                gap(),
                key("esc"),
                word(tr!(" cancel", " 取消")),
            ],
        }
    };
    frame.render_widget(Line::from(parts), area);
}

/// `text` cut with `…` to at most `w` display columns.
fn fit(text: &str, w: usize) -> String {
    if text.width() <= w {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        if out.width() + unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0) + 1 > w {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

fn strip_prefix(name: &str, prefix: Option<&str>) -> String {
    prefix
        .map(moss_core::naming::sanitize)
        .and_then(|p| name.strip_prefix(&format!("{p}_")).map(String::from))
        .unwrap_or_else(|| name.to_string())
}

fn tilde(path: &str, home: Option<&str>) -> String {
    match home.and_then(|h| path.strip_prefix(h)) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// `~/…/last` when the path is long.
fn short_path(path: &str, home: Option<&str>) -> String {
    let t = tilde(path, home);
    if t.width() <= 28 {
        return t;
    }
    let mut parts = t.rsplit('/');
    let last = parts.next().unwrap_or(&t);
    let parent = parts.next().unwrap_or("");
    format!("…/{parent}/{last}")
}

/// `now`, `3m`, `2h`, `5d`: for the list.
fn short_ago(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    match (s, crate::i18n::lang()) {
        (0..=59, Lang::En) => "now".into(),
        (0..=59, Lang::Zh) => "刚刚".into(),
        (60..=3599, Lang::En) => format!("{}m", s / 60),
        (60..=3599, Lang::Zh) => format!("{}分钟", s / 60),
        (3600..=86_399, Lang::En) => format!("{}h", s / 3600),
        (3600..=86_399, Lang::Zh) => format!("{}小时", s / 3600),
        (_, Lang::En) => format!("{}d", s / 86_400),
        (_, Lang::Zh) => format!("{}天", s / 86_400),
    }
}

/// `just now`, `3 minutes ago`: for the card.
fn long_ago(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let unit = |n: i64, en: &str, zh: &str| {
        tr!(
            format!("{n} {en}{} ago", if n == 1 { "" } else { "s" }),
            format!("{n} {zh}前")
        )
    };
    match s {
        0..=59 => tr!("just now", "刚刚").into(),
        60..=3599 => unit(s / 60, "minute", "分钟"),
        3600..=86_399 => unit(s / 3600, "hour", "小时"),
        _ => unit(s / 86_400, "day", "天"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pick::source::{SessionRow, SlotRow};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    fn render(app: &App, w: u16, h: u16) -> (String, Buffer) {
        render_icons(app, w, h, false)
    }

    fn render_icons(app: &App, w: u16, h: u16, nerd_icons: bool) -> (String, Buffer) {
        let view = View {
            nerd_icons,
            checking: false,
            now_ms: 600_000,
            home: Some("/home/u"),
            host_prefix: Some("host-a"),
            cwd: "/home/u/projects",
            version: "0.1.0",
        };
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| {
            draw(f, app, &view);
        })
        .unwrap();
        let buf = t.backend().buffer().clone();
        // A wide character fills two cells; read the second as nothing.
        let text = (0..h)
            .map(|y| {
                let mut line = String::new();
                let mut x = 0;
                while x < w {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += sym.width().max(1) as u16;
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, buf)
    }

    fn claude(
        name: &str,
        dir: &str,
        at: i64,
        busy: bool,
        note: Option<&str>,
        preview: Option<&str>,
    ) -> Item {
        Item::Session(SessionRow {
            tmux_id: format!("${name}"),
            tmux_name: format!("host-a_{name}"),
            attached: false,
            activity_ms: at,
            path: dir.into(),
            claude: Some(SlotRow {
                slot_id: format!("slot-{name}"),
                session_id: format!("{name}-id"),
                name: Some(name.into()),
                note: note.map(Into::into),
                cwd: dir.into(),
                last_active_ms: at,
                busy,
                preview: preview.map(Into::into),
                moss: false,
            }),
        })
    }

    fn sample() -> App {
        App::new(
            vec![
                claude(
                    "api-gw",
                    "/home/u/acme",
                    540_000,
                    false,
                    Some("等供应商 key"),
                    Some("把灰度开到 5%"),
                ),
                claude(
                    "docs",
                    "/home/u/acme",
                    300_000,
                    true,
                    None,
                    Some("更新 sitemap"),
                ),
                Item::Session(SessionRow {
                    tmux_id: "$9".into(),
                    tmux_name: "host-a_scratch".into(),
                    attached: false,
                    activity_ms: 0,
                    path: "/home/u/billing".into(),
                    claude: None,
                }),
            ],
            vec![],
        )
    }

    fn key(a: &mut App, code: KeyCode) {
        a.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn a_huge_paste_does_not_overflow_the_cursor() {
        // Wider than u16::MAX would wrap; exactly u16::MAX overflows the sum.
        let long = "x".repeat(usize::from(u16::MAX));
        let mut a = sample();
        a.set_query(&long);
        render(&a, 110, 26);
        a.mode = Mode::Note { text: long };
        render(&a, 110, 26);
    }

    #[test]
    fn one_line_rows_without_noise() {
        let (text, _) = render(&sample(), 110, 26);
        for want in [
            "⢎⡱ cc-moss",
            "1 working",
            "1 waiting for you",
            "3 sessions",
            "acme",
            "billing",
            "api-gw",
            "docs",
            "scratch",
            "New session here",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
        for unwanted in ["0 interrupted", "tmux ", "host-a_"] {
            assert!(
                !text.contains(unwanted),
                "unexpected {unwanted:?} in\n{text}"
            );
        }
        // One line per session: the next list line after api-gw is docs.
        let lines: Vec<&str> = text.lines().collect();
        let i = lines.iter().position(|l| l.contains("● api-gw")).unwrap();
        assert!(lines[i + 1].contains("docs"), "{}", lines[i + 1]);
        // The folder appears once in the list column (and once in the card).
        assert_eq!(text.matches("acme").count(), 2, "{text}");
    }

    #[test]
    fn detail_follows_selection() {
        let mut a = sample();
        let (text, _) = render(&a, 110, 26);
        for want in [
            "● Waiting for you",
            "1 minute ago",
            "NOTE",
            "等供应商 key",
            "LAST MESSAGE",
            "“把灰度开到 5%”",
            "FOLDER",
            "~/acme",
            "↵ Open",
            "→ More",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
        key(&mut a, KeyCode::Down);
        let (text, _) = render(&a, 110, 26);
        assert!(
            text.contains("Working") && text.contains("“更新 sitemap”"),
            "{text}"
        );
        assert!(!text.contains("等供应商 key"), "{text}");
        key(&mut a, KeyCode::Down);
        let (text, _) = render(&a, 110, 26);
        assert!(
            text.contains("Just a plain tmux session, no Claude inside."),
            "{text}"
        );
        for _ in 0..5 {
            key(&mut a, KeyCode::Up);
        }
        let (text, _) = render(&a, 110, 26);
        assert!(
            text.contains("A fresh Claude Code")
                && text.contains("~/projects")
                && text.contains("↵ Start"),
            "{text}"
        );
    }

    #[test]
    fn narrow_terminal_stacks_detail() {
        let (text, _) = render(&sample(), 70, 34);
        let lines: Vec<&str> = text.lines().collect();
        let row = lines
            .iter()
            .position(|l| l.contains("api-gw") && !l.contains("Waiting"))
            .unwrap();
        let card = lines
            .iter()
            .position(|l| l.contains("Waiting for you"))
            .unwrap();
        assert!(card > row, "card below the list:\n{text}");
        assert!(
            !text.contains(" │ "),
            "no side divider when narrow:\n{text}"
        );
    }

    #[test]
    fn selection_is_a_band() {
        let (text, buf) = render(&sample(), 110, 26);
        let y = text.lines().position(|l| l.contains("● api-gw")).unwrap() as u16;
        assert_eq!(buf[(0, y)].symbol(), "▌");
        assert_eq!(buf[(20, y)].bg, BAND);
        let other = text
            .lines()
            .position(|l| l.contains("docs") && !l.contains("Last"))
            .unwrap() as u16;
        assert_ne!(buf[(20, other)].bg, BAND);
    }

    #[test]
    fn search_matches_are_highlighted() {
        let mut a = sample();
        for c in "gw".chars() {
            key(&mut a, KeyCode::Char(c));
        }
        let (text, buf) = render(&a, 110, 26);
        assert!(text.contains("⌕ gw") && text.contains("1 of 3"), "{text}");
        let y = text.lines().position(|l| l.contains("● api-gw")).unwrap() as u16;
        let line = text.lines().nth(y as usize).unwrap();
        let x = line[..line.find("api-gw").unwrap()].width() as u16;
        assert_ne!(buf[(x, y)].fg, ORANGE);
        assert_eq!(buf[(x + 4, y)].fg, ORANGE);
    }

    #[test]
    fn the_menu_opens_right_under_the_card_not_at_the_bottom() {
        let mut a = sample();
        key(&mut a, KeyCode::Right);
        // A tall terminal: the card's content ends near the top.
        let t = render(&a, 110, 60).0;
        let rows: Vec<&str> = t.lines().collect();
        let row_of = |needle: &str| rows.iter().position(|l| l.contains(needle));
        let folder = row_of("FOLDER").expect("the card shows its folder");
        let menu = row_of("Write a note").expect("the menu is open");
        assert!(
            menu > folder && menu - folder <= 6,
            "menu at row {menu}, folder at row {folder}:\n{t}"
        );
        // The menu takes the place of the "↵ Open  → More" line.
        assert!(!t.contains("→ More"), "{t}");
        // A short terminal still shows the whole menu.
        let t = render(&a, 110, 16).0;
        assert!(t.contains("End session"), "{t}");
    }

    #[test]
    fn menus_open_in_the_card() {
        let mut a = sample();
        key(&mut a, KeyCode::Right);
        let t = render(&a, 110, 26).0;
        assert!(
            t.contains("Write a note") && t.contains("End session") && t.contains("← back"),
            "{t}"
        );
        key(&mut a, KeyCode::Down);
        key(&mut a, KeyCode::Down);
        key(&mut a, KeyCode::Enter);
        let t = render(&a, 110, 26).0;
        assert!(
            t.contains("End api-gw?") && t.contains("Yes, end it") && t.contains("Keep it"),
            "{t}"
        );
    }

    #[test]
    fn unnamed_claude_session_shows_its_tmux_name() {
        let mut item = claude("x", "/home/u/acme", 0, false, None, None);
        if let Item::Session(s) = &mut item {
            s.tmux_name = "host-a_job-api-gw".into();
            s.claude.as_mut().unwrap().name = None;
        }
        let a = App::new(vec![item], vec![]);
        let (text, _) = render(&a, 110, 20);
        assert!(text.contains("● job-api-gw"), "{text}");
        assert!(!text.contains("x-id"), "{text}");
    }

    #[test]
    fn worktrees_name_their_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("acme");
        std::fs::create_dir_all(repo.join(".git/worktrees/97-t7.1")).unwrap();
        let wt = repo.join(".claude/worktrees/97-t7.1");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(
            wt.join(".git"),
            format!("gitdir: {}/.git/worktrees/97-t7.1", repo.display()),
        )
        .unwrap();
        let a = App::new(
            vec![claude("docs", &wt.to_string_lossy(), 0, false, None, None)],
            vec![],
        );
        let (text, _) = render(&a, 140, 20);
        assert!(text.contains(" 97-t7.1  worktree · acme"), "{text}");
        assert!(text.contains("worktree 97-t7.1"), "{text}");
        assert!(!text.contains(".claude/worktrees"), "{text}");
        assert!(
            !text.contains('\u{f0645}'),
            "no icon without a Nerd Font:\n{text}"
        );
        let (text, _) = render_icons(&a, 140, 20, true);
        assert!(text.contains("\u{f0645} worktree · acme"), "{text}");
    }

    #[test]
    fn working_dots_run_along_a_path() {
        let dots = |f: &str| f.chars().next().unwrap() as u32 - 0x2800;
        for (i, frame) in SPINNER.iter().enumerate() {
            let next = SPINNER[(i + 1) % SPINNER.len()];
            assert_eq!(dots(frame).count_ones(), 4, "{frame}");
            assert_eq!(
                (dots(frame) & dots(next)).count_ones(),
                3,
                "{frame} -> {next}"
            );
        }
    }

    #[test]
    fn speaks_chinese() {
        crate::i18n::set_for_test(crate::i18n::Lang::Zh);
        let mut a = sample();
        let (text, _) = render(&a, 110, 26);
        crate::i18n::set_for_test(crate::i18n::Lang::En);
        for want in [
            "共 3 个会话",
            "1 个在忙",
            "1 个在等你",
            "搜一个会话",
            "在这里开个新会话",
            "备注",
            "最后一句",
            "目录",
            "↵ 打开",
            "→ 更多",
            "↑↓ 移动",
            "esc 退出",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
        for unwanted in ["session", "Folder", "Open", "waiting"] {
            assert!(!text.contains(unwanted), "English {unwanted:?} in\n{text}");
        }
        crate::i18n::set_for_test(crate::i18n::Lang::Zh);
        key(&mut a, KeyCode::Right);
        let (menu, _) = render(&a, 110, 26);
        crate::i18n::set_for_test(crate::i18n::Lang::En);
        assert!(
            menu.contains("写备注") && menu.contains("结束会话"),
            "{menu}"
        );
    }

    #[test]
    fn update_notice_in_the_header_and_on_top_of_the_list() {
        let mut a = sample();
        a.set_update(Some("9.9.9".into()));
        let (text, _) = render(&a, 110, 26);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[1].contains("cc-moss 9.9.9 is out"), "{text}");
        let up = lines
            .iter()
            .position(|l| l.contains("↑ Update to 9.9.9"))
            .unwrap();
        let new = lines
            .iter()
            .position(|l| l.contains("New session here"))
            .unwrap();
        assert!(up < new, "update first:\n{text}");
        key(&mut a, KeyCode::Up);
        key(&mut a, KeyCode::Up);
        let (text, _) = render(&a, 110, 26);
        assert!(
            text.contains("You have 0.1.0")
                && text.contains("↵ Update")
                && text.contains("↵ update"),
            "{text}"
        );
        crate::i18n::set_for_test(crate::i18n::Lang::Zh);
        let (zh, _) = render(&a, 110, 26);
        crate::i18n::set_for_test(crate::i18n::Lang::En);
        assert!(
            zh.contains("升级到 9.9.9") && zh.contains("选第一行就能升级"),
            "{zh}"
        );
        // Nothing to say without a newer version.
        let (plain, _) = render(&sample(), 110, 26);
        assert!(!plain.contains("Update to"), "{plain}");
    }

    #[test]
    fn helpers() {
        assert_eq!(tilde("/home/u/src", Some("/home/u")), "~/src");
        assert_eq!(
            short_path(
                "/home/u/Documents/acme/.claude/worktrees/97",
                Some("/home/u")
            ),
            "…/worktrees/97"
        );
        assert_eq!(fit("中文abc", 5), "中文…");
        assert_eq!(strip_prefix("host-a_x", Some("host-a")), "x");
        assert_eq!(short_ago(125_000), "2m");
        assert_eq!(long_ago(3_600_000), "1 hour ago");
    }
}
