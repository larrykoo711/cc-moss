//! `cc-moss send <name> -- <text>`: type text into a session's own input,
//! as if you had typed it in its terminal. One line is typed and sent with
//! Return; several lines go in as one bracketed paste, so Claude Code takes
//! them whole instead of sending at the first newline.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{bail, Context, Result};
use moss_core::registry::Registry;
use moss_core::store_dir;
use moss_core::tmux::Socket;

pub fn run(name: &str, text: &str) -> Result<()> {
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        bail!(tr!(
            "the text holds control characters (such as Ctrl-C); they would act as keys, so nothing was sent",
            "文字里有控制字符（比如 Ctrl-C），它们会被当成按键，所以没有发送"
        ));
    }
    let text = text.trim_end_matches('\n');
    if text.trim().is_empty() {
        bail!(tr!("there is nothing to send", "没有要发送的内容"));
    }
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let slot = super::resolve(&slots, name)?;
    if slot.effective_status(moss_core::proc::is_alive) != moss_core::slot::Status::Running {
        bail!(tr!(
            format!("`{name}` is not running; bring it back first with `cc-moss restore {name}`"),
            format!("`{name}` 现在没在运行，先用 `cc-moss restore {name}` 把它找回来")
        ));
    }
    let Some(loc) = slot.tmux.as_ref() else {
        if super::attach::background_job(slot).is_some() {
            bail!(tr!(
                format!(
                    "`{name}` runs in Claude Code's background; open it with `cc-moss attach {name}` and type there"
                ),
                format!("`{name}` 在 Claude Code 后台运行，先用 `cc-moss attach {name}` 打开它，再在里面输入")
            ));
        }
        bail!(tr!(
            format!(
                "`{name}` is not running in tmux, so there is no way to type into it from here"
            ),
            format!("`{name}` 不在 tmux 里运行，没法从这里替你输入")
        ));
    };
    let socket = Socket::Path(loc.socket.clone());
    let pane = loc.pane_id.as_str();
    if text.contains('\n') {
        paste(&socket, pane, text)?;
    } else {
        tmux(&socket, &["send-keys", "-t", pane, "-l", text])?;
    }
    // A moment for the input to take the text before Return.
    std::thread::sleep(std::time::Duration::from_millis(60));
    tmux(&socket, &["send-keys", "-t", pane, "Enter"])?;
    Ok(())
}

/// Several lines as one bracketed paste, through a private buffer file.
fn paste(socket: &Socket, pane: &str, text: &str) -> Result<()> {
    let dir = store_dir::state_dir();
    store_dir::ensure_private_dir(&dir)?;
    let file = dir.join(format!("send-{}.txt", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&file)
            .context("buffer file")?;
        f.write_all(text.as_bytes())?;
    }
    let buffer = format!("cc-moss-send-{}", std::process::id());
    let file_arg = file.to_string_lossy().into_owned();
    let result = tmux(socket, &["load-buffer", "-b", &buffer, &file_arg]).and_then(|_| {
        tmux(
            socket,
            &["paste-buffer", "-p", "-d", "-b", &buffer, "-t", pane],
        )
    });
    let _ = std::fs::remove_file(&file);
    result
}

fn tmux(socket: &Socket, args: &[&str]) -> Result<()> {
    let out = moss_core::tmux::command()
        .args(socket.args())
        .args(args)
        .output()
        .context("running tmux")?;
    if !out.status.success() {
        bail!(
            "tmux {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}
