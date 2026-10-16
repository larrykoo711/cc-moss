//! `cc-moss update`: look for a new version right now and, once the user
//! agrees, install it the way this copy was installed: Homebrew, cargo, or a
//! release download that cc-moss replaces itself.
//!
//! A self-replace never touches the running program until the very end: the
//! release is downloaded next to it, checked against its `.sha256`, unpacked,
//! asked for its version, and only then renamed over the old one.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use moss_core::update::{self, Version};

use super::{app, confirm};

/// `cc-moss update` asks for the latest version with this much patience.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// A whole release tarball.
const DOWNLOAD_TIMEOUT_SECS: &str = "300";
/// The new binary answering `--version`.
const SELF_TEST_TIMEOUT: Duration = Duration::from_secs(10);
const TAP: &str = "larrykoo711/tap/cc-moss";
const GIT_URL: &str = "https://github.com/larrykoo711/cc-moss";

pub struct UpdateArgs {
    /// Only report; change nothing.
    pub check: bool,
    /// With `check`: answer as JSON, `{schema, current, latest, available, notes}`.
    pub json: bool,
    /// Do not ask.
    pub yes: bool,
}

/// How this copy of cc-moss was installed, and so how to update it.
#[derive(Debug, PartialEq, Eq)]
enum Method {
    Homebrew,
    Cargo,
    /// A release download: replace the file at this path.
    Binary(PathBuf),
}

impl Method {
    fn detect(exe: &Path) -> Method {
        if exe.to_string_lossy().contains("/Cellar/cc-moss/") {
            Method::Homebrew
        } else if cargo_bin().is_some_and(|bin| exe.parent() == Some(bin.as_path())) {
            Method::Cargo
        } else {
            Method::Binary(exe.to_path_buf())
        }
    }

    /// The command a package manager runs, as the user would type it.
    fn command(&self, version: &Version) -> Option<Vec<String>> {
        let words: Vec<String> = match self {
            Method::Homebrew => vec!["brew".into(), "upgrade".into(), TAP.into()],
            Method::Cargo => ["cargo", "install", "--locked", "--git", GIT_URL, "--tag"]
                .iter()
                .map(|s| s.to_string())
                .chain([format!("v{version}"), "cc-moss".into()])
                .collect(),
            Method::Binary(_) => return None,
        };
        Some(words)
    }

    /// Where the upgraded program can be asked for its version: Homebrew's
    /// `<prefix>/bin/cc-moss` (the Cellar path of the old version goes away),
    /// cargo's `bin/cc-moss` in place. `None` when there is no such file.
    fn installed_path(&self, exe: &Path) -> Option<PathBuf> {
        let path = match self {
            Method::Homebrew => {
                let text = exe.to_string_lossy();
                let (prefix, _) = text.split_once("/Cellar/cc-moss/")?;
                Path::new(prefix).join("bin").join("cc-moss")
            }
            Method::Cargo => exe.to_path_buf(),
            Method::Binary(_) => return None,
        };
        path.exists().then_some(path)
    }
}

/// `$CARGO_HOME/bin`, else `~/.cargo/bin`, with symlinks resolved.
fn cargo_bin() -> Option<PathBuf> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let home = env("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    std::fs::canonicalize(home.join("bin")).ok()
}

/// The running program, symlinks resolved (a Homebrew `bin/cc-moss` link
/// leads into the Cellar). `CC_MOSS_UPDATE_EXE` stands in for it in tests,
/// so they update a copy and never the test binary itself.
fn current_exe() -> Result<PathBuf> {
    // Only debug builds (the tests) honour the override: a release build
    // must never be talked into replacing some other file.
    let test_exe =
        std::env::var_os("CC_MOSS_UPDATE_EXE").filter(|v| cfg!(debug_assertions) && !v.is_empty());
    let exe = match test_exe {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe().context("cannot find the running cc-moss")?,
    };
    std::fs::canonicalize(&exe).with_context(|| format!("cannot resolve {}", exe.display()))
}

/// The release build for this machine, as named in release.yml.
fn target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        _ => return None,
    })
}

pub fn run(args: UpdateArgs) -> Result<()> {
    let current = Version::current();
    let latest = update::check_and_cache(CHECK_TIMEOUT).map_err(|e| {
        anyhow!(tr!(
            format!("could not check for a new version ({e}); check your connection and try again"),
            format!("没能检查新版本（{e}），看看网络再试一次")
        ))
    })?;
    if args.json {
        let available = latest.is_newer_than(&current);
        let notes = available.then(|| format!("{}/tag/v{latest}", update::releases_url()));
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "current": current.to_string(),
                "latest": latest.to_string(),
                "available": available,
                "notes": notes,
            })
        );
        return Ok(());
    }
    if !latest.is_newer_than(&current) {
        println!(
            "{}",
            tr!(
                format!("You are already on the latest cc-moss ({current}). Nothing to do."),
                format!("你用的已经是最新的 cc-moss（{current}），不用升级。")
            )
        );
        return Ok(());
    }
    let notes = format!("{}/tag/v{latest}", update::releases_url());
    println!(
        "{}",
        tr!(
            format!("cc-moss {latest} is out (you have {current})."),
            format!("cc-moss {latest} 发布了（你现在用的是 {current}）。")
        )
    );
    println!(
        "{}",
        tr!(
            format!("What's new: {notes}"),
            format!("更新了什么：{notes}")
        )
    );
    if args.check {
        println!(
            "{}",
            tr!(
                "Run `cc-moss update` to get it.",
                "运行 `cc-moss update` 就能升级。"
            )
        );
        return Ok(());
    }

    let exe = current_exe()?;
    let method = Method::detect(&exe);
    println!();
    match (&method, method.command(&latest)) {
        (Method::Homebrew, Some(cmd)) => println!(
            "{}",
            tr!(
                format!("Installed with Homebrew, so this runs: {}", cmd.join(" ")),
                format!("你是用 Homebrew 装的，会运行：{}", cmd.join(" "))
            )
        ),
        (Method::Cargo, Some(cmd)) => println!(
            "{}",
            tr!(
                format!("Installed with cargo, so this runs: {}", cmd.join(" ")),
                format!("你是用 cargo 装的，会运行：{}", cmd.join(" "))
            )
        ),
        (Method::Binary(exe), _) => println!(
            "{}",
            tr!(
                format!(
                    "cc-moss will download {latest}, check it, and put it in place of {}.",
                    exe.display()
                ),
                format!(
                    "cc-moss 会下载 {latest}，校验无误后替换 {}。",
                    exe.display()
                )
            )
        ),
        _ => {}
    }
    let question = tr!(
        format!("Update to {latest} now?"),
        format!("现在升级到 {latest} 吗？")
    );
    let refused = tr!(
        "re-run with --yes to update without asking",
        "加上 --yes 重新运行就不用问了"
    );
    if !confirm::ask_default_yes(args.yes, &question, refused)? {
        println!(
            "{}",
            tr!(
                "Okay, maybe later. `cc-moss update` whenever you are ready.",
                "好，下次再说。想升级时运行 `cc-moss update` 就行。"
            )
        );
        return Ok(());
    }

    match &method {
        Method::Binary(exe) => replace_self(exe, &latest)?,
        managed => {
            run_manager(&managed.command(&latest).expect("a package manager"))?;
            confirm_upgraded(managed, &latest)?;
            // Homebrew's new keg carries the new app; cargo builds none.
            let new_app = managed
                .installed_path(&exe)
                .and_then(|p| std::fs::canonicalize(p).ok())
                .and_then(|p| app::bundled_near(&p));
            app::follow_update(new_app.as_deref());
        }
    }
    println!(
        "{}",
        tr!(
            format!("Updated to {latest}. Same MOSS, freshly grown."),
            format!("已升级到 {latest}。还是那个 MOSS，又精神了一点。")
        )
    );
    Ok(())
}

/// A package manager can finish happily without upgrading (a tap that has
/// not caught up yet, `HOMEBREW_NO_AUTO_UPDATE`): ask the installed program.
fn confirm_upgraded(method: &Method, latest: &Version) -> Result<()> {
    let Some(installed) = method.installed_path(&current_exe()?) else {
        return Ok(());
    };
    let out = moss_core::command::run_with_timeout(
        Command::new(&installed).arg("--version"),
        SELF_TEST_TIMEOUT,
    );
    let said = out
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if said == format!("cc-moss {latest}") {
        return Ok(());
    }
    let now = said.strip_prefix("cc-moss ").unwrap_or(&said).to_string();
    bail!(tr!(
        format!(
            "the package manager finished, but {} is still {now}, not {latest}. Its list of versions may not know {latest} yet; try again in a while (with Homebrew: `brew update` first)",
            installed.display()
        ),
        format!(
            "包管理器跑完了，但 {} 还是 {now}，不是 {latest}。可能它的版本列表还没收录 {latest}；过一会儿再试（Homebrew 可以先 `brew update`）",
            installed.display()
        )
    ))
}

/// Run `brew upgrade …` or `cargo install …` in the user's terminal.
fn run_manager(cmd: &[String]) -> Result<()> {
    let line = cmd.join(" ");
    match Command::new(&cmd[0]).args(&cmd[1..]).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!(tr!(
            format!("`{line}` did not succeed ({s}); its output above says why"),
            format!("`{line}` 没有成功（{s}），原因见上面的输出")
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(tr!(
            format!("{} is not on PATH; run `{line}` where it is", cmd[0]),
            format!("PATH 里找不到 {}；请在能找到它的地方运行 `{line}`", cmd[0])
        )),
        Err(e) => bail!(tr!(
            format!("could not run `{line}`: {e}"),
            format!("没能运行 `{line}`：{e}")
        )),
    }
}

/// Removes the work folder however the update ends.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Download, verify, test and swap in the release for this machine.
fn replace_self(exe: &Path, version: &Version) -> Result<()> {
    let Some(target) = target() else {
        bail!(tr!(
            format!(
                "there is no release build for this system; get {version} from {}",
                update::releases_url()
            ),
            format!(
                "这个系统没有现成的发布包；请到 {} 获取 {version}",
                update::releases_url()
            )
        ));
    };
    let name = format!("cc-moss-v{version}-{target}");
    let tarball = format!("{name}.tar.gz");
    let url = format!("{}/download/v{version}/{tarball}", update::releases_url());
    let dir = exe.parent().context("the program has no folder")?;

    // Work next to the program, so the final rename stays on one filesystem.
    let work = dir.join(format!(".cc-moss-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    if let Err(e) = std::fs::create_dir(&work) {
        if matches!(
            e.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
        ) {
            bail!(by_hand(dir, exe, &url, &tarball, &name));
        }
        return Err(unchanged(anyhow!(e).context(format!(
            "cannot create a work folder in {}",
            dir.display()
        ))));
    }
    let work = WorkDir(work);
    swap_in(exe, version, &work.0, &url, &tarball, &name).map_err(unchanged)?;
    // The macOS archive carries the app next to the binary; take it before
    // the work folder goes.
    app::follow_update(app::bundled_near(&work.0.join(&name).join("cc-moss")).as_deref());
    Ok(())
}

fn swap_in(
    exe: &Path,
    version: &Version,
    work: &Path,
    url: &str,
    tarball: &str,
    name: &str,
) -> Result<()> {
    println!(
        "{}",
        tr!(
            format!("Downloading {tarball} …"),
            format!("正在下载 {tarball}……")
        )
    );
    let archive = work.join(tarball);
    let sums = work.join(format!("{tarball}.sha256"));
    download(url, &archive)?;
    download(&format!("{url}.sha256"), &sums)?;
    verify(&archive, &sums)?;

    // The archive carries the build machine's uid; under sudo, tar would
    // restore it and hand the binary to whichever local user has that uid.
    let out = Command::new("tar")
        .args(["--no-same-owner", "--no-same-permissions"])
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(work)
        .output()
        .map_err(|e| {
            anyhow!(tr!(
                format!("could not run tar: {e}"),
                format!("没能运行 tar：{e}")
            ))
        })?;
    let new = work.join(name).join("cc-moss");
    // A plain file, not a link to something else: checked before anything runs.
    let plain_file = std::fs::symlink_metadata(&new).is_ok_and(|m| m.file_type().is_file());
    if !out.status.success() || !plain_file {
        bail!(tr!(
            format!(
                "the download did not unpack into {name}/cc-moss: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            format!(
                "下载的包没能解出 {name}/cc-moss：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            )
        ));
    }
    let old = std::fs::metadata(exe)?;
    std::fs::set_permissions(&new, old.permissions())?;
    keep_owner(&new, &old)?;
    self_test(&new, version)?;
    std::fs::rename(&new, exe).map_err(|e| {
        anyhow!(tr!(
            format!("could not put the new version in place: {e}"),
            format!("没能把新版本放到位：{e}")
        ))
    })
}

/// The new file belongs to whoever owned the old one. Changing the owner
/// needs root; without it the file is already ours, and ours is only wrong
/// when the old one belonged to someone else.
fn keep_owner(new: &Path, old: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if std::os::unix::fs::chown(new, Some(old.uid()), Some(old.gid())).is_ok() {
        return Ok(());
    }
    let now = std::fs::metadata(new)?;
    if now.uid() != old.uid() {
        bail!(tr!(
            format!(
                "the new file would belong to a different user than {}",
                new.display()
            ),
            format!("新文件的所有者会和原来的 {} 不一样", new.display())
        ));
    }
    Ok(())
}

/// Wrap a failure so it says the old program is still there.
fn unchanged(e: anyhow::Error) -> anyhow::Error {
    anyhow!(tr!(
        format!("the update did not go through, so your cc-moss is unchanged: {e:#}"),
        format!("这次没升级成功，你的 cc-moss 原样没动：{e:#}")
    ))
}

/// What to say when the program's folder is not writable (cc-moss never
/// runs sudo itself).
fn by_hand(dir: &Path, exe: &Path, url: &str, tarball: &str, name: &str) -> String {
    let steps = format!(
        "  curl -fsSLO {url}\n  tar -xzf {tarball}\n  sudo install -m 755 {name}/cc-moss {}",
        exe.display()
    );
    tr!(
        format!(
            "cannot write to {}, so cc-moss cannot replace itself; nothing was changed. To update by hand:\n{steps}",
            dir.display()
        ),
        format!(
            "没有 {} 的写权限，cc-moss 没法替换自己；什么都没改。可以手动升级：\n{steps}",
            dir.display()
        )
    )
}

fn download(url: &str, to: &Path) -> Result<()> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--max-time",
            DOWNLOAD_TIMEOUT_SECS,
            "-o",
        ])
        .arg(to)
        .arg(url)
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => bail!(tr!(
            format!(
                "could not download {url}: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            format!(
                "没能下载 {url}：{}",
                String::from_utf8_lossy(&o.stderr).trim()
            )
        )),
        Err(e) => bail!(tr!(
            format!("could not download {url}: curl: {e}"),
            format!("没能下载 {url}：curl：{e}")
        )),
    }
}

/// The download's SHA-256 against the first word of its `.sha256` file.
fn verify(archive: &Path, sums: &Path) -> Result<()> {
    let expected = std::fs::read_to_string(sums)?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let actual = sha256(archive)?;
    if expected.len() != 64 || expected != actual {
        bail!(tr!(
            "the download does not match its published checksum",
            "下载的文件和发布的校验和对不上"
        ));
    }
    println!("{}", tr!("Checksum matches.", "校验和一致。"));
    Ok(())
}

/// SHA-256 with the system's tool: `sha256sum` (Linux), else `shasum`
/// (macOS, and Linux with perl). Both print `<hex>  <file>`.
fn sha256(path: &Path) -> Result<String> {
    let tools: [(&str, &[&str]); 2] = [("sha256sum", &[]), ("shasum", &["-a", "256"])];
    for (tool, args) in tools {
        if let Ok(out) = Command::new(tool).args(args).arg(path).output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(hex) = text.split_whitespace().next() {
                    return Ok(hex.to_ascii_lowercase());
                }
            }
        }
    }
    bail!(tr!(
        "cannot verify the download: neither sha256sum nor shasum is installed",
        "没法校验下载的文件：sha256sum 和 shasum 都没装"
    ))
}

/// The new program must start and name the version it should be.
fn self_test(new: &Path, version: &Version) -> Result<()> {
    let want = format!("cc-moss {version}");
    let out =
        moss_core::command::run_with_timeout(Command::new(new).arg("--version"), SELF_TEST_TIMEOUT);
    let said = out
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if said != want {
        bail!(tr!(
            format!("the new cc-moss did not start properly (expected `{want}`, got `{said}`)"),
            format!("新版本没能正常启动（应该输出 `{want}`，实际是 `{said}`）")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_manager_commands() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(
            Method::Homebrew.command(&v).unwrap().join(" "),
            "brew upgrade larrykoo711/tap/cc-moss"
        );
        assert_eq!(
            Method::Cargo.command(&v).unwrap().join(" "),
            "cargo install --locked --git https://github.com/larrykoo711/cc-moss --tag v1.2.3 cc-moss"
        );
        assert_eq!(Method::Binary(PathBuf::from("/x")).command(&v), None);
    }

    #[test]
    fn the_cellar_means_homebrew() {
        let exe = Path::new("/opt/homebrew/Cellar/cc-moss/0.1.0/bin/cc-moss");
        assert_eq!(Method::detect(exe), Method::Homebrew);
        let exe = Path::new("/usr/local/bin/cc-moss");
        assert_eq!(Method::detect(exe), Method::Binary(exe.to_path_buf()));
    }

    #[test]
    fn this_machine_has_a_release_build() {
        assert!(target().is_some());
    }
}
