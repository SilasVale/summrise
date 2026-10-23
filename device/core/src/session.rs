//! The session state machine (design §4.3): five states, five legal moves, no way back.
//!
//! The states are [`crate::state::SessionState`], re-exported here. The wire document and the state
//! machine must name the same states; a second enum would force a conversion on every session row
//! and give "is this session alive?" two answers.

use std::fmt;

pub use crate::state::SessionState;

/// Why a session move was refused.
///
/// The caller is holding a value the state machine cannot produce: either a move the design does not
/// contain, or an attempt to bring a finished session back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionError {
    /// The move is not an arrow in the design's diagram.
    Illegal {
        /// The state the session is in.
        from: SessionState,
        /// The state the caller asked for.
        to: SessionState,
    },
    /// A terminal session — [`SessionState::Reaped`] or [`SessionState::Closed`] — cannot move.
    Resurrection {
        /// The terminal state the session is in.
        from: SessionState,
        /// The state the caller asked for.
        to: SessionState,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionError::Illegal { from, to } => {
                write!(f, "a session cannot move from {from:?} to {to:?}")
            }
            TransitionError::Resurrection { from, to } => {
                write!(
                    f,
                    "a {from:?} session is finished; it cannot move to {to:?}"
                )
            }
        }
    }
}

impl std::error::Error for TransitionError {}

/// Apply one move of the session state machine.
///
/// ```text
/// Creating -> Attached -> Detached -> Reaped
///                 \-> Closed <--------/
/// ```
///
/// The rules the diagram encodes, and why each refusal exists:
///
/// - Reaping needs a detach first: a session with a live client must not vanish underneath it.
/// - `Closed` is reached from `Attached` or `Detached` — a session that was never attached was never
///   a session, and `Creating` has no arrow to any terminal state.
/// - `Reaped` and `Closed` are terminal. A move from either is a
///   [`TransitionError::Resurrection`], because a state file that says otherwise is a bug the
///   surface must not paper over.
/// - A same-state move is not a transition: nothing in the diagram produces it, so a caller that
///   asks has already misunderstood the current state.
pub fn transition(from: SessionState, to: SessionState) -> Result<SessionState, TransitionError> {
    let legal = matches!(
        (from, to),
        (SessionState::Creating, SessionState::Attached)
            | (SessionState::Attached, SessionState::Detached)
            | (SessionState::Detached, SessionState::Reaped)
            | (SessionState::Attached, SessionState::Closed)
            | (SessionState::Detached, SessionState::Closed)
    );
    if legal {
        return Ok(to);
    }

    if matches!(from, SessionState::Reaped | SessionState::Closed) {
        return Err(TransitionError::Resurrection { from, to });
    }
    Err(TransitionError::Illegal { from, to })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SessionState::{Attached, Closed, Creating, Detached, Reaped};

    const LEGAL: [(SessionState, SessionState); 5] = [
        (Creating, Attached),
        (Attached, Detached),
        (Detached, Reaped),
        (Attached, Closed),
        (Detached, Closed),
    ];

    const ALL: [SessionState; 5] = [Creating, Attached, Detached, Reaped, Closed];

    #[test]
    fn every_legal_move_returns_the_destination() {
        for (from, to) in LEGAL {
            assert_eq!(transition(from, to), Ok(to), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn every_move_outside_the_diagram_is_refused() {
        for from in ALL {
            for to in ALL {
                if LEGAL.contains(&(from, to)) {
                    continue;
                }
                assert!(
                    transition(from, to).is_err(),
                    "{from:?} -> {to:?} is not in the design's diagram and must be refused"
                );
            }
        }
    }

    #[test]
    fn the_terminal_states_do_not_resurrect() {
        for from in [Reaped, Closed] {
            for to in ALL {
                assert_eq!(
                    transition(from, to),
                    Err(TransitionError::Resurrection { from, to }),
                    "{from:?} -> {to:?} tries to bring a terminal session back"
                );
            }
        }
    }

    #[test]
    fn the_rest_of_the_illegal_moves_are_named_as_illegal() {
        let cases = [
            // Reaping is only legal after a detach: a live client must not have its session vanish.
            (Attached, Reaped),
            // Creation only becomes attached; a failed creation is not a closed session in this
            // state machine (it never existed as one).
            (Creating, Closed),
            (Creating, Reaped),
            // Same-state moves are not transitions; a caller that asks is confused about the state.
            (Creating, Creating),
            (Attached, Attached),
            (Detached, Detached),
        ];

        for (from, to) in cases {
            assert_eq!(
                transition(from, to),
                Err(TransitionError::Illegal { from, to }),
                "{from:?} -> {to:?}"
            );
        }
    }
}
