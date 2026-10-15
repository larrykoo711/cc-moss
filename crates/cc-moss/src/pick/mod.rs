//! `cc-moss` / `t`: a full-screen picker for Claude Code sessions.

pub mod app;
pub mod source;
pub mod ui;

use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use moss_core::config::Config;
use moss_core::registry::Registry;
use moss_core::slot::{now_ms, Notes, Slot, Status};
use moss_core::tmux::{ProcessRunner, Socket, Tmux};
use moss_core::{proc, store_dir};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::cmd;
use app::{Action, App};
use source::Item;

/// Redraw often enough for the working spinner.
const TICK: Duration = Duration::from_millis(120);
/// Pick up new, ended and busy sessions without asking.
const REFRESH: Duration = Duration::from_secs(2);

pub fn run() -> Result<()> {
    let config = Config::load()?;
    let socket = Socket::from_config(config.tmux_socket().as_deref());
    let tmux = Tmux::with_socket(socket.clone());
    // Adopt sessions the hooks missed in the background: `claude agents` can
    // take a second to start, and the first frame must not wait for it.
    let reconcile = std::thread::spawn(|| crate::reconcile::run().map(drop));
    let (sessions, interrupted) = load(&tmux)?;
    let mut app = App::new(sessions, interrupted);
    let host_prefix = if config.naming.host_prefix {
        moss_core::naming::cached_host_prefix(&store_dir::state_dir())
    } else {
        None
    };
    let home = std::env::var("HOME").ok();
    let cwd = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut view = ui::View {
        now_ms: now_ms(),
        home: home.as_deref(),
        host_prefix: host_prefix.as_deref(),
        cwd: &cwd,
        version: env!("CARGO_PKG_VERSION"),
        nerd_icons: config
            .ui
            .icons
            .enabled(std::env::var("STARSHIP_SHELL").ok().as_deref()),
        checking: true,
    };

    let action = {
        let _screen = Screen::enter()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        event_loop(&mut terminal, &mut app, &tmux, &mut view, Some(reconcile))?
    };

    // The screen is restored: hand the terminal to whatever comes next.
    match action {
        Action::Attach(s) => cmd::attach::attach_tmux(&socket, &s.tmux_id),
        Action::New { name } => cmd::new::run(cmd::new::NewArgs {
            name,
            dir: None,
            detach: false,
            yes: false,
            claude_args: Vec::new(),
        }),
        Action::Restore(r) => match &r.name {
            // `new <name>` restores an interrupted session of that name and attaches (T1.5).
            Some(name) => cmd::new::run(cmd::new::NewArgs {
                name: Some(name.clone()),
                dir: None,
                detach: false,
                yes: true,
                claude_args: Vec::new(),
            }),
            None => cmd::restore::run(cmd::restore::RestoreArgs {
                yes: true,
                dry_run: false,
                names: vec![r.session_id.clone()],
            }),
        },
        _ => Ok(()),
    }
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    tmux: &Tmux<ProcessRunner>,
    view: &mut ui::View,
    mut reconcile: Option<std::thread::JoinHandle<Result<()>>>,
) -> Result<Action> {
    let mut last_refresh = Instant::now();
    let mut hits = ui::HitMap::default();
    loop {
        view.now_ms = now_ms();
        view.checking = reconcile.is_some();
        terminal.draw(|f| hits = ui::draw(f, app, view))?;
        let reconciled = match reconcile.take_if(|h| h.is_finished()) {
            Some(handle) => {
                match handle.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        app.message = Some(tr!(
                            format!("Could not check running sessions: {e:#}"),
                            format!("没能检查正在运行的会话：{e:#}")
                        ))
                    }
                    Err(_) => {
                        app.message = Some(
                            tr!("Could not check running sessions", "没能检查正在运行的会话")
                                .into(),
                        )
                    }
                }
                true
            }
            None => false,
        };
        if reconciled || last_refresh.elapsed() >= REFRESH {
            let (sessions, interrupted) = load(tmux)?;
            app.set_items(sessions, interrupted);
            last_refresh = Instant::now();
        }
        if !event::poll(TICK)? {
            continue;
        }
        let action = match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.handle(key),
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) => match hits.row_at(m.row) {
                    Some(row) if app.click(row) => app.open(),
                    _ => Action::None,
                },
                MouseEventKind::ScrollDown => {
                    app.scroll(true);
                    Action::None
                }
                MouseEventKind::ScrollUp => {
                    app.scroll(false);
                    Action::None
                }
                _ => Action::None,
            },
            _ => Action::None,
        };
        match action {
            Action::None => {}
            Action::End(item) => {
                let message = end(&item, tmux);
                let (sessions, interrupted) = load(tmux)?;
                app.set_items(sessions, interrupted);
                app.message = Some(message);
            }
            Action::Remove(r) => {
                app.message = Some(match registry()?.close_by_user(&r.slot_id, now_ms()) {
                    Ok(()) => tr!(
                        format!("Removed {} from the list.", r.label()),
                        format!("已把 {} 移出列表。", r.label())
                    ),
                    Err(e) => tr!(
                        format!("Could not remove it: {e:#}"),
                        format!("没能移除：{e:#}")
                    ),
                });
                let (sessions, interrupted) = load(tmux)?;
                app.set_items(sessions, interrupted);
            }
            Action::SetNote { slot_id, text } => {
                let notes = Notes {
                    state: Some(text.clone()).filter(|t| !t.is_empty()),
                    ..Notes::default()
                };
                app.message = Some(match registry()?.set_notes(&slot_id, &notes) {
                    Ok(()) if text.is_empty() => tr!("Note cleared.", "备注已清空。").into(),
                    Ok(()) => tr!("Note saved.", "备注已保存。").into(),
                    Err(e) => tr!(
                        format!("Could not save the note: {e:#}"),
                        format!("备注没存上：{e:#}")
                    ),
                });
                let (sessions, interrupted) = load(tmux)?;
                app.set_items(sessions, interrupted);
            }
            done => return Ok(done),
        }
    }
}

fn registry() -> Result<Registry> {
    Registry::open(&store_dir::registry_path())
}

/// End a tmux session. A Claude Code session ended here is closed first, so
/// it is not offered for restore afterwards.
fn end(item: &Item, tmux: &Tmux<ProcessRunner>) -> String {
    let Item::Session(s) = item else {
        return String::new();
    };
    let result = s
        .claude
        .as_ref()
        .map_or(Ok(()), |c| {
            registry().and_then(|r| r.close_by_user(&c.slot_id, now_ms()))
        })
        .and_then(|()| tmux.kill_session(&s.tmux_id));
    match result {
        Ok(()) => tr!(
            format!("Ended {}. It had a good run.", item.title()),
            format!("已结束 {}，辛苦啦。", item.title())
        ),
        Err(e) => tr!(
            format!("Could not end {}: {e:#}", item.title()),
            format!("没能结束 {}：{e:#}", item.title())
        ),
    }
}

fn load(tmux: &Tmux<ProcessRunner>) -> Result<(Vec<Item>, Vec<Item>)> {
    let path = store_dir::registry_path();
    let rows: Vec<(Status, Slot)> = if path.exists() {
        Registry::open(&path)?
            .slots()?
            .into_iter()
            .map(|s| (s.effective_status(proc::is_alive), s))
            .collect()
    } else {
        Vec::new()
    };
    Ok((
        source::sessions(&tmux.session_infos(), tmux.socket_path().as_deref(), &rows),
        source::interrupted(&rows),
    ))
}

/// Raw mode, the alternate screen and mouse reporting, undone on drop and on
/// panic so a crash never leaves the terminal unusable.
struct Screen;

impl Screen {
    fn enter() -> Result<Self> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            Screen::leave();
            previous(info);
        }));
        enable_raw_mode().context("cannot use this terminal")?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Screen)
    }

    fn leave() {
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        Screen::leave();
    }
}
