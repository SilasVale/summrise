//! `txn`: the declaration's implementation. The record itself lives in [`crate::txns`].
//!
//! The three tools are thin on purpose. `txn.begin` validates the purpose before it writes a
//! session's goal (a refused begin must not leave a claim behind), snapshots the goal the session
//! carries, and asks the registry for a record. `txn.commit` and `txn.abort` read the record, ask
//! the session for its goal at the end, and let the registry check the author and the state. Every
//! refusal is a `ToolError::Declined` with the kind the audit and the client both name.

use serde::Deserialize;
use serde_json::{json, Value};
use summrise_core::txn::{Txn, TxnError, TxnId, TxnState};
use summrise_core::{Goal, Holder, SessionRef};

use crate::capabilities::terminal::{goal_json, PtyError, PtyRegistry, SessionId};
use crate::capabilities::Ctx;
use crate::mcp::{parse, ToolError};
use crate::txns::{BeginError, TxnRegistry};

/// The tools this capability implements.
pub const NAMES: &[&str] = &["txn.begin", "txn.commit", "txn.abort"];

pub fn owns(tool: &str) -> bool {
    NAMES.contains(&tool)
}

#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    NAMES.iter().copied()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeginArgs {
    purpose: String,
    session: Option<String>,
    goal: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitArgs {
    txn: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AbortArgs {
    txn: String,
    reason: Option<String>,
}

pub fn call(
    tool: &str,
    arguments: serde_json::Map<String, Value>,
    ctx: &Ctx<'_>,
) -> Result<String, ToolError> {
    let by = &ctx.holder;
    match tool {
        "txn.begin" => begin(ctx.terminal, ctx.txns, by, parse(arguments, tool)?),
        "txn.commit" => end(
            ctx.terminal,
            ctx.txns,
            by,
            tool,
            parse(arguments, tool)?,
            TxnState::Committed,
        ),
        "txn.abort" => abort(ctx.terminal, ctx.txns, by, parse(arguments, tool)?),
        other => Err(ToolError::Internal(format!(
            "{other}: the txn capability does not declare this tool"
        ))),
    }
}

fn begin(
    terminal: &PtyRegistry,
    txns: &TxnRegistry,
    by: &Holder,
    arguments: BeginArgs,
) -> Result<String, ToolError> {
    if arguments.goal.is_some() && arguments.session.is_none() {
        return Err(ToolError::Arguments(
            "txn.begin: a goal belongs to a session; a device-scope begin has no session goal to \
             write"
                .to_owned(),
        ));
    }
    // The purpose is validated before anything is written: a begin that is refused for its text
    // must not leave a goal behind. `Txn::open` is the one validator; this probe record is dropped.
    Txn::open(
        TxnId::new("txn-probe"),
        &arguments.purpose,
        by.clone(),
        None,
        0,
        None,
    )
    .map_err(|error| text_error("txn.begin", error))?;

    let now = crate::http::now_ms();
    let session = arguments.session.as_deref().map(SessionRef::new);
    let id = match &session {
        Some(session) => {
            let id = live_session(terminal, session)?;
            // Keep the preflight's refusal order, but reserve nothing here: admission is checked
            // again under the guard that also obtains the goal and inserts the record.
            if let Err(error) = txns.check_begin(by, Some(session)) {
                return Err(begin_refusal(txns, by, Some(session), error));
            }
            #[cfg(test)]
            after_begin_check();
            Some(id)
        }
        None => {
            if let Err(error) = txns.check_begin(by, None) {
                return Err(begin_refusal(txns, by, None, error));
            }
            None
        }
    };

    let txn = txns
        .begin_with_goal(
            by.clone(),
            &arguments.purpose,
            session.clone(),
            now,
            || match (&id, arguments.goal.as_deref()) {
                (Some(id), Some(text)) => terminal.set_goal(id, Some(text), by, now),
                (Some(id), None) => crate::txns::registered_goal(terminal, id).ok_or_else(|| {
                    PtyError::UnknownSession {
                        id: id.as_str().to_owned(),
                    }
                }),
                (None, _) => Ok(None),
            },
        )
        .map_err(|error| match error {
            BeginError::Txn(error) => begin_refusal(txns, by, session.as_ref(), error),
            BeginError::Goal(error) => goal_error("txn.begin", error),
        })?;
    Ok(json!({
        "txn": txn.id().as_str(),
        "started_at_ms": txn.started_at_ms(),
        "goal_at_start": txn.goal_at_start().map(goal_json),
    })
    .to_string())
}

// A one-shot scheduling seam at the old check/write boundary; compiled out of shipped code.
#[cfg(test)]
thread_local! {
    static AFTER_BEGIN_CHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn after_begin_check() {
    AFTER_BEGIN_CHECK.with(|hook| {
        let pause = hook.borrow_mut().take();
        if let Some(pause) = pause {
            pause();
        }
    });
}

fn end(
    terminal: &PtyRegistry,
    txns: &TxnRegistry,
    by: &Holder,
    tool: &str,
    arguments: CommitArgs,
    state: TxnState,
) -> Result<String, ToolError> {
    let id = TxnId::new(&arguments.txn);
    let txn = working(txns, tool, &id)?;
    let receipt = txns
        .end(
            &id,
            by,
            state,
            None,
            goal_now(terminal, &txn),
            crate::http::now_ms(),
        )
        .map_err(|error| txn_error(tool, error))?;
    Ok(receipt.to_json().to_string())
}

fn abort(
    terminal: &PtyRegistry,
    txns: &TxnRegistry,
    by: &Holder,
    arguments: AbortArgs,
) -> Result<String, ToolError> {
    let tool = "txn.abort";
    let id = TxnId::new(&arguments.txn);
    let txn = working(txns, tool, &id)?;
    let receipt = txns
        .end(
            &id,
            by,
            TxnState::Aborted,
            arguments.reason.as_deref(),
            goal_now(terminal, &txn),
            crate::http::now_ms(),
        )
        .map_err(|error| txn_error(tool, error))?;
    Ok(receipt.to_json().to_string())
}

/// The record `txn.commit`/`txn.abort` names, or the typed refusal that says why there is none.
fn working(txns: &TxnRegistry, tool: &str, id: &TxnId) -> Result<Txn, ToolError> {
    let Some(txn) = txns.get(id) else {
        return Err(declined(
            tool,
            "txn_unknown",
            "no transaction with that id is open on this device",
        ));
    };
    if txn.state().is_ended() {
        return Err(declined(
            tool,
            "txn_ended",
            "that transaction has ended; begin a new one",
        ));
    }
    Ok(txn)
}

/// The session's goal now, so the receipt can say whether the umbrella changed under the work.
///
/// A session that is **gone** (closed after the transaction began) has no goal to read, and that is
/// not the same fact as "the goal was cleared while the session lived": reporting `null` there
/// would make a commit-after-close look like a clear. The snapshot the begin took is the honest
/// answer — the umbrella under which the work ran — so it is returned unchanged when the session
/// cannot answer.
fn goal_now(terminal: &PtyRegistry, txn: &Txn) -> Option<Goal> {
    crate::txns::goal_now(terminal, txn)
}

/// The live session a `txn.begin {session}` binds to, or the refusal that says it is not live.
fn live_session(terminal: &PtyRegistry, session: &SessionRef) -> Result<SessionId, ToolError> {
    let Some(id) = SessionId::parse(session.as_str()) else {
        return Err(declined(
            "txn.begin",
            "unknown_session",
            "that session id is not one this device mints",
        ));
    };
    if terminal.holder_of(&id).is_none() {
        return Err(declined(
            "txn.begin",
            "unknown_session",
            "that session is unknown or has been closed",
        ));
    }
    Ok(id)
}

/// A refusal with the kind the audit and the client both carry.
fn declined(tool: &str, kind: &'static str, sentence: &str) -> ToolError {
    ToolError::Declined {
        kind,
        message: format!("{tool}: {kind}: {sentence}"),
        data: None,
    }
}

/// The refusal for a taken begin slot: **it names the record that holds the slot.**
///
/// The holder is the caller itself — on the console channel, `console·operator` is the same holder
/// before and after a console restart — so without the id a restarted console can never adopt or
/// end the record it left behind. The data is a fact, not a claim: the device's own open txn and
/// its declared purpose. Every other begin refusal keeps its plain sentence.
fn begin_refusal(
    txns: &TxnRegistry,
    by: &Holder,
    session: Option<&SessionRef>,
    error: TxnError,
) -> ToolError {
    if error != TxnError::AlreadyOpen {
        return txn_error("txn.begin", error);
    }
    let open = match session {
        Some(session) => txns.working_on(session),
        None => txns.working_device_scope(by),
    }
    .and_then(|id| txns.get(&id));
    match open {
        Some(open) => ToolError::Declined {
            kind: "txn_already_open",
            message: format!("txn.begin: txn_already_open: {error}"),
            data: Some(json!({
                "txn": open.id().as_str(),
                "purpose": open.purpose(),
            })),
        },
        None => txn_error("txn.begin", error),
    }
}

/// A text that is not one purpose or reason is the caller's argument, not a device refusal.
fn text_error(tool: &str, error: TxnError) -> ToolError {
    match error {
        TxnError::TooManyText => ToolError::Arguments(format!("{tool}: {error}")),
        other => txn_error(tool, other),
    }
}

/// The registry's refusals, each with the kind the decision names.
fn txn_error(tool: &str, error: TxnError) -> ToolError {
    let kind = match error {
        TxnError::Unknown => "txn_unknown",
        TxnError::Ended => "txn_ended",
        TxnError::NotAuthor => "txn_not_author",
        TxnError::AlreadyOpen => "txn_already_open",
        TxnError::SessionRequired => "session_required",
        TxnError::SessionMismatch => "txn_session_mismatch",
        TxnError::ScopeMismatch => "txn_scope_mismatch",
        TxnError::TooManyOpen => "too_many_txns",
        TxnError::TooManyText => "arguments",
    };
    if kind == "arguments" {
        return ToolError::Arguments(format!("{tool}: {error}"));
    }
    declined(tool, kind, &error.to_string())
}

/// A goal-writing failure as the tool's answer: a text that is not one sentence is an argument,
/// the writer rule is a typed decline, and anything else is the capability's refusal.
fn goal_error(tool: &str, error: PtyError) -> ToolError {
    match error {
        PtyError::Goal(error) => ToolError::Arguments(format!("{tool}: {error}")),
        PtyError::NotGoalWriter { .. } => declined(tool, "not_goal_writer", &error.to_string()),
        PtyError::UnknownSession { .. } => declined(tool, "unknown_session", &error.to_string()),
        other => ToolError::Refused(format!("{tool}: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    mod atomic {
        use super::*;
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        use crate::capabilities::terminal::{CreateTarget, Limits};
        use crate::config::Release;
        use crate::store::Store;
        use crate::test_support::TempDir;
        use summrise_core::{HolderKind, MAX_OPEN_TXNS};

        struct Fixture {
            terminal: Arc<PtyRegistry>,
            txns: Arc<TxnRegistry>,
            by: Holder,
            _dir: TempDir,
        }

        impl Fixture {
            fn new(label: &str) -> Self {
                let dir = TempDir::new(label);
                let data = dir.join("data");
                std::fs::create_dir_all(&data).expect("the fixture data directory exists");
                let store = Store::new(dir.join("install"), data, Release::standalone());
                store
                    .save(&store.initial())
                    .expect("the fixture state saves");
                Self {
                    terminal: Arc::new(PtyRegistry::new(
                        store,
                        Arc::new(tokio::sync::Mutex::new(())),
                        Limits::default(),
                    )),
                    txns: Arc::new(TxnRegistry::new("atomic")),
                    by: Holder {
                        kind: HolderKind::Token,
                        token_id: Some("srm_atomic".to_owned()),
                    },
                    _dir: dir,
                }
            }

            async fn registered_session(&self, goal: &str) -> SessionId {
                // The short-lived child may already have exited. The real registry entry and its
                // goal remain until the fixture drops; no running process is needed by this test.
                self.terminal
                    .create(
                        self.by.clone(),
                        Some(goal),
                        CreateTarget::Pty {
                            shell: Some("/bin/true"),
                        },
                        24,
                        80,
                    )
                    .await
                    .expect("a local registered session is created")
            }

            fn begin(&self, id: &SessionId, goal: Option<&str>) -> Result<Value, ToolError> {
                begin(
                    &self.terminal,
                    &self.txns,
                    &self.by,
                    BeginArgs {
                        purpose: "accepted work".to_owned(),
                        session: Some(id.as_str().to_owned()),
                        goal: goal.map(str::to_owned),
                    },
                )
                .map(|text| serde_json::from_str(&text).expect("a begin answers JSON"))
            }

            fn contending_begin(
                &self,
                winner_session: &SessionId,
                loser_session: &SessionId,
            ) -> (Value, ToolError) {
                self.contending_begin_with(winner_session, loser_session, || {})
            }

            fn contending_begin_with(
                &self,
                winner_session: &SessionId,
                loser_session: &SessionId,
                after_winner: impl FnOnce(),
            ) -> (Value, ToolError) {
                std::thread::scope(|scope| {
                    let (checked, checked_rx) = mpsc::channel();
                    // On scheduler failure this sender drops before scope joins its worker, so a
                    // positive-check timeout or a failed winner cannot strand the paused loser.
                    let (release, release_rx) = mpsc::channel();
                    let loser = scope.spawn(|| {
                        AFTER_BEGIN_CHECK.with(|hook| {
                            *hook.borrow_mut() = Some(Box::new(move || {
                                checked.send(()).expect("the scheduler receives the check");
                                release_rx.recv().expect("the scheduler releases the loser");
                            }));
                        });
                        self.begin(loser_session, Some("rejected goal"))
                    });
                    checked_rx
                        .recv_timeout(Duration::from_secs(10))
                        .expect("the losing begin reached the old check/write boundary");
                    let winner = self.begin(winner_session, Some("accepted goal"));
                    if winner.is_ok() {
                        after_winner();
                    }
                    release.send(()).expect("the losing begin can continue");
                    let loser = loser.join().expect("the losing caller returns");
                    let winner = winner
                        .unwrap_or_else(|_| panic!("the competing begin takes the available slot"));
                    let refused = loser.expect_err("the competing begin must be refused");
                    (winner, refused)
                })
            }
        }

        fn assert_declined(error: ToolError, expected_kind: &str) -> Option<Value> {
            let ToolError::Declined { kind, data, .. } = error else {
                panic!("a concurrent loser is a typed decline");
            };
            assert_eq!(kind, expected_kind);
            data
        }

        fn finish(
            terminal: &PtyRegistry,
            txns: &TxnRegistry,
            by: &Holder,
            id: &TxnId,
            state: TxnState,
        ) -> Result<String, ToolError> {
            match state {
                TxnState::Committed => end(
                    terminal,
                    txns,
                    by,
                    "txn.commit",
                    CommitArgs {
                        txn: id.as_str().to_owned(),
                    },
                    state,
                ),
                TxnState::Aborted => abort(
                    terminal,
                    txns,
                    by,
                    AbortArgs {
                        txn: id.as_str().to_owned(),
                        reason: Some("stopped work".to_owned()),
                    },
                ),
                TxnState::Working => panic!("this fixture only ends transactions"),
            }
        }

        async fn end_during_close(fixture: &Fixture, state: TxnState) -> Value {
            let session = fixture.registered_session("umbrella goal").await;
            let started = fixture
                .begin(&session, None)
                .unwrap_or_else(|_| panic!("the transaction begins"));
            let txn = TxnId::new(started["txn"].as_str().unwrap());
            let terminal = Arc::clone(&fixture.terminal);
            let txns = Arc::clone(&fixture.txns);
            let by = fixture.by.clone();
            let (checked, checked_rx) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            let ending = std::thread::spawn(move || {
                crate::txns::AFTER_GOAL_PRESENCE_CHECK.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        checked
                            .send(())
                            .expect("the scheduler observes the presence check");
                        release_rx
                            .recv()
                            .expect("the scheduler releases the ending");
                    }));
                });
                finish(&terminal, &txns, &by, &txn, state)
            });
            let checked = checked_rx.recv_timeout(Duration::from_secs(10));
            let closed = fixture.terminal.close(&session).await;
            let released = release.send(());
            let ending = ending.join();
            // Complete release/join before any assertion can unwind the scheduler.
            checked.expect("the ending reached the old presence/goal boundary");
            closed.expect("the actual registered session closes");
            released.expect("the ending is released after close");
            let text = ending
                .expect("the ending returns")
                .unwrap_or_else(|_| panic!("the author may end after close"));
            assert!(fixture.terminal.holder_of(&session).is_none());
            serde_json::from_str(&text).expect("the actual capability returns a receipt")
        }

        #[tokio::test]
        async fn close_goal_commit_during_close_keeps_the_original_goal() {
            let fixture = Fixture::new("close-goal-capability-commit");
            let receipt = end_during_close(&fixture, TxnState::Committed).await;
            assert_eq!(receipt["state"], "committed");
            assert_eq!(receipt["goal_at_start"]["text"], "umbrella goal");
            assert!(
                receipt.get("goal_at_end").is_none(),
                "a concurrent close is not a goal clear: {receipt}"
            );
        }

        #[tokio::test]
        async fn close_goal_abort_during_close_keeps_the_original_goal() {
            let fixture = Fixture::new("close-goal-capability-abort");
            let receipt = end_during_close(&fixture, TxnState::Aborted).await;
            assert_eq!(receipt["state"], "aborted");
            assert_eq!(receipt["reason"], "stopped work");
            assert_eq!(receipt["goal_at_start"]["text"], "umbrella goal");
            assert!(
                receipt.get("goal_at_end").is_none(),
                "a concurrent close is not a goal clear: {receipt}"
            );
        }

        #[tokio::test]
        async fn close_goal_no_goal_begin_after_close_is_refused_without_a_record_or_id() {
            let fixture = Fixture::new("close-goal-no-goal-begin");
            let session = fixture.registered_session("umbrella goal").await;
            let terminal = Arc::clone(&fixture.terminal);
            let txns = Arc::clone(&fixture.txns);
            let by = fixture.by.clone();
            let session_text = session.as_str().to_owned();
            let (checked, checked_rx) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            let beginning = std::thread::spawn(move || {
                AFTER_BEGIN_CHECK.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        checked
                            .send(())
                            .expect("the scheduler observes the admission check");
                        release_rx.recv().expect("the scheduler releases the begin");
                    }));
                });
                begin(
                    &terminal,
                    &txns,
                    &by,
                    BeginArgs {
                        purpose: "work after close".to_owned(),
                        session: Some(session_text),
                        goal: None,
                    },
                )
            });
            let checked = checked_rx.recv_timeout(Duration::from_secs(10));
            let closed = fixture.terminal.close(&session).await;
            let released = release.send(());
            let beginning = beginning.join();
            checked.expect("the begin reached its advisory admission check");
            closed.expect("the actual registered session closes");
            released.expect("the begin is released after close");
            match beginning.expect("the begin returns") {
                Err(error) => {
                    assert_declined(error, "unknown_session");
                }
                Ok(accepted) => {
                    panic!("a closed session must not admit a no-goal begin: {accepted}")
                }
            }
            assert_eq!(fixture.txns.working_count(), 0);
            assert_eq!(
                fixture.txns.working_on(&SessionRef::new(session.as_str())),
                None
            );
            let next_session = fixture.registered_session("next goal").await;
            let next = fixture
                .begin(&next_session, None)
                .unwrap_or_else(|_| panic!("a real session can still begin"));
            assert_eq!(
                next["txn"], "txn-atomic-1",
                "the missing session minted no id"
            );
        }

        #[tokio::test]
        async fn close_goal_explicit_clear_and_missing_session_remain_distinct_for_endings() {
            for state in [TxnState::Committed, TxnState::Aborted] {
                for clear in [true, false] {
                    let fixture = Fixture::new("close-goal-capability-clear-or-missing");
                    let session = fixture.registered_session("umbrella goal").await;
                    let started = fixture
                        .begin(&session, None)
                        .unwrap_or_else(|_| panic!("the transaction begins"));
                    let txn = TxnId::new(started["txn"].as_str().unwrap());
                    if clear {
                        fixture
                            .terminal
                            .set_goal(&session, None, &fixture.by, 30)
                            .unwrap();
                    } else {
                        fixture.terminal.close(&session).await.unwrap();
                    }
                    let text = finish(&fixture.terminal, &fixture.txns, &fixture.by, &txn, state)
                        .unwrap_or_else(|_| panic!("the transaction can end"));
                    let receipt: Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(receipt["goal_at_start"]["text"], "umbrella goal");
                    assert_eq!(
                        receipt.get("goal_at_end").is_some(),
                        clear,
                        "only an explicit clear adds the field: {receipt}"
                    );
                    if clear {
                        assert!(receipt["goal_at_end"].is_null());
                    }
                }
            }
            for scope in [None, Some(SessionRef::new("not-a-session"))] {
                let fixture = Fixture::new("close-goal-capability-fallback");
                let initial = Goal::new("stored umbrella", Holder::install(), 1).unwrap();
                let txn = fixture
                    .txns
                    .begin(Holder::install(), "existing scope", scope, Some(initial), 1)
                    .unwrap();
                let text = finish(
                    &fixture.terminal,
                    &fixture.txns,
                    &Holder::install(),
                    txn.id(),
                    TxnState::Committed,
                )
                .unwrap_or_else(|_| panic!("existing scope fallback is retained"));
                let receipt: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(receipt["goal_at_start"]["text"], "stored umbrella");
                assert!(receipt.get("goal_at_end").is_none());
            }
        }

        #[tokio::test]
        async fn atomic_begin_same_session_refusal_preserves_the_winning_goal() {
            let fixture = Fixture::new("atomic-begin-same-session");
            let session = fixture.registered_session("initial goal").await;
            let (winner, refused) = fixture.contending_begin(&session, &session);
            let refused_data = assert_declined(refused, "txn_already_open");
            let accepted = fixture
                .txns
                .get(&TxnId::new(
                    winner["txn"].as_str().expect("a transaction id"),
                ))
                .expect("the actual accepted record is retained");
            assert_eq!(
                accepted.goal_at_start().map(Goal::text),
                Some("accepted goal")
            );
            assert_eq!(winner["goal_at_start"]["text"], "accepted goal");
            assert_eq!(fixture.txns.working_count(), 1);
            assert_eq!(
                fixture.terminal.goal_of(&session).as_ref().map(Goal::text),
                Some("accepted goal"),
                "a refused concurrent begin must not publish its goal"
            );
            assert_eq!(
                fixture.terminal.goal_of(&session).as_ref(),
                accepted.goal_at_start()
            );
            assert_eq!(
                refused_data.expect("the refusal names the winner")["txn"],
                winner["txn"]
            );
            assert_eq!(
                fixture.txns.working_on(&SessionRef::new(session.as_str())),
                Some(accepted.id().clone())
            );
            fixture
                .txns
                .end(
                    accepted.id(),
                    &fixture.by,
                    TxnState::Committed,
                    None,
                    accepted.goal_at_start().cloned(),
                    20,
                )
                .expect("the author ends the winner");
            let next = fixture
                .begin(&session, None)
                .unwrap_or_else(|_| panic!("the freed slot admits a begin"));
            assert_eq!(next["txn"], "txn-atomic-2", "the refusal mints no id");
        }

        #[tokio::test]
        async fn atomic_begin_global_capacity_refusal_preserves_the_losing_session_goal() {
            let fixture = Fixture::new("atomic-begin-global-capacity");
            let winner_session = fixture.registered_session("winner initial goal").await;
            let loser_session = fixture.registered_session("loser initial goal").await;
            // Existing registry callers may open multiple explicit install transactions. Leave one
            // global slot free while both real sessions still have empty session-bound slots.
            for _ in 0..MAX_OPEN_TXNS - 1 {
                fixture
                    .txns
                    .begin(Holder::install(), "existing work", None, None, 1)
                    .expect("a global slot is available");
            }
            let initial = fixture.terminal.goal_of(&loser_session);
            let (winner, refused) = fixture.contending_begin(&winner_session, &loser_session);
            assert!(assert_declined(refused, "too_many_txns").is_none());
            let accepted = fixture
                .txns
                .get(&TxnId::new(
                    winner["txn"].as_str().expect("a transaction id"),
                ))
                .expect("the actual accepted record is retained");
            assert_eq!(
                accepted.goal_at_start().map(Goal::text),
                Some("accepted goal")
            );
            assert_eq!(
                fixture.terminal.goal_of(&winner_session).as_ref(),
                accepted.goal_at_start()
            );
            assert_eq!(fixture.txns.working_count(), MAX_OPEN_TXNS);
            assert_eq!(
                fixture
                    .txns
                    .working_on(&SessionRef::new(loser_session.as_str())),
                None
            );
            assert_eq!(
                fixture.terminal.goal_of(&loser_session),
                initial,
                "a capacity-refused concurrent begin must leave its own session goal untouched"
            );
            fixture
                .txns
                .end(
                    accepted.id(),
                    &fixture.by,
                    TxnState::Committed,
                    None,
                    accepted.goal_at_start().cloned(),
                    20,
                )
                .expect("the author frees one global slot");
            let next = fixture
                .begin(&loser_session, None)
                .unwrap_or_else(|_| panic!("the freed global slot admits a begin"));
            assert_eq!(
                next["txn"],
                format!("txn-atomic-{}", MAX_OPEN_TXNS + 1),
                "the refused begin mints no id"
            );
            assert_eq!(next["goal_at_start"]["text"], "loser initial goal");
        }
        #[tokio::test]
        async fn a_refused_begin_preserves_a_later_legitimate_goal_update() {
            let fixture = Fixture::new("atomic-begin-later-owner-goal");
            let session = fixture.registered_session("initial goal").await;
            let mut later = None;
            let (winner, refused) = fixture.contending_begin_with(&session, &session, || {
                later = fixture
                    .terminal
                    .set_goal(&session, Some("later owner goal"), &Holder::panel(), 42)
                    .expect("an independent Owner goal update is authorized");
            });
            assert_declined(refused, "txn_already_open");
            assert_eq!(winner["goal_at_start"]["text"], "accepted goal");
            assert_eq!(
                fixture.terminal.goal_of(&session),
                later,
                "a refused begin neither publishes its own goal nor rolls back a later writer"
            );
            assert_eq!(later.as_ref().map(Goal::text), Some("later owner goal"));
            assert_eq!(later.as_ref().map(Goal::by), Some(&Holder::panel()));
        }

        #[tokio::test]
        async fn refused_begin_inputs_leave_goal_slots_records_and_ids_untouched() {
            let fixture = Fixture::new("atomic-begin-refused-inputs");
            let session = fixture.registered_session("initial goal").await;
            let initial = fixture.terminal.goal_of(&session);
            let refuse = |by: &Holder, arguments: BeginArgs, expected: &str| {
                let error = begin(&fixture.terminal, &fixture.txns, by, arguments)
                    .expect_err("this input must be refused");
                if expected == "arguments" {
                    assert!(matches!(error, ToolError::Arguments(_)));
                } else {
                    assert_declined(error, expected);
                }
                assert_eq!(fixture.terminal.goal_of(&session), initial);
                assert_eq!(fixture.txns.working_count(), 0);
                assert_eq!(
                    fixture.txns.working_on(&SessionRef::new(session.as_str())),
                    None
                );
            };
            let over = "x".repeat(201);
            for text in ["", "  ", "two\nlines", "a\u{7}b", over.as_str()] {
                refuse(
                    &fixture.by,
                    BeginArgs {
                        purpose: text.to_owned(),
                        session: Some(session.as_str().to_owned()),
                        goal: Some("must not be written".to_owned()),
                    },
                    "arguments",
                );
                refuse(
                    &fixture.by,
                    BeginArgs {
                        purpose: "valid work".to_owned(),
                        session: Some(session.as_str().to_owned()),
                        goal: Some(text.to_owned()),
                    },
                    "arguments",
                );
            }
            for unknown in ["not-a-session", "session-999999"] {
                refuse(
                    &fixture.by,
                    BeginArgs {
                        purpose: "valid work".to_owned(),
                        session: Some(unknown.to_owned()),
                        goal: Some("must not be written".to_owned()),
                    },
                    "unknown_session",
                );
            }
            let other = Holder {
                kind: HolderKind::Token,
                token_id: Some("srm_other".to_owned()),
            };
            refuse(
                &other,
                BeginArgs {
                    purpose: "valid work".to_owned(),
                    session: Some(session.as_str().to_owned()),
                    goal: Some("must not be written".to_owned()),
                },
                "not_goal_writer",
            );
            refuse(
                &fixture.by,
                BeginArgs {
                    purpose: "valid work".to_owned(),
                    session: None,
                    goal: None,
                },
                "session_required",
            );
            refuse(
                &fixture.by,
                BeginArgs {
                    purpose: "valid work".to_owned(),
                    session: None,
                    goal: Some("no session".to_owned()),
                },
                "arguments",
            );
            let accepted = fixture
                .begin(&session, None)
                .unwrap_or_else(|_| panic!("refusals leave the session available"));
            assert_eq!(
                accepted["txn"], "txn-atomic-1",
                "no refused input minted an id"
            );
            assert_eq!(fixture.terminal.goal_of(&session), initial);
        }

        #[tokio::test]
        async fn no_goal_begins_snapshot_current_metadata_and_keep_scope_and_writer_rules() {
            let fixture = Fixture::new("atomic-begin-snapshots");
            let session = fixture.registered_session("initial goal").await;
            let initial = fixture.terminal.goal_of(&session);
            let other = Holder {
                kind: HolderKind::Token,
                token_id: Some("srm_other".to_owned()),
            };
            let first = begin(
                &fixture.terminal,
                &fixture.txns,
                &other,
                BeginArgs {
                    purpose: "read current goal".to_owned(),
                    session: Some(session.as_str().to_owned()),
                    goal: None,
                },
            )
            .unwrap_or_else(|_| panic!("a no-goal begin does not require goal-writer permission"));
            let first: Value = serde_json::from_str(&first).expect("a begin answers JSON");
            let record = fixture
                .txns
                .get(&TxnId::new(first["txn"].as_str().unwrap()))
                .unwrap();
            assert_eq!(record.goal_at_start(), initial.as_ref());
            assert_eq!(record.by(), &other);
            fixture
                .txns
                .end(record.id(), &other, TxnState::Committed, None, initial, 20)
                .unwrap();
            fixture
                .terminal
                .set_goal(&session, None, &fixture.by, 30)
                .unwrap();
            let second = fixture
                .begin(&session, None)
                .unwrap_or_else(|_| panic!("no goal is a valid snapshot"));
            assert!(second["goal_at_start"].is_null());
            let device = begin(
                &fixture.terminal,
                &fixture.txns,
                &Holder::install(),
                BeginArgs {
                    purpose: "device work".to_owned(),
                    session: None,
                    goal: None,
                },
            )
            .unwrap_or_else(|_| panic!("the install door keeps device-scope begins"));
            let device: Value = serde_json::from_str(&device).unwrap();
            let record = fixture
                .txns
                .get(&TxnId::new(device["txn"].as_str().unwrap()))
                .unwrap();
            assert!(record.is_device_scope());
            assert!(record.goal_at_start().is_none());
        }
    }

    /// **A CLIENT'S SESSION-LESS BEGIN IS A TYPED DECLINE, NOT A GENERIC REFUSAL (round 27).** The
    /// gate never sees `txn.begin` (it is exempt, or a transaction could not open a transaction),
    /// so the rule's answer is this mapping: the audit row and the live event both get a closed kind,
    /// and the sentence the client reads names the fix.
    #[test]
    fn a_client_begin_without_a_session_is_a_session_required_decline() {
        let error = txn_error("txn.begin", TxnError::SessionRequired);
        let ToolError::Declined {
            kind,
            message,
            data,
        } = error
        else {
            panic!("a session refusal is a typed decline");
        };
        assert_eq!(kind, "session_required");
        assert!(data.is_none(), "the refusal names no record: {data:?}");
        assert!(
            message.starts_with("txn.begin: session_required: "),
            "the message carries the tool and the kind: {message}"
        );
        for fix in ["terminal.create", "txn.begin {session, purpose}"] {
            assert!(
                message.contains(fix),
                "the refusal names the fix {fix:?}: {message}"
            );
        }
    }

    #[test]
    fn the_router_names_exactly_the_three_transaction_tools() {
        assert_eq!(
            names().collect::<Vec<_>>(),
            ["txn.begin", "txn.commit", "txn.abort"]
        );
        assert!(owns("txn.begin"));
        assert!(!owns("txn"));
        assert!(!owns("terminal.write"));
    }
}
