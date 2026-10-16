//! The tmux config cc-moss installs, loaded by the real tmux on a private
//! socket. Older tmux (3.0, 3.1) must load everything but the popup binding.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::World;

/// The asset as `install` writes it, reloading from a file that is not there.
fn rendered(w: &World) -> std::path::PathBuf {
    let asset = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/tmux.conf");
    let text = std::fs::read_to_string(asset)
        .unwrap()
        .replace("{{USER_CONF}}", "/nonexistent/tmux.conf");
    let path = w.path("cc-moss-tmux.conf");
    std::fs::write(&path, text).unwrap();
    path
}

/// `tmux -L <socket> args`, with `path` as the PATH the server inherits.
fn tmux(w: &World, path: &str, args: &[&str]) -> Output {
    Command::new("tmux")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("PATH", path)
        .args(["-L", &w.socket])
        .args(args)
        .output()
        .unwrap()
}

/// Load the config into a fresh server and return the prefix + t binding.
fn load(w: &World, path: &str) -> String {
    let conf = rendered(w);
    let start = tmux(
        w,
        path,
        &["-f", "/dev/null", "new-session", "-d", "-s", "probe"],
    );
    assert!(start.status.success(), "{start:?}");
    let out = tmux(w, path, &["source-file", conf.to_str().unwrap()]);
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "config errors: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The rest of the file took effect.
    let mouse = tmux(w, path, &["show-options", "-g", "mouse"]);
    assert_eq!(String::from_utf8_lossy(&mouse.stdout).trim(), "mouse on");
    let keys = tmux(w, path, &["list-keys", "-T", "prefix"]);
    String::from_utf8_lossy(&keys.stdout)
        .lines()
        .find(|l| l.split_whitespace().nth(3) == Some("t"))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn loads_cleanly_and_binds_the_popup_on_this_tmux() {
    let w = World::new("conf-real");
    let keys = load(&w, &std::env::var("PATH").unwrap());
    // The local tmux is 3.2 or newer on every machine these tests run on.
    assert!(keys.contains("display-popup"), "{keys}");
}

#[test]
fn an_older_tmux_never_sees_the_popup_binding() {
    let w = World::new("conf-old");
    // A `tmux` that reports 3.1c and otherwise is the real one.
    let shim = w.path("shim");
    std::fs::create_dir_all(&shim).unwrap();
    let real = String::from_utf8(Command::new("which").arg("tmux").output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_string();
    std::fs::write(
        shim.join("tmux"),
        format!("#!/bin/sh\n[ \"$1\" = -V ] && {{ echo 'tmux 3.1c'; exit 0; }}\nexec \"{real}\" \"$@\"\n"),
    )
    .unwrap();
    Command::new("chmod")
        .arg("+x")
        .arg(shim.join("tmux"))
        .status()
        .unwrap();
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    let keys = load(&w, &path);
    assert!(!keys.contains("display-popup"), "{keys}");
}

#[test]
fn the_popup_binding_is_only_ever_a_guarded_string() {
    let asset = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/tmux.conf");
    let text = std::fs::read_to_string(asset).unwrap();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && l.contains("display-popup"))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = lines[0];
    assert!(line.starts_with("if-shell '"), "{line}");
    assert!(line.contains("-ge 302"), "{line}");
    // The command is the last single-quoted argument, never bare.
    assert!(
        line.ends_with("'bind t display-popup -E -w 90% -h 85% \"cc-moss pick\"'"),
        "{line}"
    );
}
