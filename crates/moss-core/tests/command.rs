//! Helper processes: output of any size, and why a run failed.

use std::process::Command;
use std::time::{Duration, Instant};

use moss_core::command::{output_with_timeout, run_with_timeout, RunError};

#[test]
fn large_output_does_not_deadlock() {
    // Far beyond a pipe buffer, on both streams at once.
    let t = Instant::now();
    let out = run_with_timeout(
        Command::new("sh").args([
            "-c",
            "head -c 2000000 /dev/zero; head -c 1500000 /dev/zero >&2",
        ]),
        Duration::from_secs(10),
    )
    .expect("finishes");
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), 2_000_000);
    assert_eq!(out.stderr.len(), 1_500_000);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());

    let out = output_with_timeout(
        Command::new("sh").args(["-c", "head -c 1200000 /dev/zero"]),
        Duration::from_secs(10),
    )
    .expect("the Option form copes too");
    assert_eq!(out.stdout.len(), 1_200_000);
}

#[test]
fn missing_program_is_not_a_timeout() {
    let err = run_with_timeout(
        &mut Command::new("/nonexistent/cc-moss-test-tool"),
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(err.is_not_installed(), "{err:?}");
    assert!(output_with_timeout(
        &mut Command::new("/nonexistent/cc-moss-test-tool"),
        Duration::from_secs(5)
    )
    .is_none());
}

#[test]
fn slow_program_times_out() {
    let t = Instant::now();
    let err =
        run_with_timeout(Command::new("sleep").arg("5"), Duration::from_millis(100)).unwrap_err();
    assert!(matches!(err, RunError::TimedOut), "{err:?}");
    assert!(!err.is_not_installed());
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
}

#[test]
fn stderr_is_captured() {
    let out = run_with_timeout(
        Command::new("sh").args(["-c", "echo usage >&2; exit 1"]),
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(!out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stderr), "usage\n");
}
