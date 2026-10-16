//! The menubar app (macOS), which ships with cc-moss: same version, same
//! release. Homebrew puts it in the keg and the release archive next to the
//! binary; `install` copies it into `~/Applications` (macOS shows an ad-hoc
//! signed app's notifications only from an Applications folder), and
//! `update` swaps in the new one, restarting it if it was running.
//!
//! The app is quit by its bundle id and found running by the full path of
//! its executable, never by name: `cc-moss` is also the CLI the hooks run.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use moss_core::platform::Platform;

use super::install;

pub const BUNDLE_ID: &str = "io.github.larrykoo711.cc-moss";
pub const APP_NAME: &str = "cc-moss.app";
/// How long a running app gets to quit before it is replaced.
const QUIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Only macOS has the app.
pub fn applies() -> bool {
    install::platform() == Platform::MacOs
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `~/Applications/cc-moss.app`: where `install` puts the app.
pub fn user_app() -> Result<PathBuf> {
    Ok(home()
        .context("HOME is not set")?
        .join("Applications")
        .join(APP_NAME))
}

/// The copy in an Applications folder, the user's own first.
pub fn installed() -> Option<PathBuf> {
    let mut places = Vec::new();
    if let Ok(p) = user_app() {
        places.push(p);
    }
    places.push(Path::new("/Applications").join(APP_NAME));
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
/// stands in for it in tests; only debug builds honour it, so a release
/// build is never talked into copying some other folder.
pub fn bundled() -> Option<PathBuf> {
    let test_app = std::env::var_os("CC_MOSS_BUNDLED_APP").filter(|_| cfg!(debug_assertions));
    if let Some(p) = test_app {
        let p = PathBuf::from(p);
        return (!p.as_os_str().is_empty() && p.join("Contents").is_dir()).then_some(p);
    }
    let exe = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    bundled_near(&exe)
}

/// The app's version, from its Info.plist.
pub fn version(app: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app.join("Contents").join("Info.plist")).ok()?;
    plist_string(&text, "CFBundleShortVersionString")
}

/// A `<string>` value from an XML property list.
fn plist_string(xml: &str, key: &str) -> Option<String> {
    let (_, rest) = xml.split_once(&format!("<key>{key}</key>"))?;
    let rest = rest.trim_start().strip_prefix("<string>")?;
    let (value, _) = rest.split_once("</string>")?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
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

/// Whether this copy of the app is running.
pub fn is_running(app: &Path) -> bool {
    Command::new("pgrep")
        .arg("-f")
        .arg(process_pattern(app))
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Ask the app to quit and wait for it to go: opening a new copy while the
/// old one is still shutting down fails (LaunchServices error -609).
fn quit(app: &Path) -> Result<()> {
    let script = format!("tell application id \"{BUNDLE_ID}\" to quit");
    let _ = Command::new("osascript").args(["-e", &script]).output();
    let deadline = Instant::now() + QUIT_TIMEOUT;
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

/// Put a copy of `source` at `dest`: copied next to it under a temporary
/// name first, so a failed copy leaves the old app as it was.
fn put_in_place(source: &Path, dest: &Path) -> Result<()> {
    let dir = dest.parent().context("the app has no folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let staged = dir.join(format!(".{APP_NAME}.new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staged);
    if let Err(e) = copy_tree(source, &staged) {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(e);
    }
    match std::fs::symlink_metadata(dest) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(dest)?,
        Ok(_) => std::fs::remove_file(dest)?,
        Err(_) => {}
    }
    std::fs::rename(&staged, dest)
        .with_context(|| format!("cannot move the app to {}", dest.display()))
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
pub fn place(source: &Path, dest: &Path) -> Result<Placed> {
    let existed = std::fs::symlink_metadata(dest).is_ok();
    let running = existed && is_running(dest);
    if running {
        quit(dest)?;
    }
    put_in_place(source, dest)?;
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

/// `cc-moss install`'s app step: offer to copy the bundled app into
/// `~/Applications` when it is missing there or a different version.
pub fn offer(dry_run: bool, yes: bool) -> Result<()> {
    if !applies() {
        return Ok(());
    }
    let Some(source) = bundled() else {
        return Ok(());
    };
    let dest = user_app()?;
    let Some(want) = version(&source) else {
        bail!(tr!(
            format!("{} has no version in its Info.plist", source.display()),
            format!("{} 的 Info.plist 里没有版本号", source.display())
        ));
    };
    let have = version(&dest);
    if have.as_deref() == Some(want.as_str()) {
        println!(
            "\n{}",
            tr!(
                format!("The menubar app in {} is up to date.", display(&dest)),
                format!("{} 里的状态栏 App 已经是最新的。", display(&dest))
            )
        );
        return Ok(());
    }
    let why = tr!(
        "macOS shows its notifications only for an app in an Applications folder",
        "macOS 只给放在「应用程序」目录里的 App 发通知"
    );
    println!(
        "\n{}",
        match &have {
            None => tr!(
                format!(
                    "The menubar app {want} will be copied to {} ({why}).",
                    display(&dest)
                ),
                format!("状态栏 App {want} 会复制到 {}（{why}）。", display(&dest))
            ),
            Some(old) => tr!(
                format!(
                    "The menubar app in {} is {old}; it will be replaced with {want}.",
                    display(&dest)
                ),
                format!(
                    "{} 里的状态栏 App 是 {old}，会换成 {want}。",
                    display(&dest)
                )
            ),
        }
    );
    if dry_run {
        return Ok(());
    }
    let question = match have {
        None => tr!(
            "Put the menubar app in ~/Applications and start it?",
            "把状态栏 App 放进 ~/Applications 并启动吗？"
        ),
        Some(_) => tr!("Replace the menubar app?", "替换状态栏 App 吗？"),
    };
    let refused = tr!(
        "re-run with --yes, or leave the app out with --skip app",
        "加上 --yes 重新运行，或者用 --skip app 跳过 App"
    );
    if !super::confirm::ask(yes, question, refused)? {
        println!(
            "{}",
            tr!(
                "Okay. `cc-moss install` offers it again whenever you like.",
                "好的。什么时候想要了，再运行 `cc-moss install` 就行。"
            )
        );
        return Ok(());
    }
    let done = place(&source, &dest)?;
    println!(
        "{}",
        match done {
            Placed::New => tr!(
                format!("Put the menubar app in {} and started it.", display(&dest)),
                format!("状态栏 App 已放进 {} 并启动。", display(&dest))
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
    );
    Ok(())
}

/// After `update` put a new cc-moss in place: bring the app in
/// `~/Applications` along, from `source` (the new keg's or the download's
/// app; `None` when the install method has none, as with cargo). Never
/// fails the update, which has already gone through: problems are told.
pub fn follow_update(source: Option<&Path>) {
    if !applies() {
        return;
    }
    let Ok(dest) = user_app() else { return };
    if std::fs::symlink_metadata(&dest).is_err() {
        return;
    }
    let have = version(&dest).unwrap_or_else(|| "?".into());
    let Some((source, want)) = source.and_then(|s| Some((s, version(s)?))) else {
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
    if want == have {
        return;
    }
    match place(source, &dest) {
        Ok(Placed::Replaced { restarted: true }) => println!(
            "{}",
            tr!(
                format!("Updated the menubar app to {want} and restarted it."),
                format!("状态栏 App 已更新到 {want} 并重新打开。")
            )
        ),
        Ok(_) => println!(
            "{}",
            tr!(
                format!("Updated the menubar app to {want}."),
                format!("状态栏 App 已更新到 {want}。")
            )
        ),
        Err(e) => println!(
            "{}",
            tr!(
                format!("Could not update the menubar app ({e:#}); `cc-moss install` tries again."),
                format!("没能更新状态栏 App（{e:#}）；运行 `cc-moss install` 再试一次。")
            )
        ),
    }
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
        let names: Vec<_> = std::fs::read_dir(dir.path().join("Applications"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["cc-moss.app"]);
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
