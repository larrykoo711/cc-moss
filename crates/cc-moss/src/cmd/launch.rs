//! What starting Claude Code in tmux needs from this process: tmux itself,
//! the `claude` binary as an absolute path, and a check that it did not exit
//! right away.
//!
//! tmux starts a pane's program with the *server's* environment, and a
//! long-running server may have a PATH without the directory `claude` lives
//! in. Resolving it here, with the user's PATH, avoids that.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use moss_core::platform::on_path;
use moss_core::tmux::{Pane, Runner, Tmux};

/// How long a fresh pane has to stay up before it counts as started.
const STARTUP_GRACE: Duration = Duration::from_millis(500);

/// `CC_MOSS_STARTUP_GRACE_MS` lets tests on a loaded machine wait longer: a
/// program that exits at once can take more than 500 ms to do so there.
fn startup_grace() -> Duration {
    std::env::var("CC_MOSS_STARTUP_GRACE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(STARTUP_GRACE, Duration::from_millis)
}

/// Fails with a hint when tmux is not on the PATH.
pub fn require_tmux() -> Result<()> {
    if on_path(&["tmux"]) {
        return Ok(());
    }
    bail!(tr!(
        "tmux is not installed (or not on your PATH). Run `cc-moss doctor` to see how to install it",
        "没找到 tmux（没装，或者不在 PATH 里）。运行 `cc-moss doctor` 看看怎么装"
    ))
}

/// The `claude` to run, as an absolute path: `CC_MOSS_CLAUDE_BIN` (a path,
/// or a name looked up on the PATH), else `claude` on the PATH, else where
/// Claude Code's installers put it.
pub fn claude_bin() -> Result<String> {
    let wanted = std::env::var("CC_MOSS_CLAUDE_BIN")
        .ok()
        .filter(|v| !v.is_empty());
    let found = match wanted.as_deref() {
        Some(p) if p.contains('/') => {
            if !is_executable(Path::new(p)) {
                bail!(tr!(
                    format!("CC_MOSS_CLAUDE_BIN is {p}, which is not an executable file"),
                    format!("CC_MOSS_CLAUDE_BIN 指向 {p}，但它不是可执行文件")
                ));
            }
            Some(PathBuf::from(p))
        }
        Some(name) => find_on_path(name),
        None => find_on_path("claude").or_else(installer_location),
    };
    let Some(path) = found else {
        let name = wanted.as_deref().unwrap_or("claude");
        bail!(tr!(
            format!(
                "Claude Code (`{name}`) was not found on your PATH. Install it, or set CC_MOSS_CLAUDE_BIN to its path; `cc-moss doctor` checks the setup"
            ),
            format!(
                "没找到 Claude Code（`{name}` 不在 PATH 里）。先装好它，或者把 CC_MOSS_CLAUDE_BIN 设成它的路径；`cc-moss doctor` 可以帮忙检查"
            )
        ));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
    Ok(path.to_string_lossy().into_owned())
}

/// `~/.local/bin/claude` (the native installer) or `~/.claude/local/claude`
/// (the older local install), which a PATH may not have yet.
fn installer_location() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").filter(|h| !h.is_empty())?);
    [".local/bin/claude", ".claude/local/claude"]
        .iter()
        .map(|rel| home.join(rel))
        .find(|p| is_executable(p))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Whether the pane's program ended within the first moments: the pane is
/// gone (the session ends with it) or dead (`remain-on-exit`).
pub fn exited_at_start<R: Runner>(tmux: &Tmux<R>, pane: &Pane) -> bool {
    let deadline = Instant::now() + startup_grace();
    loop {
        let dead = match tmux.runner().run(&[
            "display-message".into(),
            "-p".into(),
            "-t".into(),
            pane.pane_id.clone(),
            "#{pane_dead}".into(),
        ]) {
            Ok(out) => out.trim() == "1",
            // The pane, and with it the session, is gone.
            Err(_) => true,
        };
        if dead {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// What to say when [`exited_at_start`] is true.
pub fn exited_message(label: &str, claude: &str, cwd: &str) -> String {
    tr!(
        format!(
            "{label}: Claude Code exited right after it started. To see why, run `{claude}` in {cwd} yourself; `cc-moss doctor` checks the setup"
        ),
        format!(
            "{label}：Claude Code 刚启动就退出了。想看原因，可以在 {cwd} 里自己运行一下 `{claude}`；`cc-moss doctor` 也能帮忙检查"
        )
    )
}
