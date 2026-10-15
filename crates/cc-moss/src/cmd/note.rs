//! `cc-moss note`: where a session stopped and what it is waiting for.

use anyhow::Result;
use moss_core::registry::Registry;

use moss_core::slot::Notes;
use moss_core::store_dir;

pub struct NoteArgs {
    pub name: String,
    pub goal: Option<String>,
    pub state: Option<String>,
    pub text: Option<String>,
    pub clear: bool,
}

pub fn run(args: NoteArgs) -> Result<()> {
    let registry = Registry::open(&store_dir::registry_path())?;
    let slots = registry.slots()?;
    let slot = super::resolve(&slots, &args.name)?;
    if args.clear {
        registry.set_notes(&slot.slot_id, &Notes::default())?;
        println!(
            "{}",
            tr!(
                format!("Cleared the notes for {}.", args.name),
                format!("已清空 {} 的备注。", args.name)
            )
        );
        return Ok(());
    }
    if args.goal.is_none() && args.state.is_none() && args.text.is_none() {
        let n = &slot.notes;
        for (label, value) in [
            (tr!("goal", "目标"), &n.goal),
            (tr!("state", "进度"), &n.state),
            (tr!("note", "备注"), &n.note),
        ] {
            println!(
                "{}{}",
                super::pad(label, 6),
                value.as_deref().unwrap_or("-")
            );
        }
        return Ok(());
    }
    // Fields not given keep their value.
    let notes = Notes {
        goal: args.goal.or_else(|| slot.notes.goal.clone()),
        state: args.state.or_else(|| slot.notes.state.clone()),
        note: args.text.or_else(|| slot.notes.note.clone()),
    };
    registry.set_notes(&slot.slot_id, &notes)?;
    println!(
        "{}",
        tr!(
            format!("Noted. Saved for {}.", args.name),
            format!("记下了，已保存到 {}。", args.name)
        )
    );
    Ok(())
}
