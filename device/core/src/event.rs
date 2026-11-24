//! The record's row classes (design §4, decision §3.5): what a row is, and whose words it carries.
//!
//! The device's record is one stream of rows — a session opening, a goal changing, a transaction
//! beginning or ending, a call starting, finishing or being refused. [`RowKind`] is the closed set
//! of what a row can be, and [`RowKind::of_row`] is the one total function that classifies one. The
//! renderers (the device's pages and the console) classify by this value and never parse prose or
//! arguments for meaning: a row the function cannot place is [`RowKind::Unknown`], rendered
//! neutrally and counted, never dropped.
//!
//! There is still one stream. A `txn.begin` call row carries the purpose, a `txn.commit` row the
//! state, a `terminal.goal` row the sentence or the clear — a second event stream would draw the
//! same fact twice (decision §3.5). The kinds here are how those rows are read, not a second
//! recording.
//!
//! [`Family`] answers the other half of the question: a row whose payload is the caller's own words
//! is a *claim* (drawn with its author), and a row the device observed is a *fact* (drawn as the
//! spine). The distinction is the round's third plane (decision §3.4) and it is a value, not a
//! rendering convention.

/// What one row of the record is.
///
/// A closed set: a row that fits none of these is [`RowKind::Unknown`], which is a member rather
/// than an error, so an unrecognised row stays visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowKind {
    /// A session was opened (the shipped `session.opened`, and the amendment's `session.created`).
    SessionOpened,
    /// A session was closed.
    SessionClosed,
    /// A goal sentence was written, by the holder the row names.
    GoalSet,
    /// The goal was cleared: the session has none now.
    GoalCleared,
    /// A transaction began; its purpose is the row's claim.
    TxnStarted,
    /// The transaction's author declared it committed.
    TxnCommitted,
    /// The transaction's author declared the attempt not achieved, with a reason.
    TxnAborted,
    /// A tool call: started, or finished.
    Call,
    /// A call refused before it ran; the reason is the row's detail.
    CallDenied,
    /// A row this build cannot classify. Visible and counted, never dropped.
    Unknown,
}

/// Whose words a row carries: the caller's claim, or the device's observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// The row's payload is what a caller said — a goal, a purpose, a declared end, a reason.
    Claim,
    /// The device observed the act: a session opened, a call ran, a call was refused.
    Fact,
}

impl RowKind {
    /// Whose words this class of row carries.
    ///
    /// The match is exhaustive with no wildcard, so a new class cannot compile without a family.
    pub fn family(self) -> Family {
        match self {
            // The caller's own words: the goal sentence, the purpose, the declared end and reason.
            RowKind::GoalSet
            | RowKind::TxnStarted
            | RowKind::TxnCommitted
            | RowKind::TxnAborted => Family::Claim,
            // What the device saw happen. `GoalCleared` is a fact even though it is about a claim:
            // it records that the sentence is gone, and carries no caller text of its own.
            RowKind::SessionOpened
            | RowKind::SessionClosed
            | RowKind::GoalCleared
            | RowKind::Call
            | RowKind::CallDenied
            | RowKind::Unknown => Family::Fact,
        }
    }

    /// Classify one row of the record.
    ///
    /// `tool` is the tool the row names (`""` when it names none), `action` is the `action`
    /// argument of a `terminal.goal` call, and `event` is the row's own kind string as the device
    /// wrote it (`call.started`, `session.opened`, `begin`, …). The function is total: every input
    /// has an answer, and the answer is [`RowKind::Unknown`] rather than a guess.
    ///
    /// A refusal wins over everything: a denied call is never rendered as the act it named, so a
    /// refused `terminal.goal` is a refusal and not a goal that was set.
    ///
    /// A tool row is a call, `terminal.create`'s included: the session fact itself arrives as the
    /// device's own `session.opened` row, which names no tool.
    pub fn of_row(tool: &str, action: Option<&str>, event: &str) -> RowKind {
        if event == "call.denied" {
            return RowKind::CallDenied;
        }

        if !tool.is_empty() {
            return match tool {
                // The goal's two facts share one tool; the `action` is what tells them apart, and a
                // row with no action is left unknown rather than drawn as "goal set" with no
                // sentence.
                "terminal.goal" => match action {
                    Some("set") => RowKind::GoalSet,
                    Some("clear") => RowKind::GoalCleared,
                    _ => RowKind::Unknown,
                },
                "txn.begin" => RowKind::TxnStarted,
                "txn.commit" => RowKind::TxnCommitted,
                "txn.abort" => RowKind::TxnAborted,
                // A tool this build declares is a call. One it does not is left unknown: the row is
                // still visible, and its meaning is not invented (decision §3.5).
                name if crate::registry::tool(name).is_some() => RowKind::Call,
                _ => RowKind::Unknown,
            };
        }

        match event {
            // The amendment names the typed kind `session.created`; the shipped stream publishes
            // `session.opened`. They are one fact, so they are one class.
            "session.opened" | "session.created" => RowKind::SessionOpened,
            "session.closed" => RowKind::SessionClosed,
            "call.started" | "call.finished" => RowKind::Call,
            _ => RowKind::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_set_and_goal_cleared_are_told_apart() {
        // The same tool writes two different facts. The `action` argument is what tells them apart;
        // a cleared goal must not render as a set one with no sentence.
        assert_eq!(
            RowKind::of_row("terminal.goal", Some("set"), "begin"),
            RowKind::GoalSet
        );
        assert_eq!(
            RowKind::of_row("terminal.goal", Some("clear"), "begin"),
            RowKind::GoalCleared
        );
        // Without an action the row cannot be classified, and is not guessed.
        assert_eq!(
            RowKind::of_row("terminal.goal", None, "begin"),
            RowKind::Unknown
        );
        assert_eq!(
            RowKind::of_row("terminal.goal", Some("replace"), "begin"),
            RowKind::Unknown
        );
    }

    /// The index of a class. The match has **no wildcard**: a `RowKind` variant added without a
    /// place here fails to compile, and a place added without a row in the table below fails the
    /// coverage assertion — so a new class cannot be folded into a family silently.
    fn class_index(kind: RowKind) -> usize {
        match kind {
            RowKind::SessionOpened => 0,
            RowKind::SessionClosed => 1,
            RowKind::GoalSet => 2,
            RowKind::GoalCleared => 3,
            RowKind::TxnStarted => 4,
            RowKind::TxnCommitted => 5,
            RowKind::TxnAborted => 6,
            RowKind::Call => 7,
            RowKind::CallDenied => 8,
            RowKind::Unknown => 9,
        }
    }

    /// Every class, in index order. The table below must name every entry exactly once, and
    /// `class_index` must agree with this order; the two guards together are what makes a new
    /// variant loud instead of silently unclassified.
    const CLASSES: [RowKind; 10] = [
        RowKind::SessionOpened,
        RowKind::SessionClosed,
        RowKind::GoalSet,
        RowKind::GoalCleared,
        RowKind::TxnStarted,
        RowKind::TxnCommitted,
        RowKind::TxnAborted,
        RowKind::Call,
        RowKind::CallDenied,
        RowKind::Unknown,
    ];

    #[test]
    fn every_class_is_a_family() {
        // Every class of the closed set, one row that produces it, and whose words it carries: a
        // claim is the caller's own sentence or declared end, a fact is what the device observed.
        // The table is written out rather than counted from itself, and `CLASSES` is asserted
        // against it below.
        let rows: [(&str, Option<&str>, &str, RowKind, Family); 10] = [
            (
                "",
                None,
                "session.opened",
                RowKind::SessionOpened,
                Family::Fact,
            ),
            (
                "",
                None,
                "session.closed",
                RowKind::SessionClosed,
                Family::Fact,
            ),
            (
                "terminal.goal",
                Some("set"),
                "begin",
                RowKind::GoalSet,
                Family::Claim,
            ),
            (
                "terminal.goal",
                Some("clear"),
                "begin",
                RowKind::GoalCleared,
                Family::Fact,
            ),
            (
                "txn.begin",
                None,
                "begin",
                RowKind::TxnStarted,
                Family::Claim,
            ),
            (
                "txn.commit",
                None,
                "begin",
                RowKind::TxnCommitted,
                Family::Claim,
            ),
            (
                "txn.abort",
                None,
                "begin",
                RowKind::TxnAborted,
                Family::Claim,
            ),
            (
                "files.read",
                None,
                "call.finished",
                RowKind::Call,
                Family::Fact,
            ),
            (
                "files.read",
                None,
                "call.denied",
                RowKind::CallDenied,
                Family::Fact,
            ),
            ("", None, "device.started", RowKind::Unknown, Family::Fact),
        ];

        for (tool, action, event, kind, family) in rows {
            assert_eq!(
                RowKind::of_row(tool, action, event),
                kind,
                "{tool:?}/{action:?}/{event:?}"
            );
            assert_eq!(kind.family(), family, "{kind:?}");
        }

        // The table names every class exactly once: the indices it covers are all of them, and in
        // the order `class_index` and `CLASSES` agree on.
        for (index, class) in CLASSES.iter().enumerate() {
            assert_eq!(
                class_index(*class),
                index,
                "{class:?} is out of order in CLASSES vs class_index"
            );
        }
        let mut covered: Vec<usize> = rows.iter().map(|row| class_index(row.3)).collect();
        covered.sort_unstable();
        assert_eq!(
            covered,
            (0..CLASSES.len()).collect::<Vec<usize>>(),
            "the table names every class of RowKind exactly once"
        );

        // The amendment's typed spelling is the shipped fact, so it is the same class.
        assert_eq!(
            RowKind::of_row("", None, "session.created"),
            RowKind::SessionOpened
        );
    }

    #[test]
    fn an_unknown_row_is_visible() {
        // An unknown row is a row: it renders neutrally and is counted, never dropped. A tool this
        // build does not declare is unknown here, not presented as a call whose meaning we would
        // have to invent.
        assert_eq!(
            RowKind::of_row("", None, "device.started"),
            RowKind::Unknown
        );
        assert_eq!(
            RowKind::of_row("future.tool", None, "begin"),
            RowKind::Unknown
        );
        assert_eq!(RowKind::of_row("", None, ""), RowKind::Unknown);
        assert_eq!(RowKind::Unknown.family(), Family::Fact);
    }
}
