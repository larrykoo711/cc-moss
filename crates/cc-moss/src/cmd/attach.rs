//! `cc-moss attach <name>`: jump to the tmux session of a Claude Code session.

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{bail, Context, Result};
use moss_core::registry::Registry;
use moss_core::restore::resolve;
use moss_core::store_dir;
use moss_core::tmux::Socket;

pub fn run(name: &str) -> Result<()> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let slot = resolve(&slots, name)?;
    // A stopped session's tmux ids belong to a dead server and may now point
    // at another session.
    if slot.effective_status(moss_core::proc::is_alive) != moss_core::slot::Status::Running {
        bail!("`{name}` is not running; see `cc-moss restore`");
    }
    let Some(loc) = slot.tmux.as_ref() else {
        bail!("`{name}` is not running in tmux");
    };
    let Some(target) = loc.session_id.as_deref().or(loc.session_name.as_deref()) else {
        bail!("`{name}` has no known tmux session");
    };
    attach_tmux(&Socket::Path(loc.socket.clone()), target)
}

/// Switch to `target` when already inside tmux, otherwise replace this
/// process with `tmux attach-session`.
pub fn attach_tmux(socket: &Socket, target: &str) -> Result<()> {
    let mut cmd = Command::new("tmux");
    cmd.args(socket.args());
    if std::env::var_os("TMUX").is_some() {
        let status = cmd
            .args(["switch-client", "-t", target])
            .status()
            .context("tmux switch-client")?;
        anyhow::ensure!(status.success(), "tmux switch-client failed");
        Ok(())
    } else {
        cmd.args(["attach-session", "-t", target]);
        Err(cmd.exec()).context("tmux attach-session")
    }
}
