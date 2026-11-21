//! The declared unit of work (design §2.2): a transaction groups the calls of one intention.
//!
//! A transaction is a **record**, not an atomic operation. `commit` and `abort` are the caller's
//! declarations; they revert nothing. The state is exactly `Working | Committed | Aborted` — there
//! is no declared-outcome enum, because a caller that says "failed" would be declaring a fact it
//! does not own. What happened is the device's observation ([`Txn::calls`], [`Txn::failures`],
//! [`Txn::refusals`]), and the receipt puts that beside the declaration.
//!
//! **Scope.** A transaction is *session-bound* when it acts on a session (`Some(SessionRef)`) and
//! *device-scope* when it acts on the machine (`None`). The two never mix ([`Txn::admits`]): a
//! machine call may not ride a session's transaction, and a session call may not ride the machine's
//! ([`TxnError::ScopeMismatch`]). There is no cross-session transaction — a session-bound
//! transaction carries only its own session's calls ([`TxnError::SessionMismatch`]).
//!
//! **Bounds.** [`MAX_OPEN_TXNS`] is a *chosen* backstop, not a derived count: the per-session and
//! per-caller slots are what bound live work, and the device-wide cap keeps a flood of distinct
//! holders from growing the registry without limit. [`MAX_RETAINED_TXNS`] is what the device keeps
//! of the ended ones. The collections that count to those caps live with the registry that holds
//! the transactions; this module owns the values and the rules that answer them.

use std::fmt;

use crate::session::{Goal, Holder, HolderKind};

/// The most transactions that may be working at once, device-wide.
///
/// A **chosen** cap, not a derived count: one working session-bound transaction per session and one
/// working device-scope transaction per caller are the rules that bound live work, and this is the
/// backstop behind them — twenty is twice the sixteen sessions the device already allows, so the
/// slot rules refuse first in normal use and only a new class of holder meets this one.
pub const MAX_OPEN_TXNS: usize = 20;

/// The most ended transactions the device retains: the newest are kept, the oldest evicted.
///
/// Enough for a page's window to show a session's recent history without a store; the audit is
/// the durable history, and an evicted receipt is still readable there.
pub const MAX_RETAINED_TXNS: usize = 64;

/// The longest purpose a transaction may carry, in characters.
///
/// The same bound as a goal's sentence: a purpose is one line a human reads beside the work, so it
/// is bounded in `char`s (not bytes, which would shorten a Chinese purpose) and refused rather than
/// truncated — a truncated purpose is a different declaration.
pub const TXN_PURPOSE_MAX_CHARS: usize = 200;

/// The longest abort reason a transaction may carry, in characters.
pub const TXN_REASON_MAX_CHARS: usize = 200;

/// A session, as this crate sees it: the id, and nothing else.
///
/// A newtype of its own, because `summrise-core` must not import the device's `SessionId`: the rule
/// "a transaction belongs to one session" has to be expressible here, in values, and the device's
/// type lives in the crate that owns the PTY. Absence is `None` on [`Txn::session`], never an empty
/// reference, so the type never has to carry a null.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionRef(String);

impl SessionRef {
    /// The reference a device's session id names.
    pub fn new(id: impl Into<String>) -> SessionRef {
        SessionRef(id.into().trim().to_owned())
    }

    /// The id, as the device minted it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A transaction's id, as the device minted it (`txn-<n>`).
///
/// Its own type for the same reason as [`SessionRef`]: a string that is a transaction would
/// otherwise be indistinguishable from a string that is a session, and the gates' refusals name the
/// difference.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TxnId(String);

impl TxnId {
    /// The id the device minted for a transaction.
    pub fn new(id: impl Into<String>) -> TxnId {
        TxnId(id.into().trim().to_owned())
    }

    /// The id as it appears on the wire and in the audit.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TxnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The three states of a transaction — exactly three.
///
/// `Working` is in flight; `Committed` and `Aborted` are the two endings a caller may declare.
/// There is no `Done`/`Failed`/`Cancelled`: a failure is the device's observation, counted as
/// [`Txn::failures`], and an abort carries its reason instead of a word. A transaction left
/// `Working` is shown as "open — no end recorded" and is never closed by the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TxnState {
    /// In flight: calls may be tagged with it.
    Working,
    /// The caller declared the work done. Reverts nothing.
    Committed,
    /// The caller declared the attempt not achieved, with a reason. Reverts nothing.
    Aborted,
}

impl TxnState {
    /// Whether the transaction has ended (the two endings; `Working` has not).
    pub fn is_ended(self) -> bool {
        !matches!(self, TxnState::Working)
    }
}

/// Why a transaction move was refused.
///
/// Each variant is a refusal the device can hand to a client: the value layer decides it, the
/// device only renders the sentence. `Unknown` and `TooManyOpen` are decided by the registry that
/// holds the transactions; the rest are decided here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxnError {
    /// No transaction with that id is open on this device.
    Unknown,
    /// The transaction has already ended.
    Ended,
    /// The caller may not end this transaction: only its author or an Owner door may.
    NotAuthor,
    /// A transaction is already working in that slot; it is never replaced.
    AlreadyOpen,
    /// The transaction belongs to another session: there is no cross-session transaction.
    SessionMismatch,
    /// A session-bound call reached a device-scope transaction, or the reverse.
    ScopeMismatch,
    /// The device-wide backstop is reached.
    TooManyOpen,
    /// A purpose or reason is not one line within its cap.
    TooManyText,
}

impl fmt::Display for TxnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxnError::Unknown => write!(f, "no transaction with that id is open on this device"),
            TxnError::Ended => write!(f, "that transaction has ended; begin a new one"),
            TxnError::NotAuthor => write!(
                f,
                "only the transaction's author (or an Owner door) may end it"
            ),
            TxnError::AlreadyOpen => write!(
                f,
                "a transaction is already open for that scope; commit or abort it first"
            ),
            TxnError::SessionMismatch => write!(f, "that transaction belongs to another session"),
            TxnError::ScopeMismatch => write!(
                f,
                "a session-bound call needs a session-bound transaction, and a machine call needs \
                 a device-scope one"
            ),
            TxnError::TooManyOpen => {
                write!(
                    f,
                    "too many transactions are open on this device; end one first"
                )
            }
            TxnError::TooManyText => write!(
                f,
                "a purpose is one line of at most {TXN_PURPOSE_MAX_CHARS} characters and a reason \
                 one line of at most {TXN_REASON_MAX_CHARS}"
            ),
        }
    }
}

impl std::error::Error for TxnError {}

/// One transaction: what the caller declared, and what the device observed under it.
///
/// The fields are private and the only writers are [`Txn::open`], the three `record_*` counters,
/// [`Txn::commit`] and [`Txn::abort`]. Nothing is *assignable*: the state moves only through an
/// ending, the reason is set only by an abort, and the counters only count what the device observed.
/// That is the contract's "a claim is never rewritten" enforced by the type rather than by
/// convention — a caller that wants a different purpose begins a different transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Txn {
    id: TxnId,
    purpose: String,
    by: Holder,
    session: Option<SessionRef>,
    state: TxnState,
    reason: Option<String>,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    goal_at_start: Option<Goal>,
    calls: u64,
    failures: u64,
    refusals: u64,
}

impl Txn {
    /// Begin a transaction, or say why its purpose cannot be one.
    pub fn open(
        id: TxnId,
        purpose: &str,
        by: Holder,
        session: Option<SessionRef>,
        at_ms: i64,
        goal_at_start: Option<Goal>,
    ) -> Result<Txn, TxnError> {
        Ok(Txn {
            id,
            purpose: bounded_text(purpose, TXN_PURPOSE_MAX_CHARS)?.to_owned(),
            by,
            session,
            state: TxnState::Working,
            reason: None,
            started_at_ms: at_ms,
            ended_at_ms: None,
            goal_at_start,
            calls: 0,
            failures: 0,
            refusals: 0,
        })
    }

    /// The id the device minted.
    pub fn id(&self) -> &TxnId {
        &self.id
    }

    /// Why this series of operations exists — the caller's sentence, trimmed and bounded.
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// Who declared it. Only this holder (or an Owner door) may end it.
    pub fn by(&self) -> &Holder {
        &self.by
    }

    /// `Some` when the transaction acts on a session; `None` when it acts on the machine.
    pub fn session(&self) -> Option<&SessionRef> {
        self.session.as_ref()
    }

    /// Where it is: `Working` until an author ends it.
    pub fn state(&self) -> TxnState {
        self.state
    }

    /// The abort reason, when one was given.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// When it began, on the device's clock.
    pub fn started_at_ms(&self) -> i64 {
        self.started_at_ms
    }

    /// When it ended, if it has.
    pub fn ended_at_ms(&self) -> Option<i64> {
        self.ended_at_ms
    }

    /// The session's goal when the transaction began, if the session had one: the umbrella the work
    /// ran under, snapshotted at the start rather than read at the end.
    pub fn goal_at_start(&self) -> Option<&Goal> {
        self.goal_at_start.as_ref()
    }

    /// Calls admitted under this transaction, as the device observed them.
    pub fn calls(&self) -> u64 {
        self.calls
    }

    /// Admitted calls whose dispatch returned an error.
    pub fn failures(&self) -> u64 {
        self.failures
    }

    /// Calls denied before dispatch with this transaction resolved.
    pub fn refusals(&self) -> u64 {
        self.refusals
    }

    /// Whether this transaction acts on the machine rather than on a session.
    pub fn is_device_scope(&self) -> bool {
        self.session.is_none()
    }

    /// Whether it is still open.
    pub fn is_working(&self) -> bool {
        self.state == TxnState::Working
    }

    /// Whether this transaction may carry a call that names `session`.
    ///
    /// The two scopes never mix, and a session-bound transaction carries only its own session's
    /// calls. `Ok(())` is the gate's permission to resolve this transaction for the call; the
    /// refusal names which rule said no, so the client can act on it.
    pub fn admits(&self, session: Option<&SessionRef>) -> Result<(), TxnError> {
        match (self.session.as_ref(), session) {
            (Some(own), Some(named)) if own == named => Ok(()),
            (Some(_), Some(_)) => Err(TxnError::SessionMismatch),
            (None, None) => Ok(()),
            _ => Err(TxnError::ScopeMismatch),
        }
    }

    /// Count one admitted call. The device observes; the caller does not report.
    pub fn record_call(&mut self) {
        self.calls += 1;
    }

    /// Count one admitted call whose dispatch returned an error.
    pub fn record_failure(&mut self) {
        self.failures += 1;
    }

    /// Count one call denied before dispatch, with this transaction resolved.
    pub fn record_refusal(&mut self) {
        self.refusals += 1;
    }

    /// End it as committed: the caller's declaration, not a fact about the work.
    pub fn commit(&mut self, by: &Holder, at_ms: i64) -> Result<(), TxnError> {
        self.end(by, TxnState::Committed, None, at_ms)
    }

    /// End it as aborted, with the reason it did not achieve its purpose. Reverts nothing.
    pub fn abort(&mut self, by: &Holder, reason: Option<&str>, at_ms: i64) -> Result<(), TxnError> {
        self.end(by, TxnState::Aborted, reason, at_ms)
    }

    /// The one ending path: the author rule, then the state, then the reason — each checked before
    /// anything is written, so a refused end leaves the record exactly as it was.
    fn end(
        &mut self,
        by: &Holder,
        state: TxnState,
        reason: Option<&str>,
        at_ms: i64,
    ) -> Result<(), TxnError> {
        debug_assert!(state.is_ended(), "a commit or abort is an ending");
        if self.state.is_ended() {
            return Err(TxnError::Ended);
        }
        if !may_end(self, by) {
            return Err(TxnError::NotAuthor);
        }
        let reason = match reason {
            Some(text) => Some(bounded_text(text, TXN_REASON_MAX_CHARS)?.to_owned()),
            None => None,
        };
        self.state = state;
        self.reason = reason;
        self.ended_at_ms = Some(at_ms);
        Ok(())
    }
}

/// Whether a begin may have its slot.
///
/// `slot` is the working transaction already occupying it: the named session's working transaction
/// for a session-bound begin, or the caller's own working device-scope transaction for a
/// device-scope one. One working transaction per slot, so a second begin is
/// [`TxnError::AlreadyOpen`] — it never replaces the open one, because that record's author and
/// counters are facts. A slot holding an ended record is not an occupant: the device moves the id
/// out of the slot when it ends, and this accepts the move if it is asked early.
pub fn may_begin(slot: Option<&Txn>) -> Result<(), TxnError> {
    match slot {
        Some(txn) if txn.is_working() => Err(TxnError::AlreadyOpen),
        _ => Ok(()),
    }
}

/// Only the transaction's author, or an Owner door (`install`, `panel`), may end it.
///
/// An Owner door may end anyone's transaction because it is the device's own operator (the direct
/// `/mcp` door is `install`); no other holder may, so a client token cannot close another client's
/// work, and the counters cannot be rewritten by someone who did not declare the transaction.
pub fn may_end(txn: &Txn, by: &Holder) -> bool {
    txn.by == *by || matches!(by.kind, HolderKind::Install | HolderKind::Panel)
}

/// The text a transaction may carry: trimmed, one line, 1..=`cap` characters.
///
/// [`TxnError::TooManyText`] is the only text refusal this vocabulary has (the error set is the
/// contract's), so an empty or multi-line purpose or reason is refused with it too: a purpose that
/// is not one line is not a smaller purpose, it is not a purpose. `txn.begin`'s declaration refuses
/// the same shapes as `arguments` before this runs; this is what the value itself guarantees.
fn bounded_text(text: &str, cap: usize) -> Result<&str, TxnError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > cap || text.chars().any(char::is_control) {
        return Err(TxnError::TooManyText);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Goal, Holder, HolderKind};

    fn holder(kind: HolderKind, token_id: Option<&str>) -> Holder {
        Holder {
            kind,
            token_id: token_id.map(str::to_owned),
        }
    }

    fn session(id: &str) -> SessionRef {
        SessionRef::new(id)
    }

    fn open(id: &str, by: Holder, on: Option<SessionRef>, at_ms: i64) -> Txn {
        Txn::open(TxnId::new(id), "verify the suite", by, on, at_ms, None)
            .expect("a purpose is one line")
    }

    #[test]
    fn two_direct_clients_each_begin_and_use_their_own_device_scope_transaction() {
        // Both direct clients arrive through the same door (`install`), so the holder alone cannot
        // tell them apart: each names its own transaction id, and the id is what every row and
        // every receipt carries. Two of them are two records, never one shared slot.
        let install = holder(HolderKind::Install, None);
        let mut a = open("txn-1", install.clone(), None, 10);
        let mut b = open("txn-2", install.clone(), None, 11);

        assert_eq!((a.id().as_str(), b.id().as_str()), ("txn-1", "txn-2"));
        for txn in [&a, &b] {
            assert_eq!(
                txn.state(),
                TxnState::Working,
                "a begin is open until it ends"
            );
            assert!(
                txn.is_device_scope(),
                "a begin with no session acts on the machine"
            );
        }

        a.record_call();
        a.record_call();
        b.record_call();
        assert_eq!(
            (a.calls(), b.calls()),
            (2, 1),
            "each transaction observes its own calls"
        );

        // Ending one ends only its own record: the other client's transaction is still working.
        a.commit(&install, 20).expect("its author may end it");
        assert_eq!(
            (a.id().as_str(), a.state(), a.calls()),
            ("txn-1", TxnState::Committed, 2),
            "the committed record is its own receipt"
        );
        assert_eq!((b.state(), b.calls()), (TxnState::Working, 1));
    }

    #[test]
    fn a_transaction_is_session_scoped_or_device_scope_and_never_both() {
        let install = holder(HolderKind::Install, None);
        let on_first = open("txn-1", install.clone(), Some(session("session-1")), 10);
        let on_second = open("txn-2", install.clone(), Some(session("session-2")), 11);
        let on_machine = open("txn-3", install, None, 12);

        assert_eq!(
            on_first.session().map(SessionRef::as_str),
            Some("session-1")
        );
        assert!(on_machine.is_device_scope());

        // A session-bound transaction carries its own session's calls...
        assert_eq!(on_first.admits(Some(&session("session-1"))), Ok(()));
        // ...and no other session's: the session is bigger than the transaction.
        assert_eq!(
            on_first.admits(Some(&session("session-2"))),
            Err(TxnError::SessionMismatch)
        );
        assert_eq!(
            on_first.admits(on_second.session()),
            Err(TxnError::SessionMismatch)
        );

        // The scopes never mix, in either direction.
        assert_eq!(on_first.admits(None), Err(TxnError::ScopeMismatch));
        assert_eq!(
            on_machine.admits(Some(&session("session-1"))),
            Err(TxnError::ScopeMismatch)
        );
        assert_eq!(on_machine.admits(None), Ok(()));
    }

    #[test]
    fn one_device_scope_transaction_per_caller() {
        let by = holder(HolderKind::Token, Some("srm_a"));
        let mut first = open("txn-1", by.clone(), None, 10);
        first.record_call();

        assert_eq!(may_begin(None), Ok(()), "an empty slot takes a begin");
        assert_eq!(
            may_begin(Some(&first)),
            Err(TxnError::AlreadyOpen),
            "the caller's working device-scope transaction occupies the slot"
        );

        // Never replaced: the open record's author and counters are facts.
        assert_eq!(first.state(), TxnState::Working);
        assert_eq!(first.calls(), 1);
        assert_eq!(*first.by(), by);

        // The slot frees when it ends, and an ended record is not an occupant.
        first.commit(&by, 20).expect("its author may end it");
        assert_eq!(may_begin(Some(&first)), Ok(()));
    }

    #[test]
    fn a_transaction_is_open_until_its_author_ends_it() {
        let by = holder(HolderKind::Console, Some("operator"));
        let mut txn = open("txn-1", by.clone(), Some(session("session-1")), 10);

        assert_eq!(
            (txn.state(), txn.ended_at_ms(), txn.reason()),
            (TxnState::Working, None, None)
        );
        assert!(may_end(&txn, &by), "the author ends it");
        assert!(
            may_end(&txn, &holder(HolderKind::Install, None)),
            "the install door is an Owner door"
        );
        assert!(
            may_end(&txn, &holder(HolderKind::Panel, None)),
            "the panel is an Owner door"
        );
        for other in [
            holder(HolderKind::Console, Some("operator-2")),
            holder(HolderKind::Token, Some("srm_a")),
            holder(HolderKind::Grant, None),
            holder(HolderKind::Mesh, None),
        ] {
            assert!(!may_end(&txn, &other), "{:?} may not end it", other.kind);
        }

        // A refused end leaves the record exactly as it was.
        assert_eq!(
            txn.commit(&holder(HolderKind::Token, Some("srm_a")), 20),
            Err(TxnError::NotAuthor)
        );
        assert_eq!(txn.state(), TxnState::Working);
        assert_eq!(txn.ended_at_ms(), None);

        assert_eq!(txn.commit(&by, 20), Ok(()));
        assert_eq!(
            (txn.state(), txn.ended_at_ms()),
            (TxnState::Committed, Some(20))
        );
        assert_eq!(txn.commit(&by, 21), Err(TxnError::Ended));
        assert_eq!(
            txn.abort(&by, Some("too late"), 22),
            Err(TxnError::Ended),
            "an ended transaction cannot end twice"
        );
        assert_eq!(
            (txn.state(), txn.ended_at_ms()),
            (TxnState::Committed, Some(20)),
            "a refused end rewrites nothing"
        );
    }

    #[test]
    fn the_bounds_are_values() {
        // The two caps are chosen, not derived: the backstop bounds live work device-wide, the
        // retention bounds the history the page can show. The live set is smaller than the
        // retention, or a full device could fill its history with its own working set.
        assert_eq!(MAX_OPEN_TXNS, 20, "the device-wide backstop is a chosen 20");
        assert_eq!(
            MAX_RETAINED_TXNS, 64,
            "the 64 newest ended transactions are kept"
        );
        assert_eq!(TXN_PURPOSE_MAX_CHARS, 200);
        assert_eq!(TXN_REASON_MAX_CHARS, 200);
        const {
            assert!(
                MAX_OPEN_TXNS < MAX_RETAINED_TXNS,
                "the working set is smaller than the retained history"
            );
        };

        // The caps are enforced at the value, not only declared.
        let by = holder(HolderKind::Install, None);
        let at_cap = "x".repeat(TXN_PURPOSE_MAX_CHARS);
        assert!(
            Txn::open(TxnId::new("txn-1"), &at_cap, by.clone(), None, 10, None).is_ok(),
            "{TXN_PURPOSE_MAX_CHARS} characters is a purpose"
        );
        let over = "x".repeat(TXN_PURPOSE_MAX_CHARS + 1);
        assert_eq!(
            Txn::open(TxnId::new("txn-1"), &over, by.clone(), None, 10, None),
            Err(TxnError::TooManyText)
        );
        for text in ["", "  ", "two\nlines"] {
            assert_eq!(
                Txn::open(TxnId::new("txn-1"), text, by.clone(), None, 10, None),
                Err(TxnError::TooManyText),
                "{text:?} is not one line of a purpose"
            );
        }

        let mut txn = open("txn-1", by.clone(), None, 10);
        let reason_over = "x".repeat(TXN_REASON_MAX_CHARS + 1);
        assert_eq!(
            txn.abort(&by, Some(&reason_over), 20),
            Err(TxnError::TooManyText)
        );
        assert_eq!(
            (txn.state(), txn.ended_at_ms()),
            (TxnState::Working, None),
            "a refused abort leaves the transaction working"
        );
    }

    #[test]
    fn an_aborted_transaction_keeps_its_calls() {
        let by = holder(HolderKind::Token, Some("srm_a"));
        let mut txn = open("txn-1", by.clone(), Some(session("session-1")), 10);
        txn.record_call();
        txn.record_call();
        txn.record_failure();
        txn.record_refusal();
        let observed = (txn.calls(), txn.failures(), txn.refusals());

        assert_eq!(txn.abort(&by, Some("the build was red"), 30), Ok(()));
        assert_eq!(txn.state(), TxnState::Aborted);
        assert_eq!(txn.reason(), Some("the build was red"));
        assert_eq!(txn.ended_at_ms(), Some(30));
        assert_eq!(
            (txn.calls(), txn.failures(), txn.refusals()),
            observed,
            "an abort reverts nothing: the observed calls stay the record"
        );
    }

    #[test]
    fn a_goal_is_snapshotted_at_the_start() {
        let author = holder(HolderKind::Console, Some("operator"));
        let goal = Goal::new("ship the spine", author, 5).expect("one sentence");
        let txn = Txn::open(
            TxnId::new("txn-1"),
            "verify the suite",
            holder(HolderKind::Token, Some("srm_a")),
            Some(session("session-1")),
            10,
            Some(goal.clone()),
        )
        .expect("a purpose is one line");
        assert_eq!(txn.goal_at_start(), Some(&goal));
    }
}
