use std::process::Command;
use std::time::{Duration, Instant};

use moss_core::command::output_with_timeout;
use moss_core::proc::{is_alive, start_time};

#[test]
fn start_time_is_locale_and_timezone_independent() {
    // F13 / reviewer C2: hooks and `ls` can run under different locales.
    let me = std::process::id();
    let before = start_time(me).expect("own process has a start time");
    std::env::set_var("LC_ALL", "zh_CN.UTF-8");
    std::env::set_var("LANG", "de_DE.UTF-8");
    std::env::set_var("TZ", "Asia/Tokyo");
    let after = start_time(me).unwrap();
    assert_eq!(before, after);
    assert!(before.is_ascii(), "{before}");
}

#[test]
fn dead_pid_is_not_alive() {
    let mut child = Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert!(!is_alive(pid, None));
}

#[test]
fn reused_pid_is_not_alive() {
    let me = std::process::id();
    assert!(is_alive(me, start_time(me).as_deref()));
    assert!(!is_alive(me, Some("Thu Jan  1 00:00:00 1970")));
}

#[test]
fn slow_command_times_out() {
    let t = Instant::now();
    let out = output_with_timeout(Command::new("sleep").arg("5"), Duration::from_millis(100));
    assert!(out.is_none());
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
}

#[test]
fn unreaped_child_is_not_alive() {
    // Exited but not waited for: a zombie, as in a container without init.
    let mut child = Command::new("true").spawn().unwrap();
    let pid = child.id();
    std::thread::sleep(Duration::from_millis(300));
    let alive = is_alive(pid, None);
    child.wait().unwrap();
    assert!(!alive, "a zombie counts as dead");
}
