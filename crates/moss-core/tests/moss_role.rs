//! Closing an old Moss closes only Moss's records: the role on Moss's own
//! tmux server, never a session elsewhere that carries the role, and never
//! the Moss that just started.

use moss_core::event::{Context, Event, StartSource, TmuxLoc};
use moss_core::registry::Registry;
use moss_core::slot::Status;

fn start(reg: &mut Registry, id: &str, socket: &str, pid: u32) {
    let ctx = Context {
        pid: Some(pid),
        pid_start: Some(format!("start-{pid}")),
        tmux: Some(TmuxLoc {
            socket: socket.into(),
            session_id: Some(format!("${pid}")),
            session_name: Some("moss".into()),
            pane_id: format!("%{pid}"),
        }),
        launch_args: None,
        now_ms: 1_000,
    };
    let event = Event::Start {
        source: StartSource::Startup,
        session_id: id.into(),
        cwd: "/state/moss".into(),
        title: None,
        transcript_path: None,
    };
    reg.apply(&event, &ctx).unwrap();
    let slot = reg.slot_for_event(id, &ctx).unwrap().unwrap();
    reg.set_role(&slot.slot_id, "moss").unwrap();
}

fn status(reg: &Registry, id: &str) -> (Status, Option<String>) {
    let s = reg
        .slots()
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == id)
        .unwrap();
    (s.status, s.end_reason)
}

#[test]
fn closing_moss_keeps_to_its_own_server_and_the_new_one() {
    let mut reg = Registry::open_in_memory().unwrap();
    start(
        &mut reg,
        "old-moss",
        "/private/tmp/tmux-501/cc-moss-moss",
        10,
    );
    start(
        &mut reg,
        "new-moss",
        "/private/tmp/tmux-501/cc-moss-moss",
        11,
    );
    // Carries the role, but runs on the user's own tmux server.
    start(&mut reg, "elsewhere", "/private/tmp/tmux-501/default", 12);
    // A server whose name only ends like Moss's.
    start(
        &mut reg,
        "lookalike",
        "/private/tmp/tmux-501/x-cc-moss-moss",
        13,
    );

    let closed = reg
        .close_role(
            "moss",
            "cc-moss-moss",
            Some("new-moss"),
            "moss-restarted",
            2_000,
        )
        .unwrap();
    assert_eq!(closed, 1);
    assert_eq!(
        status(&reg, "old-moss"),
        (Status::Closed, Some("moss-restarted".into()))
    );
    for open in ["new-moss", "elsewhere", "lookalike"] {
        assert_eq!(status(&reg, open).0, Status::Running, "{open}");
    }
    // Switched off: every Moss on its server, the new one too.
    assert_eq!(
        reg.close_role("moss", "cc-moss-moss", None, "moss-stopped", 3_000)
            .unwrap(),
        1
    );
    assert_eq!(status(&reg, "new-moss").0, Status::Closed);
    assert_eq!(status(&reg, "elsewhere").0, Status::Running);
}
