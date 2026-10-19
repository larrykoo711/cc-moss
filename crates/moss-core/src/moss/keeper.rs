//! Looking after Moss: whether it should run, whether it does, and when to
//! start it again. Pure logic; the caller asks tmux and passes the answer
//! and the time in, so tests never sleep and never start anything.
//!
//! Two rules (design A10): only tmux saying plainly that the session or its
//! server is gone counts as dead, anything else counts as alive (missing a
//! death is cheaper than a second Moss); and restarts back off along the
//! daemon's ladder (30 s, 2 min, 10 min, then hourly).

use std::time::{Duration, Instant};

use crate::daemon::ladder::{RestartLadder, Step, Watchdog};
use crate::proc::Liveness;

/// How often the daemon asks tmux whether Moss is still there.
pub const CHECK_EVERY: Duration = Duration::from_secs(60);

/// What `tmux has-session -t =moss` said: its exit status and stderr.
/// Only "can't find session", "no server running" and a socket file that is
/// not there ("error connecting to … (No such file or directory)") are dead;
/// everything else (a timeout, tmux missing, a socket it may not open, any
/// other complaint) is unknown.
pub fn classify_has_session(success: bool, stderr: &str) -> Liveness {
    if success {
        return Liveness::Alive;
    }
    let said = stderr.to_ascii_lowercase();
    let no_socket =
        said.contains("error connecting to") && said.contains("no such file or directory");
    let gone = [
        "can't find session",
        "cannot find session",
        "no server running",
    ];
    if no_socket || gone.iter().any(|g| said.contains(g)) {
        Liveness::Dead
    } else {
        Liveness::Unknown
    }
}

/// The `list-panes -a` format the daemon reads Moss's server with.
pub const PANES_FORMAT: &str = "#{session_id}|#{@cc-moss-role}|#{pane_dead}";

/// What Moss's tmux server holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub liveness: Liveness,
    /// Moss's sessions (marked with the role option) with a live pane.
    pub live: Vec<String>,
    /// Moss's sessions whose panes all died but stayed (`remain-on-exit`):
    /// Claude Code is gone, so this is a dead Moss to clear away.
    pub dead: Vec<String>,
}

/// What `tmux list-panes -a -F` [`PANES_FORMAT`] said. A session counts as
/// Moss only by its role option, whatever its name; Moss is alive when one
/// of them has a pane that has not died. A failure is read like
/// [`classify_has_session`]: only a server that is plainly not there is
/// dead.
pub fn classify_panes(success: bool, stdout: &str, stderr: &str) -> Seen {
    if !success {
        return Seen {
            liveness: classify_has_session(false, stderr),
            live: vec![],
            dead: vec![],
        };
    }
    let mut live: Vec<String> = Vec::new();
    let mut dead: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(id), Some(role), Some(pane_dead)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if role != super::ROLE || id.is_empty() {
            continue;
        }
        if pane_dead.trim() == "1" {
            if !dead.iter().any(|d| d == id) {
                dead.push(id.to_string());
            }
        } else if !live.iter().any(|l| l == id) {
            live.push(id.to_string());
        }
    }
    dead.retain(|d| !live.contains(d));
    Seen {
        liveness: if live.is_empty() {
            Liveness::Dead
        } else {
            Liveness::Alive
        },
        live,
        dead,
    }
}

/// What the daemon should do about Moss now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Start Moss (look once more first; it may be back by itself), then
    /// call [`Keeper::started`], [`Keeper::adopted`] or [`Keeper::start_failed`].
    Start,
    /// It is switched off: end it if it runs. Also while off, whenever it is
    /// seen running again (a stop that did not take, a Moss started by hand).
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Nothing decided yet (the daemon just started).
    Unset,
    Off,
    On,
}

/// One Moss, as the daemon sees it.
#[derive(Debug, Clone)]
pub struct Keeper {
    steps: Vec<Duration>,
    dog: Watchdog,
    mode: Mode,
    /// Started (or adopted) and not seen dead since.
    up: bool,
    /// It died; the next successful start is a restart.
    died: bool,
    restarts: u32,
}

impl Keeper {
    /// `steps` replaces the ladder's waits when not empty (tests).
    pub fn new(steps: Vec<Duration>) -> Keeper {
        Keeper {
            dog: Watchdog::with_ladder(RestartLadder::with_steps(steps.clone())),
            steps,
            mode: Mode::Unset,
            up: false,
            died: false,
            restarts: 0,
        }
    }

    /// `probe` is what tmux said this tick, or `None` when it was not asked.
    pub fn tick(&mut self, now: Instant, enabled: bool, probe: Option<Liveness>) -> Action {
        if !enabled {
            if self.mode == Mode::Off {
                return if probe == Some(Liveness::Alive) {
                    Action::Stop
                } else {
                    Action::Nothing
                };
            }
            self.reset(Mode::Off);
            return Action::Stop;
        }
        if self.mode != Mode::On {
            self.reset(Mode::On);
        }
        let liveness = probe.unwrap_or(Liveness::Unknown);
        match self.dog.check(now, liveness) {
            Step::Running => Action::Nothing,
            Step::WaitUntil(_) => {
                if self.up {
                    self.up = false;
                    self.died = true;
                }
                Action::Nothing
            }
            Step::StartNow => Action::Start,
        }
    }

    fn reset(&mut self, mode: Mode) {
        self.dog = Watchdog::with_ladder(RestartLadder::with_steps(self.steps.clone()));
        self.mode = mode;
        self.up = false;
        self.died = false;
        self.restarts = 0;
    }

    /// A new Moss runs.
    pub fn started(&mut self, now: Instant) {
        self.dog.started(now);
        self.up = true;
        if self.died {
            self.died = false;
            self.restarts += 1;
        }
    }

    /// One that was already running is looked after from now on, with the
    /// restarts counted so far.
    pub fn adopted(&mut self, now: Instant, restarts: u32) {
        self.dog.started(now);
        self.up = true;
        self.died = false;
        self.restarts = restarts;
    }

    /// Starting it failed: when to try again.
    pub fn start_failed(&mut self, now: Instant) -> Instant {
        self.up = false;
        self.dog.start_failed(now)
    }

    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    pub fn is_up(&self) -> bool {
        self.up
    }

    /// Failures in a row (for the log).
    pub fn failures(&self) -> usize {
        self.dog.ladder().failures()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    fn running(t0: Instant) -> Keeper {
        let mut k = Keeper::new(vec![]);
        assert_eq!(k.tick(t0, true, None), Action::Start);
        k.started(t0);
        k
    }

    #[test]
    fn has_session_is_dead_only_when_tmux_says_so() {
        assert_eq!(classify_has_session(true, ""), Liveness::Alive);
        for gone in [
            "can't find session: moss\n",
            "no server running on /tmp/tmux-501/cc-moss-moss\n",
            "error connecting to /tmp/tmux-501/cc-moss-moss (No such file or directory)\n",
        ] {
            assert_eq!(classify_has_session(false, gone), Liveness::Dead, "{gone}");
        }
        for unsure in [
            "",
            "tmux did not finish",
            "lost server",
            "server exited unexpectedly",
            "error connecting to /tmp/tmux-501/cc-moss-moss (Permission denied)\n",
        ] {
            assert_eq!(
                classify_has_session(false, unsure),
                Liveness::Unknown,
                "{unsure}"
            );
        }
    }

    #[test]
    fn moss_present_is_left_alone() {
        let t0 = Instant::now();
        let mut k = running(t0);
        for i in 1..10 {
            assert_eq!(
                k.tick(t0 + i * 60 * S, true, Some(Liveness::Alive)),
                Action::Nothing
            );
            assert_eq!(k.tick(t0 + i * 60 * S, true, None), Action::Nothing);
        }
        assert!(k.is_up());
        assert_eq!(k.restarts(), 0);
    }

    #[test]
    fn moss_unknown_liveness_does_not_restart() {
        let t0 = Instant::now();
        let mut k = running(t0);
        for i in 1..100 {
            assert_eq!(
                k.tick(t0 + i * 60 * S, true, Some(Liveness::Unknown)),
                Action::Nothing,
                "an unclear answer is not a death"
            );
        }
        assert!(k.is_up());
        assert_eq!(k.failures(), 0);
        assert_eq!(k.restarts(), 0);
    }

    #[test]
    fn moss_dead_restarts_after_its_wait() {
        let t0 = Instant::now();
        let mut k = running(t0);
        let dead = t0 + 60 * S;
        assert_eq!(k.tick(dead, true, Some(Liveness::Dead)), Action::Nothing);
        assert!(!k.is_up());
        assert_eq!(k.tick(dead + 29 * S, true, None), Action::Nothing);
        assert_eq!(k.tick(dead + 30 * S, true, None), Action::Start);
        k.started(dead + 30 * S);
        assert_eq!(k.restarts(), 1);
        // Dies again at once: the next wait is longer.
        let again = dead + 31 * S;
        assert_eq!(k.tick(again, true, Some(Liveness::Dead)), Action::Nothing);
        assert_eq!(k.tick(again + 119 * S, true, None), Action::Nothing);
        assert_eq!(k.tick(again + 120 * S, true, None), Action::Start);
        k.started(again + 120 * S);
        assert_eq!(k.restarts(), 2);
    }

    #[test]
    fn moss_failed_start_backs_off() {
        let t0 = Instant::now();
        let mut k = Keeper::new(vec![]);
        assert_eq!(k.tick(t0, true, None), Action::Start);
        assert_eq!(k.start_failed(t0), t0 + 30 * S);
        for s in [1, 10, 29] {
            assert_eq!(
                k.tick(t0 + s * S, true, Some(Liveness::Dead)),
                Action::Nothing
            );
        }
        assert_eq!(k.tick(t0 + 30 * S, true, None), Action::Start);
        let second = t0 + 30 * S;
        assert_eq!(k.start_failed(second), second + 120 * S);
        assert_eq!(k.tick(second + 119 * S, true, None), Action::Nothing);
        assert_eq!(k.tick(second + 120 * S, true, None), Action::Start);
        k.started(second + 120 * S);
        assert_eq!(k.restarts(), 0, "it never ran, so this is no restart");
    }

    #[test]
    fn switching_off_stops_and_on_starts_afresh() {
        let t0 = Instant::now();
        let mut k = running(t0);
        assert_eq!(k.tick(t0 + S, true, Some(Liveness::Dead)), Action::Nothing);
        assert_eq!(k.tick(t0 + 2 * S, false, None), Action::Stop);
        assert_eq!(k.tick(t0 + 3 * S, false, None), Action::Nothing);
        // On again: no wait left over from before, and no restart counted.
        assert_eq!(k.tick(t0 + 4 * S, true, None), Action::Start);
        k.started(t0 + 4 * S);
        assert_eq!(k.restarts(), 0);
        // Off on the very first tick still ends one left running.
        let mut fresh = Keeper::new(vec![]);
        assert_eq!(fresh.tick(t0, false, None), Action::Stop);
    }

    #[test]
    fn moss_seen_running_while_off_is_stopped_again() {
        let t0 = Instant::now();
        let mut k = running(t0);
        assert_eq!(k.tick(t0 + S, false, None), Action::Stop);
        // The stop did not take (tmux did not answer): the next look that
        // finds it running ends it again; unclear answers do nothing.
        assert_eq!(
            k.tick(t0 + 2 * S, false, Some(Liveness::Unknown)),
            Action::Nothing
        );
        assert_eq!(
            k.tick(t0 + 3 * S, false, Some(Liveness::Alive)),
            Action::Stop
        );
        assert_eq!(
            k.tick(t0 + 4 * S, false, Some(Liveness::Dead)),
            Action::Nothing
        );
        assert_eq!(
            k.tick(t0 + 5 * S, false, Some(Liveness::Alive)),
            Action::Stop
        );
    }

    #[test]
    fn panes_find_moss_by_its_mark_and_see_a_dead_pane() {
        let seen = classify_panes(true, "$0|moss|0\n$1||0\n", "");
        assert_eq!(seen.liveness, Liveness::Alive);
        assert_eq!(seen.live, ["$0"]);
        // Renamed by hand: still Moss, because of the mark.
        // A session without the mark (the user's own, even named moss) is not.
        let unmarked = classify_panes(true, "$3||0\n", "");
        assert_eq!(unmarked.liveness, Liveness::Dead);
        // remain-on-exit kept a dead pane: Moss is dead, and that session is
        // to be cleared away.
        let kept = classify_panes(true, "$0|moss|1\n", "");
        assert_eq!(kept.liveness, Liveness::Dead);
        assert_eq!(kept.dead, ["$0"]);
        // One live pane in a session is enough.
        let mixed = classify_panes(true, "$0|moss|1\n$0|moss|0\n$2|moss|1\n", "");
        assert_eq!(mixed.liveness, Liveness::Alive);
        assert_eq!(
            (mixed.live, mixed.dead),
            (vec!["$0".to_string()], vec!["$2".to_string()])
        );
        // Failures read as before.
        assert_eq!(
            classify_panes(false, "", "no server running on /tmp/tmux-501/x\n").liveness,
            Liveness::Dead
        );
        assert_eq!(
            classify_panes(false, "", "lost server").liveness,
            Liveness::Unknown
        );
    }

    #[test]
    fn adopting_keeps_the_count_and_shorter_steps_apply() {
        let t0 = Instant::now();
        let mut k = Keeper::new(vec![Duration::from_millis(200)]);
        assert_eq!(k.tick(t0, true, None), Action::Start);
        k.adopted(t0, 3);
        assert_eq!(k.restarts(), 3);
        let dead = t0 + S;
        assert_eq!(k.tick(dead, true, Some(Liveness::Dead)), Action::Nothing);
        assert_eq!(
            k.tick(dead + Duration::from_millis(200), true, None),
            Action::Start
        );
        k.started(dead + Duration::from_millis(200));
        assert_eq!(k.restarts(), 4);
    }
}
