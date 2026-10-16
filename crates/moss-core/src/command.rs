//! Running helper processes without letting them stall a hook.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Why [`run_with_timeout`] has no output.
#[derive(Debug)]
pub enum RunError {
    /// The program could not be started, typically because it is not installed.
    NotStarted(std::io::Error),
    /// It ran past the timeout and was killed.
    TimedOut,
}

impl RunError {
    /// True when the program is not installed (not found on `PATH`).
    pub fn is_not_installed(&self) -> bool {
        matches!(self, RunError::NotStarted(e) if e.kind() == std::io::ErrorKind::NotFound)
    }
}

/// Run `cmd` and collect its stdout and stderr, or kill it once `timeout`
/// passes. Both streams are drained while the child runs, so output larger
/// than a pipe buffer cannot block it.
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<Output, RunError> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::NotStarted)?;
    let deadline = Instant::now() + timeout;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::TimedOut);
            }
        }
    };
    // The child has exited; its pipes close unless it left a grandchild
    // holding them, so wait for the readers only until the deadline (plus a
    // little grace for a child that exited right at it).
    let wait = deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(100);
    let until = Instant::now() + wait;
    let collect = |rx: mpsc::Receiver<Vec<u8>>| {
        rx.recv_timeout(until.saturating_duration_since(Instant::now()))
            .map_err(|_| RunError::TimedOut)
    };
    Ok(Output {
        status,
        stdout: collect(stdout)?,
        stderr: collect(stderr)?,
    })
}

/// [`run_with_timeout`] for callers that only care whether there is output:
/// `None` if the program could not start or timed out.
pub fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> Option<Output> {
    run_with_timeout(cmd, timeout).ok()
}

/// Read `pipe` to the end on its own thread.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        None => {
            let _ = tx.send(Vec::new());
        }
    }
    rx
}
