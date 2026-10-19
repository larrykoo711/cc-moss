//! `cc-moss suggest <name>`: the dim next-prompt suggestion Claude Code
//! shows in a session's input box, read from its tmux pane. Nothing when
//! the session is not idle at an empty prompt, or shows no suggestion.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use moss_core::registry::Registry;
use moss_core::slot::{Slot, Status};
use moss_core::store_dir;
use moss_core::tmux::Socket;

pub fn run(name: &str, json: bool) -> Result<()> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let slot = super::resolve(&slots, name)?;
    let found = read(slot);
    if json {
        println!(
            "{}",
            serde_json::json!({ "schema": 1, "suggestion": found })
        );
    } else if let Some(text) = found {
        println!("{text}");
    }
    Ok(())
}

/// The suggestion, if the session runs in tmux, idle, and shows one.
fn read(slot: &Slot) -> Option<String> {
    if slot.effective_status(moss_core::proc::is_alive) != Status::Running || slot.busy {
        return None;
    }
    let in_turn = slot
        .transcript_path
        .as_deref()
        .and_then(|p| moss_core::transcript::turn_open(Path::new(p)))
        == Some(true);
    if in_turn {
        return None;
    }
    let loc = slot.tmux.as_ref()?;
    let out = moss_core::command::run_with_timeout(
        Command::new("tmux")
            .args(Socket::Path(loc.socket.clone()).args())
            .args(["capture-pane", "-p", "-J", "-e", "-t", &loc.pane_id]),
        Duration::from_secs(3),
    )
    .ok()
    .filter(|o| o.status.success())?;
    moss_core::moss::inbox::suggestion(&String::from_utf8_lossy(&out.stdout))
}
