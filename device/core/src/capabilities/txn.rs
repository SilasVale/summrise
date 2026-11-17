//! `txn`: the declaration. The implementation lives in `device/src/capabilities/txn.rs`.
//!
//! A transaction is the caller's declared unit of work (decision §3.3). The three tools are one
//! record's beginning and its two endings: `txn.begin` opens it with a purpose, and
//! `txn.commit`/`txn.abort` end it with a declaration that reverts nothing. The declaration says
//! what the gate requires and what the tools take; the registry that holds the records lives in
//! the device, because it is memory beside the sessions.

use serde_json::{json, Value};

use crate::registry::{Capability, ToolDef};
use crate::txn::{TXN_PURPOSE_MAX_CHARS, TXN_REASON_MAX_CHARS};

/// The manual entry, rendered into the guide beside the tool list.
pub const GUIDE: &str = "The transaction capability records the unit of work a machine call belongs to. Every machine call needs a working transaction: the direct `/mcp` client begins one with txn.begin {purpose} for work on the machine (files, journal, browser, monitor, plugins, runs, system), or txn.begin {session, purpose} for work on a terminal session. Calls on that session inherit it; a call that names no session inherits the caller's device-scope transaction; an explicit txn argument overrides both. txn.begin {session, purpose, goal?} opens the unit and answers its id, its start time and the goal it snapshotted; a session-bound begin may also write that session's goal. txn.commit {txn} and txn.abort {txn, reason?} end it: commit states the work is done, abort states the attempt did not achieve its purpose. Neither reverts a side effect, and the device never ends a transaction for you. Only the author (or an Owner door: the install door or the panel) may end one. The receipt puts the declaration beside what the device observed: calls, failures, refusals and how long it ran. One working session-bound transaction per session, one working device-scope transaction per caller, twenty working transactions device-wide, purpose and reason at most 200 characters, and sixty-four ended transactions retained.";

/// This capability's declaration: the one place its tools and their schemas are written down.
pub const CAPABILITY: Capability = Capability {
    name: "txn",
    guide: GUIDE,
    tools: &[
        ToolDef {
            name: "txn.begin",
            destructive: false,
            description: "Begin the unit of work a machine call belongs to. purpose is required: one sentence (1..=200 characters after trimming, one line, no control characters) saying what this series of operations is for. session binds the transaction to a live session, so its calls inherit it; omit session for work on the machine itself (files, journal, browser, monitor, plugins, runs, system), which the caller's device-scope transaction covers. goal, allowed only with a session, writes that session's goal in the same act and is snapshotted as the transaction's goal at its start. A second begin on a session that already has a working transaction, or by a caller that already has a working device-scope transaction, is refused txn_already_open. Answers the transaction id, its start time and the goal at its start.",
            schema: Some(begin_schema),
        },
        ToolDef {
            name: "txn.commit",
            destructive: false,
            description: "Declare the transaction's work done. This is a declaration, not an atomic operation: it reverts nothing. Only the transaction's author (or an Owner door: the install door or the panel) may end it. Answers the receipt: the purpose and author, the state, both ends, the observed calls, failures and refusals, how long it ran, and the goal at its start (and at its end when that changed). Ending an ended transaction is refused txn_ended.",
            schema: Some(commit_schema),
        },
        ToolDef {
            name: "txn.abort",
            destructive: false,
            description: "Declare that the transaction's attempt did not achieve its purpose, with an optional reason (1..=200 characters after trimming, one line, no control characters). This reverts nothing: the calls it observed stay in the record. Only the author (or an Owner door) may end it. Answers the same receipt as txn.commit, with state aborted and the reason. Ending an ended transaction is refused txn_ended.",
            schema: Some(abort_schema),
        },
    ],
};

fn begin_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "purpose": {
                "type": "string",
                "minLength": 1,
                "maxLength": TXN_PURPOSE_MAX_CHARS,
                "description": format!("What this series of operations is for: one sentence, 1..={TXN_PURPOSE_MAX_CHARS} characters after trimming, one line, no control characters. Required.")
            },
            "session": {
                "type": "string",
                "description": "A live session id, when this unit of work acts on that session. Omit it for work on the machine (files, journal, browser, monitor, plugins, runs, system)."
            },
            "goal": {
                "type": "string",
                "minLength": 1,
                "maxLength": crate::session::GOAL_MAX_CHARS,
                "description": format!("For a session-bound begin only: one sentence saying what the session is for, 1..={} characters after trimming, one line, no control characters. It is written to the session with the caller as its author and snapshotted as the transaction's goal at its start.", crate::session::GOAL_MAX_CHARS)
            }
        },
        "required": ["purpose"],
        "additionalProperties": false
    })
}

fn commit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "txn": {
                "type": "string",
                "description": "The transaction id txn.begin answered."
            }
        },
        "required": ["txn"],
        "additionalProperties": false
    })
}

fn abort_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "txn": {
                "type": "string",
                "description": "The transaction id txn.begin answered."
            },
            "reason": {
                "type": "string",
                "minLength": 1,
                "maxLength": TXN_REASON_MAX_CHARS,
                "description": format!("Why the attempt did not achieve its purpose: one line, at most {TXN_REASON_MAX_CHARS} characters after trimming. The abort reverts nothing; this is its declaration.")
            }
        },
        "required": ["txn"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{txn_policy, TxnPolicy};

    #[test]
    fn the_three_transaction_tools_are_one_record_s_beginning_and_endings() {
        let names: Vec<&str> = CAPABILITY.tools.iter().map(|tool| tool.name).collect();
        assert_eq!(names, ["txn.begin", "txn.commit", "txn.abort"]);
        for tool in CAPABILITY.tools {
            assert_eq!(
                txn_policy(tool.name),
                Some(TxnPolicy::ExemptRead),
                "{} is the record's own tool, so it needs no transaction",
                tool.name
            );
            assert!(
                !tool.destructive,
                "{} changes the record, not the machine",
                tool.name
            );
        }
    }

    #[test]
    fn the_begin_schema_names_both_scopes_and_the_goal_only_binds_to_a_session() {
        let schema = begin_schema();
        assert_eq!(schema["required"], json!(["purpose"]));
        assert_eq!(
            schema["properties"]["purpose"]["maxLength"],
            json!(TXN_PURPOSE_MAX_CHARS)
        );
        assert_eq!(
            schema["properties"]["session"]["type"],
            json!("string"),
            "a begin names the session it binds to, or names none for the machine"
        );
        assert_eq!(
            schema["properties"]["goal"]["type"],
            json!("string"),
            "a session-bound begin may write the session's goal"
        );
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "the tool takes exactly what it declares"
        );

        for (tool, required) in [
            ("txn.commit", json!(["txn"])),
            ("txn.abort", json!(["txn"])),
        ] {
            let schema = if tool == "txn.commit" {
                commit_schema()
            } else {
                abort_schema()
            };
            assert_eq!(schema["required"], required, "{tool}");
        }
        assert_eq!(
            abort_schema()["properties"]["reason"]["maxLength"],
            json!(TXN_REASON_MAX_CHARS)
        );
    }
}
