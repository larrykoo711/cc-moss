use std::cell::RefCell;

use anyhow::Result;
use moss_core::tmux::{Runner, Tmux};

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<Vec<String>>>,
    reply: String,
}

impl Runner for Recorder {
    fn run(&self, args: &[String]) -> Result<String> {
        self.calls.borrow_mut().push(args.to_vec());
        Ok(self.reply.clone())
    }
}

#[test]
fn new_session_passes_argv_without_a_shell() {
    let t = Tmux::new(Recorder {
        reply: "4242\t$3\t%7\n".into(),
        ..Default::default()
    });
    let pane = t
        .new_session("h_em-a", "/work dir", &["claude", "--name=em a"])
        .unwrap();
    assert_eq!(
        (pane.pid, pane.session_id.as_str(), pane.pane_id.as_str()),
        (4242, "$3", "%7")
    );
    let call = &t.runner().calls.borrow()[0];
    assert_eq!(
        call[..5],
        [
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{pane_pid}\t#{session_id}\t#{pane_id}\t#{pane_start_command}"
        ]
    );
    assert_eq!(
        call[5..],
        ["-s", "h_em-a", "-c", "/work dir", "claude", "--name=em a"]
    );
}

#[test]
fn tmux_syntax_in_cwd_and_args_is_escaped() {
    let t = Tmux::new(Recorder {
        reply: "1\t$0\t%0\n".into(),
        ..Default::default()
    });
    t.new_session("s", "/a/C#;", &["claude", "--name=wip;"])
        .unwrap();
    let call = &t.runner().calls.borrow()[0];
    assert_eq!(call[8], r"/a/C##\;");
    assert_eq!(call[10], r"--name=wip\;");
}

#[test]
fn rename_targets_session_id_not_name() {
    let t = Tmux::new(Recorder::default());
    t.rename_session("$3", "h_em-b").unwrap();
    assert_eq!(
        t.runner().calls.borrow()[0],
        ["rename-session", "-t", "$3", "h_em-b"]
    );
}

#[test]
fn session_names_parses_lines() {
    let t = Tmux::new(Recorder {
        reply: "a\nb\n".into(),
        ..Default::default()
    });
    assert_eq!(t.session_names().unwrap(), vec!["a", "b"]);
}

#[test]
fn access_probe_command() {
    let t = Tmux::new(Recorder {
        reply: "$0\tmain\n".into(),
        ..Default::default()
    });
    assert_eq!(
        t.probe_access("/a/it's here"),
        moss_core::tmux::Access::Readable
    );
    let calls = t.runner().calls.borrow();
    // The probe runs inside the server, whose macOS privacy grants are what matter.
    assert_eq!(calls[1][0], "run-shell");
    assert_eq!(
        calls[1][1],
        r#"test -r '/a/it'\''s here' && test -x '/a/it'\''s here'"#
    );
}
