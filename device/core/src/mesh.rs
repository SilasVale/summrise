//! The mesh path (design §5.2): direct when the punch works, one relay when it does not.
//!
//! A [`Path`] is the decision half of the mesh — which route tool traffic takes, and what to do when
//! it dies. It opens no socket and reads no clock: the caller feeds it [`PathEvent`]s and the
//! instant of each one, and it answers with [`PathAction`]s. That is what makes the fallback a
//! decision instead of a race: the state machine names when the punch starts, how long it may take,
//! and exactly which failure hands over to the relay.
//!
//! ```text
//! Idle --Start--> Rendezvous --Candidates{fresh}--> Punching --PunchOpen--> Direct
//!                      |                              |
//!                      | deadline                     | deadline / PunchFailed
//!                      v                              v
//!                    Relayed <---------------------- Relayed
//!                      |
//!                      | RelayClosed (and DirectClosed from Direct)
//!                      v
//!                  Rendezvous --...--> Closed (sixth close: GiveUp)
//! ```
//!
//! Three things the diagram does not show:
//!
//! - **One timeout, two reasons.** [`Path::new`] takes one timeout. It bounds both the wait for
//!   candidates ([`RelayReason::RendezvousTimeout`]) and the punch
//!   ([`RelayReason::PunchTimeout`]), so a punch cannot be configured to outlive its own fallback.
//! - **A retry has no timer until it is started.** A close moves the path back to `Rendezvous` and
//!   answers [`PathAction::Retry`] with the backoff. The caller waits, then sends
//!   [`PathEvent::Start`]; that is the instant the next rendezvous timer is armed. Starting it at
//!   the close instead would spend the timeout during the backoff, and attempts 4 and 5 (8 s and
//!   16 s of wait) would time out before a candidate could arrive.
//! - **Retries are consecutive, not lifetime.** A close counts toward the six-close budget, but a
//!   path that was usable for `stable_window_ms` starts the count over before the close is counted:
//!   a path that worked for a minute and then drops is retry 1, not retry 7. A path that flaps six
//!   times inside the window still gives up.
//!
//! `Start` is accepted in `Idle` and `Rendezvous` only: starting from a live `Direct` or `Relayed`
//! path would tear down a working route on a stray event. [`PathState::Closed`] is terminal — once
//! the budget is gone, no event revives the path.

use std::fmt;

const MAX_ATTEMPTS: u8 = 5;
const BACKOFF_BASE_MS: u64 = 1_000;
const BACKOFF_CAP_MS: u64 = 60_000;

/// Where a path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathState {
    /// Nothing has been asked for yet.
    Idle,
    /// Waiting for the peer's candidates from the rendezvous.
    Rendezvous,
    /// Candidates arrived; the punch is running under a deadline.
    Punching,
    /// The punch worked; tool traffic goes peer to peer.
    Direct,
    /// No direct path could be established; tool traffic goes through the relay.
    Relayed,
    /// Too many paths have closed; the path is finished and no event revives it.
    Closed,
}

/// Something the mesh observed, fed to [`Path::apply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathEvent {
    /// Begin (or restart) looking for the peer.
    Start,
    /// The rendezvous returned the peer's addresses.
    Candidates {
        /// Whether the addresses are new enough to punch. A stale answer asks for a backoff
        /// instead of spending a punch on an address the rendezvous already knows is old.
        fresh: bool,
    },
    /// The punch was confirmed from the peer's address.
    PunchOpen,
    /// The punch failed before its deadline.
    PunchFailed,
    /// The direct connection closed after it had been in use.
    DirectClosed,
    /// The relay connection closed after it had been in use.
    RelayClosed,
    /// The caller's clock reached `now_ms`.
    Tick {
        /// The instant of the tick, on the same clock as [`Path::apply`]'s `now_ms`.
        now_ms: u64,
    },
}

/// What the caller should do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathAction {
    /// Ask the rendezvous for the peer's current candidates.
    RequestRendezvous,
    /// Run a punch, and treat it as failed at `deadline_ms`.
    StartPunch {
        /// The punch's deadline, on the same clock as [`Path::apply`]'s `now_ms`.
        deadline_ms: u64,
    },
    /// Carry traffic on the direct connection.
    UseDirect,
    /// Carry traffic through the relay, for `reason`.
    UseRelay {
        /// Why direct stopped being an option.
        reason: RelayReason,
    },
    /// Wait `after_ms`, then send [`PathEvent::Start`] to arm the next rendezvous timer.
    Retry {
        /// The backoff for this attempt, from [`backoff`].
        after_ms: u64,
        /// The path's close count since the last stable window: 1 for the first retry, 0 before any
        /// close.
        attempt: u8,
    },
    /// Stop trying, and say why.
    GiveUp {
        /// Why the path is finished.
        reason: GiveUpReason,
        /// The close count that exhausted the budget.
        attempt: u8,
    },
}

/// Why the relay became the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayReason {
    /// The punch reported a failure before its deadline.
    PunchFailed,
    /// The punch did not open by its deadline.
    PunchTimeout,
    /// Candidates did not arrive before the rendezvous deadline.
    RendezvousTimeout,
}

/// Why the path gave up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GiveUpReason {
    /// The path closed more times than the retry budget allows.
    AttemptsExhausted,
}

impl fmt::Display for RelayReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelayReason::PunchFailed => write!(f, "the punch failed"),
            RelayReason::PunchTimeout => write!(f, "the punch timed out"),
            RelayReason::RendezvousTimeout => write!(f, "the rendezvous timed out"),
        }
    }
}

impl fmt::Display for GiveUpReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GiveUpReason::AttemptsExhausted => write!(f, "too many path closes"),
        }
    }
}

/// One peer path's decisions.
///
/// Create one per peer the mesh is trying to reach. The type owns no connection: the actions it
/// returns are the mesh's orders, and the events it accepts are what the mesh already observed.
///
/// A [`PathAction::Retry`] is not immediate. The caller must wait its `after_ms`, then send
/// [`PathEvent::Start`]; the retry itself arms no timer, which is how the backoff stays outside the
/// rendezvous timeout budget. Every other order can be applied as soon as it is returned.
#[derive(Clone, Debug)]
pub struct Path {
    state: PathState,
    /// The one timeout: used for the rendezvous wait and for the punch.
    timeout_ms: u64,
    /// How long a usable path must stay up before a close resets the retry count.
    stable_window_ms: u64,
    /// When the current rendezvous or punch expires; `None` while no timer is running.
    deadline_ms: Option<u64>,
    /// When the path became usable, if it has; `None` while no direct or relay path is up.
    connected_at_ms: Option<u64>,
    /// How many paths have closed since the last stable window.
    attempts: u8,
}

impl Path {
    /// A path in [`PathState::Idle`].
    ///
    /// `punch_timeout_ms` bounds both the wait for candidates and the punch itself (the design's
    /// "one timer, two reasons"). `stable_window_ms` is how long a usable direct or relay path must
    /// stay up before a close resets the retry count; a path that closes sooner accumulates
    /// attempts and gives up on the sixth. Both are the caller's values, not constants here, so the
    /// device and the tests can run the same machine on different budgets.
    pub fn new(punch_timeout_ms: u64, stable_window_ms: u64) -> Self {
        Self {
            state: PathState::Idle,
            timeout_ms: punch_timeout_ms,
            stable_window_ms,
            deadline_ms: None,
            connected_at_ms: None,
            attempts: 0,
        }
    }

    /// The path's current state.
    pub fn state(&self) -> PathState {
        self.state
    }

    /// Apply one event and return the orders it produces, if any.
    ///
    /// `now_ms` is the instant the event is applied; it arms the deadline when a rendezvous or a
    /// punch begins, records the instant a path becomes usable (`PunchOpen`, or a relay fallback
    /// after a punch), and measures a close against `stable_window_ms`. A [`PathEvent::Tick`]
    /// carries its own instant in the event, and that is the one compared to the deadline. The
    /// module reads no clock of its own, so the same events and instants always produce the same
    /// actions.
    ///
    /// An event that is not an edge out of the current state returns an empty vector and changes
    /// nothing. [`PathState::Closed`] ignores every event.
    pub fn apply(&mut self, event: PathEvent, now_ms: u64) -> Vec<PathAction> {
        if self.state == PathState::Closed {
            return Vec::new();
        }

        match event {
            PathEvent::Start => {
                if !matches!(self.state, PathState::Idle | PathState::Rendezvous) {
                    return Vec::new();
                }
                self.begin_rendezvous(now_ms);
                vec![PathAction::RequestRendezvous]
            }
            PathEvent::Candidates { fresh: true } => {
                if self.state != PathState::Rendezvous {
                    return Vec::new();
                }
                let deadline_ms = now_ms.saturating_add(self.timeout_ms);
                self.state = PathState::Punching;
                self.deadline_ms = Some(deadline_ms);
                self.connected_at_ms = None;
                vec![PathAction::StartPunch { deadline_ms }]
            }
            PathEvent::Candidates { fresh: false } => {
                if self.state != PathState::Rendezvous {
                    return Vec::new();
                }
                vec![PathAction::Retry {
                    after_ms: backoff(self.attempts),
                    attempt: self.attempts,
                }]
            }
            PathEvent::PunchOpen => {
                if self.state != PathState::Punching {
                    return Vec::new();
                }
                self.state = PathState::Direct;
                self.deadline_ms = None;
                self.connected_at_ms = Some(now_ms);
                vec![PathAction::UseDirect]
            }
            PathEvent::PunchFailed => {
                if self.state != PathState::Punching {
                    return Vec::new();
                }
                self.use_relay(RelayReason::PunchFailed, now_ms)
            }
            PathEvent::DirectClosed => {
                if self.state != PathState::Direct {
                    return Vec::new();
                }
                self.after_close(now_ms)
            }
            PathEvent::RelayClosed => {
                if self.state != PathState::Relayed {
                    return Vec::new();
                }
                self.after_close(now_ms)
            }
            PathEvent::Tick { now_ms } => self.tick(now_ms),
        }
    }

    /// Enter `Rendezvous` and arm the one timer from `now_ms`.
    fn begin_rendezvous(&mut self, now_ms: u64) {
        self.state = PathState::Rendezvous;
        self.deadline_ms = Some(now_ms.saturating_add(self.timeout_ms));
        self.connected_at_ms = None;
    }

    /// The current rendezvous or punch expired, if it has a timer and the timer is due.
    fn tick(&mut self, now_ms: u64) -> Vec<PathAction> {
        let reason = match self.state {
            PathState::Rendezvous => RelayReason::RendezvousTimeout,
            PathState::Punching => RelayReason::PunchTimeout,
            // Direct and Relayed have no deadline; a tick cannot move them.
            _ => return Vec::new(),
        };

        match self.deadline_ms {
            Some(deadline_ms) if now_ms >= deadline_ms => self.use_relay(reason, now_ms),
            _ => Vec::new(),
        }
    }

    /// Hand the path to the relay, for `reason`.
    fn use_relay(&mut self, reason: RelayReason, now_ms: u64) -> Vec<PathAction> {
        self.state = PathState::Relayed;
        self.deadline_ms = None;
        self.connected_at_ms = match reason {
            // A rendezvous that never found a peer never made a usable path, even though the relay
            // is the order; a later close is not evidence of stability.
            RelayReason::RendezvousTimeout => None,
            RelayReason::PunchFailed | RelayReason::PunchTimeout => Some(now_ms),
        };
        vec![PathAction::UseRelay { reason }]
    }

    /// A direct or relay path closed: retry with the backoff, or give up on the sixth consecutive
    /// close.
    ///
    /// A path that was usable for at least `stable_window_ms` has its count reset before this close
    /// is counted, so a long-lived path that drops is retry 1. A rendezvous timeout never set
    /// `connected_at_ms`, so a relay close after one cannot look stable.
    ///
    /// The retry enters `Rendezvous` with **no** timer: the caller waits `Retry::after_ms`, then
    /// sends `Start`, which arms the rendezvous deadline. See the module docs for why the backoff
    /// must not be spent inside the timeout.
    fn after_close(&mut self, now_ms: u64) -> Vec<PathAction> {
        let was_stable = self.connected_at_ms.is_some_and(|connected_at_ms| {
            now_ms.saturating_sub(connected_at_ms) >= self.stable_window_ms
        });
        if was_stable {
            self.attempts = 0;
        }
        self.connected_at_ms = None;
        self.deadline_ms = None;
        self.attempts = self.attempts.saturating_add(1);

        if self.attempts > MAX_ATTEMPTS {
            self.state = PathState::Closed;
            vec![PathAction::GiveUp {
                reason: GiveUpReason::AttemptsExhausted,
                attempt: self.attempts,
            }]
        } else {
            self.state = PathState::Rendezvous;
            vec![PathAction::Retry {
                after_ms: backoff(self.attempts),
                attempt: self.attempts,
            }]
        }
    }
}

/// The wait before retry number `attempt`: 1 s, 2 s, 4 s, 8 s, 16 s, 32 s, then the 60 s cap.
///
/// The sequence is monotonic and never exceeds the 60 s cap. Attempt 0 — no path has closed
/// yet, which a stale candidate answer can produce at the first rendezvous — gets the base delay.
/// The doubling is saturating, so no `attempt` value can overflow or panic.
pub fn backoff(attempt: u8) -> u64 {
    let mut delay_ms = BACKOFF_BASE_MS;
    for _ in 1..attempt {
        delay_ms = delay_ms.saturating_mul(2).min(BACKOFF_CAP_MS);
    }
    delay_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT_MS: u64 = 500;
    const STABLE_WINDOW_MS: u64 = 60_000;
    const START_AT: u64 = 1_000;

    fn new_path() -> Path {
        Path::new(TIMEOUT_MS, STABLE_WINDOW_MS)
    }

    fn in_rendezvous() -> Path {
        let mut path = new_path();
        assert_eq!(
            path.apply(PathEvent::Start, START_AT),
            vec![PathAction::RequestRendezvous]
        );
        path
    }

    fn in_punching() -> Path {
        let mut path = in_rendezvous();
        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: true }, 2_000),
            vec![PathAction::StartPunch { deadline_ms: 2_500 }]
        );
        path
    }

    fn in_direct() -> Path {
        let mut path = in_punching();
        assert_eq!(
            path.apply(PathEvent::PunchOpen, 3_000),
            vec![PathAction::UseDirect]
        );
        path
    }

    fn in_relayed() -> Path {
        let mut path = in_punching();
        assert_eq!(
            path.apply(PathEvent::PunchFailed, 3_000),
            vec![PathAction::UseRelay {
                reason: RelayReason::PunchFailed
            }]
        );
        path
    }

    fn punch_again(path: &mut Path, now_ms: u64) {
        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: true }, now_ms),
            vec![PathAction::StartPunch {
                deadline_ms: now_ms + TIMEOUT_MS
            }]
        );
        assert_eq!(
            path.apply(PathEvent::PunchOpen, now_ms),
            vec![PathAction::UseDirect]
        );
    }

    fn in_closed() -> Path {
        let mut path = in_direct();
        let mut now_ms = 10_000;
        for _ in 0..5 {
            assert!(matches!(
                path.apply(PathEvent::DirectClosed, now_ms).as_slice(),
                [PathAction::Retry { .. }]
            ));
            punch_again(&mut path, now_ms);
            now_ms += 1_000;
        }
        assert_eq!(
            path.apply(PathEvent::DirectClosed, now_ms),
            vec![PathAction::GiveUp {
                reason: GiveUpReason::AttemptsExhausted,
                attempt: 6,
            }]
        );
        assert_eq!(path.state(), PathState::Closed);
        path
    }

    #[derive(Clone, Copy, Debug)]
    enum Place {
        Idle,
        Rendezvous,
        Punching,
        Direct,
        Relayed,
    }

    impl Place {
        fn state(self) -> PathState {
            match self {
                Place::Idle => PathState::Idle,
                Place::Rendezvous => PathState::Rendezvous,
                Place::Punching => PathState::Punching,
                Place::Direct => PathState::Direct,
                Place::Relayed => PathState::Relayed,
            }
        }
    }

    fn at(place: Place) -> Path {
        match place {
            Place::Idle => new_path(),
            Place::Rendezvous => in_rendezvous(),
            Place::Punching => in_punching(),
            Place::Direct => in_direct(),
            Place::Relayed => in_relayed(),
        }
    }

    #[test]
    fn a_new_path_is_idle_and_ignores_a_tick() {
        let mut path = new_path();
        assert_eq!(path.state(), PathState::Idle);
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 10_000 }, 10_000),
            Vec::new()
        );
        assert_eq!(path.state(), PathState::Idle);
    }

    #[test]
    fn start_moves_idle_to_rendezvous_and_asks_for_one() {
        let mut path = new_path();
        assert_eq!(
            path.apply(PathEvent::Start, START_AT),
            vec![PathAction::RequestRendezvous]
        );
        assert_eq!(path.state(), PathState::Rendezvous);
    }

    #[test]
    fn a_start_while_rendezvousing_restarts_the_timer() {
        let mut path = in_rendezvous();
        assert_eq!(
            path.apply(PathEvent::Start, 1_400),
            vec![PathAction::RequestRendezvous]
        );
        assert_eq!(path.state(), PathState::Rendezvous);
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 1_899 }, 1_899),
            Vec::new()
        );
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 1_900 }, 1_900),
            vec![PathAction::UseRelay {
                reason: RelayReason::RendezvousTimeout
            }]
        );
    }

    #[test]
    fn fresh_candidates_start_a_punch_at_the_deadline() {
        let mut path = in_rendezvous();
        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: true }, 2_000),
            vec![PathAction::StartPunch { deadline_ms: 2_500 }]
        );
        assert_eq!(path.state(), PathState::Punching);
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 2_499 }, 2_499),
            Vec::new()
        );
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 2_500 }, 2_500),
            vec![PathAction::UseRelay {
                reason: RelayReason::PunchTimeout
            }]
        );
    }

    #[test]
    fn stale_candidates_retry_without_leaving_rendezvous() {
        let mut path = in_rendezvous();
        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: false }, 1_200),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 0,
            }]
        );
        assert_eq!(path.state(), PathState::Rendezvous);
    }

    #[test]
    fn a_stale_candidate_retry_carries_the_attempt_count() {
        let mut path = in_direct();
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 10_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );
        punch_again(&mut path, 10_000);
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 11_000),
            vec![PathAction::Retry {
                after_ms: 2_000,
                attempt: 2,
            }]
        );
        assert_eq!(path.state(), PathState::Rendezvous);
        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: false }, 11_100),
            vec![PathAction::Retry {
                after_ms: 2_000,
                attempt: 2,
            }]
        );
    }

    #[test]
    fn punch_open_takes_the_direct_path() {
        let mut path = in_punching();
        assert_eq!(
            path.apply(PathEvent::PunchOpen, 3_000),
            vec![PathAction::UseDirect]
        );
        assert_eq!(path.state(), PathState::Direct);
    }

    #[test]
    fn punch_failure_hands_over_to_the_relay_immediately() {
        let mut path = in_punching();
        assert_eq!(
            path.apply(PathEvent::PunchFailed, 2_001),
            vec![PathAction::UseRelay {
                reason: RelayReason::PunchFailed
            }]
        );
        assert_eq!(path.state(), PathState::Relayed);
    }

    #[test]
    fn a_punch_past_its_deadline_times_out_to_the_relay() {
        let cases: [(u64, Vec<PathAction>, PathState); 3] = [
            (2_499, Vec::new(), PathState::Punching),
            (
                2_500,
                vec![PathAction::UseRelay {
                    reason: RelayReason::PunchTimeout,
                }],
                PathState::Relayed,
            ),
            (
                9_000,
                vec![PathAction::UseRelay {
                    reason: RelayReason::PunchTimeout,
                }],
                PathState::Relayed,
            ),
        ];

        for (now_ms, actions, state) in cases {
            let mut path = in_punching();
            assert_eq!(
                path.apply(PathEvent::Tick { now_ms }, now_ms),
                actions,
                "tick at {now_ms}"
            );
            assert_eq!(path.state(), state, "tick at {now_ms}");
        }
    }

    #[test]
    fn a_tick_uses_the_instant_in_the_event() {
        let mut path = in_punching();

        // `apply`'s argument is when the event is applied. A Tick names its own instant, and that
        // is the one compared to the deadline — the tick cannot be stale or early behind its name.
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 2_499 }, 90_000),
            Vec::new()
        );
        assert_eq!(path.state(), PathState::Punching);
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 2_500 }, 0),
            vec![PathAction::UseRelay {
                reason: RelayReason::PunchTimeout
            }]
        );
        assert_eq!(path.state(), PathState::Relayed);
    }

    #[test]
    fn a_rendezvous_past_its_deadline_times_out_with_its_own_reason() {
        let mut path = in_rendezvous();
        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: false }, 1_400),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 0,
            }]
        );
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 1_499 }, 1_499),
            Vec::new()
        );
        assert_eq!(path.state(), PathState::Rendezvous);
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 1_500 }, 1_500),
            vec![PathAction::UseRelay {
                reason: RelayReason::RendezvousTimeout
            }]
        );
        assert_eq!(path.state(), PathState::Relayed);
    }

    #[test]
    fn a_direct_close_retries_with_the_backoff() {
        let mut path = in_direct();
        let expected = [1_000u64, 2_000, 4_000, 8_000, 16_000];
        let mut now_ms = 10_000;

        for (index, delay) in expected.into_iter().enumerate() {
            let attempt = u8::try_from(index + 1).expect("at most five attempts");
            assert_eq!(
                path.apply(PathEvent::DirectClosed, now_ms),
                vec![PathAction::Retry {
                    after_ms: delay,
                    attempt,
                }],
                "attempt {attempt}"
            );
            assert_eq!(path.state(), PathState::Rendezvous);
            punch_again(&mut path, now_ms);
            now_ms += 1_000;
        }
    }

    #[test]
    fn a_relay_close_is_treated_like_a_direct_close() {
        let mut path = in_relayed();
        assert_eq!(
            path.apply(PathEvent::RelayClosed, 20_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );
        assert_eq!(path.state(), PathState::Rendezvous);

        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: true }, 21_000),
            vec![PathAction::StartPunch {
                deadline_ms: 21_500
            }]
        );
        assert_eq!(
            path.apply(PathEvent::PunchFailed, 21_000),
            vec![PathAction::UseRelay {
                reason: RelayReason::PunchFailed
            }]
        );
        assert_eq!(
            path.apply(PathEvent::RelayClosed, 21_000),
            vec![PathAction::Retry {
                after_ms: 2_000,
                attempt: 2,
            }]
        );
        assert_eq!(path.state(), PathState::Rendezvous);
    }

    #[test]
    fn a_sixth_close_gives_up_and_the_path_closes() {
        let mut path = in_direct();
        let mut now_ms = 10_000;

        for attempt in 1u8..=5 {
            assert_eq!(
                path.apply(PathEvent::DirectClosed, now_ms),
                vec![PathAction::Retry {
                    after_ms: backoff(attempt),
                    attempt,
                }],
                "attempt {attempt}"
            );
            punch_again(&mut path, now_ms);
            now_ms += 1_000;
        }

        assert_eq!(
            path.apply(PathEvent::DirectClosed, now_ms),
            vec![PathAction::GiveUp {
                reason: GiveUpReason::AttemptsExhausted,
                attempt: 6,
            }]
        );
        assert_eq!(path.state(), PathState::Closed);
    }

    #[test]
    fn a_path_up_for_the_stable_window_starts_the_retries_over() {
        // The path is usable from 3_000 (PunchOpen). Closing at 5_000 is a quick flip: attempt 1.
        let mut path = in_direct();
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 5_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );

        // Up again from 5_000 and closed at 66_000: 61 s of use resets the budget before this close
        // counts, so it is retry 1 rather than retry 2.
        punch_again(&mut path, 5_000);
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 66_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );
        assert_eq!(path.state(), PathState::Rendezvous);
    }

    #[test]
    fn a_path_up_for_less_than_the_stable_window_keeps_counting() {
        let mut path = in_direct();
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 4_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );

        punch_again(&mut path, 4_000);
        // 59_999 ms of use is short of the 60 s window, so this is the second consecutive close.
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 63_999),
            vec![PathAction::Retry {
                after_ms: 2_000,
                attempt: 2,
            }]
        );
    }

    #[test]
    fn the_stable_window_boundary_is_inclusive() {
        let cases = [(STABLE_WINDOW_MS - 1, 2u8), (STABLE_WINDOW_MS, 1)];

        for (up_for_ms, expected_attempt) in cases {
            let mut path = in_direct();
            assert_eq!(
                path.apply(PathEvent::DirectClosed, 4_000),
                vec![PathAction::Retry {
                    after_ms: 1_000,
                    attempt: 1,
                }]
            );
            punch_again(&mut path, 4_000);
            assert_eq!(
                path.apply(PathEvent::DirectClosed, 4_000 + up_for_ms),
                vec![PathAction::Retry {
                    after_ms: backoff(expected_attempt),
                    attempt: expected_attempt,
                }],
                "up for {up_for_ms} ms"
            );
        }
    }

    #[test]
    fn a_rendezvous_timeout_does_not_make_the_relay_stable() {
        let mut path = in_direct();
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 5_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );

        // The retry never finds a peer: the rendezvous times out into the relay. No path was ever
        // usable, so a later relay close is not evidence of stability and must not reset the count.
        assert_eq!(
            path.apply(PathEvent::Start, 10_000),
            vec![PathAction::RequestRendezvous]
        );
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 10_500 }, 10_500),
            vec![PathAction::UseRelay {
                reason: RelayReason::RendezvousTimeout
            }]
        );
        assert_eq!(path.state(), PathState::Relayed);

        assert_eq!(
            path.apply(PathEvent::RelayClosed, 80_000),
            vec![PathAction::Retry {
                after_ms: 2_000,
                attempt: 2,
            }]
        );
    }

    #[test]
    fn a_relay_after_a_failed_punch_can_be_stable() {
        let mut path = in_relayed();
        assert_eq!(
            path.apply(PathEvent::RelayClosed, 4_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );

        // Back through the rendezvous and a punch that times out: the relay is usable from the
        // moment the fallback is ordered, so 61 s of it resets the budget.
        assert_eq!(
            path.apply(PathEvent::Start, 4_000),
            vec![PathAction::RequestRendezvous]
        );
        assert_eq!(
            path.apply(PathEvent::Candidates { fresh: true }, 4_100),
            vec![PathAction::StartPunch { deadline_ms: 4_600 }]
        );
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 4_600 }, 4_600),
            vec![PathAction::UseRelay {
                reason: RelayReason::PunchTimeout
            }]
        );
        assert_eq!(
            path.apply(PathEvent::RelayClosed, 65_600),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );
        assert_eq!(path.state(), PathState::Rendezvous);
    }

    #[test]
    fn a_retry_runs_no_timer_until_the_rendezvous_is_restarted() {
        let mut path = in_direct();
        assert_eq!(
            path.apply(PathEvent::DirectClosed, 10_000),
            vec![PathAction::Retry {
                after_ms: 1_000,
                attempt: 1,
            }]
        );
        assert_eq!(path.state(), PathState::Rendezvous);

        // The backoff is the caller's wait: a tick during it must not burn the rendezvous budget.
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 99_000 }, 99_000),
            Vec::new()
        );
        assert_eq!(path.state(), PathState::Rendezvous);

        assert_eq!(
            path.apply(PathEvent::Start, 100_000),
            vec![PathAction::RequestRendezvous]
        );
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 100_499 }, 100_499),
            Vec::new()
        );
        assert_eq!(
            path.apply(PathEvent::Tick { now_ms: 100_500 }, 100_500),
            vec![PathAction::UseRelay {
                reason: RelayReason::RendezvousTimeout
            }]
        );
        assert_eq!(path.state(), PathState::Relayed);
    }

    #[test]
    fn a_closed_path_does_not_resurrect() {
        let mut path = in_closed();
        assert_eq!(path.state(), PathState::Closed);

        let events = [
            PathEvent::Start,
            PathEvent::Candidates { fresh: true },
            PathEvent::Candidates { fresh: false },
            PathEvent::PunchOpen,
            PathEvent::PunchFailed,
            PathEvent::DirectClosed,
            PathEvent::RelayClosed,
            PathEvent::Tick { now_ms: u64::MAX },
        ];

        for event in events {
            assert_eq!(
                path.apply(event, u64::MAX),
                Vec::new(),
                "{event:?} on a closed path"
            );
            assert_eq!(path.state(), PathState::Closed, "{event:?}");
        }
    }

    #[test]
    fn a_tick_before_the_deadline_changes_nothing() {
        let cases = [(Place::Rendezvous, 1_499u64), (Place::Punching, 2_499)];

        for (place, now_ms) in cases {
            let mut path = at(place);
            assert_eq!(
                path.apply(PathEvent::Tick { now_ms }, now_ms),
                Vec::new(),
                "{place:?} ticked early"
            );
            assert_eq!(path.state(), place.state(), "{place:?} ticked early");
        }
    }

    #[test]
    fn a_settled_path_ignores_ticks() {
        for place in [Place::Direct, Place::Relayed] {
            let mut path = at(place);
            assert_eq!(
                path.apply(PathEvent::Tick { now_ms: u64::MAX }, u64::MAX),
                Vec::new(),
                "{place:?} must not silently fall back"
            );
            assert_eq!(path.state(), place.state(), "{place:?}");
        }
    }

    #[test]
    fn events_outside_their_state_change_nothing() {
        let cases = [
            (Place::Idle, PathEvent::Candidates { fresh: true }),
            (Place::Idle, PathEvent::Candidates { fresh: false }),
            (Place::Idle, PathEvent::PunchOpen),
            (Place::Idle, PathEvent::PunchFailed),
            (Place::Idle, PathEvent::DirectClosed),
            (Place::Idle, PathEvent::RelayClosed),
            (Place::Rendezvous, PathEvent::PunchOpen),
            (Place::Rendezvous, PathEvent::PunchFailed),
            (Place::Rendezvous, PathEvent::DirectClosed),
            (Place::Rendezvous, PathEvent::RelayClosed),
            (Place::Punching, PathEvent::Start),
            (Place::Punching, PathEvent::Candidates { fresh: true }),
            (Place::Punching, PathEvent::Candidates { fresh: false }),
            (Place::Punching, PathEvent::DirectClosed),
            (Place::Punching, PathEvent::RelayClosed),
            (Place::Direct, PathEvent::Start),
            (Place::Direct, PathEvent::Candidates { fresh: true }),
            (Place::Direct, PathEvent::Candidates { fresh: false }),
            (Place::Direct, PathEvent::PunchOpen),
            (Place::Direct, PathEvent::PunchFailed),
            (Place::Direct, PathEvent::RelayClosed),
            (Place::Relayed, PathEvent::Start),
            (Place::Relayed, PathEvent::Candidates { fresh: true }),
            (Place::Relayed, PathEvent::Candidates { fresh: false }),
            (Place::Relayed, PathEvent::PunchOpen),
            (Place::Relayed, PathEvent::PunchFailed),
            (Place::Relayed, PathEvent::DirectClosed),
        ];

        for (place, event) in cases {
            let mut path = at(place);
            assert_eq!(
                path.apply(event, 90_000),
                Vec::new(),
                "{place:?} must ignore {event:?}"
            );
            assert_eq!(
                path.state(),
                place.state(),
                "{place:?} must stay put after {event:?}"
            );
        }
    }

    #[test]
    fn backoff_doubles_from_one_second_to_the_cap() {
        let cases = [
            (0u8, 1_000u64),
            (1, 1_000),
            (2, 2_000),
            (3, 4_000),
            (4, 8_000),
            (5, 16_000),
            (6, 32_000),
            (7, 60_000),
            (8, 60_000),
            (u8::MAX, 60_000),
        ];

        for (attempt, expected) in cases {
            assert_eq!(backoff(attempt), expected, "attempt {attempt}");
        }
    }

    #[test]
    fn backoff_never_decreases() {
        let mut previous = 0;
        for attempt in 0..=u8::MAX {
            let delay = backoff(attempt);
            assert!(
                delay >= previous,
                "attempt {attempt}: {delay} ms is below the previous {previous} ms"
            );
            assert!(
                delay <= 60_000,
                "attempt {attempt}: {delay} ms is above the cap"
            );
            previous = delay;
        }
    }

    #[test]
    fn the_machine_takes_its_clock_as_an_argument() {
        const SOURCE: &str = include_str!("mesh.rs");
        for forbidden in [concat!("In", "stant"), concat!("System", "Time")] {
            assert!(
                !SOURCE.contains(forbidden),
                "the path state machine takes now_ms; it must not read {forbidden} from the clock"
            );
        }
    }
}
