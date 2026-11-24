//! The device's transaction registry (decision §3.3): the declared units of work, in memory.
//!
//! A transaction is a **record**, not an atomic operation. The registry owns the ids the device
//! mints, the two scope slots (one working session-bound transaction per session, one working
//! device-scope transaction per caller), the device-wide backstop and the retained endings. The
//! value rules — the text bounds, the author rule, the scope rule — live in `summrise_core::txn`;
//! this module is the collection those rules are applied to, plus the observation counters the
//! device writes as calls come and go.
//!
//! **The device-scope slot is keyed by the [`Holder`].** The direct `/mcp` door is `install`, and
//! every direct client arrives as the same holder, so `install` has **no slot**: a direct client
//! always names its transaction explicitly and may hold several. The console, a token, a grant, a
//! mesh peer and the panel each have a slot, because their ids differ; two tokens are two slots.
//!
//! Explicit transactions end only on their author's declaration. The panel's implicit units have
//! a private participant reservation: the last completed panel request declares their ending. A
//! caller that never ends an explicit record leaves it "open — no end recorded".
//! Ended records are kept newest-first up to [`MAX_RETAINED_TXNS`]; the audit is the durable
//! history, so an evicted receipt is still readable there.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use summrise_core::{
    may_begin, Goal, Holder, HolderKind, SessionRef, Txn, TxnError, TxnId, TxnState, MAX_OPEN_TXNS,
    MAX_RETAINED_TXNS,
};

use crate::capabilities::terminal::{PtyRegistry, SessionId};

/// An owned presence-and-goal snapshot from one terminal sessions lock. The outer absence means
/// the session is gone; `Some(None)` means the registered session has no goal. Keeping the two
/// facts together prevents a concurrent close from being mistaken for a clear.
pub(crate) fn registered_goal(terminal: &PtyRegistry, id: &SessionId) -> Option<Option<Goal>> {
    let snapshot = terminal
        .list()
        .into_iter()
        .find(|session| &session.id == id)
        .map(|session| session.goal);
    #[cfg(test)]
    after_goal_presence_check();
    snapshot
}

/// Endings keep the start snapshot when the session is gone; a present session may report a clear.
pub(crate) fn goal_now(terminal: &PtyRegistry, txn: &Txn) -> Option<Goal> {
    let Some(id) = txn
        .session()
        .and_then(|session| SessionId::parse(session.as_str()))
    else {
        return txn.goal_at_start().cloned();
    };
    registered_goal(terminal, &id).unwrap_or_else(|| txn.goal_at_start().cloned())
}

#[cfg(test)]
thread_local! {
    pub(crate) static AFTER_GOAL_PRESENCE_CHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn after_goal_presence_check() {
    AFTER_GOAL_PRESENCE_CHECK.with(|hook| {
        let pause = hook.borrow_mut().take();
        if let Some(pause) = pause {
            pause();
        }
    });
}

/// One transaction's receipt: the declaration the caller made, beside what the device observed.
///
/// `goal_at_end` is `None` when the session's goal did not change, so the receipt does not restate
/// it; `Some(None)` is an explicit clear (JSON `null`), because "cleared" is a change the receipt
/// must name. There is no declared-outcome field: `state` is `Committed` or `Aborted`, and a
/// failure is the device's `observed.failures`, not a word the caller asks us to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub(crate) txn: TxnId,
    pub(crate) session: Option<SessionRef>,
    pub(crate) purpose: String,
    pub(crate) by: Holder,
    pub(crate) state: TxnState,
    pub(crate) reason: Option<String>,
    pub(crate) started_at_ms: i64,
    pub(crate) ended_at_ms: i64,
    pub(crate) calls: u64,
    pub(crate) failures: u64,
    pub(crate) refusals: u64,
    pub(crate) duration_ms: u64,
    pub(crate) goal_at_start: Option<Goal>,
    pub(crate) goal_at_end: Option<Option<Goal>>,
}

impl Receipt {
    fn of(txn: &Txn, goal_at_end: Option<Option<Goal>>) -> Receipt {
        let ended_at_ms = txn.ended_at_ms().unwrap_or(txn.started_at_ms());
        Receipt {
            txn: txn.id().clone(),
            session: txn.session().cloned(),
            purpose: txn.purpose().to_owned(),
            by: txn.by().clone(),
            state: txn.state(),
            reason: txn.reason().map(str::to_owned),
            started_at_ms: txn.started_at_ms(),
            ended_at_ms,
            calls: txn.calls(),
            failures: txn.failures(),
            refusals: txn.refusals(),
            duration_ms: ended_at_ms.saturating_sub(txn.started_at_ms()) as u64,
            goal_at_start: txn.goal_at_start().cloned(),
            goal_at_end,
        }
    }

    /// The receipt as the wire answers it. The field names are the contract's, and `goal_at_end`
    /// is omitted unless the goal changed (a clear is an explicit `null`).
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let mut body = serde_json::json!({
            "txn": self.txn.as_str(),
            "session": self.session.as_ref().map(SessionRef::as_str),
            "purpose": self.purpose,
            "by": crate::capabilities::terminal::holder_json(&self.by),
            "state": state_name(self.state),
            "reason": self.reason,
            "started_at_ms": self.started_at_ms,
            "ended_at_ms": self.ended_at_ms,
            "observed": {
                "calls": self.calls,
                "failures": self.failures,
                "refusals": self.refusals,
                "duration_ms": self.duration_ms,
            },
            "goal_at_start": self.goal_at_start.as_ref().map(crate::capabilities::terminal::goal_json),
        });
        if let Some(goal) = &self.goal_at_end {
            body["goal_at_end"] = goal
                .as_ref()
                .map(crate::capabilities::terminal::goal_json)
                .unwrap_or(serde_json::Value::Null);
        }
        body
    }
}

/// This boot's short nonce for transaction ids: six lowercase hex characters.
///
/// Minted once, at process start, from the OS random source; the registry never calls a clock.
pub(crate) fn boot_nonce() -> String {
    use rand_core::{OsRng, RngCore};

    let mut bytes = [0u8; 3];
    OsRng.fill_bytes(&mut bytes);
    format!("{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2])
}

/// The wire spelling of an ending: `committed` or `aborted` (`working` is never a receipt).
pub(crate) fn state_name(state: TxnState) -> &'static str {
    match state {
        TxnState::Working => "working",
        TxnState::Committed => "committed",
        TxnState::Aborted => "aborted",
    }
}

/// The records and their two slots. One lock for both, so a begin can never occupy a slot whose
/// record another thread is ending.
#[derive(Default)]
struct Inner {
    txns: HashMap<TxnId, Txn>,
    /// Ended ids, oldest first: the retention window's order.
    ended: VecDeque<TxnId>,
    /// The working session-bound transaction of each session.
    sessions: HashMap<SessionRef, TxnId>,
    /// The working device-scope transaction of each holder that has a slot (never `install`).
    devices: HashMap<Holder, TxnId>,
    /// Only implicit panel units have a tag; explicit records, even by panel, never do.
    panel: HashMap<TxnId, PanelParticipants>,
}

#[derive(Default)]
struct PanelParticipants {
    active: usize,
    reason: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PanelAdmissionError {
    SessionMissing,
    Txn(TxnError),
}

/// One reserved request, whose HTTP guard releases it exactly once.
#[derive(Debug)]
pub(crate) struct PanelTxn {
    pub(crate) id: TxnId,
    pub(crate) owned: bool,
}

/// A begin can fail admission or the caller's synchronous goal operation. Keeping that operation's
/// error generic leaves the registry independent of the capability's transport/error vocabulary.
#[derive(Debug)]
pub(crate) enum BeginError<E> {
    Txn(TxnError),
    Goal(E),
}

/// The device's transactions, one registry per process.
///
/// **AN ID IS SCOPED TO ONE BOOT.** The counter restarts with the process, so a bare `txn-1` would
/// name a different transaction after every restart; a row still inside the audit's bounded tail
/// would join the new process's record and print a receipt beside work it never described. The id
/// is `txn-<boot>-<n>`, where `boot` is a short nonce passed in once at process start (never read
/// from a clock here), so an id from a previous boot can never equal one this process mints.
pub(crate) struct TxnRegistry {
    boot: String,
    inner: Mutex<Inner>,
    next_id: AtomicU64,
}

impl TxnRegistry {
    pub(crate) fn new(boot: impl Into<String>) -> TxnRegistry {
        TxnRegistry {
            boot: boot.into(),
            inner: Mutex::new(Inner::default()),
            next_id: AtomicU64::new(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// An advisory admission check; it reserves nothing after this call returns.
    ///
    /// Callers that write a session goal must use [`Self::begin_with_goal`] for the final check and
    /// write. This preflight alone cannot protect a goal from a concurrent slot/capacity refusal.
    pub(crate) fn check_begin(
        &self,
        by: &Holder,
        session: Option<&SessionRef>,
    ) -> Result<(), TxnError> {
        self.lock().check(by, session)
    }

    /// Begin a transaction, or say why its slot is taken or its text is not one line.
    ///
    /// `goal_at_start` is the session's goal at this moment (after a begin that writes one), and
    /// `at_ms` is the device's clock. The id is minted here; a refused begin mints nothing.
    #[cfg(test)]
    pub(crate) fn begin(
        &self,
        by: Holder,
        purpose: &str,
        session: Option<SessionRef>,
        goal_at_start: Option<Goal>,
        at_ms: i64,
    ) -> Result<Txn, TxnError> {
        match self.begin_with_goal(by, purpose, session, at_ms, || {
            Ok::<_, std::convert::Infallible>(goal_at_start)
        }) {
            Ok(txn) => Ok(txn),
            Err(BeginError::Txn(error)) => Err(error),
            Err(BeginError::Goal(never)) => match never {},
        }
    }

    /// Admit, obtain the goal, and publish its record under one registry guard.
    ///
    /// `goal` may synchronously update/snapshot terminal metadata only after admission and purpose
    /// validation succeed. A failed goal operation must leave its metadata unchanged. It must not
    /// reenter this registry, await, publish events, or perform I/O: the nested lock order is this
    /// registry → terminal sessions, whose private guard never calls back into this registry.
    /// No id/slot is reserved until all fallible operations have succeeded, so refusal needs no
    /// rollback and cannot overwrite a goal another legitimate writer published in the meantime.
    pub(crate) fn begin_with_goal<E>(
        &self,
        by: Holder,
        purpose: &str,
        session: Option<SessionRef>,
        at_ms: i64,
        goal: impl FnOnce() -> Result<Option<Goal>, E>,
    ) -> Result<Txn, BeginError<E>> {
        let mut inner = self.lock();
        self.begin_locked(&mut inner, by, purpose, session, at_ms, goal)
    }

    fn begin_locked<E>(
        &self,
        inner: &mut Inner,
        by: Holder,
        purpose: &str,
        session: Option<SessionRef>,
        at_ms: i64,
        goal: impl FnOnce() -> Result<Option<Goal>, E>,
    ) -> Result<Txn, BeginError<E>> {
        inner
            .check(&by, session.as_ref())
            .map_err(BeginError::Txn)?;
        // The value type owns validation. Probe before the goal operation or id mint, so no later
        // text refusal can leave a goal behind or consume an id.
        Txn::open(TxnId::new("txn-probe"), purpose, by.clone(), None, 0, None)
            .map_err(BeginError::Txn)?;
        let goal_at_start = goal().map_err(BeginError::Goal)?;
        let id = TxnId::new(format!(
            "txn-{}-{}",
            self.boot,
            self.next_id.fetch_add(1, Ordering::Relaxed)
        ));
        let txn = Txn::open(
            id.clone(),
            purpose,
            by.clone(),
            session.clone(),
            at_ms,
            goal_at_start,
        )
        .expect("the same purpose was validated before the goal operation");
        inner.txns.insert(id.clone(), txn.clone());
        match session {
            Some(session) => {
                inner.sessions.insert(session, id);
            }
            None => {
                // `install` is the direct `/mcp` door, and every direct client is the same holder:
                // a slot would make the second client's begin `txn_already_open`. It is absent, so
                // a direct client always names its transaction explicitly.
                if by.kind != HolderKind::Install {
                    inner.devices.insert(by, id);
                }
            }
        }
        Ok(txn)
    }

    /// Resolve or create a panel unit and reserve its participant in the same scope lock.
    /// Only owned terminal metadata is read here; no guard crosses audit or capability awaits.
    pub(crate) fn admit_panel(
        &self,
        terminal: &PtyRegistry,
        session: &SessionId,
        at_ms: i64,
    ) -> Result<PanelTxn, PanelAdmissionError> {
        let mut inner = self.lock();
        let goal = registered_goal(terminal, session).ok_or(PanelAdmissionError::SessionMissing)?;
        let scope = SessionRef::new(session.as_str());
        if let Some(id) = inner.sessions.get(&scope).cloned() {
            if inner.txns.get(&id).is_some_and(Txn::is_working) {
                let owned = if let Some(participants) = inner.panel.get_mut(&id) {
                    participants.active += 1;
                    true
                } else {
                    false
                };
                return Ok(PanelTxn { id, owned });
            }
        }
        let opened = match self.begin_locked(
            &mut inner,
            Holder::panel(),
            &format!("your panel on {}", session.as_str()),
            Some(scope),
            at_ms,
            || Ok::<_, std::convert::Infallible>(goal),
        ) {
            Ok(opened) => opened,
            Err(BeginError::Txn(error)) => return Err(PanelAdmissionError::Txn(error)),
            Err(BeginError::Goal(never)) => match never {},
        };
        let id = opened.id().clone();
        inner.panel.insert(
            id.clone(),
            PanelParticipants {
                active: 1,
                reason: None,
            },
        );
        Ok(PanelTxn { id, owned: true })
    }

    /// Release one implicit participant. Zero participants and the ending are one atomic move.
    /// An explicit owner's successful end clears the tag immediately; later releases are no-ops.
    pub(crate) fn finish_panel(
        &self,
        terminal: &PtyRegistry,
        id: &TxnId,
        owned: bool,
        reason: Option<&'static str>,
        at_ms: i64,
    ) -> Option<Receipt> {
        if !owned {
            return None;
        }
        let mut inner = self.lock();
        let participants = inner.panel.get_mut(id)?;
        debug_assert!(
            participants.active > 0,
            "a reserved participant is consumed once"
        );
        participants.active -= 1;
        participants.reason = participants.reason.or(reason);
        if participants.active != 0 {
            return None;
        }
        let reason = participants.reason;
        // `goal_now` uses the already-locked record and the terminal's owned snapshot. It never
        // reenters this registry. The established nesting remains registry -> sessions -> output.
        let goal = goal_now(
            terminal,
            inner.txns.get(id).expect("a tagged unit owns its record"),
        );
        let state = if reason.is_some() {
            TxnState::Aborted
        } else {
            TxnState::Committed
        };
        Some(
            inner
                .end_record(id, &Holder::panel(), state, reason, goal, at_ms)
                .expect(
                    "a working auto unit ends by its own author with a bounded internal reason",
                ),
        )
    }

    /// The working session-bound transaction of `session`, if it has one.
    pub(crate) fn working_on(&self, session: &SessionRef) -> Option<TxnId> {
        let inner = self.lock();
        inner
            .sessions
            .get(session)
            .filter(|id| inner.txns.get(*id).is_some_and(Txn::is_working))
            .cloned()
    }

    /// The working device-scope transaction of `by`, if it has one (`install` never does).
    pub(crate) fn working_device_scope(&self, by: &Holder) -> Option<TxnId> {
        if by.kind == HolderKind::Install {
            return None;
        }
        let inner = self.lock();
        inner
            .devices
            .get(by)
            .filter(|id| inner.txns.get(*id).is_some_and(Txn::is_working))
            .cloned()
    }

    /// One transaction by id, ended ones included (the retention window answers a receipt).
    pub(crate) fn get(&self, id: &TxnId) -> Option<Txn> {
        self.lock().txns.get(id).cloned()
    }

    /// End a transaction as committed or aborted, and answer its receipt.
    ///
    /// The author rule, the state and the reason are checked before anything changes, so a refused
    /// ending leaves the record exactly as it was. The slot frees, the ending joins the retention
    /// window, and the oldest retired record is evicted past [`MAX_RETAINED_TXNS`].
    pub(crate) fn end(
        &self,
        id: &TxnId,
        by: &Holder,
        state: TxnState,
        reason: Option<&str>,
        goal_now: Option<Goal>,
        at_ms: i64,
    ) -> Result<Receipt, TxnError> {
        self.lock()
            .end_record(id, by, state, reason, goal_now, at_ms)
    }

    /// Count one admitted call under `id`. A record that ended cannot gain observations.
    pub(crate) fn record_call(&self, id: &TxnId) {
        self.observe(id, |txn| txn.record_call());
    }

    /// Count one admitted call whose dispatch returned an error.
    pub(crate) fn record_failure(&self, id: &TxnId) {
        self.observe(id, |txn| txn.record_failure());
    }

    /// Count one call denied before dispatch with `id` resolved.
    pub(crate) fn record_refusal(&self, id: &TxnId) {
        self.observe(id, |txn| txn.record_refusal());
    }

    fn observe(&self, id: &TxnId, count: impl FnOnce(&mut Txn)) {
        let mut inner = self.lock();
        if let Some(txn) = inner.txns.get_mut(id) {
            if txn.is_working() {
                count(txn);
            }
        }
    }

    /// How many transactions are working, for the tests and the backstop's own assertion.
    #[cfg(test)]
    pub(crate) fn working_count(&self) -> usize {
        self.lock()
            .txns
            .values()
            .filter(|txn| txn.is_working())
            .count()
    }

    /// How many ended transactions are retained, for the eviction test.
    #[cfg(test)]
    pub(crate) fn retained_count(&self) -> usize {
        self.lock().ended.len()
    }
}

impl Inner {
    fn end_record(
        &mut self,
        id: &TxnId,
        by: &Holder,
        state: TxnState,
        reason: Option<&str>,
        goal_now: Option<Goal>,
        at_ms: i64,
    ) -> Result<Receipt, TxnError> {
        debug_assert!(state.is_ended(), "an ending is committed or aborted");
        let Some(existing) = self.txns.get(id) else {
            return Err(TxnError::Unknown);
        };
        // The receipt's "goal at end" is only present when the goal changed. `Some(None)` is a
        // clear: the field is present and null, which is the claim that it is gone.
        let changed = goal_now.as_ref() != existing.goal_at_start();
        let goal_at_end = changed.then_some(goal_now);
        {
            let txn = self
                .txns
                .get_mut(id)
                .expect("the record was just read under the same lock");
            match state {
                TxnState::Committed => txn.commit(by, at_ms)?,
                TxnState::Aborted => txn.abort(by, reason, at_ms)?,
                TxnState::Working => unreachable!("an ending is committed or aborted"),
            }
        }
        let ended = self
            .txns
            .get(id)
            .cloned()
            .expect("the ending kept the record");
        self.sessions.retain(|_, open| open != id);
        self.devices.retain(|_, open| open != id);
        // A refused end reaches none of these mutations, including the private origin tag.
        self.panel.remove(id);
        self.ended.push_back(id.clone());
        while self.ended.len() > MAX_RETAINED_TXNS {
            if let Some(oldest) = self.ended.pop_front() {
                self.txns.remove(&oldest);
            }
        }
        Ok(Receipt::of(&ended, goal_at_end))
    }

    /// The slot and backstop rules, decided before an id is minted or a goal is written.
    fn check(&self, by: &Holder, session: Option<&SessionRef>) -> Result<(), TxnError> {
        // **A CLIENT'S WORK IS SESSION-BOUND.** A token, a grant or a mesh peer may not open a
        // device-scope transaction: the machine's own surfaces (the console's operator, the panel
        // and the install door) are the ones that act on the machine without a session. This is the
        // one point both `begin` and `check_begin` pass through, and no other call mints a record.
        if session.is_none() && by.kind.is_client_credential() {
            return Err(TxnError::SessionRequired);
        }
        let slot = match session {
            Some(session) => self.sessions.get(session).and_then(|id| self.txns.get(id)),
            None if by.kind == HolderKind::Install => None,
            None => self.devices.get(by).and_then(|id| self.txns.get(id)),
        };
        may_begin(slot)?;
        if self.txns.values().filter(|txn| txn.is_working()).count() >= MAX_OPEN_TXNS {
            return Err(TxnError::TooManyOpen);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use summrise_core::txn::{MAX_OPEN_TXNS, MAX_RETAINED_TXNS};

    fn holder(kind: HolderKind, token_id: Option<&str>) -> Holder {
        Holder {
            kind,
            token_id: token_id.map(str::to_owned),
        }
    }

    fn session(id: &str) -> SessionRef {
        SessionRef::new(id)
    }

    fn begin(
        registry: &TxnRegistry,
        by: Holder,
        session: Option<SessionRef>,
    ) -> Result<Txn, TxnError> {
        registry.begin(by, "a unit of work", session, None, 10)
    }

    #[test]
    fn an_invalid_purpose_never_runs_the_goal_operation_or_mints_an_id() {
        let registry = TxnRegistry::new("test");
        let refused = registry.begin_with_goal(
            Holder::install(),
            "two\nlines",
            None,
            1,
            || -> Result<Option<Goal>, ()> {
                panic!("an invalid purpose must not reach the goal operation")
            },
        );
        assert!(matches!(
            refused,
            Err(BeginError::Txn(TxnError::TooManyText))
        ));
        assert_eq!(registry.working_count(), 0);
        let accepted =
            begin(&registry, Holder::install(), None).expect("the refusal reserved no slot");
        assert_eq!(
            accepted.id().as_str(),
            "txn-test-1",
            "the refusal minted no id"
        );
    }

    #[test]
    fn a_goal_failure_leaves_admission_available_without_minting_an_id() {
        let registry = TxnRegistry::new("test");
        let by = holder(HolderKind::Token, Some("srm_a"));
        let on = session("session-1");
        let refused =
            registry.begin_with_goal(by.clone(), "valid work", Some(on.clone()), 1, || {
                Err::<Option<Goal>, _>("goal refused")
            });
        assert!(matches!(refused, Err(BeginError::Goal("goal refused"))));
        assert_eq!(registry.working_count(), 0);
        assert_eq!(registry.working_on(&on), None);
        let accepted = begin(&registry, by, Some(on)).expect("the failed goal reserved no slot");
        assert_eq!(
            accepted.id().as_str(),
            "txn-test-1",
            "the goal failure minted no id"
        );
    }

    #[test]
    fn a_client_cannot_begin_without_a_session_but_the_machine_surfaces_can() {
        // A token, a grant and a mesh peer are clients: their work is session-bound, so a begin
        // naming no session is refused and mints nothing. The console's operator, the panel and the
        // install door are the machine's own surfaces and keep device-scope transactions.
        let registry = TxnRegistry::new("test");
        for (kind, token_id) in [
            (HolderKind::Token, Some("srm_a")),
            (HolderKind::Grant, None),
            (HolderKind::Mesh, None),
        ] {
            assert_eq!(
                begin(&registry, holder(kind, token_id), None),
                Err(TxnError::SessionRequired),
                "{kind:?} may not begin outside a session"
            );
            assert_eq!(
                registry.working_count(),
                0,
                "{kind:?}: a refused begin mints nothing"
            );
        }
        for (kind, token_id) in [
            (HolderKind::Install, None),
            (HolderKind::Panel, None),
            (HolderKind::Console, Some("operator")),
        ] {
            let txn = begin(&registry, holder(kind, token_id), None)
                .unwrap_or_else(|error| panic!("{kind:?} keeps device-scope begins: {error}"));
            assert!(
                txn.is_device_scope(),
                "{kind:?} begins a transaction on the machine"
            );
        }
    }

    #[test]
    fn two_direct_clients_each_begin_and_use_their_own_device_scope_transaction() {
        // Every direct `/mcp` client arrives as `install`, and `install` has no slot, so the second
        // client's begin is not `txn_already_open`: each names its own record. The slot rule that
        // protects a console or a token is not imposed on the door that has no credential id.
        let registry = TxnRegistry::new("test");
        let install = holder(HolderKind::Install, None);
        let first = begin(&registry, install.clone(), None).expect("the first client begins");
        let second = begin(&registry, install.clone(), None).expect("the second client begins");

        assert_ne!(first.id(), second.id(), "two begins are two records");
        assert_eq!(
            registry.working_device_scope(&install),
            None,
            "install has no device-scope slot to inherit from"
        );
        assert!(first.is_device_scope() && second.is_device_scope());

        registry.record_call(first.id());
        registry.record_call(second.id());
        registry.record_call(second.id());
        assert_eq!(
            (
                registry.get(first.id()).map(|txn| txn.calls()),
                registry.get(second.id()).map(|txn| txn.calls())
            ),
            (Some(1), Some(2)),
            "each record observes its own calls"
        );

        let receipt = registry
            .end(first.id(), &install, TxnState::Committed, None, None, 20)
            .expect("its author may end it");
        assert_eq!((receipt.calls, receipt.state), (1, TxnState::Committed));
        assert_eq!(
            registry.get(second.id()).map(|txn| txn.state()),
            Some(TxnState::Working),
            "ending one record ends only that record"
        );
    }

    #[test]
    fn the_console_holds_one_device_scope_transaction_across_calls() {
        let registry = TxnRegistry::new("test");
        let console = holder(HolderKind::Console, Some("operator"));
        let first = begin(&registry, console.clone(), None).expect("the console begins one");
        assert_eq!(
            registry.working_device_scope(&console).as_ref(),
            Some(first.id()),
            "the console's working device-scope transaction occupies its slot"
        );
        assert_eq!(
            begin(&registry, console.clone(), None),
            Err(TxnError::AlreadyOpen),
            "a second begin is refused, never a replacement"
        );

        registry.record_call(first.id());
        let receipt = registry
            .end(first.id(), &console, TxnState::Committed, None, None, 30)
            .expect("its author ends it");
        assert_eq!(receipt.calls, 1, "the console's calls are on its receipt");
        assert_eq!(
            registry.working_device_scope(&console),
            None,
            "ending frees the slot"
        );
        let second = begin(&registry, console.clone(), None).expect("a new unit may begin");
        assert_ne!(first.id(), second.id());
    }

    #[test]
    fn a_second_begin_on_the_same_session_is_refused() {
        let registry = TxnRegistry::new("test");
        let by = holder(HolderKind::Token, Some("srm_a"));
        begin(&registry, by.clone(), Some(session("session-1"))).expect("the first begin");
        assert_eq!(
            begin(&registry, by.clone(), Some(session("session-1"))),
            Err(TxnError::AlreadyOpen)
        );
        // A different session is a different slot: the rule is per session, not per caller.
        begin(&registry, by.clone(), Some(session("session-2")))
            .expect("another session may begin");
    }

    #[test]
    fn ending_frees_only_the_slot_it_occupied() {
        let registry = TxnRegistry::new("test");
        let by = holder(HolderKind::Grant, None);
        let first = begin(&registry, by.clone(), Some(session("session-1"))).expect("a begin");
        let second = begin(&registry, by.clone(), Some(session("session-2"))).expect("a begin");
        registry
            .end(
                first.id(),
                &by,
                TxnState::Aborted,
                Some("not this one"),
                None,
                20,
            )
            .expect("its author ends it");
        assert_eq!(registry.working_on(&session("session-1")), None);
        assert_eq!(
            registry.working_on(&session("session-2")).as_ref(),
            Some(second.id()),
            "the other session's record is untouched"
        );
    }

    #[test]
    fn an_id_from_a_previous_boot_never_joins_a_new_transaction() {
        // The tail a restarted device still holds: a group from the previous boot, whose counter
        // also started at one.
        let tail = [
            serde_json::json!({
                "event": "begin", "id": 1, "tool": "txn.begin", "txn": "txn-1",
                "args": "purpose=before the restart",
            }),
            serde_json::json!({ "event": "end", "id": 1, "txn": "txn-1", "outcome": "started" }),
        ];
        let old: Vec<&str> = tail.iter().filter_map(|row| row["txn"].as_str()).collect();
        assert_eq!(old, vec!["txn-1", "txn-1"], "the old tail is one group");

        let registry = TxnRegistry::new("newb00t");
        let new = registry
            .begin(
                holder(HolderKind::Install, None),
                "after the restart",
                None,
                None,
                10,
            )
            .expect("a begin")
            .id()
            .as_str()
            .to_owned();

        assert_eq!(new, "txn-newb00t-1", "the boot scopes the id");
        assert!(
            !old.contains(&new.as_str()),
            "the new record cannot equal a row the old boot left behind"
        );
        let mut groups: Vec<&str> = old.clone();
        groups.push(new.as_str());
        groups.sort_unstable();
        groups.dedup();
        assert_eq!(
            groups.len(),
            2,
            "the old tail and the new record are two groups, not one merged group: {groups:?}"
        );
    }

    #[test]
    fn the_twenty_first_open_transaction_is_refused_with_synthetic_refs() {
        // Twenty is the chosen device-wide backstop, and it is a cap on WORKING records: twenty
        // may be open, so it is the twenty-first begin that is refused. The holders are synthetic
        // because this test is about the collection's bound, not any credential's slot. The install
        // door begins them because it has no device-scope slot to fill: a client's begins are
        // session-bound (round 27), and each of those would need its own live session.
        let registry = TxnRegistry::new("test");
        for index in 0..MAX_OPEN_TXNS {
            begin(
                &registry,
                holder(HolderKind::Install, None),
                Some(session(&format!("session-{index}"))),
            )
            .unwrap_or_else(|error| panic!("begin {index} is admitted: {error}"));
        }
        assert_eq!(registry.working_count(), MAX_OPEN_TXNS);
        assert_eq!(
            begin(
                &registry,
                holder(HolderKind::Install, None),
                Some(session("session-one-too-many"))
            ),
            Err(TxnError::TooManyOpen),
            "a twenty-first working transaction is the backstop's refusal"
        );
        assert_eq!(
            registry.working_count(),
            MAX_OPEN_TXNS,
            "nothing was admitted"
        );
    }

    #[test]
    fn the_65th_ended_transaction_evicts_the_oldest() {
        let registry = TxnRegistry::new("test");
        let by = holder(HolderKind::Install, None);
        let mut ids = Vec::new();
        for index in 0..=MAX_RETAINED_TXNS {
            let txn = begin(
                &registry,
                by.clone(),
                Some(session(&format!("session-{index}"))),
            )
            .expect("a synthetic session begins");
            ids.push(txn.id().clone());
            registry
                .end(txn.id(), &by, TxnState::Committed, None, None, 20)
                .expect("its author ends it");
        }
        assert_eq!(
            registry.retained_count(),
            MAX_RETAINED_TXNS,
            "the retention window is exactly the newest {MAX_RETAINED_TXNS}"
        );
        assert_eq!(
            registry.get(&ids[0]),
            None,
            "the oldest ended record was evicted"
        );
        assert!(
            registry.get(&ids[1]).is_some(),
            "the second-oldest is the window's oldest retained record"
        );
        assert!(registry.get(&ids[ids.len() - 1]).is_some());
    }

    #[test]
    fn the_receipt_puts_the_declaration_beside_the_observation() {
        let registry = TxnRegistry::new("test");
        let by = holder(HolderKind::Token, Some("srm_a"));
        let txn = registry
            .begin(
                by.clone(),
                "two writes and a denial",
                Some(session("session-1")),
                None,
                100,
            )
            .expect("a purpose is one line");
        registry.record_call(txn.id());
        registry.record_call(txn.id());
        registry.record_failure(txn.id());
        registry.record_refusal(txn.id());

        let receipt = registry
            .end(txn.id(), &by, TxnState::Committed, None, None, 1_350)
            .expect("its author ends it");
        assert_eq!(receipt.purpose, "two writes and a denial");
        assert_eq!(receipt.by, by);
        assert_eq!(receipt.state, TxnState::Committed);
        assert_eq!(
            (receipt.calls, receipt.failures, receipt.refusals),
            (2, 1, 1)
        );
        assert_eq!(receipt.duration_ms, 1_250);
        assert_eq!(
            receipt.goal_at_end, None,
            "an unchanged goal is not restated"
        );
        let json = receipt.to_json();
        assert_eq!(json["purpose"], "two writes and a denial");
        assert_eq!(json["observed"]["duration_ms"], 1_250);
        assert!(json.get("goal_at_end").is_none());
        assert_eq!(json["state"], "committed");
        assert_eq!(json["session"], "session-1");
        assert_eq!(json["by"]["kind"], "token");
    }

    #[test]
    fn an_ended_record_observes_nothing_more() {
        let registry = TxnRegistry::new("test");
        let by = holder(HolderKind::Install, None);
        let txn = begin(&registry, by.clone(), None).expect("a begin");
        registry
            .end(txn.id(), &by, TxnState::Committed, None, None, 20)
            .expect("its author ends it");
        registry.record_call(txn.id());
        registry.record_failure(txn.id());
        registry.record_refusal(txn.id());
        let ended = registry.get(txn.id()).expect("retained");
        assert_eq!(
            (ended.calls(), ended.failures(), ended.refusals()),
            (0, 0, 0),
            "a receipt is not rewritten after it is issued"
        );
    }

    #[test]
    fn an_ending_by_another_holder_leaves_the_record_working() {
        let registry = TxnRegistry::new("test");
        let author = holder(HolderKind::Token, Some("srm_a"));
        let txn = begin(&registry, author.clone(), Some(session("session-a")))
            .expect("a begin inside a session");
        assert_eq!(
            registry.end(
                txn.id(),
                &holder(HolderKind::Token, Some("srm_b")),
                TxnState::Committed,
                None,
                None,
                20,
            ),
            Err(TxnError::NotAuthor)
        );
        assert_eq!(
            registry.get(txn.id()).map(|record| record.state()),
            Some(TxnState::Working)
        );
    }
}
