//! The session state machine (design §4.3): five states, five legal moves, no way back — and who
//! holds the session and what it is for (design §2.1).
//!
//! The states are [`crate::state::SessionState`], re-exported here. The wire document and the state
//! machine must name the same states; a second enum would force a conversion on every session row
//! and give "is this session alive?" two answers.
//!
//! Beside the machine, and deliberately not part of it: a [`Holder`] (the door that created the
//! session — attribution, never authority) and an optional [`Goal`] (one attributed sentence saying
//! what the session is for). Neither moves the state machine, neither is persisted, and neither
//! gates anything.

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

/// The longest goal a session may carry, in characters.
///
/// A chosen bound, not a schema limit: long enough for one sentence and short enough that
/// `terminal.list` can answer every live session's goal in one frame. Counted in `char`s, not
/// bytes, so a sentence in Chinese is not made shorter than the same sentence in English.
pub const GOAL_MAX_CHARS: usize = 200;

/// The door a call arrived through: the only thing a holder says about who did it.
///
/// There is no person here and no rank. `Install` is the device's own door (the direct `/mcp` door
/// arrives as its owner), `Panel` is the device page, `Console` is the console's operator, `Token`
/// is a client token, `Grant` is a grant's console arrival and `Mesh` a peer device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HolderKind {
    /// The device's own door: the direct `/mcp` client and the operator's install token.
    Install,
    /// The device's local panel.
    Panel,
    /// The console's operator.
    Console,
    /// A client token.
    Token,
    /// A grant that arrived through the console.
    Grant,
    /// A peer device.
    Mesh,
}

/// Who holds a session or wrote a claim: a door and, for a token, which token.
///
/// Equality covers `token_id`, because "which credential" is the question the record answers; two
/// tokens at the same door are two holders. The holder is attribution only (decision §3.1): it
/// gates nothing, and no rule here asks it for permission. It is a hash key because the device's
/// device-scope transaction slot is keyed by it — two tokens are two slots, while every direct
/// client shares `install` (and therefore always names its transaction explicitly).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Holder {
    /// The door the call arrived through.
    pub kind: HolderKind,
    /// The credential's id, when the door has one: a client token's record id, or the console's own
    /// name for the operator. Never a person's name.
    pub token_id: Option<String>,
}

impl Holder {
    /// The device's own door: the direct `/mcp` client and the operator's install token.
    pub fn install() -> Holder {
        Holder {
            kind: HolderKind::Install,
            token_id: None,
        }
    }

    /// The device's local panel: the person standing at this machine, with no credential record
    /// beside the door.
    pub fn panel() -> Holder {
        Holder {
            kind: HolderKind::Panel,
            token_id: None,
        }
    }
}

/// Why a text cannot be a session's goal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalError {
    /// The text is empty after trimming: a goal is a sentence, not an absence.
    Empty,
    /// The text is longer than [`GOAL_MAX_CHARS`].
    TooLong {
        /// How many characters the caller sent, so the refusal can say by how much.
        chars: usize,
    },
    /// The text contains a control character: a goal is one line.
    Control {
        /// The first control character found.
        ch: char,
    },
}

impl fmt::Display for GoalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoalError::Empty => write!(f, "a goal is one sentence; this one is empty"),
            GoalError::TooLong { chars } => write!(
                f,
                "a goal is at most {GOAL_MAX_CHARS} characters; this one is {chars}"
            ),
            GoalError::Control { ch } => write!(
                f,
                "a goal is one line; this one contains the control character {ch:?}"
            ),
        }
    }
}

impl std::error::Error for GoalError {}

/// One session's goal: the sentence, its author, and when it was written.
///
/// A **claim**, never an instruction (decision §3.2). Both surfaces print its author, and a
/// client's goal is never presented as the operator's. [`Goal::new`] is the checked constructor: it
/// stores the trimmed form [`validate_goal`] returns, so a goal built through it is one line.
///
/// The fields are private and there is no setter: [`Goal::new`] is the only writer, so a goal cannot
/// be rewritten by assignment. A correction is a **new** goal with a new author and time, which is
/// what "a claim is never rewritten" means as a type rather than as a convention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Goal {
    text: String,
    by: Holder,
    at_ms: i64,
}

impl Goal {
    /// Build a goal from the text a caller sent, or say why it is not one.
    pub fn new(text: &str, by: Holder, at_ms: i64) -> Result<Goal, GoalError> {
        Ok(Goal {
            text: validate_goal(text)?.to_owned(),
            by,
            at_ms,
        })
    }

    /// The sentence, trimmed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Who wrote it.
    pub fn by(&self) -> &Holder {
        &self.by
    }

    /// When it was written, on the device's clock.
    pub fn at_ms(&self) -> i64 {
        self.at_ms
    }
}

/// The goal a caller meant, trimmed, or the reason it is not a goal.
///
/// A goal is **one bounded line**: 1..=[`GOAL_MAX_CHARS`] characters after trimming and no control
/// character. Trimming first means a client's leading or trailing whitespace is not a refusal (its
/// formatting is not its content); an embedded newline is one, because a second line is a second
/// sentence and a session carries exactly one.
pub fn validate_goal(text: &str) -> Result<&str, GoalError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(GoalError::Empty);
    }
    if let Some(ch) = text.chars().find(|ch| ch.is_control()) {
        return Err(GoalError::Control { ch });
    }
    let chars = text.chars().count();
    if chars > GOAL_MAX_CHARS {
        return Err(GoalError::TooLong { chars });
    }
    Ok(text)
}

/// Who may write a session's goal: the holder that created the session, or an Owner door.
///
/// The Owner doors are `install` (the direct `/mcp` door, which the device's owner uses) and
/// `panel` (the device's own page). Everyone else is refused `not_goal_writer`: a second client
/// token on the first one's session, a grant, a mesh peer — and the console's operator on a
/// session a client token created, whose own goals belong on a session the operator opened. This
/// decides who may write a claim; it gives nobody permission to do anything else (decision §3.1).
pub fn may_write_goal(creator: &Holder, by: &Holder) -> bool {
    creator == by || matches!(by.kind, HolderKind::Install | HolderKind::Panel)
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

    fn holder(kind: HolderKind, token_id: Option<&str>) -> Holder {
        Holder {
            kind,
            token_id: token_id.map(str::to_owned),
        }
    }

    #[test]
    fn a_holder_names_a_door_and_never_a_person() {
        // The kind names the door a call arrived through; `token_id` names the credential. A person
        // is never a holder, and two tokens at the same door are two different holders.
        assert_ne!(
            holder(HolderKind::Token, Some("srm_a")),
            holder(HolderKind::Token, Some("srm_b")),
            "equality includes token_id: two tokens are two holders"
        );
        assert_eq!(
            holder(HolderKind::Token, Some("srm_a")),
            holder(HolderKind::Token, Some("srm_a")),
            "the same door and the same credential is the same holder"
        );
        assert_eq!(
            holder(HolderKind::Install, None).token_id,
            None,
            "a door without a credential carries no id"
        );
        assert_eq!(
            Holder::install(),
            holder(HolderKind::Install, None),
            "the install door is the direct /mcp client's holder"
        );
        assert_eq!(
            Holder::panel(),
            holder(HolderKind::Panel, None),
            "the panel is a door with no credential record"
        );
        assert_ne!(
            holder(HolderKind::Console, Some("operator")),
            holder(HolderKind::Token, Some("operator")),
            "the door is part of the value, not decoration"
        );
        for kind in [
            HolderKind::Install,
            HolderKind::Panel,
            HolderKind::Console,
            HolderKind::Token,
            HolderKind::Grant,
            HolderKind::Mesh,
        ] {
            assert_eq!(
                holder(kind, None),
                Holder {
                    kind,
                    token_id: None
                },
                "{kind:?} is a door with no credential"
            );
        }
    }

    #[test]
    fn a_goal_is_one_trimmed_line() {
        assert_eq!(validate_goal("  ship the spine  "), Ok("ship the spine"));
        assert_eq!(validate_goal(""), Err(GoalError::Empty));
        assert_eq!(validate_goal("   "), Err(GoalError::Empty));
        assert_eq!(validate_goal("a\nb"), Err(GoalError::Control { ch: '\n' }));
        assert_eq!(
            validate_goal("a\u{7}b"),
            Err(GoalError::Control { ch: '\u{7}' })
        );

        let at_cap = "x".repeat(GOAL_MAX_CHARS);
        assert_eq!(
            validate_goal(&at_cap),
            Ok(at_cap.as_str()),
            "{GOAL_MAX_CHARS} characters is one sentence"
        );
        let over = "x".repeat(GOAL_MAX_CHARS + 1);
        assert_eq!(
            validate_goal(&over),
            Err(GoalError::TooLong {
                chars: GOAL_MAX_CHARS + 1
            }),
            "one character more than {GOAL_MAX_CHARS} is not a sentence a session carries"
        );

        // The checked constructor refuses a text that is not a goal.
        assert!(Goal::new("a\nb", holder(HolderKind::Install, None), 1).is_err());
        let goal = Goal::new("  ship it ", holder(HolderKind::Token, Some("srm_a")), 7)
            .expect("one sentence");
        assert_eq!(goal.text(), "ship it", "the goal is stored trimmed");
        assert_eq!(goal.at_ms(), 7);
        assert_eq!(
            goal.by(),
            &holder(HolderKind::Token, Some("srm_a")),
            "a goal carries its author"
        );
    }

    #[test]
    fn only_the_creator_or_an_owner_door_writes_the_goal() {
        let creator = holder(HolderKind::Token, Some("srm_a"));
        assert!(
            may_write_goal(&creator, &creator),
            "the creator writes its own goal"
        );
        assert!(
            may_write_goal(&creator, &holder(HolderKind::Install, None)),
            "the install door is an Owner door"
        );
        assert!(
            may_write_goal(&creator, &holder(HolderKind::Panel, None)),
            "the panel is an Owner door"
        );
        for other in [
            holder(HolderKind::Token, Some("srm_b")),
            holder(HolderKind::Console, Some("operator")),
            holder(HolderKind::Grant, None),
            holder(HolderKind::Mesh, None),
        ] {
            assert!(
                !may_write_goal(&creator, &other),
                "{:?} is neither the creator nor an Owner door",
                other.kind
            );
        }
        // A console session belongs to the operator; a client token does not inherit the goal.
        let console = holder(HolderKind::Console, Some("operator"));
        assert!(may_write_goal(&console, &console));
        assert!(!may_write_goal(&console, &creator));
    }
}
