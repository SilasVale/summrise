//! `route-do.ts`'S DECISIONS — THE DURABLE OBJECT'S AUTH GATE, ITS THREE ROUTES, AND THE BODIES THEY ANSWER.
//!
//! ── WHY AN EMISSION, AND WHY THE HOST STAYS TYPESCRIPT ──────────────────────────────────────────────────────────
//!
//! A Durable Object class IS its runtime: `state.storage` and `fetch` come from the platform, and there is no Durable
//! Object in the Rust worker's world. **A host is kept and its decisions move** — the shape landing 6 gave the
//! Electron shell — so `route-do.ts` keeps the class, the storage calls and the `Response` construction, and the
//! rules go to the module they emit.
//!
//! **AND THE RUST WORKER IS THE DO'S CLIENT, WHICH IS WHAT MAKES THIS PORT DIFFERENT FROM THE LAST ONE.** `lib.rs`'s
//! `breaker_call` sends `x-do-auth` when it calls the breaker DO — as a LITERAL, until this landing, while the gate
//! that verifies it lived only in TypeScript. `DO_AUTH_HEADER` is now one name on both sides, and the gate the
//! client's header is checked by is this module's.
//!
//! `GET|PUT /api/me/route` is not ported (`auth_routes.rs` says why), so the Rust worker never calls the RouteDO and
//! never needed its three routes either. **The emission is the caller** — the rule the previous landing established.
//!
//! MUTATION: make `do_authorized` answer `crate::auth::safe_eq(got, expected)` with the fail-closed guard removed.
//! RESULT:   `an_unconfigured_gate_denies` fails, and so does `route-do.test.mjs`'s *"empty expected secret FAILS
//!           CLOSED"*: `safeEq("", "")` is TRUE, while an unconfigured gate must deny — which is the whole reason the
//!           guard is not the comparison.
//!
//! MUTATION: answer `RouteAction::Put` for a `PATCH /route` (move the method match's `_` arm into `"PUT"`).
//! RESULT:   `route-do.test.mjs`'s *"missing uid → 400 …; unknown path → 404"* fails on its PATCH case.

use serde_json::Value;

/// **THE HEADER, ONE NAME ON BOTH SIDES.** `route-do.ts`'s gate reads it and `lib.rs`'s `breaker_call` sends it — the
/// client sent the literal `"x-do-auth"` until this landing, beside a gate that lived only in TypeScript.
pub const DO_AUTH_HEADER: &str = "x-do-auth";

/// `authorizeDoRequest(request, expectedSecret)` — **FAIL CLOSED (Auth-core audit MED-2)**: an unconfigured
/// `DO_AUTH` must DENY every caller. **THE EMPTY-SECRET GUARD IS GATE POLICY, NOT COMPARISON SEMANTICS**:
/// `safeEq("", "")` is true, so a gate written as a bare comparison would wave every headerless caller through on a
/// deployment that never set the secret. The comparison itself is `auth::safe_eq` — the crate's one constant-time
/// primitive, which is what `auth.ts`'s header mandates reaching for instead of re-implementing.
pub fn do_authorized(got: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    crate::auth::safe_eq(got, expected)
}

/// What a DO request ASKS FOR, decided before any storage is touched.
#[derive(Debug, PartialEq, Eq)]
pub enum RouteAction {
    /// `GET /route?uid=…` — answer the stored model, or `null`.
    Get { uid: String },
    /// `PUT /route` with `{uid, model}` — `model` is `Some` when it was present and not null (a store), `None` when
    /// it was absent or null (a delete), which is the source's own `model != null` test.
    Put { uid: String, model: Option<String> },
    /// `DELETE /route?uid=…`.
    Delete { uid: String },
    /// `?uid=`/`uid` missing or EMPTY — the source tests truthiness, not presence.
    MissingUid,
    /// Any other path, and any method the route does not take (`PATCH /route` included).
    NotFound,
}

/// `RouteDO.fetch`'s dispatch, with the request already reduced to its parts (`method`, `path`, the `uid` query or
/// body field, and the parsed `model`).
///
/// **`model` IS `Option<&Value>` AND NOT `Option<&str>` ON PURPOSE**: the source's `model != null` is a test on the
/// PARSED VALUE, so `model: 0` or `model: false` are stored (`String(model)`) while `null` deletes — the distinction
/// a port that sniffed the string first would lose.
pub fn route_action(
    method: &str,
    path: &str,
    uid: Option<&str>,
    model: Option<&Value>,
) -> RouteAction {
    if path != "/route" {
        return RouteAction::NotFound;
    }
    let present = uid.filter(|u| !u.is_empty()).map(str::to_string);
    match method {
        "GET" => match present {
            Some(uid) => RouteAction::Get { uid },
            None => RouteAction::MissingUid,
        },
        "DELETE" => match present {
            Some(uid) => RouteAction::Delete { uid },
            None => RouteAction::MissingUid,
        },
        "PUT" => match present {
            Some(uid) => RouteAction::Put {
                uid,
                model: match model {
                    Some(value) if !value.is_null() => Some(crate::stream::js_text(value)),
                    _ => None,
                },
            },
            None => RouteAction::MissingUid,
        },
        _ => RouteAction::NotFound,
    }
}

/// The refusal an unauthorized DO request gets — PLAIN TEXT, not JSON, which is the source's own choice.
pub const UNAUTHORIZED_BODY: &str = "unauthorized";
/// The 404's body — plain text too.
pub const NOT_FOUND_BODY: &str = "not found";
/// The 400's body (`{error: "missing uid"}`).
pub const MISSING_UID_BODY: &str = "missing uid";
/// The prefix on a storage failure's message (`route-do: kv down`), which `route-do.test.mjs` matches on.
pub const INTERNAL_ERROR_PREFIX: &str = "route-do: ";

/// The emitted module's header — **THE SOURCE FILE'S OWN COMMENTS, CARRIED VERBATIM, PLUS THE GENERATED NOTICE.**
/// The fail-closed rationale and the truth table are the record of why this gate denies rather than compares, and
/// none of it is recoverable from the code.
const TS_HEADER: &str = r#"/**
 * route-do-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/route_do.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command to
 * fix it:
 *
 *     SUMMRISE_REFRESH_ROUTE_DO=1 cargo test --manifest-path gateway/wasm/Cargo.toml route_do::emission
 *
 * `gateway/src/route-do.ts` re-exports this module and keeps the HOST: the Durable Object classes, their storage
 * calls, and the `Response` construction. **A host is kept and its decisions move** — the shape the Electron shell
 * has — and the header name below is one name on both sides, because `lib.rs`'s `breaker_call` sends it.
 *
 * The comments below are the source file's own, carried verbatim.
 */

/**
 * DO external-address defense-in-depth, shared by every DO class (BreakerDO
 * and RouteDO used to each carry a byte-identical copy). A Durable Object
 * has its own external address even with workers_dev:false + no routes, so
 * the main router's auth is not the last line. FAIL CLOSED (Auth-core audit
 * MED-2): an unconfigured DO_AUTH must DENY every caller — never wave the
 * gate open. Constant-time compare on the x-do-auth header.
 *
 * Truth table (pinned in route-do.test.mjs):
 *   expected empty → false ALWAYS (even when the header is also absent —
 *     this is what makes it fail-closed rather than a plain equality);
 *   header missing / wrong length / wrong value → false;
 *   header equals expected → true.
 *
 * SOLID Round-8 (DRY/DIP): the comparison itself is auth.ts's `safeEq`
 * primitive — that module's header mandates reaching for its primitives
 * instead of re-implementing them, and this gate previously hand-rolled a
 * second copy of the same loop. The fail-closed empty-secret guard stays
 * HERE (it is gate policy, not comparison semantics — `safeEq("","")`
 * is true, which would wave an unconfigured gate open).
 */
import { safeEq } from "./auth.ts";

export const DO_AUTH_HEADER = "x-do-auth";

export function doAuthorized(got: string, expected: string): boolean {
  if (!expected) return false;
  return safeEq(got, expected);
}

/** What a DO request asks for, decided before any storage is touched. */
export type RouteAction =
  | { kind: "get"; uid: string }
  | { kind: "put"; uid: string; model: string | null }
  | { kind: "delete"; uid: string }
  | { kind: "missing-uid" }
  | { kind: "not-found" };

/** `RouteDO.fetch`'s dispatch, with the request already reduced to its parts. **`model` IS THE PARSED VALUE, not a
 *  string**: the source's `model != null` stores `0` and `false` (`String(model)`) and deletes on `null`, and a port
 *  that sniffed the string first would lose that. An EMPTY `uid` is missing — the source tests truthiness. */
export function routeAction(
  method: string,
  path: string,
  uid: string | null,
  model: unknown,
): RouteAction {
  if (path !== "/route") return { kind: "not-found" };
  if (!uid)
    return method === "GET" || method === "PUT" || method === "DELETE"
      ? { kind: "missing-uid" }
      : { kind: "not-found" };
  if (method === "GET") return { kind: "get", uid };
  if (method === "DELETE") return { kind: "delete", uid };
  if (method === "PUT") return { kind: "put", uid, model: model != null ? String(model) : null };
  return { kind: "not-found" };
}

/** The refusal an unauthorized DO request gets — PLAIN TEXT, not JSON. */
export const UNAUTHORIZED_BODY = "unauthorized";
/** The 404's body — plain text too. */
export const NOT_FOUND_BODY = "not found";
/** The 400's body (`{error: "missing uid"}`). */
export const MISSING_UID_BODY = "missing uid";
/** The prefix on a storage failure's message (`route-do: kv down`). */
export const INTERNAL_ERROR_PREFIX = "route-do: ";
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_HEADER.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/route-do-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape the four sibling
    /// emitters use.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_ROUTE_DO").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write route-do-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_ROUTE_DO=1 cargo test route_do::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/route-do-logic.ts is not what route_do.rs emits — \
             run SUMMRISE_REFRESH_ROUTE_DO=1 cargo test route_do::emission"
        );
    }

    /// **AND THE HEADER NAME IS ONE NAME ON BOTH SIDES, WHICH IS CHECKED RATHER THAN INTENDED**: `lib.rs`'s
    /// `breaker_call` sends the literal this constant holds, and the emitted module declares it too.
    #[test]
    fn the_header_name_is_the_one_the_client_sends() {
        let client = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
        )
        .expect("lib.rs is committed");
        assert!(
            client.contains("route_do::DO_AUTH_HEADER"),
            "lib.rs must NAME the header, not spell it: the whole point is one name on both sides"
        );
        assert!(render_typescript().contains(DO_AUTH_HEADER));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **THE TRUTH TABLE, INCLUDING THE ROW THAT DIFFERS FROM A BARE COMPARISON.** `safeEq("", "")` is true; this
    /// gate answers false, because an unconfigured deployment must refuse everyone.
    ///
    /// MUTATION: `crate::auth::safe_eq(got, expected)` without the empty-secret guard.
    /// RESULT:   the `("", "")` and `("anything", "")` rows fail.
    #[test]
    fn an_unconfigured_gate_denies() {
        assert!(!do_authorized("", ""));
        assert!(!do_authorized("anything", ""));
        assert!(!do_authorized("", "sekret"));
        assert!(do_authorized("sekret", "sekret"));
        assert!(
            !do_authorized("sekreT", "sekret"),
            "same length, one bit off"
        );
        assert!(!do_authorized("short", "sekret"), "length mismatch");
        assert!(!do_authorized("sekretsekret", "sekret"), "longer mismatch");
    }

    /// **THE DISPATCH, WITH THE TWO ARMS A CARELESS PORT COLLAPSES**: an EMPTY uid is missing (truthiness, not
    /// presence), and `model` is the PARSED value, so `0`/`false` store and `null` deletes.
    ///
    /// MUTATION: answer `Put` in the `_` arm.
    /// RESULT:   the `PATCH` assertion fails.
    #[test]
    fn the_dispatch_and_the_two_arms_that_look_alike() {
        assert_eq!(
            route_action("GET", "/route", Some("u1"), None),
            RouteAction::Get { uid: "u1".into() }
        );
        assert_eq!(
            route_action("DELETE", "/route", Some("u1"), None),
            RouteAction::Delete { uid: "u1".into() }
        );
        assert_eq!(
            route_action("PUT", "/route", Some("u1"), Some(&json!("m/1"))),
            RouteAction::Put {
                uid: "u1".into(),
                model: Some("m/1".into())
            }
        );
        assert_eq!(
            route_action("PUT", "/route", Some("u1"), Some(&Value::Null)),
            RouteAction::Put {
                uid: "u1".into(),
                model: None
            },
            "a JSON null DELETES"
        );
        assert_eq!(
            route_action("PUT", "/route", Some("u1"), Some(&json!(0))),
            RouteAction::Put {
                uid: "u1".into(),
                model: Some("0".into())
            },
            "0 is not null: it is stored, as String(0)"
        );
        assert_eq!(
            route_action("PUT", "/route", Some("u1"), Some(&json!(false))),
            RouteAction::Put {
                uid: "u1".into(),
                model: Some("false".into())
            }
        );
        assert_eq!(
            route_action("PUT", "/route", None, Some(&json!("m/1"))),
            RouteAction::MissingUid
        );
        assert_eq!(
            route_action("GET", "/route", Some(""), None),
            RouteAction::MissingUid,
            "an EMPTY uid is missing"
        );
        assert_eq!(
            route_action("GET", "/nope", Some("u1"), None),
            RouteAction::NotFound
        );
        assert_eq!(
            route_action("PATCH", "/route", Some("u1"), None),
            RouteAction::NotFound
        );
        assert_eq!(
            route_action("GET", "/route/", Some("u1"), None),
            RouteAction::NotFound
        );
    }

    /// **AND THE BODIES ARE THE WIRE SHAPES THE TEST PINS** — two plain-text ones and the prefix on the 500.
    #[test]
    fn the_bodies_are_pinned() {
        let ts = render_typescript();
        for literal in [
            UNAUTHORIZED_BODY,
            NOT_FOUND_BODY,
            MISSING_UID_BODY,
            INTERNAL_ERROR_PREFIX,
        ] {
            assert!(ts.contains(literal), "the emitter lost {literal:?}");
        }
        assert_eq!(UNAUTHORIZED_BODY, "unauthorized");
        assert_eq!(NOT_FOUND_BODY, "not found");
        assert_eq!(MISSING_UID_BODY, "missing uid");
        assert_eq!(INTERNAL_ERROR_PREFIX, "route-do: ");
    }
}
