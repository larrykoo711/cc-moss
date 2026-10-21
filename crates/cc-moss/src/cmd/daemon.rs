//! `cc-moss daemon`: one resident process per user, kept alive by launchd.
//!
//! `run` is what launchd starts: it takes the single-instance lock, writes
//! its status and a heartbeat, and stops cleanly on SIGTERM. `start`, `stop`
//! and `status` are for people and the app; `install` and `uninstall` place
//! and remove the LaunchAgent. While it runs it looks after Moss
//! ([`super::moss::Looker`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use moss_core::daemon::{self, lock, plist, status, Paths, LABEL};
use moss_core::platform::Platform;
use moss_core::proc;
use moss_core::slot::{now_ms, rfc3339};

use super::{app, install, setup};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------- run

/// `cc-moss daemon run`, in the foreground. False when another daemon
/// already holds the lock (exit 1).
pub fn run() -> Result<bool> {
    // Files root made here would lock the user's own daemon out for good.
    not_as_root()?;
    let paths = Paths::current();
    moss_core::store_dir::ensure_private_dir(&paths.dir)
        .with_context(|| format!("{}", paths.dir.display()))?;
    let Some(_lock) =
        lock::try_acquire(&paths.lock).with_context(|| format!("{}", paths.lock.display()))?
    else {
        let pid = status::read(&paths.status).map(|s| s.pid);
        let line = match pid {
            Some(pid) => format!("another daemon is running (pid {pid}); exiting"),
            None => "another daemon is running; exiting".to_string(),
        };
        let _ = daemon::append_log(&paths.log, &stamp(&line));
        if under_launchd() {
            // launchd starts this copy again 30 seconds after each exit for
            // as long as another daemon (one run in a terminal) holds the
            // lock: wait between tries, and keep its stderr.log small.
            for out in [&paths.stdout, &paths.stderr] {
                let _ = daemon::truncate_if_large(out);
            }
            std::thread::sleep(busy_wait());
            return Ok(false);
        }
        eprintln!(
            "{}",
            match pid {
                Some(pid) => tr!(
                    format!("cc-moss daemon is already running (pid {pid})."),
                    format!("cc-moss 守护进程已经在运行了（pid {pid}）。")
                ),
                None => tr!(
                    "cc-moss daemon is already running.".to_string(),
                    "cc-moss 守护进程已经在运行了。".to_string()
                ),
            }
        );
        return Ok(false);
    };
    for out in [&paths.stdout, &paths.stderr] {
        let _ = daemon::truncate_if_large(out);
    }
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&stop))
            .context("cannot listen for signals")?;
    }

    let pid = std::process::id();
    let started_ms = now_ms();
    let mut stored = status::Stored {
        schema: 1,
        pid,
        // `ps` may be slow right after login; without a start time the
        // status leans on the heartbeat alone.
        pid_start: (0..3).find_map(|_| proc::start_time(pid)),
        version: VERSION.to_string(),
        started_ms,
        heartbeat_ms: started_ms,
    };
    status::write(&paths.status, &stored).with_context(|| format!("{}", paths.status.display()))?;
    log(&paths, &format!("started: pid {pid}, version {VERSION}"));

    let request_worker = super::api_failure::worker(Arc::clone(&stop));
    let beat = heartbeat();
    let mut next = Instant::now() + beat;
    let say = |line: &str| log(&paths, line);
    let mut moss = super::moss::Looker::new(&moss_core::store_dir::state_dir(), &say);
    let mut tidy = super::archive::Tidy::new(&moss_core::store_dir::state_dir());
    let mut provider = super::provider::Watcher::new(&moss_core::store_dir::state_dir());
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(moss) = moss.as_mut() {
            moss.tick(Instant::now(), &say);
        }
        tidy.tick(Instant::now(), &say);
        provider.tick(Instant::now(), &say);
        if Instant::now() >= next {
            stored.heartbeat_ms = now_ms();
            if let Err(e) = status::write(&paths.status, &stored) {
                log(&paths, &format!("error: heartbeat: {e}"));
            }
            next = Instant::now() + beat;
        }
    }
    request_worker.thread().unpark();
    if request_worker.join().is_err() {
        log(&paths, "error: request scan worker stopped unexpectedly");
    }
    log(&paths, "stopping: asked to");
    let _ = status::remove_if_ours(&paths.status, pid);
    Ok(true)
}

/// Every 30 seconds; debug builds (the tests) may say otherwise.
fn heartbeat() -> Duration {
    test_millis("CC_MOSS_DAEMON_HEARTBEAT_MS").unwrap_or(daemon::HEARTBEAT)
}

/// How long a copy launchd started waits when another daemon has the lock.
fn busy_wait() -> Duration {
    test_millis("CC_MOSS_DAEMON_BUSY_WAIT_MS").unwrap_or(Duration::from_secs(5 * 60))
}

/// A duration in milliseconds that only debug builds read from `var`.
fn test_millis(var: &str) -> Option<Duration> {
    app::test_override(var)
        .and_then(|v| v.to_str()?.parse().ok())
        .map(Duration::from_millis)
}

/// launchd names the job it started in `XPC_SERVICE_NAME`.
fn under_launchd() -> bool {
    std::env::var("XPC_SERVICE_NAME").as_deref() == Ok(LABEL)
}

fn stamp(line: &str) -> String {
    format!("{} {line}", rfc3339(now_ms()))
}

fn log(paths: &Paths, line: &str) {
    let _ = daemon::append_log(&paths.log, &stamp(line));
}

// ---------------------------------------------------------------- status

/// What `status` says, from the status file and whether its pid is still
/// that process.
pub fn report() -> status::Report {
    let stored = status::read(&daemon_paths().status);
    status::report(stored.as_ref(), now_ms(), proc::liveness)
}

/// The daemon's files. Where its LaunchAgent says when there is one: the
/// app (started from the Dock) and a shell with its own CC_MOSS_STATE_DIR
/// or XDG_STATE_HOME then still look where the daemon writes.
fn daemon_paths() -> Paths {
    Paths::new(&state())
}

/// The state directory the daemon uses: its LaunchAgent's, else this one.
pub fn state() -> PathBuf {
    pinned_state().unwrap_or_else(moss_core::store_dir::state_dir)
}

/// The PATH the installed LaunchAgent gives the daemon.
pub fn pinned_path() -> Option<String> {
    pinned_value("PATH")
}

/// The config file the installed LaunchAgent pins.
pub fn pinned_config() -> Option<PathBuf> {
    pinned_value("CC_MOSS_CONFIG").map(PathBuf::from)
}

/// One of the installed LaunchAgent's environment variables.
fn pinned_value(name: &str) -> Option<String> {
    if !applies() {
        return None;
    }
    let text = std::fs::read_to_string(Launchd::plist_path().ok()?).ok()?;
    plist::environment_value(&text, name).filter(|s| !s.is_empty())
}

/// The state directory the installed LaunchAgent pins.
fn pinned_state() -> Option<PathBuf> {
    pinned_value("CC_MOSS_STATE_DIR").map(PathBuf::from)
}

/// `cc-moss daemon status [--json]`.
pub fn print_status(json: bool) -> Result<()> {
    let r = report();
    if json {
        println!("{}", serde_json::to_string(&r)?);
        return Ok(());
    }
    println!("{}", describe(&r));
    if let Some(have) = r.version.as_deref().filter(|v| *v != VERSION) {
        println!(
            "{}",
            tr!(
                format!("It runs {have}; `cc-moss install` swaps in {VERSION}."),
                format!("它跑的是 {have}；运行 `cc-moss install` 换成 {VERSION}。")
            )
        );
    }
    Ok(())
}

/// One line about the daemon, for `status` and `doctor`.
pub fn describe(r: &status::Report) -> String {
    if !r.running {
        return tr!(
            "The cc-moss daemon is not running. Start it: cc-moss daemon start",
            "cc-moss 守护进程没在运行。启动它：cc-moss daemon start"
        )
        .to_string();
    }
    let up = uptime(r).unwrap_or_else(|| "?".into());
    let pid = r.pid.unwrap_or_default();
    let version = r.version.as_deref().unwrap_or("?");
    tr!(
        format!("The cc-moss daemon is running: up {up}, pid {pid}, version {version}."),
        format!("cc-moss 守护进程运行中：已运行 {up}，pid {pid}，版本 {version}。")
    )
}

/// `3 hours`, `5 minutes`: how long it has been up.
pub fn uptime(r: &status::Report) -> Option<String> {
    Some(span(now_ms() - r.started_ms?))
}

pub fn span(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let unit = |n: i64, en: &str, zh: &str| {
        tr!(
            format!("{n} {en}{}", if n == 1 { "" } else { "s" }),
            format!("{n} {zh}")
        )
    };
    match s {
        0..=59 => unit(s, "second", "秒"),
        60..=3599 => unit(s / 60, "minute", "分钟"),
        3600..=86_399 => unit(s / 3600, "hour", "小时"),
        _ => unit(s / 86_400, "day", "天"),
    }
}

// ---------------------------------------------------------------- launchd

/// What cc-moss asks launchd to do. The real one runs `launchctl`; tests
/// get one that writes each call down and runs nothing.
pub trait Launchctl {
    fn bootstrap(&self, plist: &Path) -> Result<()>;
    fn bootout(&self) -> Result<()>;
    /// `kill`: stop a running instance first (`-k`), to swap in a new binary.
    fn kickstart(&self, kill: bool) -> Result<()>;
    fn enable(&self) -> Result<()>;
    fn disable(&self) -> Result<()>;
    fn is_loaded(&self) -> bool;
    fn is_disabled(&self) -> bool;
    /// Whether launchd really acts on these calls (worth waiting for).
    fn is_real(&self) -> bool;
}

/// `launchctl` in the user's GUI domain.
struct RealLaunchctl {
    domain: String,
}

/// Tests set `CC_MOSS_FORBID_LAUNCHCTL` for every run of the binary: in a
/// debug build, reaching the real launchctl then panics, so a test that would
/// touch the developer's own launchd fails loudly instead.
fn forbid_in_tests() {
    if cfg!(debug_assertions) && std::env::var_os("CC_MOSS_FORBID_LAUNCHCTL").is_some() {
        panic!("a test reached the real /bin/launchctl (CC_MOSS_FORBID_LAUNCHCTL is set)");
    }
}

impl RealLaunchctl {
    fn new() -> Self {
        forbid_in_tests();
        // SAFETY: getuid has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        RealLaunchctl {
            domain: format!("gui/{uid}"),
        }
    }

    fn service(&self) -> String {
        format!("{}/{LABEL}", self.domain)
    }

    fn call(&self, args: &[&str]) -> Result<()> {
        forbid_in_tests();
        let out = std::process::Command::new("/bin/launchctl")
            .args(args)
            .output()
            .context("cannot run launchctl")?;
        if out.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&out.stderr);
        bail!(
            "launchctl {} failed ({}): {}",
            args.join(" "),
            out.status,
            said.trim()
        )
    }
}

impl Launchctl for RealLaunchctl {
    fn bootstrap(&self, plist: &Path) -> Result<()> {
        let plist = plist.to_string_lossy();
        // Right after a bootout launchd may still be letting go of the old
        // instance and answer EIO for a moment.
        let mut last = None;
        for _ in 0..10 {
            match self.call(&["bootstrap", &self.domain, &plist]) {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
            if self.is_loaded() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("launchctl bootstrap failed")))
    }
    fn bootout(&self) -> Result<()> {
        self.call(&["bootout", &self.service()])
    }
    fn kickstart(&self, kill: bool) -> Result<()> {
        let service = self.service();
        if kill {
            self.call(&["kickstart", "-k", &service])
        } else {
            self.call(&["kickstart", &service])
        }
    }
    fn enable(&self) -> Result<()> {
        self.call(&["enable", &self.service()])
    }
    fn disable(&self) -> Result<()> {
        self.call(&["disable", &self.service()])
    }
    fn is_loaded(&self) -> bool {
        forbid_in_tests();
        std::process::Command::new("/bin/launchctl")
            .args(["print", &self.service()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    fn is_disabled(&self) -> bool {
        forbid_in_tests();
        std::process::Command::new("/bin/launchctl")
            .args(["print-disabled", &self.domain])
            .output()
            .is_ok_and(|o| disabled_in(&String::from_utf8_lossy(&o.stdout), LABEL))
    }
    fn is_real(&self) -> bool {
        true
    }
}

/// Whether `launchctl print-disabled` lists `label` as disabled. macOS 13
/// and later say `=> disabled`, earlier ones `=> true`.
fn disabled_in(listing: &str, label: &str) -> bool {
    let quoted = format!("\"{label}\"");
    listing.lines().any(|l| {
        let l = l.trim();
        l.starts_with(&quoted) && (l.ends_with("=> disabled") || l.ends_with("=> true"))
    })
}

/// Writes each call to a file and runs nothing. What is loaded or disabled
/// follows from the calls written so far. The calls named in
/// `CC_MOSS_LAUNCHCTL_FAIL` (comma separated) fail instead, unwritten.
struct RecordingLaunchctl {
    log: PathBuf,
    fail: Vec<String>,
}

impl RecordingLaunchctl {
    fn new(log: PathBuf) -> Self {
        let fail = app::test_override("CC_MOSS_LAUNCHCTL_FAIL")
            .map(|v| {
                v.to_string_lossy()
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        RecordingLaunchctl { log, fail }
    }

    fn record(&self, call: &str) -> Result<()> {
        use std::io::Write;
        let verb = call.split_whitespace().next().unwrap_or_default();
        if self.fail.iter().any(|f| f == verb) {
            bail!("launchctl {verb} failed (as the test asked)");
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)?
            .write_all(format!("{call}\n").as_bytes())?;
        Ok(())
    }

    fn last_of(&self, a: &str, b: &str) -> Option<String> {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        text.lines()
            .rev()
            .map(|l| l.split_whitespace().next().unwrap_or_default().to_string())
            .find(|w| w == a || w == b)
    }
}

impl Launchctl for RecordingLaunchctl {
    fn bootstrap(&self, plist: &Path) -> Result<()> {
        self.record(&format!("bootstrap {}", plist.display()))
    }
    fn bootout(&self) -> Result<()> {
        self.record("bootout")
    }
    fn kickstart(&self, kill: bool) -> Result<()> {
        self.record(if kill { "kickstart -k" } else { "kickstart" })
    }
    fn enable(&self) -> Result<()> {
        self.record("enable")
    }
    fn disable(&self) -> Result<()> {
        self.record("disable")
    }
    fn is_loaded(&self) -> bool {
        self.last_of("bootstrap", "bootout").as_deref() == Some("bootstrap")
    }
    fn is_disabled(&self) -> bool {
        self.last_of("enable", "disable").as_deref() == Some("disable")
    }
    fn is_real(&self) -> bool {
        false
    }
}

/// Where the LaunchAgent goes and who to tell about it.
pub struct Launchd {
    pub plist: PathBuf,
    /// Nobody to tell for a home that is not this account's own (a test's):
    /// launchd never reads LaunchAgents from there.
    ctl: Option<Box<dyn Launchctl>>,
}

impl Launchd {
    /// `CC_MOSS_LAUNCHD_DIR` (debug builds only) puts the plist in that
    /// folder and records launchctl calls in `launchctl.log` there instead
    /// of making them. Otherwise `~/Library/LaunchAgents`, and the real
    /// launchctl only when `HOME` is this account's home.
    pub fn detect() -> Result<Launchd> {
        let plist = Self::plist_path()?;
        if let Some(dir) = launchd_dir() {
            return Ok(Launchd {
                plist,
                ctl: Some(Box::new(RecordingLaunchctl::new(dir.join("launchctl.log")))),
            });
        }
        let ctl: Option<Box<dyn Launchctl>> = if cfg!(target_os = "macos") && is_own_home(&home()?)
        {
            Some(Box::new(RealLaunchctl::new()))
        } else {
            None
        };
        Ok(Launchd { plist, ctl })
    }

    /// Where the LaunchAgent goes, without asking anything of launchd.
    fn plist_path() -> Result<PathBuf> {
        let file = format!("{LABEL}.plist");
        Ok(match launchd_dir() {
            Some(dir) => dir.join(file),
            None => home()?.join("Library/LaunchAgents").join(file),
        })
    }
}

fn launchd_dir() -> Option<PathBuf> {
    app::test_override("CC_MOSS_LAUNCHD_DIR").filter(|d| !d.as_os_str().is_empty())
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .context("HOME is not set")
}

/// Whether `home` is this account's home directory. Unsure means no: then
/// cc-moss never registers anything.
fn is_own_home(home: &Path) -> bool {
    account_home().is_some_and(|real| same_dir(&real, home))
}

/// This account's home directory as the user database has it (network and
/// mobile accounts included), whatever `HOME` says.
fn account_home() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: getpwuid_r writes into `pwd` and `buf`, both ours and alive
    // for the call; `pw_dir` points into `buf` and is read before it goes.
    unsafe {
        let uid = libc::getuid();
        let mut buf = vec![0 as libc::c_char; 4096];
        loop {
            let mut pwd: libc::passwd = std::mem::zeroed();
            let mut found: *mut libc::passwd = std::ptr::null_mut();
            let rc = libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut found);
            if rc == libc::ERANGE && buf.len() < 1 << 20 {
                buf.resize(buf.len() * 2, 0);
                continue;
            }
            if rc != 0 || found.is_null() || pwd.pw_dir.is_null() {
                return None;
            }
            let dir = std::ffi::CStr::from_ptr(pwd.pw_dir).to_bytes();
            return (!dir.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(dir)));
        }
    }
}

/// Whether two paths name the same folder: through symlinks, and with or
/// without a trailing slash.
fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.components().collect());
    canon(a) == canon(b)
}

/// launchd manages the daemon on macOS only, and never for root.
pub fn applies() -> bool {
    install::platform() == Platform::MacOs
}

/// The LaunchAgent for this cc-moss and state directory.
pub fn agent_plist() -> Result<String> {
    let state = moss_core::store_dir::state_dir();
    if !state.is_absolute() {
        bail!(moss_core::store_dir::NOT_ABSOLUTE);
    }
    let exe = setup::hook_exe()?;
    let path = daemon_search_path(
        pinned_path().as_deref(),
        std::env::var("PATH").ok().as_deref(),
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .as_deref(),
        Path::new(&exe).parent(),
    );
    let config = moss_core::config::config_path();
    let config = Some(config).filter(|c| c.is_absolute());
    Ok(plist::render(&agent(
        &exe,
        &state,
        &path,
        config.as_deref(),
    )))
}

fn agent(exe: &str, state: &Path, path: &str, config: Option<&Path>) -> plist::Agent {
    let paths = Paths::new(state);
    plist::Agent {
        label: LABEL.to_string(),
        program_arguments: vec![exe.to_string(), "daemon".into(), "run".into()],
        stdout: paths.stdout,
        stderr: paths.stderr,
        // launchd does not start the daemon from the user's shell: pin the
        // state directory so the daemon, the CLI and the app agree on it,
        // and a PATH where Moss finds `claude` and `tmux`.
        environment: vec![
            (
                "CC_MOSS_STATE_DIR".to_string(),
                state.to_string_lossy().into_owned(),
            ),
            ("PATH".to_string(), path.to_string()),
        ]
        .into_iter()
        // And the config file, so the switch `moss stop` writes is the one
        // the daemon reads, whatever XDG_CONFIG_HOME a shell has.
        .chain(config.map(|c| {
            (
                "CC_MOSS_CONFIG".to_string(),
                c.to_string_lossy().into_owned(),
            )
        }))
        .collect(),
    }
}

/// The daemon's PATH: this process's PATH, then cc-moss's own folder and
/// where Homebrew and Claude Code's installers put programs, each folder
/// once. Rebuilt on every install, so a folder that left the user's PATH
/// (an old nvm) leaves the daemon's too. When the result holds the same
/// folders as the installed LaunchAgent's, that one is kept as it is, so
/// installing from another shell or the app does not reshuffle it.
fn daemon_search_path(
    pinned: Option<&str>,
    current: Option<&str>,
    home: Option<&Path>,
    exe_dir: Option<&Path>,
) -> String {
    let mut dirs: Vec<String> = Vec::new();
    let mut add = |d: String| {
        let d = d.trim_end_matches('/').to_string();
        if !d.is_empty() && d.starts_with('/') && !dirs.contains(&d) {
            dirs.push(d);
        }
    };
    for d in current.unwrap_or_default().split(':') {
        add(d.to_string());
    }
    if let Some(dir) = exe_dir {
        add(dir.to_string_lossy().into_owned());
    }
    add("/opt/homebrew/bin".into());
    add("/usr/local/bin".into());
    if let Some(home) = home {
        add(home.join(".local/bin").to_string_lossy().into_owned());
        add(home.join(".claude/local").to_string_lossy().into_owned());
    }
    for d in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        add(d.into());
    }
    if let Some(pinned) = pinned {
        let mut old: Vec<&str> = pinned.split(':').collect();
        let mut new: Vec<&str> = dirs.iter().map(String::as_str).collect();
        old.sort_unstable();
        new.sort_unstable();
        if old == new {
            return pinned.to_string();
        }
    }
    dirs.join(":")
}

/// Write the plist if it differs. True when it changed.
fn place_plist(launchd: &Launchd, text: &str) -> Result<bool> {
    if std::fs::read_to_string(&launchd.plist).ok().as_deref() == Some(text) {
        return Ok(false);
    }
    if let Some(dir) = launchd.plist.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("{}", dir.display()))?;
    }
    std::fs::write(&launchd.plist, text).with_context(|| format!("{}", launchd.plist.display()))?;
    Ok(true)
}

/// `cc-moss install`'s daemon step: place the LaunchAgent and have launchd
/// run it; after an upgrade, swap in the new binary. Problems are told and
/// never stop the rest of install.
pub fn install_step(dry_run: bool) -> Result<()> {
    if !applies() || app::as_root() {
        return Ok(());
    }
    let (launchd, text) = match Launchd::detect().and_then(|l| Ok((l, agent_plist()?))) {
        Ok(ready) => ready,
        Err(e) => {
            println!(
                "\n{}",
                tr!(
                    format!("Left the cc-moss daemon out: {e:#}."),
                    format!("没有设置 cc-moss 守护进程：{e:#}。")
                )
            );
            return Ok(());
        }
    };
    if dry_run {
        if std::fs::read_to_string(&launchd.plist).ok().as_deref() != Some(text.as_str()) {
            println!(
                "\n{}",
                tr!(
                    format!(
                        "Would set up the cc-moss daemon: {}",
                        launchd.plist.display()
                    ),
                    format!("会设置 cc-moss 守护进程：{}", launchd.plist.display())
                )
            );
        }
        return Ok(());
    }
    if let Err(e) = bring_up(&launchd, &text) {
        println!(
            "\n{}",
            tr!(
                format!("The cc-moss daemon did not start: {e:#}. Try `cc-moss daemon start`."),
                format!("cc-moss 守护进程没能启动：{e:#}。试试 `cc-moss daemon start`。")
            )
        );
    }
    Ok(())
}

fn bring_up(launchd: &Launchd, text: &str) -> Result<()> {
    let state = moss_core::store_dir::state_dir();
    let before = pinned_state();
    moss_core::store_dir::ensure_private_dir(&Paths::new(&state).dir)?;
    let changed = place_plist(launchd, text)?;
    if let Some(old) = before.filter(|old| changed && !same_dir(old, &state)) {
        println!(
            "\n{}",
            tr!(
                format!(
                    "The cc-moss daemon now keeps its files in {} (before: {}).",
                    state.display(),
                    old.display()
                ),
                format!(
                    "cc-moss 守护进程的文件现在放在 {}（之前在 {}）。",
                    state.display(),
                    old.display()
                )
            )
        );
    }
    let Some(ctl) = &launchd.ctl else {
        println!("\n{}", not_own_home_note(&launchd.plist));
        return Ok(());
    };
    if ctl.is_disabled() {
        println!(
            "\n{}",
            tr!(
                "The cc-moss daemon stays stopped, as you left it. To start it: cc-moss daemon start",
                "cc-moss 守护进程保持停止，跟你上次设的一样。要启动：cc-moss daemon start"
            )
        );
        return Ok(());
    }
    let said = if !ctl.is_loaded() {
        ctl.bootstrap(&launchd.plist)?;
        Some(tr!(
            "Started the cc-moss daemon; it starts again at login and if it stops.",
            "已启动 cc-moss 守护进程；登录时、意外退出后都会自己回来。"
        ))
    } else if changed {
        // launchd keeps the plist it loaded: load the new one.
        ctl.bootout()?;
        ctl.bootstrap(&launchd.plist)?;
        Some(tr!(
            "Updated the cc-moss daemon.",
            "已更新 cc-moss 守护进程。"
        ))
    } else {
        let r = report();
        if !r.running {
            ctl.kickstart(false)?;
            Some(tr!(
                "Started the cc-moss daemon.",
                "已启动 cc-moss 守护进程。"
            ))
        } else if r.version.as_deref() != Some(VERSION) {
            ctl.kickstart(true)?;
            Some(tr!(
                "Restarted the cc-moss daemon on the new version.",
                "已用新版本重启 cc-moss 守护进程。"
            ))
        } else {
            None
        }
    };
    if let Some(said) = said {
        println!("\n{said}");
    }
    Ok(())
}

/// Why launchd was left out: a plist under a home that is not this
/// account's is never read.
fn not_own_home_note(plist: &Path) -> String {
    tr!(
        format!(
            "Wrote {} but did not start the cc-moss daemon: HOME is not your account's home folder, so launchd never looks there.",
            plist.display()
        ),
        format!(
            "已写好 {}，但没有启动 cc-moss 守护进程：HOME 不是你这个账户的家目录，launchd 不会去那里找。",
            plist.display()
        )
    )
}

/// `cc-moss install --uninstall`'s daemon step: stop it, forget a `daemon
/// stop`, and remove the plist. A step that fails is told and the rest
/// still happens.
pub fn uninstall_step(dry_run: bool) -> Result<()> {
    if !applies() {
        return Ok(());
    }
    if app::as_root() {
        if Launchd::plist_path().is_ok_and(|p| p.exists()) {
            println!(
                "\n{}",
                tr!(
                    "Running as root, so the cc-moss daemon was left alone: run `cc-moss install --uninstall` without sudo to remove it.",
                    "正在以 root 身份运行，所以没有动 cc-moss 守护进程：不加 sudo 运行 `cc-moss install --uninstall` 就能移除它。"
                )
            );
        }
        return Ok(());
    }
    let launchd = Launchd::detect()?;
    let ctl = launchd.ctl.as_ref();
    let loaded = ctl.is_some_and(|c| c.is_loaded());
    let disabled = ctl.is_some_and(|c| c.is_disabled());
    let present = launchd.plist.exists();
    if !present && !loaded && !disabled {
        return Ok(());
    }
    if dry_run {
        println!(
            "\n{}",
            tr!(
                format!(
                    "Would stop the cc-moss daemon and delete {}.",
                    launchd.plist.display()
                ),
                format!(
                    "会停掉 cc-moss 守护进程，并删除 {}。",
                    launchd.plist.display()
                )
            )
        );
        return Ok(());
    }
    let warn = |e: anyhow::Error| {
        println!(
            "\n{}",
            tr!(
                format!("Could not stop the cc-moss daemon: {e:#}. It won't start again after you log out."),
                format!("没能停掉 cc-moss 守护进程：{e:#}。注销重新登录后它就不会再启动了。")
            )
        )
    };
    let mut stopped = false;
    if let Some(ctl) = ctl {
        if loaded {
            match ctl.bootout() {
                Ok(()) => stopped = true,
                Err(e) => warn(e),
            }
        }
        // A `daemon stop` is remembered by launchd past the plist: forget
        // it, so installing again starts the daemon again.
        if disabled {
            let _ = ctl.enable();
        }
    }
    let mut deleted = false;
    if present {
        match std::fs::remove_file(&launchd.plist) {
            Ok(()) => deleted = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => println!(
                "\n{}",
                tr!(
                    format!("Could not delete {}: {e}.", launchd.plist.display()),
                    format!("没能删除 {}：{e}。", launchd.plist.display())
                )
            ),
        }
    }
    let plist = launchd.plist.display();
    let said = match (stopped, deleted) {
        (true, true) => tr!(
            format!("Stopped the cc-moss daemon and deleted {plist}."),
            format!("已停掉 cc-moss 守护进程，并删除 {plist}。")
        ),
        (true, false) => tr!(
            "Stopped the cc-moss daemon.".to_string(),
            "已停掉 cc-moss 守护进程。".to_string()
        ),
        (false, true) if ctl.is_none() => tr!(
            format!("Deleted {plist}. launchd was not asked to stop the daemon: HOME is not your account's home folder."),
            format!("已删除 {plist}。没有让 launchd 停掉守护进程：HOME 不是你这个账户的家目录。")
        ),
        (false, true) => tr!(format!("Deleted {plist}."), format!("已删除 {plist}。")),
        (false, false) => return Ok(()),
    };
    println!("{said}");
    Ok(())
}

/// After `cc-moss update` put a new binary in place: have launchd restart
/// a running daemon on it. One you stopped stays stopped.
pub fn follow_update() {
    if !applies() || app::as_root() {
        return;
    }
    let Ok(launchd) = Launchd::detect() else {
        return;
    };
    let Some(ctl) = &launchd.ctl else {
        return;
    };
    if !launchd.plist.exists() || ctl.is_disabled() || !ctl.is_loaded() {
        return;
    }
    match ctl.kickstart(true) {
        Ok(()) => println!(
            "{}",
            tr!(
                "Restarted the cc-moss daemon on the new version.",
                "已用新版本重启 cc-moss 守护进程。"
            )
        ),
        Err(e) => println!(
            "{}",
            tr!(
                format!("Could not restart the cc-moss daemon: {e:#}. `cc-moss install` swaps in the new version."),
                format!("没能重启 cc-moss 守护进程：{e:#}。运行 `cc-moss install` 换上新版本。")
            )
        ),
    }
}

/// Whether the LaunchAgent is in place (for doctor).
pub fn plist_present() -> bool {
    Launchd::plist_path().is_ok_and(|p| p.exists())
}

// ---------------------------------------------------------------- start / stop

const NOT_OWN_HOME: &str =
    "HOME is not this account's home directory, so launchd would never see the daemon";

fn not_as_root() -> Result<()> {
    if app::as_root() {
        bail!(tr!(
            "the daemon runs as you, not as root: run this without sudo",
            "守护进程以你自己的身份运行，不用 root：去掉 sudo 再运行"
        ));
    }
    Ok(())
}

fn need_launchd() -> Result<()> {
    if !applies() {
        bail!(tr!(
            "launchd runs the daemon on macOS only; elsewhere run `cc-moss daemon run` yourself",
            "只有 macOS 上由 launchd 托管守护进程；其他系统请自己运行 `cc-moss daemon run`"
        ));
    }
    not_as_root()
}

/// `cc-moss daemon start`: set it up if needed and have launchd run it.
pub fn start() -> Result<()> {
    need_launchd()?;
    let launchd = Launchd::detect()?;
    let ctl = launchd.ctl.as_ref().context(NOT_OWN_HOME)?;
    let state = moss_core::store_dir::state_dir();
    // A LaunchAgent pinning another state directory was set up from a shell
    // that had its own: keep it rather than move the daemon behind its back
    // (the app, started from the Dock, has neither variable).
    let changed = match pinned_state().filter(|p| !same_dir(p, &state)) {
        Some(pinned) => {
            println!(
                "{}",
                tr!(
                    format!(
                        "The daemon uses {}, not {} as this terminal would. Kept it that way; to switch, run `cc-moss install` here.",
                        pinned.display(),
                        state.display()
                    ),
                    format!(
                        "守护进程用的是 {}，不是这个终端会用的 {}。先保持不变；要换过来，在这里运行 `cc-moss install`。",
                        pinned.display(),
                        state.display()
                    )
                )
            );
            false
        }
        None => place_plist(&launchd, &agent_plist()?)?,
    };
    moss_core::store_dir::ensure_private_dir(&daemon_paths().dir)?;
    if ctl.is_disabled() {
        ctl.enable()?;
    }
    // launchd may be starting it right now and refuse a kickstart: then
    // what the daemon does next decides.
    let mut refused = None;
    if !ctl.is_loaded() {
        ctl.bootstrap(&launchd.plist)?;
    } else if changed {
        ctl.bootout()?;
        ctl.bootstrap(&launchd.plist)?;
    } else if !report().running {
        refused = ctl.kickstart(false).err();
    }
    if !ctl.is_real() {
        if let Some(e) = refused {
            return Err(e);
        }
        println!(
            "{}",
            tr!(
                "Asked launchd to start the daemon.",
                "已请 launchd 启动守护进程。"
            )
        );
        return Ok(());
    }
    let r = wait_for(true);
    if r.running {
        println!("{}", describe(&r));
        return Ok(());
    }
    if let Some(e) = refused {
        return Err(e);
    }
    let log = daemon_paths().log;
    bail!(tr!(
        format!("launchd did not bring the daemon up; see {}", log.display()),
        format!("launchd 没能把守护进程拉起来；看看 {}", log.display())
    ))
}

/// `cc-moss daemon stop`: stop it until `daemon start`, across logins too.
pub fn stop() -> Result<()> {
    need_launchd()?;
    let launchd = Launchd::detect()?;
    let ctl = launchd.ctl.as_ref().context(NOT_OWN_HOME)?;
    ctl.disable()?;
    if ctl.is_loaded() {
        ctl.bootout()?;
    }
    if ctl.is_real() && wait_for(false).running {
        bail!(tr!(
            "the daemon is still running; try again in a moment",
            "守护进程还在运行；稍等一下再试一次"
        ));
    }
    println!(
        "{}",
        tr!(
            "Stopped the cc-moss daemon. It stays off until `cc-moss daemon start`.",
            "cc-moss 守护进程已停止；在你运行 `cc-moss daemon start` 之前不会再启动。"
        )
    );
    // Moss runs on its own tmux server: without the daemon nobody looks
    // after it, so it stops too (its switch stays as it is, and `daemon
    // start` brings it back). An upgrade restarts the daemon with
    // `kickstart -k` instead, which leaves Moss running.
    match super::moss::end_with_daemon() {
        Ok(true) => println!("{}", tr!("Moss stopped with it.", "Moss 也跟着停下了。")),
        Ok(false) => {}
        Err(e) => eprintln!(
            "{}",
            tr!(
                format!("Could not stop Moss: {e:#}"),
                format!("没能停掉 Moss：{e:#}")
            )
        ),
    }
    Ok(())
}

/// The report once it says `running`, or after five seconds.
fn wait_for(running: bool) -> status::Report {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let r = report();
        if r.running == running || Instant::now() >= until {
            return r;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_plist_contents() {
        let text = plist::render(&agent(
            "/Users/someone/My Tools/cc-moss",
            Path::new("/Users/someone/.local/state/cc-moss"),
            "/opt/homebrew/bin:/usr/bin:/bin",
            Some(Path::new("/Users/someone/.config/cc-moss/config.toml")),
        ));
        for want in [
            "<key>Label</key>\n    <string>io.github.larrykoo711.cc-moss.daemon</string>",
            "<array>\n        <string>/Users/someone/My Tools/cc-moss</string>\n        <string>daemon</string>\n        <string>run</string>\n    </array>",
            "<key>KeepAlive</key>\n    <true/>",
            "<key>RunAtLoad</key>\n    <true/>",
            "<key>ThrottleInterval</key>\n    <integer>30</integer>",
            "<key>ProcessType</key>\n    <string>Standard</string>",
            "<key>PATH</key>\n        <string>/opt/homebrew/bin:/usr/bin:/bin</string>",
            "<string>/Users/someone/.local/state/cc-moss/daemon/stdout.log</string>",
            "<string>/Users/someone/.local/state/cc-moss/daemon/stderr.log</string>",
            "<key>CC_MOSS_STATE_DIR</key>\n        <string>/Users/someone/.local/state/cc-moss</string>",
            "<key>CC_MOSS_CONFIG</key>\n        <string>/Users/someone/.config/cc-moss/config.toml</string>",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
    }

    #[test]
    fn daemon_path_follows_the_current_path_and_stays_put_when_unchanged() {
        let path = daemon_search_path(
            Some("/custom/bin:/usr/bin"),
            Some("/usr/bin:/Users/a/.npm/bin::relative:/opt/homebrew/bin/"),
            Some(Path::new("/Users/a")),
            Some(Path::new("/opt/homebrew/Cellar/cc-moss/1/bin")),
        );
        assert_eq!(
            path,
            "/usr/bin:/Users/a/.npm/bin:/opt/homebrew/bin:\
             /opt/homebrew/Cellar/cc-moss/1/bin:/usr/local/bin:/Users/a/.local/bin:\
             /Users/a/.claude/local:/bin:/usr/sbin:/sbin",
            "a folder that left the PATH (/custom/bin) leaves the daemon's too"
        );
        // The same folders in another order: the installed one is kept.
        let reordered = "/sbin:/usr/sbin:/bin:/Users/a/.claude/local:/Users/a/.local/bin:\
                         /usr/local/bin:/opt/homebrew/Cellar/cc-moss/1/bin:/opt/homebrew/bin:\
                         /Users/a/.npm/bin:/usr/bin";
        assert_eq!(
            daemon_search_path(
                Some(reordered),
                Some("/usr/bin:/Users/a/.npm/bin:/opt/homebrew/bin"),
                Some(Path::new("/Users/a")),
                Some(Path::new("/opt/homebrew/Cellar/cc-moss/1/bin")),
            ),
            reordered
        );
        let bare = daemon_search_path(None, None, None, None);
        assert_eq!(
            bare,
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }

    #[test]
    fn disabled_listing_is_read_in_both_formats() {
        let new = "disabled services = {\n\t\"com.other\" => enabled\n\t\"io.github.larrykoo711.cc-moss.daemon\" => disabled\n}\n";
        let old = "disabled services = {\n\t\"io.github.larrykoo711.cc-moss.daemon\" => true\n}\n";
        let on =
            "disabled services = {\n\t\"io.github.larrykoo711.cc-moss.daemon\" => enabled\n}\n";
        assert!(disabled_in(new, LABEL));
        assert!(disabled_in(old, LABEL));
        assert!(!disabled_in(on, LABEL));
        assert!(!disabled_in(new, "com.other"));
    }

    #[test]
    fn recorded_calls_track_loaded_and_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let ctl = RecordingLaunchctl {
            log: dir.path().join("launchctl.log"),
            fail: vec![],
        };
        assert!(!ctl.is_loaded() && !ctl.is_disabled());
        ctl.bootstrap(Path::new("/x.plist")).unwrap();
        assert!(ctl.is_loaded());
        ctl.kickstart(true).unwrap();
        assert!(ctl.is_loaded());
        ctl.disable().unwrap();
        ctl.bootout().unwrap();
        assert!(!ctl.is_loaded() && ctl.is_disabled());
        ctl.enable().unwrap();
        assert!(!ctl.is_disabled());
    }

    #[test]
    fn spans_read_naturally() {
        crate::i18n::set_for_test(crate::i18n::Lang::En);
        assert_eq!(span(5_000), "5 seconds");
        assert_eq!(span(60_000), "1 minute");
        assert_eq!(span(3 * 3_600_000 + 10), "3 hours");
        assert_eq!(span(2 * 86_400_000), "2 days");
    }

    #[test]
    fn same_dir_sees_through_symlinks_and_slashes() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&home, &link).unwrap();
        let slash = PathBuf::from(format!("{}/", home.display()));
        assert!(same_dir(&home, &home));
        assert!(same_dir(&home, &link), "a symlink to it");
        assert!(same_dir(&home, &slash), "a trailing slash");
        assert!(!same_dir(&home, dir.path()), "its parent");
        assert!(
            !same_dir(&home, &dir.path().join("other")),
            "a missing folder"
        );
        let gone = Path::new("/nonexistent/cc-moss-home");
        assert!(same_dir(gone, Path::new("/nonexistent/cc-moss-home/")));
    }

    #[test]
    fn a_temporary_home_is_not_this_accounts() {
        // A container may run as a uid with no entry in the user database.
        let Some(real) = account_home() else { return };
        assert!(real.is_absolute(), "{}", real.display());
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_own_home(dir.path()));
        assert!(is_own_home(&real));
    }
}
