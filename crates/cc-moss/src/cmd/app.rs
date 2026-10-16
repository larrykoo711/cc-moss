//! The menubar app (macOS), which ships with cc-moss: same version, same
//! release. Homebrew puts it in the keg and the release archive next to the
//! binary; `install` copies it into `~/Applications` (macOS shows an ad-hoc
//! signed app's notifications only from an Applications folder), `update`
//! swaps in the new one, restarting it if it was running, and
//! `install --uninstall` takes it out again.
//!
//! The app is quit by its bundle id and found running by the full path of
//! its executable, never by name: `cc-moss` is also the CLI the hooks run.
//! Under sudo nothing here touches the app: it belongs to the user.

use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use moss_core::platform::Platform;

use super::{confirm, install};

pub const BUNDLE_ID: &str = "io.github.larrykoo711.cc-moss";
pub const APP_NAME: &str = "cc-moss.app";
/// The launch agent the app writes for "Open at Login" (LoginItem.swift).
const LOGIN_ITEM: &str = "io.github.larrykoo711.cc-moss.login.plist";
/// How long a running app gets to quit before it is replaced.
const QUIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Only macOS has the app.
pub fn applies() -> bool {
    install::platform() == Platform::MacOs
}

/// Running as root or under sudo. The app in `~/Applications` belongs to
/// the user and runs in their session; root neither replaces nor starts it.
fn as_root() -> bool {
    std::env::var_os("SUDO_USER").is_some_and(|v| !v.is_empty()) || install::is_root()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// A path that only debug builds (the tests) let an environment variable
/// replace, so a release build is never talked into some other folder.
fn test_override(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|_| cfg!(debug_assertions))
        .map(PathBuf::from)
}

/// `~/Applications/cc-moss.app`: where `install` puts the app.
pub fn user_app() -> Result<PathBuf> {
    Ok(home()
        .context("HOME is not set")?
        .join("Applications")
        .join(APP_NAME))
}

/// `/Applications`; `CC_MOSS_SYSTEM_APPLICATIONS` in tests.
fn system_applications() -> PathBuf {
    test_override("CC_MOSS_SYSTEM_APPLICATIONS")
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/Applications"))
}

/// The copy in an Applications folder, the user's own first.
pub fn installed() -> Option<PathBuf> {
    let mut places = Vec::new();
    if let Ok(p) = user_app() {
        places.push(p);
    }
    places.push(system_applications().join(APP_NAME));
    places.into_iter().find(|p| p.join("Contents").is_dir())
}

/// The app that came with this cc-moss: next to the binary (the release
/// archive) or one folder up (Homebrew's keg, with the binary in `bin/`).
pub fn bundled_near(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    [Some(dir), dir.parent()]
        .into_iter()
        .flatten()
        .map(|d| d.join(APP_NAME))
        .find(|p| p.join("Contents").is_dir())
}

/// The app that came with the running cc-moss. `CC_MOSS_BUNDLED_APP`
/// stands in for it in tests (debug builds only).
pub fn bundled() -> Option<PathBuf> {
    if let Some(p) = test_override("CC_MOSS_BUNDLED_APP") {
        return (!p.as_os_str().is_empty() && p.join("Contents").is_dir()).then_some(p);
    }
    let exe = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    bundled_near(&exe)
}

fn info_plist(app: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(app.join("Contents").join("Info.plist")).ok()?;
    plist_string(&text, key)
}

/// The app's version, from its Info.plist.
pub fn version(app: &Path) -> Option<String> {
    info_plist(app, "CFBundleShortVersionString")
}

/// Whether `app` is cc-moss's menubar app, by its bundle id.
fn is_ours(app: &Path) -> bool {
    info_plist(app, "CFBundleIdentifier").as_deref() == Some(BUNDLE_ID)
}

/// A `<string>` value from an XML property list.
fn plist_string(xml: &str, key: &str) -> Option<String> {
    let (_, rest) = xml.split_once(&format!("<key>{key}</key>"))?;
    let rest = rest.trim_start().strip_prefix("<string>")?;
    let (value, _) = rest.split_once("</string>")?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// `MAJOR.MINOR.PATCH[-PRE][+BUILD]`, as semver reads it; `None` for
/// anything else (a development build says `dev`).
fn semver(text: &str) -> Option<([u64; 3], Vec<&str>)> {
    let text = text.trim();
    let text = text.split_once('+').map_or(text, |(v, _)| v);
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, pre.split('.').collect()),
        None => (text, Vec::new()),
    };
    if pre.iter().any(|p: &&str| p.is_empty()) {
        return None;
    }
    let mut parts = core.split('.');
    let mut numbers = [0; 3];
    for n in &mut numbers {
        *n = number(parts.next()?)?;
    }
    parts.next().is_none().then_some((numbers, pre))
}

fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `a` against `b` in semver order: a pre-release comes before its release.
/// `None` when either is not a version (a development build).
fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let (a_num, a_pre) = semver(a)?;
    let (b_num, b_pre) = semver(b)?;
    Some(
        a_num
            .cmp(&b_num)
            .then_with(|| match (a_pre.is_empty(), b_pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (x, y) in a_pre.iter().zip(&b_pre) {
                        let o = match (number(x), number(y)) {
                            (Some(m), Some(n)) => m.cmp(&n),
                            (Some(_), None) => Ordering::Less,
                            (None, Some(_)) => Ordering::Greater,
                            (None, None) => x.cmp(y),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                    a_pre.len().cmp(&b_pre.len())
                }
            }),
    )
}

/// Whether `have` is a version older than `want` (never for a development
/// build, whose version says nothing).
pub fn is_older(have: &str, want: &str) -> bool {
    compare_versions(have, want) == Some(Ordering::Less)
}

/// `~/…` for paths in the home folder, which reads better.
pub fn display(path: &Path) -> String {
    match home().and_then(|h| path.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// `pgrep -f` pattern for exactly this copy's executable.
fn process_pattern(app: &Path) -> String {
    let exe = app.join("Contents").join("MacOS").join("cc-moss");
    let mut out = String::new();
    for c in exe.display().to_string().chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    format!("^{out}( |$)")
}

fn pgrep(app: &Path) -> Option<std::process::Output> {
    Command::new("pgrep")
        .arg("-f")
        .arg(process_pattern(app))
        .output()
        .ok()
}

/// Whether this copy of the app is running.
pub fn is_running(app: &Path) -> bool {
    pgrep(app).is_some_and(|o| o.status.success())
}

/// The process ids of this copy of the app, and only this copy.
fn running_pids(app: &Path) -> Vec<String> {
    let Some(out) = pgrep(app).filter(|o| o.status.success()) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter(|p| number(p).is_some())
        .map(String::from)
        .collect()
}

/// `CC_MOSS_APP_QUIT_TIMEOUT_MS` shortens the wait in tests (debug builds).
fn quit_timeout() -> Duration {
    std::env::var("CC_MOSS_APP_QUIT_TIMEOUT_MS")
        .ok()
        .filter(|_| cfg!(debug_assertions))
        .and_then(|ms| ms.parse().ok())
        .map_or(QUIT_TIMEOUT, Duration::from_millis)
}

/// Ask the app to quit and wait for it to go: opening a new copy while the
/// old one is still shutting down fails (LaunchServices error -609). When
/// asking fails (Automation permission denied), this copy's processes get
/// SIGTERM instead, by their exact ids.
fn quit(app: &Path) -> Result<()> {
    let script = format!("tell application id \"{BUNDLE_ID}\" to quit");
    let asked = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .is_ok_and(|o| o.status.success());
    if !asked {
        let pids = running_pids(app);
        if !pids.is_empty() {
            let _ = Command::new("kill").arg("-TERM").args(&pids).output();
        }
    }
    let deadline = Instant::now() + quit_timeout();
    while is_running(app) {
        if Instant::now() > deadline {
            bail!(tr!(
                "the menubar app did not quit; quit it from its menu and try again",
                "状态栏 App 没有退出；从它的菜单里退出后再试一次"
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Start the app without taking the focus.
fn start(app: &Path) -> Result<()> {
    let status = Command::new("open").arg("-g").arg(app).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!("open {}: {s}", app.display()),
        Err(e) => bail!("open {}: {e}", app.display()),
    }
}

/// Copy a folder, keeping file modes and symbolic links as they are.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir(to).with_context(|| format!("cannot create {}", to.display()))?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
        } else if kind.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).with_context(|| format!("cannot copy {}", src.display()))?;
        }
    }
    Ok(())
}

/// Delete a folder, or only the link when it is a symbolic link.
fn remove_any(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Put a copy of `source` at `dest`, all or nothing.
fn put_in_place(source: &Path, dest: &Path) -> Result<()> {
    put_in_place_with(source, dest, |from, to| std::fs::rename(from, to))
}

/// The new copy is made next to `dest` under a temporary name; the old one
/// is renamed aside, the new one renamed in, and only then is the old one
/// deleted. Any failure puts the old one back and deletes the new copy, so
/// the old app is intact and no `.cc-moss.app.*` is left. A link at `dest`
/// is replaced itself, never what it points to.
fn put_in_place_with(
    source: &Path,
    dest: &Path,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<()> {
    let dir = dest.parent().context("the app has no folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let pid = std::process::id();
    let staged = dir.join(format!(".{APP_NAME}.new-{pid}"));
    let backup = dir.join(format!(".{APP_NAME}.old-{pid}"));
    let _ = remove_any(&staged);
    if let Err(e) = copy_tree(source, &staged) {
        let _ = remove_any(&staged);
        return Err(e);
    }
    let had = std::fs::symlink_metadata(dest).is_ok();
    if had {
        let _ = remove_any(&backup);
        if let Err(e) = rename(dest, &backup) {
            let _ = remove_any(&staged);
            return Err(e).with_context(|| format!("cannot move {} aside", dest.display()));
        }
    }
    if let Err(e) = rename(&staged, dest) {
        if had {
            let _ = rename(&backup, dest);
        }
        let _ = remove_any(&staged);
        return Err(e).with_context(|| format!("cannot move the app to {}", dest.display()));
    }
    if had {
        let _ = remove_any(&backup);
    }
    Ok(())
}

/// What happened to the app in an Applications folder.
#[derive(Debug, PartialEq, Eq)]
pub enum Placed {
    /// It was not there; now it is, and running.
    New,
    /// An older copy was replaced; `restarted` if it had been running.
    Replaced { restarted: bool },
}

/// Copy `source` to `dest`. A running copy is quit first and started again
/// afterwards; a new copy is started so its icon shows in the menu bar.
/// When the copy cannot go in, the old app stays and runs again if it ran.
pub fn place(source: &Path, dest: &Path) -> Result<Placed> {
    let existed = std::fs::symlink_metadata(dest).is_ok();
    let running = existed && is_running(dest);
    if running {
        quit(dest)?;
    }
    if let Err(e) = put_in_place(source, dest) {
        if running {
            let _ = start(dest);
        }
        return Err(e);
    }
    if !existed || running {
        if let Err(e) = start(dest) {
            println!(
                "{}",
                tr!(
                    format!("Could not start the menubar app ({e:#}); open it from Applications."),
                    format!("没能启动状态栏 App（{e:#}）；请从「应用程序」里打开它。")
                )
            );
        }
    }
    Ok(if existed {
        Placed::Replaced { restarted: running }
    } else {
        Placed::New
    })
}

fn root_note() -> &'static str {
    tr!(
        "Running as root, so the menubar app was left alone: run `cc-moss install` without sudo to update it.",
        "正在以 root 身份运行，所以没有动状态栏 App：不加 sudo 运行 `cc-moss install` 就能更新它。"
    )
}

/// Where the app is when it is not next to this cc-moss.
fn where_to_get_it() -> &'static str {
    tr!(
        "The menubar app is not next to this cc-moss. With the macOS release download, run `./cc-moss install` in the unpacked folder (cc-moss.app sits next to the binary there), or drag cc-moss.app into Applications.",
        "这个 cc-moss 旁边没有状态栏 App。用的是 macOS 的 Release 下载包的话，在解压出的文件夹里运行 `./cc-moss install`（cc-moss.app 就在程序旁边），或者把 cc-moss.app 拖进「应用程序」。"
    )
}

/// The installed app is newer than the one at hand: never downgraded.
fn newer_note(dest: &Path, have: &str, want: &str) -> String {
    tr!(
        format!(
            "The menubar app in {} is {have}, newer than {want}; it is left as it is.",
            display(dest)
        ),
        format!(
            "{} 里的状态栏 App 是 {have}，比 {want} 还新，保持不动。",
            display(dest)
        )
    )
}

/// The installed app's version cannot be compared (a development build).
fn unknown_note(dest: &Path, have: Option<&str>, want: &str) -> String {
    let have = have.unwrap_or("?");
    tr!(
        format!(
            "The menubar app in {} has an unknown version ({have}, a development build?); it is left as it is. To use {want}, delete it and run `cc-moss install`.",
            display(dest)
        ),
        format!(
            "{} 里的状态栏 App 版本未知（{have}，开发版？），保持不动。想换成 {want}，删掉它再运行 `cc-moss install`。",
            display(dest)
        )
    )
}

/// `cc-moss install`'s app step: offer to copy the bundled app into
/// `~/Applications` when it is missing there or older. Problems with the
/// app are told and never stop the rest of install; only a refusal to ask
/// (no terminal, no `--yes`) is an error, as everywhere in install.
pub fn offer(dry_run: bool, yes: bool) -> Result<()> {
    if !applies() {
        return Ok(());
    }
    if as_root() {
        println!("\n{}", root_note());
        return Ok(());
    }
    let Some(source) = bundled() else {
        println!("\n{}", where_to_get_it());
        return Ok(());
    };
    let dest = user_app()?;
    let Some(want) = version(&source) else {
        println!(
            "\n{}",
            tr!(
                format!(
                    "Left the menubar app out: {} has no version in its Info.plist.",
                    source.display()
                ),
                format!(
                    "跳过了状态栏 App：{} 的 Info.plist 里没有版本号。",
                    source.display()
                )
            )
        );
        return Ok(());
    };
    let present = std::fs::symlink_metadata(&dest).is_ok();
    let have = version(&dest);
    if present {
        match have.as_deref().and_then(|h| compare_versions(h, &want)) {
            Some(Ordering::Less) => {}
            Some(Ordering::Equal) => {
                println!(
                    "\n{}",
                    tr!(
                        format!("The menubar app in {} is up to date.", display(&dest)),
                        format!("{} 里的状态栏 App 已经是最新的。", display(&dest))
                    )
                );
                return Ok(());
            }
            Some(Ordering::Greater) => {
                println!(
                    "\n{}",
                    newer_note(&dest, have.as_deref().unwrap_or("?"), &want)
                );
                return Ok(());
            }
            None => {
                println!("\n{}", unknown_note(&dest, have.as_deref(), &want));
                return Ok(());
            }
        }
    }
    let why = tr!(
        "macOS shows its notifications only for an app in an Applications folder",
        "macOS 只给放在「应用程序」目录里的 App 发通知"
    );
    println!(
        "\n{}",
        match &have {
            Some(old) if present => tr!(
                format!(
                    "The menubar app in {} is {old}; it will be replaced with {want}.",
                    display(&dest)
                ),
                format!(
                    "{} 里的状态栏 App 是 {old}，会换成 {want}。",
                    display(&dest)
                )
            ),
            _ => tr!(
                format!(
                    "The menubar app {want} will be copied to {} ({why}).",
                    display(&dest)
                ),
                format!("状态栏 App {want} 会复制到 {}（{why}）。", display(&dest))
            ),
        }
    );
    if dry_run {
        return Ok(());
    }
    let question = if present {
        tr!("Replace the menubar app?", "替换状态栏 App 吗？")
    } else {
        tr!(
            "Put the menubar app in ~/Applications and start it?",
            "把状态栏 App 放进 ~/Applications 并启动吗？"
        )
    };
    let refused = tr!(
        "re-run with --yes, or leave the app out with --skip app",
        "加上 --yes 重新运行，或者用 --skip app 跳过 App"
    );
    if !confirm::ask(yes, question, refused)? {
        println!(
            "{}",
            tr!(
                "Okay. `cc-moss install` offers it again whenever you like.",
                "好的。什么时候想要了，再运行 `cc-moss install` 就行。"
            )
        );
        return Ok(());
    }
    match place(&source, &dest) {
        Ok(done) => println!("{}", placed_note(&done, &dest, &want)),
        Err(e) => println!("{}", failed_note(&e, &dest, present)),
    }
    Ok(())
}

fn placed_note(done: &Placed, dest: &Path, want: &str) -> String {
    match done {
        Placed::New => tr!(
            format!("Put the menubar app in {} and started it.", display(dest)),
            format!("状态栏 App 已放进 {} 并启动。", display(dest))
        ),
        Placed::Replaced { restarted: true } => tr!(
            format!("Updated the menubar app to {want} and restarted it."),
            format!("状态栏 App 已更新到 {want} 并重新打开。")
        ),
        Placed::Replaced { restarted: false } => tr!(
            format!("Updated the menubar app to {want}."),
            format!("状态栏 App 已更新到 {want}。")
        ),
    }
}

fn failed_note(e: &anyhow::Error, dest: &Path, present: bool) -> String {
    if present {
        tr!(
            format!(
                "Could not update the menubar app ({e:#}); the one in {} is unchanged. `cc-moss install` tries again.",
                display(dest)
            ),
            format!(
                "没能更新状态栏 App（{e:#}）；{} 里原来的那份没动。运行 `cc-moss install` 再试一次。",
                display(dest)
            )
        )
    } else {
        tr!(
            format!(
                "Could not put the menubar app in {} ({e:#}); `cc-moss install` tries again.",
                display(dest)
            ),
            format!(
                "没能把状态栏 App 放进 {}（{e:#}）；运行 `cc-moss install` 再试一次。",
                display(dest)
            )
        )
    }
}

/// After `update` put a new cc-moss in place: bring the app in
/// `~/Applications` along, from `source` (the new keg's or the download's
/// app; `None` when the install method has none, as with cargo). Never
/// fails the update, which has already gone through: problems are told.
/// A newer or unknown (development) app is left as it is.
pub fn follow_update(source: Option<&Path>) {
    if !applies() {
        return;
    }
    let Ok(dest) = user_app() else { return };
    if std::fs::symlink_metadata(&dest).is_err() {
        return;
    }
    if as_root() {
        println!("{}", root_note());
        return;
    }
    let have = version(&dest);
    let Some((source, want)) = source.and_then(|s| Some((s, version(s)?))) else {
        let have = have.as_deref().unwrap_or("?");
        println!(
            "{}",
            tr!(
                format!(
                    "The menubar app in {} stays at {have}: this way of installing cc-moss does not include it.",
                    display(&dest)
                ),
                format!(
                    "{} 里的状态栏 App 还是 {have}：这种安装方式不带 App。",
                    display(&dest)
                )
            )
        );
        return;
    };
    match have.as_deref().and_then(|h| compare_versions(h, &want)) {
        Some(Ordering::Less) => {}
        Some(Ordering::Equal) => return,
        Some(Ordering::Greater) => {
            println!(
                "{}",
                newer_note(&dest, have.as_deref().unwrap_or("?"), &want)
            );
            return;
        }
        None => {
            println!("{}", unknown_note(&dest, have.as_deref(), &want));
            return;
        }
    }
    match place(source, &dest) {
        Ok(done) => println!("{}", placed_note(&done, &dest, &want)),
        Err(e) => println!("{}", failed_note(&e, &dest, true)),
    }
}

/// `cc-moss install --uninstall`'s app step: delete `~/Applications/cc-moss.app`
/// when it is cc-moss's (by bundle id), quitting it first, and the launch
/// agent it wrote for "Open at Login". Asks like the rest of uninstall.
pub fn remove(dry_run: bool, yes: bool) -> Result<()> {
    if !applies() {
        return Ok(());
    }
    let home = home().context("HOME is not set")?;
    let dest = user_app()?;
    let agent = home.join("Library").join("LaunchAgents").join(LOGIN_ITEM);
    let app_there = std::fs::symlink_metadata(&dest).is_ok();
    let agent_there = std::fs::symlink_metadata(&agent).is_ok();
    if !app_there && !agent_there {
        return Ok(());
    }
    if as_root() {
        println!(
            "\n{}",
            tr!(
                "Running as root, so the menubar app was left alone: run `cc-moss install --uninstall` without sudo to remove it.",
                "正在以 root 身份运行，所以没有动状态栏 App：不加 sudo 运行 `cc-moss install --uninstall` 就能移除它。"
            )
        );
        return Ok(());
    }
    let app_ours = app_there && is_ours(&dest);
    if app_there && !app_ours {
        println!(
            "\n{}",
            tr!(
                format!("Kept {}: it is not cc-moss's menubar app.", display(&dest)),
                format!("保留了 {}：它不是 cc-moss 的状态栏 App。", display(&dest))
            )
        );
    }
    let doomed: Vec<&Path> = [(app_ours, dest.as_path()), (agent_there, agent.as_path())]
        .into_iter()
        .filter_map(|(go, p)| go.then_some(p))
        .collect();
    if doomed.is_empty() {
        return Ok(());
    }
    println!(
        "\n{}",
        tr!(
            "The menubar app goes too (quit first if it is running):",
            "状态栏 App 也会移除（开着的话先退出）："
        )
    );
    for p in &doomed {
        println!("  {}", display(p));
    }
    if dry_run {
        return Ok(());
    }
    let refused = tr!(
        "re-run with --yes to remove it as well",
        "加上 --yes 重新运行，就会一起移除"
    );
    if !confirm::ask(
        yes,
        tr!("Remove the menubar app?", "移除状态栏 App 吗？"),
        refused,
    )? {
        println!(
            "{}",
            tr!("Okay, the menubar app stays.", "好的，状态栏 App 留着。")
        );
        return Ok(());
    }
    if app_ours && is_running(&dest) {
        if let Err(e) = quit(&dest) {
            println!(
                "{}",
                tr!(
                    format!("Kept the menubar app: {e:#}."),
                    format!("状态栏 App 没有移除：{e:#}。")
                )
            );
            return Ok(());
        }
    }
    for p in doomed {
        match remove_any(p) {
            Ok(()) => println!(
                "{}",
                tr!(
                    format!("Deleted {}.", display(p)),
                    format!("已删除 {}。", display(p))
                )
            ),
            Err(e) => println!(
                "{}",
                tr!(
                    format!("Could not delete {} ({e}).", display(p)),
                    format!("没能删除 {}（{e}）。", display(p))
                )
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_from_a_plist() {
        let xml =
            "<dict>\n  <key>CFBundleShortVersionString</key>\n  <string> 1.2.3 </string>\n</dict>";
        assert_eq!(
            plist_string(xml, "CFBundleShortVersionString").as_deref(),
            Some("1.2.3")
        );
        let inline = "<key>CFBundleShortVersionString</key><string>0.2.0</string>";
        assert_eq!(
            plist_string(inline, "CFBundleShortVersionString").as_deref(),
            Some("0.2.0")
        );
        assert_eq!(plist_string(xml, "CFBundleVersion"), None);
        let empty = "<key>CFBundleShortVersionString</key><string></string>";
        assert_eq!(plist_string(empty, "CFBundleShortVersionString"), None);
    }

    #[test]
    fn versions_compare_like_semver() {
        use Ordering::*;
        let cases = [
            ("0.1.1", "0.1.1", Some(Equal)),
            ("0.1.1", "0.2.0", Some(Less)),
            ("0.10.0", "0.9.9", Some(Greater)),
            ("1.0.0-rc.1", "1.0.0", Some(Less)),
            ("1.0.0-rc.2", "1.0.0-rc.10", Some(Less)),
            ("1.0.0-alpha", "1.0.0-alpha.1", Some(Less)),
            ("1.0.0-1", "1.0.0-alpha", Some(Less)),
            ("1.0.0+build.5", "1.0.0", Some(Equal)),
            ("dev", "1.0.0", None),
            ("1.0", "1.0.0", None),
            ("1.0.0-", "1.0.0", None),
            ("1.0.+1", "1.0.1", None),
        ];
        for (a, b, want) in cases {
            assert_eq!(compare_versions(a, b), want, "{a} vs {b}");
        }
    }

    #[test]
    fn finds_the_app_next_to_the_binary_or_in_the_keg() {
        let dir = tempfile::tempdir().unwrap();
        let keg = dir.path().join("Cellar/cc-moss/1.0.0");
        std::fs::create_dir_all(keg.join("bin")).unwrap();
        std::fs::create_dir_all(keg.join("cc-moss.app/Contents")).unwrap();
        assert_eq!(
            bundled_near(&keg.join("bin/cc-moss")),
            Some(keg.join("cc-moss.app"))
        );
        let flat = dir.path().join("cc-moss-v1.0.0-aarch64-apple-darwin");
        std::fs::create_dir_all(flat.join("cc-moss.app/Contents")).unwrap();
        assert_eq!(
            bundled_near(&flat.join("cc-moss")),
            Some(flat.join("cc-moss.app"))
        );
        assert_eq!(bundled_near(&dir.path().join("elsewhere/cc-moss")), None);
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn copies_keep_modes_and_links() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.app");
        std::fs::create_dir_all(src.join("Contents/MacOS")).unwrap();
        let exe = src.join("Contents/MacOS/cc-moss");
        std::fs::write(&exe, "x").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("MacOS/cc-moss", src.join("Contents/link")).unwrap();
        let dest = dir.path().join("Applications/cc-moss.app");
        put_in_place(&src, &dest).unwrap();
        let mode = std::fs::metadata(dest.join("Contents/MacOS/cc-moss"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            std::fs::read_link(dest.join("Contents/link")).unwrap(),
            Path::new("MacOS/cc-moss")
        );
        // Again over the top: replaced, nothing left beside it.
        put_in_place(&src, &dest).unwrap();
        assert_eq!(names(&dir.path().join("Applications")), ["cc-moss.app"]);
    }

    /// An old app at `Applications/cc-moss.app` and a new one to put there.
    fn old_and_new() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("new.app");
        std::fs::create_dir_all(src.join("Contents")).unwrap();
        std::fs::write(src.join("Contents/which"), "new").unwrap();
        let dest = dir.path().join("Applications/cc-moss.app");
        std::fs::create_dir_all(dest.join("Contents")).unwrap();
        std::fs::write(dest.join("Contents/which"), "old").unwrap();
        (dir, src, dest)
    }

    fn which(app: &Path) -> String {
        std::fs::read_to_string(app.join("Contents/which")).unwrap()
    }

    fn fail_when(part: &'static str) -> impl Fn(&Path, &Path) -> io::Result<()> {
        move |from, to| {
            let names = format!("{} {}", from.display(), to.display());
            if names.contains(part) {
                Err(io::Error::other("simulated"))
            } else {
                std::fs::rename(from, to)
            }
        }
    }

    #[test]
    fn a_failed_move_in_puts_the_old_app_back() {
        let (dir, src, dest) = old_and_new();
        // Moving the old one aside works; moving the new one in fails.
        let err = put_in_place_with(&src, &dest, fail_when(".new-")).unwrap_err();
        assert!(format!("{err:#}").contains("simulated"), "{err:#}");
        assert_eq!(which(&dest), "old");
        assert_eq!(names(&dir.path().join("Applications")), ["cc-moss.app"]);
    }

    #[test]
    fn a_failed_move_aside_leaves_the_old_app() {
        let (dir, src, dest) = old_and_new();
        let err = put_in_place_with(&src, &dest, fail_when(".old-")).unwrap_err();
        assert!(format!("{err:#}").contains("simulated"), "{err:#}");
        assert_eq!(which(&dest), "old");
        assert_eq!(names(&dir.path().join("Applications")), ["cc-moss.app"]);
    }

    #[test]
    fn a_link_is_replaced_not_its_target() {
        let (dir, src, _) = old_and_new();
        let target = dir.path().join("elsewhere.app");
        std::fs::create_dir_all(target.join("Contents")).unwrap();
        std::fs::write(target.join("Contents/which"), "target").unwrap();
        let dest = dir.path().join("Links/cc-moss.app");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &dest).unwrap();
        put_in_place(&src, &dest).unwrap();
        assert!(!std::fs::symlink_metadata(&dest).unwrap().is_symlink());
        assert_eq!(which(&dest), "new");
        assert_eq!(which(&target), "target");
        assert_eq!(names(&dir.path().join("Links")), ["cc-moss.app"]);
    }

    #[test]
    fn the_process_pattern_is_this_copy_only() {
        let p = process_pattern(Path::new("/Users/demo/Applications/cc-moss.app"));
        assert_eq!(
            p,
            "^/Users/demo/Applications/cc-moss\\.app/Contents/MacOS/cc-moss( |$)"
        );
    }
}
