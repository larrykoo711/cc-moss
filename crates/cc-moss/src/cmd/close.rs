//! `cc-moss close <name>…`: stop offering a session for restore.

use anyhow::Result;
use moss_core::registry::Registry;

use moss_core::slot::{now_ms, Status};
use moss_core::store_dir;

pub fn run(names: &[String]) -> Result<()> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let mut refused = false;
    for name in names {
        let slot = super::resolve(&slots, name)?;
        // cc-moss keeps Moss going; only switching it off stops it.
        if slot.is_moss() {
            refused = true;
            continue;
        }
        match slot.effective_status(moss_core::proc::is_alive) {
            Status::Running => {
                println!(
                    "{}",
                    tr!(
                        format!("{name} is still running; exit it in Claude Code instead."),
                        format!("{name} 还在运行，请在 Claude Code 里退出它。")
                    )
                );
                continue;
            }
            Status::Closed => {
                println!(
                    "{}",
                    tr!(
                        format!("{name} is already closed."),
                        format!("{name} 已经关闭了。")
                    )
                );
                continue;
            }
            Status::Interrupted => {}
        }
        registry.close_by_user(&slot.slot_id, now_ms())?;
        println!(
            "{}",
            tr!(
                format!("Closed {name}. It won't be offered for restore again."),
                format!("已关闭 {name}，以后不会再提示恢复它。")
            )
        );
    }
    if refused {
        anyhow::bail!(super::moss::protected_hint());
    }
    Ok(())
}
