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

use crate::capabilities::terminal::{goal_json, PtyError, SessionId};
use crate::capabilities::Ctx;
use crate::mcp::{parse, ToolError};

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
        "txn.begin" => begin(ctx, by, parse(arguments, tool)?),
        "txn.commit" => end(ctx, by, tool, parse(arguments, tool)?, TxnState::Committed),
        "txn.abort" => abort(ctx, by, parse(arguments, tool)?),
        other => Err(ToolError::Internal(format!(
            "{other}: the txn capability does not declare this tool"
        ))),
    }
}

fn begin(ctx: &Ctx<'_>, by: &Holder, arguments: BeginArgs) -> Result<String, ToolError> {
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
    let goal_at_start = match &session {
        Some(session) => {
            let id = live_session(ctx, session)?;
            if let Err(error) = ctx.txns.check_begin(by, Some(session)) {
                return Err(begin_refusal(ctx, by, Some(session), error));
            }
            match arguments.goal.as_deref() {
                Some(text) => ctx
                    .terminal
                    .set_goal(&id, Some(text), by, now)
                    .map_err(|error| goal_error("txn.begin", error))?,
                None => ctx.terminal.goal_of(&id),
            }
        }
        None => {
            if let Err(error) = ctx.txns.check_begin(by, None) {
                return Err(begin_refusal(ctx, by, None, error));
            }
            None
        }
    };

    let txn = ctx
        .txns
        .begin(by.clone(), &arguments.purpose, session, goal_at_start, now)
        .map_err(|error| txn_error("txn.begin", error))?;
    Ok(json!({
        "txn": txn.id().as_str(),
        "started_at_ms": txn.started_at_ms(),
        "goal_at_start": txn.goal_at_start().map(goal_json),
    })
    .to_string())
}

fn end(
    ctx: &Ctx<'_>,
    by: &Holder,
    tool: &str,
    arguments: CommitArgs,
    state: TxnState,
) -> Result<String, ToolError> {
    let id = TxnId::new(&arguments.txn);
    let txn = working(ctx, tool, &id)?;
    let receipt = ctx
        .txns
        .end(
            &id,
            by,
            state,
            None,
            goal_now(ctx, &txn),
            crate::http::now_ms(),
        )
        .map_err(|error| txn_error(tool, error))?;
    Ok(receipt.to_json().to_string())
}

fn abort(ctx: &Ctx<'_>, by: &Holder, arguments: AbortArgs) -> Result<String, ToolError> {
    let tool = "txn.abort";
    let id = TxnId::new(&arguments.txn);
    let txn = working(ctx, tool, &id)?;
    let receipt = ctx
        .txns
        .end(
            &id,
            by,
            TxnState::Aborted,
            arguments.reason.as_deref(),
            goal_now(ctx, &txn),
            crate::http::now_ms(),
        )
        .map_err(|error| txn_error(tool, error))?;
    Ok(receipt.to_json().to_string())
}

/// The record `txn.commit`/`txn.abort` names, or the typed refusal that says why there is none.
fn working(ctx: &Ctx<'_>, tool: &str, id: &TxnId) -> Result<Txn, ToolError> {
    let Some(txn) = ctx.txns.get(id) else {
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
fn goal_now(ctx: &Ctx<'_>, txn: &Txn) -> Option<Goal> {
    let Some(id) = txn
        .session()
        .and_then(|session| SessionId::parse(session.as_str()))
    else {
        return txn.goal_at_start().cloned();
    };
    if ctx.terminal.holder_of(&id).is_none() {
        return txn.goal_at_start().cloned();
    }
    ctx.terminal.goal_of(&id)
}

/// The live session a `txn.begin {session}` binds to, or the refusal that says it is not live.
fn live_session(ctx: &Ctx<'_>, session: &SessionRef) -> Result<SessionId, ToolError> {
    let Some(id) = SessionId::parse(session.as_str()) else {
        return Err(declined(
            "txn.begin",
            "unknown_session",
            "that session id is not one this device mints",
        ));
    };
    if ctx.terminal.holder_of(&id).is_none() {
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
    ctx: &Ctx<'_>,
    by: &Holder,
    session: Option<&SessionRef>,
    error: TxnError,
) -> ToolError {
    if error != TxnError::AlreadyOpen {
        return txn_error("txn.begin", error);
    }
    let open = match session {
        Some(session) => ctx.txns.working_on(session),
        None => ctx.txns.working_device_scope(by),
    }
    .and_then(|id| ctx.txns.get(&id));
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
