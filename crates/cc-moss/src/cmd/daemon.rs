//! `cc-moss daemon`: one resident process per user, kept alive by launchd.
//!
//! `run` is what launchd starts: it takes the single-instance lock, writes
//! its status and a heartbeat, and stops cleanly on SIGTERM. `start`, `stop`
//! and `status` are for people and the app; `install` and `uninstall` place
//! and remove the LaunchAgent. Today the daemon only stays alive and says so;
//! the Moss session it will look after comes later.

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
        pid_start: proc::start_time(pid),
        version: VERSION.to_string(),
        started_ms,
        heartbeat_ms: started_ms,
    };
    status::write(&paths.status, &stored).with_context(|| format!("{}", paths.status.display()))?;
    log(&paths, &format!("started: pid {pid}, version {VERSION}"));

    let beat = heartbeat();
    let mut next = Instant::now() + beat;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
        if Instant::now() >= next {
            stored.heartbeat_ms = now_ms();
            if let Err(e) = status::write(&paths.status, &stored) {
                log(&paths, &format!("error: heartbeat: {e}"));
            }
            next = Instant::now() + beat;
        }
    }
    log(&paths, "stopping: asked to");
    let _ = status::remove_if_ours(&paths.status, pid);
    Ok(true)
}

/// Every 30 seconds; debug builds (the tests) may say otherwise.
fn heartbeat() -> Duration {
    std::env::var("CC_MOSS_DAEMON_HEARTBEAT_MS")
        .ok()
        .filter(|_| cfg!(debug_assertions))
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(daemon::HEARTBEAT)
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
    let stored = status::read(&Paths::current().status);
    status::report(stored.as_ref(), proc::liveness)
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

fn span(ms: i64) -> String {
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

impl RealLaunchctl {
    fn new() -> Result<Self> {
        let out = std::process::Command::new("id")
            .arg("-u")
            .output()
            .context("cannot run `id -u`")?;
        let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if uid.is_empty() {
            bail!("cannot tell the user id (`id -u` said nothing)");
        }
        Ok(RealLaunchctl {
            domain: format!("gui/{uid}"),
        })
    }

    fn service(&self) -> String {
        format!("{}/{LABEL}", self.domain)
    }

    fn call(&self, args: &[&str]) -> Result<()> {
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
        std::process::Command::new("/bin/launchctl")
            .args(["print", &self.service()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    fn is_disabled(&self) -> bool {
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
/// follows from the calls written so far.
struct RecordingLaunchctl {
    log: PathBuf,
}

impl RecordingLaunchctl {
    fn record(&self, call: &str) -> Result<()> {
        use std::io::Write;
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
        let file = format!("{LABEL}.plist");
        if let Some(dir) = std::env::var_os("CC_MOSS_LAUNCHD_DIR")
            .filter(|d| !d.is_empty() && cfg!(debug_assertions))
            .map(PathBuf::from)
        {
            return Ok(Launchd {
                plist: dir.join(file),
                ctl: Some(Box::new(RecordingLaunchctl {
                    log: dir.join("launchctl.log"),
                })),
            });
        }
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .context("HOME is not set")?;
        let ctl: Option<Box<dyn Launchctl>> = if cfg!(target_os = "macos") && is_own_home(&home) {
            Some(Box::new(RealLaunchctl::new()?))
        } else {
            None
        };
        Ok(Launchd {
            plist: home.join("Library/LaunchAgents").join(file),
            ctl,
        })
    }
}

/// Whether `home` is this account's home directory, as the directory
/// service has it. Unsure means no: then cc-moss never registers anything.
fn is_own_home(home: &Path) -> bool {
    let Some(user) = std::process::Command::new("id")
        .arg("-un")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|u| !u.is_empty())
    else {
        return false;
    };
    let Ok(out) = std::process::Command::new("/usr/bin/dscl")
        .args([".", "-read", &format!("/Users/{user}"), "NFSHomeDirectory"])
        .output()
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(real) = text
        .split_once("NFSHomeDirectory:")
        .map(|(_, rest)| rest.trim())
        .filter(|r| !r.is_empty())
    else {
        return false;
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(Path::new(real)) == canon(home)
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
    Ok(plist::render(&agent(&setup::hook_exe()?, &state)))
}

fn agent(exe: &str, state: &Path) -> plist::Agent {
    let paths = Paths::new(state);
    plist::Agent {
        label: LABEL.to_string(),
        program_arguments: vec![exe.to_string(), "daemon".into(), "run".into()],
        stdout: paths.stdout,
        stderr: paths.stderr,
        // launchd does not start the daemon from the user's shell: pin the
        // state directory so the daemon, the CLI and the app agree on it.
        environment: vec![(
            "CC_MOSS_STATE_DIR".to_string(),
            state.to_string_lossy().into_owned(),
        )],
    }
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
    if !applies() {
        return Ok(());
    }
    if app::as_root() {
        return Ok(());
    }
    let launchd = Launchd::detect()?;
    let text = agent_plist()?;
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
    moss_core::store_dir::ensure_private_dir(&Paths::current().dir)?;
    let changed = place_plist(launchd, text)?;
    let Some(ctl) = &launchd.ctl else {
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

/// `cc-moss install --uninstall`'s daemon step: stop it and remove the plist.
pub fn uninstall_step(dry_run: bool) -> Result<()> {
    if !applies() {
        return Ok(());
    }
    let launchd = Launchd::detect()?;
    let loaded = launchd.ctl.as_ref().is_some_and(|c| c.is_loaded());
    if !launchd.plist.exists() && !loaded {
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
    if let Some(ctl) = launchd.ctl.as_ref().filter(|_| loaded) {
        ctl.bootout()?;
    }
    match std::fs::remove_file(&launchd.plist) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("{}", launchd.plist.display())),
    }
    println!(
        "{}",
        tr!(
            format!(
                "Stopped the cc-moss daemon and deleted {}.",
                launchd.plist.display()
            ),
            format!(
                "已停掉 cc-moss 守护进程，并删除 {}。",
                launchd.plist.display()
            )
        )
    );
    Ok(())
}

/// Whether the LaunchAgent is in place (for doctor).
pub fn plist_present() -> bool {
    Launchd::detect().is_ok_and(|l| l.plist.exists())
}

// ---------------------------------------------------------------- start / stop

const NOT_OWN_HOME: &str =
    "HOME is not this account's home directory, so launchd would never see the daemon";

fn need_launchd() -> Result<()> {
    if !applies() {
        bail!(tr!(
            "launchd runs the daemon on macOS only; elsewhere run `cc-moss daemon run` yourself",
            "只有 macOS 上由 launchd 托管守护进程；其他系统请自己运行 `cc-moss daemon run`"
        ));
    }
    if app::as_root() {
        bail!(tr!(
            "the daemon runs as you, not as root: run this without sudo",
            "守护进程以你自己的身份运行，不用 root：去掉 sudo 再运行"
        ));
    }
    Ok(())
}

/// `cc-moss daemon start`: set it up if needed and have launchd run it.
pub fn start() -> Result<()> {
    need_launchd()?;
    let launchd = Launchd::detect()?;
    moss_core::store_dir::ensure_private_dir(&Paths::current().dir)?;
    let changed = place_plist(&launchd, &agent_plist()?)?;
    let ctl = launchd.ctl.as_ref().context(NOT_OWN_HOME)?;
    if ctl.is_disabled() {
        ctl.enable()?;
    }
    if !ctl.is_loaded() {
        ctl.bootstrap(&launchd.plist)?;
    } else if changed {
        ctl.bootout()?;
        ctl.bootstrap(&launchd.plist)?;
    } else if !report().running {
        ctl.kickstart(false)?;
    }
    if !ctl.is_real() {
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
        Ok(())
    } else {
        bail!(tr!(
            format!(
                "launchd did not bring the daemon up; see {}",
                Paths::current().log.display()
            ),
            format!(
                "launchd 没能把守护进程拉起来；看看 {}",
                Paths::current().log.display()
            )
        ))
    }
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
        ));
        for want in [
            "<key>Label</key>\n    <string>io.github.larrykoo711.cc-moss.daemon</string>",
            "<array>\n        <string>/Users/someone/My Tools/cc-moss</string>\n        <string>daemon</string>\n        <string>run</string>\n    </array>",
            "<key>KeepAlive</key>\n    <true/>",
            "<key>RunAtLoad</key>\n    <true/>",
            "<key>ThrottleInterval</key>\n    <integer>30</integer>",
            "<key>ProcessType</key>\n    <string>Background</string>",
            "<string>/Users/someone/.local/state/cc-moss/daemon/stdout.log</string>",
            "<string>/Users/someone/.local/state/cc-moss/daemon/stderr.log</string>",
            "<key>CC_MOSS_STATE_DIR</key>\n        <string>/Users/someone/.local/state/cc-moss</string>",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
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
}
