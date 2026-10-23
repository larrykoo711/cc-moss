//! Process liveness, robust against pid reuse.

use std::process::{Command, Output};
use std::time::Duration;

use crate::command::{output_with_timeout, run_with_timeout};

const PS_TIMEOUT: Duration = Duration::from_millis(500);

/// Start times read from `/proc` carry this prefix, so they are never
/// confused with a `ps` start time recorded by an earlier version.
const PROC_PREFIX: &str = "proc:";

/// The process start time, or `None` if the pid is gone or it cannot be
/// read in time. Only compared for equality.
///
/// On Linux it is `/proc/<pid>/stat`'s start time (no process spawned).
/// Elsewhere it comes from `ps`, rendered in a fixed locale and timezone:
/// hooks and `ls` may run under different environments (F13).
pub fn start_time(pid: u32) -> Option<String> {
    match proc_start_time(pid) {
        ProcFs::Start(s) => Some(s),
        ProcFs::Gone => None,
        ProcFs::Unavailable => ps_start_time(pid),
    }
}

fn ps_start_time(pid: u32) -> Option<String> {
    match ps_liveness(pid, None) {
        (Liveness::Alive, start) => start,
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Dead,
    /// The start time could not be read (`ps` too slow, missing, or not
    /// understanding the flags): we cannot tell.
    Unknown,
}

/// Whether `pid` is still the recorded process. A missing pid is dead; a pid
/// whose start time differs from the recorded one is another process (dead).
pub fn liveness(pid: u32, recorded_start: Option<&str>) -> Liveness {
    let from_proc = !matches!(recorded_start, Some(r) if !r.starts_with(PROC_PREFIX));
    if from_proc {
        match proc_start_time(pid) {
            ProcFs::Start(now) => return compare(&now, recorded_start),
            ProcFs::Gone => return Liveness::Dead,
            ProcFs::Unavailable => {}
        }
    }
    // A `/proc` start time cannot be compared with `ps` output; with `/proc`
    // gone, only ask whether the pid exists.
    let recorded_start = recorded_start.filter(|r| !r.starts_with(PROC_PREFIX));
    ps_liveness(pid, recorded_start).0
}

fn compare(now: &str, recorded_start: Option<&str>) -> Liveness {
    match recorded_start {
        Some(then) if then != now => Liveness::Dead,
        _ => Liveness::Alive,
    }
}

/// Liveness from `ps`, plus the start time it printed.
fn ps_liveness(pid: u32, recorded_start: Option<&str>) -> (Liveness, Option<String>) {
    let out = run_with_timeout(
        Command::new("ps")
            .args(["-o", "stat=", "-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("TZ", "UTC0"),
        PS_TIMEOUT,
    );
    classify_ps(out.ok().as_ref(), recorded_start)
}

/// Read a `ps -o stat= -o lstart= -p PID` result: the state, then the start
/// time. A zombie (state `Z`: exited, not yet reaped, common in containers
/// without an init process) is dead. A pid that is gone makes `ps` exit
/// non-zero quietly. A `ps` that complains on stderr (busybox knows neither
/// `-p` nor `lstart`) says nothing about the process, so that is unknown:
/// calling a live session dead would let restore run it twice.
fn classify_ps(out: Option<&Output>, recorded_start: Option<&str>) -> (Liveness, Option<String>) {
    let Some(out) = out else {
        return (Liveness::Unknown, None);
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let text = text.trim();
    if out.status.success() && !text.is_empty() {
        let (state, now) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        if state.starts_with('Z') {
            return (Liveness::Dead, None);
        }
        let now = now.trim();
        if now.is_empty() {
            return (Liveness::Unknown, None);
        }
        return (compare(now, recorded_start), Some(now.to_string()));
    }
    if !out.stderr.iter().all(u8::is_ascii_whitespace) {
        return (Liveness::Unknown, None);
    }
    (Liveness::Dead, None)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
enum ProcFs {
    Start(String),
    /// `/proc` works but has no such pid.
    Gone,
    /// No usable `/proc` (not Linux, or not mounted): ask `ps`.
    Unavailable,
}

#[cfg(target_os = "linux")]
fn proc_start_time(pid: u32) -> ProcFs {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => parse_proc_stat(&stat),
        Err(_) if std::path::Path::new("/proc/self/stat").exists() => ProcFs::Gone,
        Err(_) => ProcFs::Unavailable,
    }
}

#[cfg(not(target_os = "linux"))]
fn proc_start_time(_pid: u32) -> ProcFs {
    ProcFs::Unavailable
}

/// Field 22 of `/proc/<pid>/stat` (start time in clock ticks after boot).
/// The command name (field 2) may contain spaces and parentheses, so fields
/// are counted from the last `)`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_proc_stat(stat: &str) -> ProcFs {
    let Some(close) = stat.rfind(')') else {
        return ProcFs::Unavailable;
    };
    // Fields from 3 on: the state, …, the start time as the 20th.
    let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    // A zombie has exited and only waits to be reaped (containers without
    // an init process may never reap it): it is gone.
    if matches!(fields.first(), Some(&"Z") | Some(&"X")) {
        return ProcFs::Gone;
    }
    match fields.get(19) {
        Some(ticks) if ticks.parse::<u64>().is_ok() => {
            ProcFs::Start(format!("{PROC_PREFIX}{ticks}"))
        }
        _ => ProcFs::Unavailable,
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

/// The executable image reported by the OS, not a mutable argv/title word.
pub fn executable(pid: u32) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        let mut path = vec![0u8; 4096];
        // SAFETY: writable owned buffer of the supplied size; no process mutation.
        let size = unsafe {
            libc::proc_pidpath(
                pid as libc::c_int,
                path.as_mut_ptr().cast(),
                path.len() as u32,
            )
        };
        if size <= 0 {
            return None;
        }
        let end = path.iter().position(|byte| *byte == 0)?;
        use std::os::unix::ffi::OsStrExt;
        Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
            &path[..end],
        )))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn ps_out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    const START: &str = "Thu Oct 15 09:14:48 2026";

    #[test]
    fn ps_that_rejects_the_flags_is_unknown_not_dead() {
        let busybox = ps_out(
            1,
            "",
            "ps: invalid option -- 'p'\nBusyBox v1.36.1 multi-call binary.\n",
        );
        assert_eq!(
            classify_ps(Some(&busybox), Some(START)).0,
            Liveness::Unknown
        );
        let bad_field = ps_out(1, "", "ps: bad -o argument 'lstart'\n");
        assert_eq!(classify_ps(Some(&bad_field), None).0, Liveness::Unknown);
    }

    #[test]
    fn quiet_ps_failure_is_dead() {
        assert_eq!(
            classify_ps(Some(&ps_out(1, "", "")), Some(START)).0,
            Liveness::Dead
        );
    }

    #[test]
    fn ps_start_time_is_compared() {
        let out = ps_out(0, &format!("Ss   {START}\n"), "");
        assert_eq!(
            classify_ps(Some(&out), Some(START)),
            (Liveness::Alive, Some(START.to_string()))
        );
        assert_eq!(classify_ps(Some(&out), Some("other")).0, Liveness::Dead);
        assert_eq!(classify_ps(Some(&out), None).0, Liveness::Alive);
        assert_eq!(
            classify_ps(None, Some(START)).0,
            Liveness::Unknown,
            "timeout"
        );
    }

    #[test]
    fn proc_stat_start_time_survives_odd_command_names() {
        let tail = "S 1 4242 4242 0 -1 4194560 1234 0 0 0 10 5 0 0 20 0 3 0 987654 1000000 500";
        let start = ProcFs::Start("proc:987654".into());
        assert_eq!(parse_proc_stat(&format!("4242 (claude) {tail}")), start);
        assert_eq!(
            parse_proc_stat(&format!("4242 (a) b (c) {tail}")),
            start,
            "a `)` inside the name"
        );
        assert_eq!(
            parse_proc_stat("4242 (claude) S 1"),
            ProcFs::Unavailable,
            "truncated"
        );
        assert_eq!(parse_proc_stat("garbage"), ProcFs::Unavailable);
    }

    #[test]
    fn zombies_are_dead() {
        let tail = "1 4242 4242 0 -1 4194560 1234 0 0 0 10 5 0 0 20 0 3 0 987654 0 0";
        assert_eq!(
            parse_proc_stat(&format!("4242 (claude) Z {tail}")),
            ProcFs::Gone
        );
        assert_eq!(
            parse_proc_stat(&format!("4242 (Z) S {tail}")),
            ProcFs::Start("proc:987654".into()),
            "a name that looks like a state"
        );
        let zombie = ps_out(0, &format!("Z+   {START}\n"), "");
        assert_eq!(classify_ps(Some(&zombie), Some(START)).0, Liveness::Dead);
    }

    #[test]
    fn start_time_round_trips_through_liveness() {
        let me = std::process::id();
        let start = start_time(me).expect("own start time");
        if cfg!(target_os = "linux") {
            assert!(start.starts_with(PROC_PREFIX), "{start}");
        }
        assert_eq!(liveness(me, Some(&start)), Liveness::Alive);
        if cfg!(target_os = "linux") {
            assert_eq!(liveness(me, Some("proc:1")), Liveness::Dead);
        }
        assert_eq!(
            liveness(me, Some("Thu Jan  1 00:00:00 1970")),
            Liveness::Dead,
            "a `ps` start time from an earlier version still compares"
        );
    }
}
