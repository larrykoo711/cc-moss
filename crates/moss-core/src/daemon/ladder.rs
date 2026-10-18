//! Watching over a child process: is it still there, and when to start it
//! again. Pure logic; the caller passes the time, so tests never sleep.
//!
//! Two rules from the reference project (MIT) and the design (A10):
//! a check that cannot tell counts as alive (missing a death is cheaper than
//! killing a live process twice), and restarts back off 30 s, 2 min, 10 min,
//! then every hour, starting over once a run lasted 10 minutes.

use std::time::{Duration, Instant};

use crate::proc::Liveness;

/// Waits before each restart; the last one repeats.
pub const STEPS: [Duration; 4] = [
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(10 * 60),
    Duration::from_secs(60 * 60),
];

/// A run this long counts as stable: the next failure waits the first step again.
pub const STABLE: Duration = Duration::from_secs(10 * 60);

/// How long to wait after each failure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestartLadder {
    failures: usize,
    started: Option<Instant>,
}

impl RestartLadder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The child was (re)started at `now`.
    pub fn started(&mut self, now: Instant) {
        self.started = Some(now);
    }

    /// The child died at `now`: how long to wait before starting it again.
    pub fn failed(&mut self, now: Instant) -> Duration {
        if self
            .started
            .is_some_and(|at| now.saturating_duration_since(at) >= STABLE)
        {
            self.failures = 0;
        }
        self.started = None;
        let wait = STEPS[self.failures.min(STEPS.len() - 1)];
        self.failures += 1;
        wait
    }

    /// Failures in a row so far.
    pub fn failures(&self) -> usize {
        self.failures
    }
}

/// What the caller should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Running (or cannot tell): leave it.
    Running,
    /// Down: start it again at this time.
    WaitUntil(Instant),
    /// Down and its wait is over (or it never ran): start it, then call
    /// [`Watchdog::started`].
    StartNow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Up,
    Down { retry_at: Option<Instant> },
}

/// One watched child: liveness in, what to do out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watchdog {
    ladder: RestartLadder,
    state: State,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl Watchdog {
    /// Not started yet: the first check says [`Step::StartNow`].
    pub fn new() -> Self {
        Watchdog {
            ladder: RestartLadder::new(),
            state: State::Down { retry_at: None },
        }
    }

    pub fn started(&mut self, now: Instant) {
        self.ladder.started(now);
        self.state = State::Up;
    }

    /// `liveness` is only read while the child is up.
    pub fn check(&mut self, now: Instant, liveness: Liveness) -> Step {
        match self.state {
            State::Up => match liveness {
                Liveness::Alive | Liveness::Unknown => Step::Running,
                Liveness::Dead => {
                    let at = now + self.ladder.failed(now);
                    self.state = State::Down { retry_at: Some(at) };
                    Step::WaitUntil(at)
                }
            },
            State::Down { retry_at: None } => Step::StartNow,
            State::Down { retry_at: Some(at) } if now >= at => Step::StartNow,
            State::Down { retry_at: Some(at) } => Step::WaitUntil(at),
        }
    }

    pub fn ladder(&self) -> &RestartLadder {
        &self.ladder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn restart_ladder_backs_off() {
        let t0 = Instant::now();
        let mut ladder = RestartLadder::new();
        let mut now = t0;
        let mut waits = Vec::new();
        for _ in 0..6 {
            ladder.started(now);
            now += 5 * S; // dies quickly every time
            let wait = ladder.failed(now);
            waits.push(wait.as_secs());
            now += wait;
        }
        assert_eq!(waits, [30, 120, 600, 3600, 3600, 3600]);
    }

    #[test]
    fn ladder_resets_after_stable_run() {
        let t0 = Instant::now();
        let mut ladder = RestartLadder::new();
        ladder.started(t0);
        assert_eq!(ladder.failed(t0 + S), 30 * S);
        ladder.started(t0 + 31 * S);
        assert_eq!(ladder.failed(t0 + 32 * S), 120 * S);
        // A run of exactly ten minutes is stable.
        let back = t0 + 200 * S;
        ladder.started(back);
        assert_eq!(ladder.failed(back + STABLE), 30 * S);
        assert_eq!(ladder.failures(), 1);
        // Just short of it is not.
        ladder.started(back + STABLE + 30 * S);
        assert_eq!(ladder.failed(back + STABLE + 30 * S + STABLE - S), 120 * S);
    }

    #[test]
    fn unknown_liveness_counts_as_alive() {
        let t0 = Instant::now();
        let mut dog = Watchdog::new();
        dog.started(t0);
        for i in 1..5 {
            assert_eq!(dog.check(t0 + i * S, Liveness::Unknown), Step::Running);
        }
        assert_eq!(dog.ladder().failures(), 0);
    }

    #[test]
    fn watchdog_starts_waits_and_restarts() {
        let t0 = Instant::now();
        let mut dog = Watchdog::new();
        assert_eq!(dog.check(t0, Liveness::Dead), Step::StartNow);
        dog.started(t0);
        assert_eq!(dog.check(t0 + S, Liveness::Alive), Step::Running);
        let down = t0 + 2 * S;
        assert_eq!(
            dog.check(down, Liveness::Dead),
            Step::WaitUntil(down + 30 * S)
        );
        // While down, liveness is not read: no second failure is counted.
        assert_eq!(
            dog.check(down + 10 * S, Liveness::Dead),
            Step::WaitUntil(down + 30 * S)
        );
        assert_eq!(dog.check(down + 30 * S, Liveness::Dead), Step::StartNow);
        dog.started(down + 30 * S);
        let again = down + 31 * S;
        assert_eq!(
            dog.check(again, Liveness::Dead),
            Step::WaitUntil(again + 120 * S)
        );
    }
}
