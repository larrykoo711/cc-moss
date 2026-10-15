//! Process liveness, robust against pid reuse.

use std::process::Command;
use std::time::Duration;

use crate::command::output_with_timeout;

const PS_TIMEOUT: Duration = Duration::from_millis(500);

/// The process start time as reported by `ps`, or `None` if the pid is gone
/// or `ps` does not answer in time. Only compared for equality, so it is
/// rendered in a fixed locale and timezone: hooks and `ls` may run under
/// different environments (F13).
pub fn start_time(pid: u32) -> Option<String> {
    let out = output_with_timeout(
        Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("TZ", "UTC0"),
        PS_TIMEOUT,
    )?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Dead,
    /// `ps` did not answer in time: we cannot tell.
    Unknown,
}

/// Whether `pid` is still the recorded process. A missing pid is dead; a pid
/// whose start time differs from the recorded one is another process (dead).
pub fn liveness(pid: u32, recorded_start: Option<&str>) -> Liveness {
    let out = output_with_timeout(
        Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("TZ", "UTC0"),
        PS_TIMEOUT,
    );
    let Some(out) = out else {
        return Liveness::Unknown;
    };
    let now = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || now.is_empty() {
        return Liveness::Dead;
    }
    match recorded_start {
        Some(then) if then != now => Liveness::Dead,
        _ => Liveness::Alive,
    }
}

/// True unless the process is known to be gone. An unanswered `ps` counts as
/// alive: offering a live session for restore would run it twice.
pub fn is_alive(pid: u32, recorded_start: Option<&str>) -> bool {
    liveness(pid, recorded_start) != Liveness::Dead
}

/// When the machine booted, in Unix milliseconds; cached. `None` if unknown.
pub fn boot_time_ms() -> Option<i64> {
    static BOOT: std::sync::OnceLock<Option<i64>> = std::sync::OnceLock::new();
    *BOOT.get_or_init(read_boot_time_ms)
}

#[cfg(target_os = "macos")]
fn read_boot_time_ms() -> Option<i64> {
    // "{ sec = 1792026888, usec = 76729 } Thu Oct 15 09:14:48 2026"
    let out = output_with_timeout(
        Command::new("sysctl").args(["-n", "kern.boottime"]),
        PS_TIMEOUT,
    )?;
    let text = String::from_utf8_lossy(&out.stdout);
    let sec = text.split("sec = ").nth(1)?.split(',').next()?.trim();
    sec.parse::<i64>().ok().map(|s| s * 1000)
}

#[cfg(not(target_os = "macos"))]
fn read_boot_time_ms() -> Option<i64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().find(|l| l.starts_with("btime "))?;
    line[6..].trim().parse::<i64>().ok().map(|s| s * 1000)
}

/// Every process's parent, from one `ps` call.
pub fn parents() -> std::collections::HashMap<u32, u32> {
    let Some(out) = output_with_timeout(
        Command::new("ps").args(["-A", "-o", "pid=,ppid="]),
        Duration::from_secs(2),
    ) else {
        return Default::default();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some((f.next()?.parse().ok()?, f.next()?.parse().ok()?))
        })
        .collect()
}
