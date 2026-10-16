//! The picker in a real terminal: a tmux pane on an isolated server, driven
//! with send-keys and read back with capture-pane.

mod common;

use common::{ls_json, wait_for, World};

/// Start `cc-moss pick` in its own session ("driver") and wait for it to draw.
fn open_picker(w: &World) {
    open_picker_with(w, &[]);
}

/// Like [`open_picker`], with arguments for `pick`.
fn open_picker_with(w: &World, extra: &[&str]) {
    let bin = env!("CARGO_BIN_EXE_cc-moss");
    let state = w.state();
    let config = w.path("config.toml");
    let fifos = w.path("fifos");
    let args: Vec<String> = [
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        "driver".into(),
        "-x".into(),
        "120".into(),
        "-y".into(),
        "30".into(),
        "env".into(),
        "-u".into(),
        "TMUX".into(),
        "-u".into(),
        "TMUX_PANE".into(),
        format!("CC_MOSS_STATE_DIR={}", state.display()),
        format!("CC_MOSS_CONFIG={}", config.display()),
        format!("CC_MOSS_TMUX_SOCKET={}", w.socket),
        "CC_MOSS_HOST_PREFIX=host".into(),
        format!("CC_MOSS_CLAUDE_BIN={}", w.claude.display()),
        format!("CC_MOSS_BIN={bin}"),
        format!("CC_MOSS_FAKE_DIR={}", fifos.display()),
        "HOME=/nonexistent/cc-moss-test-home".into(),
        "CC_MOSS_LANG=en".into(),
        bin.into(),
        "pick".into(),
    ]
    .into_iter()
    .chain(extra.iter().map(|s| s.to_string()))
    .collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    w.tmux(&args);
    wait_for(|| screen(w).contains("→ more").then_some(()));
}

fn screen(w: &World) -> String {
    w.tmux(&["capture-pane", "-p", "-t", "driver"])
}

/// Wait until the screen satisfies `ok`; on timeout, show the screen.
fn wait_screen(w: &World, what: &str, ok: impl Fn(&str) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let s = screen(w);
        if ok(&s) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}:\n{s}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn keys(w: &World, keys: &[&str]) {
    let mut args = vec!["send-keys", "-t", "driver"];
    args.extend_from_slice(keys);
    w.tmux(&args);
}

#[test]
fn search_and_enter_a_session() {
    let w = World::new("pick-enter");
    w.new_session("api");
    w.new_session("web");
    w.session_id("api");
    w.session_id("web");
    open_picker(&w);

    // Grouped by project (the "work" folder), the selected session's card on the side.
    let s = screen(&w);
    assert!(s.contains("⌕") && s.contains(" work "), "{s}");
    assert!(s.contains("● api") && s.contains("● web"), "{s}");
    assert!(
        s.contains("2 waiting for you") && s.contains("↵ Open"),
        "{s}"
    );

    // Letters are search input, not shortcuts.
    keys(&w, &["w", "e", "b"]);
    wait_for(|| {
        let s = screen(&w);
        (s.contains("⌕ web") && s.contains("1 of")).then_some(())
    });

    // The exact match is selected; Enter attaches the driver pane to it.
    keys(&w, &["Enter"]);
    wait_for(|| {
        let clients = w.tmux(&["list-clients", "-F", "#{client_session}"]);
        clients.lines().any(|c| c == "host_web").then_some(())
    });
}

#[test]
fn end_a_session_from_the_menu() {
    let w = World::new("pick-end");
    w.new_session("doomed");
    w.session_id("doomed");
    open_picker(&w);

    keys(&w, &["d", "o", "o", "m", "Right"]);
    wait_screen(&w, "the menu", |s| s.contains("End session"));
    keys(&w, &["Down", "Down", "Enter"]);
    wait_screen(&w, "the confirmation", |s| s.contains("End doomed?"));
    // Cancel is the default.
    keys(&w, &["Enter"]);
    wait_screen(&w, "the confirmation to close", |s| {
        !s.contains("End doomed?")
    });
    assert!(w.session_names().iter().any(|n| n == "host_doomed"));

    keys(&w, &["Right", "Down", "Down", "Enter", "Left", "Enter"]);
    wait_for(|| (!w.session_names().iter().any(|n| n == "host_doomed")).then_some(()));
    // Ended on purpose: closed, not offered for restore.
    wait_for(|| (ls_json(&w.state(), true)["sessions"][0]["status"] == "closed").then_some(()));
    keys(&w, &["Escape"]);
}

#[test]
fn pick_takes_an_initial_search() {
    let w = World::new("pick-query");
    w.new_session("api");
    w.new_session("web");
    w.session_id("api");
    w.session_id("web");
    open_picker_with(&w, &["web"]);
    wait_screen(&w, "the search", |s| {
        s.contains("⌕ web") && s.contains("1 of")
    });
    keys(&w, &["Escape", "Escape"]);
}
