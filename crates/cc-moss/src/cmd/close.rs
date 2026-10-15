//! `cc-moss close <name>…`: stop offering a session for restore.

use anyhow::Result;
use moss_core::registry::Registry;
use moss_core::restore::resolve;
use moss_core::slot::{now_ms, Status};
use moss_core::store_dir;

pub fn run(names: &[String]) -> Result<()> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    for name in names {
        let slot = resolve(&slots, name)?;
        match slot.effective_status(moss_core::proc::is_alive) {
            Status::Running => {
                println!("{name} is still running; exit it in Claude Code instead.");
                continue;
            }
            Status::Closed => {
                println!("{name} is already closed.");
                continue;
            }
            Status::Interrupted => {}
        }
        registry.close_by_user(&slot.slot_id, now_ms())?;
        println!("Closed {name}: it will not be offered for restore.");
    }
    Ok(())
}
