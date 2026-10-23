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
use moss_core::{proc, store_dir, update};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::cmd;
use app::{Action, App};
use source::Item;

/// Redraw often enough for the working spinner.
const TICK: Duration = Duration::from_millis(120);
/// Pick up new, ended and busy sessions without asking.
const REFRESH: Duration = Duration::from_secs(2);

/// Open the picker, with `query` already in the search line.
pub fn run(query: &str) -> Result<()> {
    let config = Config::load()?;
    let socket = Socket::from_config(config.tmux_socket().as_deref());
    let tmux = Tmux::with_socket(socket.clone());
    // Adopt sessions the hooks missed in the background: `claude agents` can
    // take a second to start, and the first frame must not wait for it.
    let reconcile = std::thread::spawn(|| crate::reconcile::run().map(drop));
    // Look for a new cc-moss at most once a day, also in the background; a
    // version already known is shown straight away.
    let checks_updates = update::enabled_here(&config);
    let update_check = (checks_updates && update::cache_is_stale())
        .then(|| std::thread::spawn(|| drop(update::check_and_cache(update::QUICK_TIMEOUT))));
    let (sessions, interrupted) = load(&tmux)?;
    let mut app = App::new(sessions, interrupted);
    app.set_query(query);
    if checks_updates {
        app.set_update(newer_version());
    }
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
        let background = Background {
            reconcile: Some(reconcile),
            update_check,
        };
        event_loop(&mut terminal, &mut app, &tmux, &mut view, background)?
    };

    // The screen is restored: hand the terminal to whatever comes next.
    match action {
        Action::Attach(s) => cmd::attach::attach_tmux(&socket, &s.tmux_id),
        action @ (Action::New { .. } | Action::Restore(_)) => {
            dispatch_launch(action, cmd::new::run, restore_selected)
        }
        Action::Update => {
            let result = cmd::update::run(cmd::update::UpdateArgs {
                check: false,
                json: false,
                yes: false,
            });
            if let Err(e) = &result {
                eprintln!("cc-moss: {e:#}");
            }
            // The picker may live in a tmux popup that closes when we exit:
            // keep the result on screen until it has been read.
            hold_until_enter();
            // Already shown above; a second copy after the pause would be lost.
            Ok(())
        }
        _ => Ok(()),
    }
}

fn dispatch_launch(
    action: Action,
    new: impl FnOnce(cmd::new::NewArgs) -> Result<()>,
    restore: impl FnOnce(&source::SlotRow) -> Result<()>,
) -> Result<()> {
    match action {
        Action::New { name } => new(cmd::new::NewArgs {
            name,
            dir: None,
            detach: false,
            yes: false,
            safe: false,
            claude_args: Vec::new(),
        }),
        Action::Restore(row) => restore(&row),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestoredTarget {
    socket: Socket,
    session_id: String,
    pane_id: String,
    pid: u32,
    pid_start: String,
}

/// 隐藏的精确恢复（contracts.md AMEND-5）：slot 与完整 UUID 同时匹配，显示名无法遮蔽。
fn restore_argv(row: &source::SlotRow) -> Vec<String> {
    [
        "restore",
        "--exact-slot-id",
        &row.slot_id,
        "--yes",
        "--json",
        "--",
        &row.session_id,
    ]
    .map(String::from)
    .into()
}

fn restore_selected(row: &source::SlotRow) -> Result<()> {
    restore_then_attach(
        row,
        |_| {
            // 与 Moss 的显式恢复相同：复用 CLI 的 typed 结果，不把 skipped 的 Ok 当成功。
            let out = moss_core::command::run_with_timeout(
                std::process::Command::new(std::env::current_exe()?).args(restore_argv(row)),
                Duration::from_secs(90),
            )
            .map_err(|error| {
                anyhow::anyhow!("restore did not finish: {error:?}; no terminal was attached")
            })?;
            anyhow::ensure!(
                out.status.success(),
                "restore failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            serde_json::from_slice(&out.stdout)
                .context("restore returned an unreadable result; no terminal was attached")
        },
        |row| {
            let registry = registry()?;
            let slots = registry.slots()?;
            let slot = slots
                .iter()
                .find(|slot| slot.slot_id == row.slot_id)
                .context("the selected session disappeared after restore")?;
            let alive = slot
                .pid
                .zip(slot.pid_start.as_deref())
                .is_some_and(|(pid, start)| {
                    proc::liveness(pid, Some(start)) == proc::Liveness::Alive
                });
            restored_target(row, slot, alive)
        },
        |target| {
            let tmux = Tmux::with_socket(target.socket.clone());
            anyhow::ensure!(
                proc::liveness(target.pid, Some(&target.pid_start)) == proc::Liveness::Alive
                    && tmux
                        .panes()
                        .iter()
                        .any(|(pid, session, _, pane)| *pid == target.pid
                            && session == &target.session_id
                            && pane == &target.pane_id),
                "the restored process or tmux target changed; no terminal was attached"
            );
            if let Ok(current) = std::env::var("TMUX") {
                anyhow::ensure!(tmux.matches_client(&current), "the restored session uses another tmux server; select it explicitly in cc-moss");
            }
            cmd::attach::attach_tmux(&target.socket, &target.session_id)
        },
    )
}

fn restore_then_attach(
    row: &source::SlotRow,
    restore: impl FnOnce(&str) -> Result<serde_json::Value>,
    locate: impl FnOnce(&source::SlotRow) -> Result<RestoredTarget>,
    attach: impl FnOnce(RestoredTarget) -> Result<()>,
) -> Result<()> {
    let response = restore(&row.session_id)?;
    anyhow::ensure!(
        response["schema"] == 1,
        "restore returned an unsupported result schema"
    );
    let results = response["results"]
        .as_array()
        .context("restore returned no session result")?;
    anyhow::ensure!(
        results.len() == 1,
        "restore did not return exactly the selected session"
    );
    let result = &results[0];
    anyhow::ensure!(
        result["id"] == row.slot_id
            && result["ok"] == true
            && matches!(result["result"].as_str(), Some("restored" | "starting")),
        "restore refused: {}",
        result["message"]
            .as_str()
            .unwrap_or("the selected session was not restored")
    );
    attach(locate(row)?)
}

fn restored_target(row: &source::SlotRow, slot: &Slot, alive: bool) -> Result<RestoredTarget> {
    anyhow::ensure!(
        slot.slot_id == row.slot_id
            && slot.session_id == row.session_id
            && slot.status != Status::Closed
            && alive,
        "the selected conversation identity is not running after restore"
    );
    let (pid, pid_start) = slot
        .pid
        .zip(slot.pid_start.as_ref())
        .context("the restored process identity is missing")?;
    let loc = slot
        .tmux
        .as_ref()
        .context("the restored session has no tmux identity")?;
    let session_id = loc
        .session_id
        .as_deref()
        .filter(|id| {
            id.starts_with('$') && id.len() > 1 && id[1..].bytes().all(|b| b.is_ascii_digit())
        })
        .context("the restored tmux session id is missing")?;
    anyhow::ensure!(
        std::path::Path::new(&loc.socket).is_absolute()
            && !loc.socket.chars().any(char::is_control)
            && loc.pane_id.starts_with('%')
            && loc.pane_id.len() > 1
            && loc.pane_id[1..].bytes().all(|b| b.is_ascii_digit()),
        "the restored tmux identity is invalid"
    );
    Ok(RestoredTarget {
        socket: Socket::Path(loc.socket.clone()),
        session_id: session_id.into(),
        pane_id: loc.pane_id.clone(),
        pid,
        pid_start: pid_start.clone(),
    })
}

/// Wait for Enter when someone is there to press it.
fn hold_until_enter() {
    use std::io::{BufRead, IsTerminal, Write};
    if !io::stdin().is_terminal() {
        return;
    }
    print!("\n{}", tr!("Press Enter to close.", "按回车关闭。"));
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().lock().read_line(&mut line);
}

/// A newer cc-moss, as far as the cache knows.
fn newer_version() -> Option<String> {
    update::newer(update::cached_latest()).map(|v| v.to_string())
}

/// Work the picker started without waiting for it.
struct Background {
    reconcile: Option<std::thread::JoinHandle<Result<()>>>,
    update_check: Option<std::thread::JoinHandle<()>>,
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    tmux: &Tmux<ProcessRunner>,
    view: &mut ui::View,
    background: Background,
) -> Result<Action> {
    let Background {
        mut reconcile,
        mut update_check,
    } = background;
    let mut last_refresh = Instant::now();
    let mut hits = ui::HitMap::default();
    loop {
        view.now_ms = now_ms();
        view.checking = reconcile.is_some();
        terminal.draw(|f| hits = ui::draw(f, app, view))?;
        if let Some(handle) = update_check.take_if(|h| h.is_finished()) {
            let _ = handle.join();
            app.set_update(newer_version());
        }
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
            Action::Unarchive(r) => {
                let back = registry()?.unarchive(std::slice::from_ref(&r.slot_id), "you", now_ms());
                app.message = Some(match back {
                    Ok(_) => tr!(
                        format!("{} is back in the list.", r.label()),
                        format!("{} 回到列表里了。", r.label())
                    ),
                    Err(e) => tr!(
                        format!("Could not bring it back: {e:#}"),
                        format!("没能拿回来：{e:#}")
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
    if s.claude.as_ref().is_some_and(|c| c.moss) {
        return crate::cmd::moss::protected_hint();
    }
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
        source::interrupted(&rows)
            .into_iter()
            .chain(source::archived(&rows))
            .collect(),
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

#[cfg(test)]
mod launch_tests {
    use super::*;
    use std::cell::RefCell;

    fn row(name: Option<&str>) -> source::SlotRow {
        source::SlotRow {
            slot_id: "slot-exact".into(),
            session_id: "11111111-2222-4333-8444-555555555555".into(),
            name: name.map(str::to_string),
            note: None,
            cwd: "/tmp/project".into(),
            last_active_ms: 0,
            busy: false,
            preview: None,
            moss: false,
        }
    }

    #[test]
    fn picker_restore_named_and_unnamed_never_dispatch_new() {
        for name in [Some("same-name"), None] {
            let row = row(name);
            let calls = RefCell::new(Vec::new());
            dispatch_launch(
                Action::Restore(row.clone()),
                |_| {
                    calls.borrow_mut().push("new".to_string());
                    Ok(())
                },
                |selected| {
                    calls
                        .borrow_mut()
                        .push(format!("restore:{}", selected.session_id));
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(*calls.borrow(), [format!("restore:{}", row.session_id)]);
        }
    }

    #[test]
    fn picker_restore_routes_exact_slot_and_full_uuid_not_name() {
        // AMEND-5：picker 恢复先精确绑定 slot 与完整 UUID，不经名称 resolver。
        let row = row(Some("same-name"));
        assert_eq!(
            restore_argv(&row),
            [
                "restore",
                "--exact-slot-id",
                "slot-exact",
                "--yes",
                "--json",
                "--",
                "11111111-2222-4333-8444-555555555555",
            ]
        );
    }

    #[test]
    fn picker_restore_refusal_is_not_replaced_with_new() {
        let result = dispatch_launch(
            Action::Restore(row(Some("same-name"))),
            |_| Ok(()),
            |_| anyhow::bail!("restore refused"),
        );
        assert!(result.is_err());
    }

    fn target() -> RestoredTarget {
        RestoredTarget {
            socket: Socket::Path("/tmp/picker-test-socket".into()),
            session_id: "$9".into(),
            pane_id: "%7".into(),
            pid: 42,
            pid_start: "fixture-start".into(),
        }
    }

    fn receipt(row: &source::SlotRow) -> serde_json::Value {
        serde_json::json!({"schema":1,"results":[{"id":row.slot_id,"ok":true,"result":"restored"}]})
    }

    #[test]
    fn picker_restore_attaches_only_exact_receipt_target_for_both_names() {
        for name in [Some("same-name"), None] {
            let row = row(name);
            let calls = RefCell::new(Vec::new());
            restore_then_attach(
                &row,
                |id| {
                    assert_eq!(id, row.session_id);
                    calls.borrow_mut().push(format!("restore:{id}"));
                    Ok(receipt(&row))
                },
                |selected| {
                    assert_eq!(selected.slot_id, row.slot_id);
                    assert_eq!(selected.session_id, row.session_id);
                    Ok(target())
                },
                |actual| {
                    assert_eq!(actual, target());
                    calls
                        .borrow_mut()
                        .push(format!("attach:{}", actual.session_id));
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(
                *calls.borrow(),
                [format!("restore:{}", row.session_id), "attach:$9".into()]
            );
        }
    }

    #[test]
    fn picker_restore_refused_failed_unknown_or_mismatched_receipt_never_attaches() {
        let row = row(Some("same-name"));
        for response in [
            serde_json::json!({"schema":1,"results":[{"id":row.slot_id,"ok":false,"reason":"config_source_unknown"}]}),
            serde_json::json!({"schema":1,"results":[{"id":row.slot_id,"ok":false,"reason":"tmux_failed"}]}),
            serde_json::json!({"schema":1,"results":[{"id":"other-slot","ok":true,"result":"restored"}]}),
            serde_json::json!({"schema":1,"results":[{"id":row.slot_id,"ok":true,"result":"unconfirmed"}]}),
            serde_json::json!({"schema":2,"results":[]}),
            serde_json::json!({"schema":1,"results":[]}),
        ] {
            assert!(restore_then_attach(
                &row,
                |_| Ok(response),
                |_| panic!("refusal must not resolve an attach target"),
                |_| panic!("refusal must not attach")
            )
            .is_err());
        }
        assert!(restore_then_attach(
            &row,
            |_| anyhow::bail!("restore timed out"),
            |_| panic!("failed request must not resolve a target"),
            |_| panic!("failed request must not attach")
        )
        .is_err());
        assert!(restore_then_attach(
            &row,
            |_| Ok(receipt(&row)),
            |_| anyhow::bail!("exact identity disappeared"),
            |_| panic!("missing identity must not attach")
        )
        .is_err());
    }

    #[test]
    fn picker_restore_identity_never_falls_back_to_display_name() {
        use moss_core::event::{Context, Event, StartSource, TmuxLoc};
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::open(&dir.path().join("registry.db")).unwrap();
        let selected = row(Some("same-name"));
        registry
            .apply_with(
                &Event::Start {
                    source: StartSource::Startup,
                    session_id: selected.session_id,
                    cwd: selected.cwd,
                    title: selected.name,
                    transcript_path: None,
                },
                &Context {
                    pid: Some(42),
                    pid_start: Some("fixture-start".into()),
                    tmux: Some(TmuxLoc {
                        socket: "/tmp/picker-test-socket".into(),
                        session_id: Some("$9".into()),
                        session_name: Some("same-name".into()),
                        pane_id: "%7".into(),
                    }),
                    launch_args: None,
                    now_ms: 1,
                },
                &|_, _| true,
            )
            .unwrap();
        let slot = registry.slots().unwrap().remove(0);
        let row = source::SlotRow::from_slot(&slot);
        assert_eq!(restored_target(&row, &slot, true).unwrap(), target());
        assert!(restored_target(&row, &slot, false).is_err());
        let mut changed = slot.clone();
        changed.session_id = "99999999-2222-4333-8444-555555555555".into();
        assert!(restored_target(&row, &changed, true).is_err());
        let mut changed = slot.clone();
        changed.slot_id = "other-slot".into();
        assert!(restored_target(&row, &changed, true).is_err());
        let mut changed = slot.clone();
        changed.tmux.as_mut().unwrap().session_id = None;
        assert!(restored_target(&row, &changed, true).is_err());
        let mut changed = slot.clone();
        changed.tmux = None;
        assert!(restored_target(&row, &changed, true).is_err());
    }

    #[test]
    fn picker_cancel_dispatches_neither_restore_nor_new() {
        for action in [Action::None, Action::Quit] {
            dispatch_launch(
                action,
                |_| panic!("cancel must not create"),
                |_| panic!("cancel must not restore"),
            )
            .unwrap();
        }
    }
}
