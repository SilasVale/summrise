//! `gate.ts`'S DECISIONS — THE WIRE THE DECISION ROUTE IS ASKED ON, AND THE ONE DISTINCTION ITS ANSWER CARRIES.
//!
//! ── ONE PROTOCOL, TWO SIDES, AND A LITERAL ON EACH ───────────────────────────────────────────────────────────────
//!
//! `POST /__decision` takes `{name, args}` and answers `{ok: true, value}` — the server half is `decisions.rs`
//! (`in_family` holds the method and the exact path, `handle` holds the envelope and the response shape). The client
//! half lived in `gate.ts`, where **the path was spelled a second time as `DECISION_URL`'s tail and the envelope was
//! built inline.** Two spellings of one wire is the drift last landing found on `x-do-auth`, and this module is the
//! same answer: the path is one name, `decisions::in_family` READS it, and the client is rendered from here.
//!
//! **AND THE CLIENT CANNOT BE THE RUST WORKER.** The Rust IS the server — it never calls the route — so this port has
//! no Rust caller either; the emission is the caller, and the note on `decision_value` is the reason the client is
//! worth owning rather than leaving to the file that happened to need it first.
//!
//! MUTATION: answer `Some(Value::Null)` where the body carries no `value` key (`unwrap_or(Value::Null)`).
//! RESULT:   `the_answer_carries_the_distinction` fails, and so does `plugins.test.mjs`'s *"no gate, a refusal and an
//!           unreachable gate are all undefined"* the moment a caller compares with `!== undefined` — **a missing
//!           value would become a decision that ran.**
//!
//! MUTATION: drop the `ok` test (answer whenever the status is 2xx).
//! RESULT:   `the_answer_carries_the_distinction` fails: `{ok: false}` with status 200 would be read as a decision.

use serde_json::{json, Value};

/// **THE PATH, ONE NAME FOR BOTH SIDES.** `POST` only, and exact: a prefix would let `/__decision_x` reach the table
/// (`decisions::in_family` is where that is enforced, and it reads this constant).
pub const DECISION_PATH: &str = "/__decision";

/// The route's own ADDRESS. A service binding ignores the host, so this is a placeholder that cannot resolve — the
/// source says so, and it is why the constant is not built from an env var.
pub const DECISION_URL: &str = "https://gate.invalid/__decision";

/// The one header the request carries.
pub const DECISION_CONTENT_TYPE: &str = "application/json";

/// The request body — `{name, args}`, the envelope `decisions::handle` parses.
pub fn decision_body(name: &str, args: &Value) -> String {
    json!({ "name": name, "args": args }).to_string()
}

/// **THE ANSWER'S PARSE RULE, AND THE ONE DISTINCTION IT MUST NOT COLLAPSE.**
///
/// `None` is a gate that is ABSENT, REFUSED, or UNREACHABLE — and it is the only case a caller may fall back to its
/// TypeScript twin on. `Some(Value::Null)` is a decision that RAN and answered null. **Collapsing the two would let a
/// wrong `null` from Rust be silently corrected by the twin**, which is the one failure this route must not hide: the
/// twin keeps the product working when the gate is gone, it does not make a wrong answer look right.
///
/// **AND A BODY WITHOUT A `value` KEY IS THE FIRST CASE, NOT THE SECOND** — the source reads `body.value`, which for a
/// missing key is `undefined`; the type here is what makes that unrepresentable as `null`.
pub fn decision_value(status_ok: bool, body: &Value) -> Option<Value> {
    if !status_ok {
        return None;
    }
    if body.get("ok") != Some(&Value::Bool(true)) {
        return None;
    }
    body.get("value").cloned()
}

/// The emitted module's header — **THE SOURCE FILE'S OWN COMMENTS, CARRIED VERBATIM, PLUS THE GENERATED NOTICE.**
const TS_HEADER: &str = r#"/**
 * gate-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/decision_client.rs` (`render_typescript`), which is now the single source. The
 * freshness test in that module is a byte comparison against this file, so a hand edit fails there with the text and
 * one command to fix it:
 *
 *     SUMMRISE_REFRESH_DECISION_CLIENT=1 cargo test --manifest-path gateway/wasm/Cargo.toml decision_client::emission
 *
 * `gateway/src/gate.ts` re-exports this module and keeps the BINDING: `wasmGate` and the `fetch` that may throw.
 * `gateway/wasm/src/decisions.rs` is the server half, and it reads `DECISION_PATH` from here — one wire, one name.
 */

/** The route's own path. A service binding ignores the host, and the server refuses anything but `POST` on exactly
 *  this path (`decisions::in_family`), so a prefix would let `/__decision_x` reach the table. */
export const DECISION_PATH = "/__decision";

/** The route's own address. A service binding ignores the host, so this is a placeholder that cannot resolve. */
export const DECISION_URL = "https://gate.invalid/__decision";

/** The one header the request carries. */
export const DECISION_CONTENT_TYPE = "application/json";

/** The request body — `{name, args}`, the envelope the server's `handle` parses. */
export function decisionBody(name: string, args: unknown): string {
  return JSON.stringify({ name, args });
}

/**
 * The answer's parse rule. **`undefined` IS NOT `null`.** `null` is a decision that ran and answered null;
 * `undefined` is a gate that is absent, refused, or unreachable — and the caller falls back to its TypeScript twin
 * for THAT case only. **Collapsing the two would let a wrong `null` from Rust be silently corrected by the twin**,
 * which is the one failure this route must not hide: the twin keeps the product working when the gate is gone, it
 * does not make a wrong answer look right.
 *
 * A body with no `value` key is the FIRST case, not the second: the source reads `body.value`, which for a missing
 * key is `undefined`.
 */
export function decisionValue<T>(statusOk: boolean, body: any): T | undefined {
  if (!statusOk) return undefined;
  if (body?.ok !== true) return undefined;
  return body.value as T | undefined;
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_HEADER.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/gate-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape the five sibling
    /// emitters use.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_DECISION_CLIENT").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write gate-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_DECISION_CLIENT=1 cargo test decision_client::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/gate-logic.ts is not what decision_client.rs emits — \
             run SUMMRISE_REFRESH_DECISION_CLIENT=1 cargo test decision_client::emission"
        );
    }

    /// **THE PATH IS ONE NAME ON BOTH SIDES, AND THAT IS CHECKED RATHER THAN INTENDED**: the server's `in_family`
    /// must READ it, the way `lib.rs` had to read `DO_AUTH_HEADER` — a second spelling is exactly what this landing
    /// removed.
    #[test]
    fn the_path_is_the_one_the_server_refuses_on() {
        let server = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/decisions.rs"),
        )
        .expect("decisions.rs is committed");
        assert!(
            server.contains("decision_client::DECISION_PATH"),
            "decisions.rs must NAME the path, not spell it — one wire, one name"
        );
        assert!(
            !server.contains("\"/__decision\""),
            "and the literal must be gone"
        );
        assert!(render_typescript().contains(DECISION_PATH));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE ENVELOPE AND THE DISTINCTION**, which are the two things the client owns.
    ///
    /// MUTATION: `body.get("value").cloned().unwrap_or(Value::Null)`.
    /// RESULT:   the last two assertions fail — a missing `value` is NOT a null answer.
    #[test]
    fn the_answer_carries_the_distinction() {
        let answered_null = json!({ "ok": true, "value": null });
        assert_eq!(decision_value(true, &answered_null), Some(Value::Null));
        assert_ne!(
            decision_value(true, &answered_null),
            None,
            "null is a VALUE"
        );

        let ran_no_value = json!({ "ok": true });
        assert_eq!(
            decision_value(true, &ran_no_value),
            None,
            "no `value` key is an answer that never happened, not a null one"
        );
        assert_eq!(decision_value(false, &answered_null), None, "a refusal");
        assert_eq!(
            decision_value(true, &json!({ "ok": false })),
            None,
            "ok is not the status"
        );
        assert_eq!(
            decision_value(true, &json!({ "ok": "true", "value": 1 })),
            None,
            "strictly true"
        );
        assert_eq!(
            decision_value(true, &json!({ "ok": true, "value": 0 })),
            Some(json!(0))
        );
        assert_eq!(
            decision_value(true, &json!({ "ok": true, "value": { "a": 1 } })),
            Some(json!({ "a": 1 }))
        );
    }

    /// **AND THE ENVELOPE IS THE SERVER'S OWN SPELLING** — key order included, because a body is bytes.
    #[test]
    fn the_envelope_is_name_then_args() {
        assert_eq!(
            decision_body("offered_ids", &json!({ "payload": ["a"] })),
            r#"{"name":"offered_ids","args":{"payload":["a"]}}"#
        );
        assert_eq!(DECISION_PATH, "/__decision");
        assert!(DECISION_URL.ends_with(DECISION_PATH));
        assert_eq!(DECISION_CONTENT_TYPE, "application/json");
    }
}
