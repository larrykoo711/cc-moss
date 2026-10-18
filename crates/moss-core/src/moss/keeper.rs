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

/// What the daemon should do about Moss now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Start Moss (look once more first; it may be back by itself), then
    /// call [`Keeper::started`], [`Keeper::adopted`] or [`Keeper::start_failed`].
    Start,
    /// It is switched off: end it if it runs.
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
                return Action::Nothing;
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
        assert_eq!(
            k.tick(t0 + 3 * S, false, Some(Liveness::Alive)),
            Action::Nothing
        );
        // On again: no wait left over from before, and no restart counted.
        assert_eq!(k.tick(t0 + 4 * S, true, None), Action::Start);
        k.started(t0 + 4 * S);
        assert_eq!(k.restarts(), 0);
        // Off on the very first tick still ends one left running.
        let mut fresh = Keeper::new(vec![]);
        assert_eq!(fresh.tick(t0, false, None), Action::Stop);
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
