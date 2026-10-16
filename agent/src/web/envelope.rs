//! THE TWO ENVELOPES THIS SURFACE SPEAKS, NAMED ONCE.
//!
//! Every JSON body `/api/*` answers is one of two shapes: the device did the thing
//! (`{"ok": true}`) or it refused (`{"ok": false, "error": …, "code": …}`). The failure half was
//! written out by hand 46 times in `web/mod.rs`, six more times inside closures that captured a
//! fixed code, once in `web/parse.rs` as the 400 body, and once in `web/mod.rs` as the
//! `invalid_params` helper the monitor routes already shared — four spellings of one wire shape,
//! which is the drift this module removes rather than describes.
//!
//! WHAT IS DELIBERATELY NOT HERE: the `{"ok": true, …}` records. Those carry a PAYLOAD — a session
//! list, a status record, a settings view — and every payload is a different record with its own
//! keys. Folding them in would move records into a builder, not remove a duplicate: the rule this
//! repository already applied elsewhere is "prefer unifying two things that are the same over
//! extracting a third thing that is merely large", and a `{"ok": true, <everything>}` builder is
//! the third thing. The bare `{"ok": true}` is the exception and is [`ok_body`], because it is the
//! same three tokens every time.
//!
//! THE WIRE ORDER IS NOT THE SOURCE ORDER, AND THAT IS MEASURED RATHER THAN ASSUMED. `summrise-agent`
//! builds `serde_json` without `preserve_order` (`cargo tree -p summrise-agent -e features -i
//! serde_json` lists `alloc`, `default`, `raw_value`, `std` and nothing else), so `Map` is a
//! `BTreeMap` and an object serializes ALPHABETICALLY:
//!
//! ```text
//! json!({"ok": false, "error": "boom", "code": "invalid_params"}).to_string()
//!   -> {"code":"invalid_params","error":"boom","ok":false}
//! ```
//!
//! So the `"ok"`-first spelling the call sites use is a SOURCE convention — it reads as an envelope
//! to a human — and the bytes a client parses are sorted. Both builders therefore build a `Value`
//! and neither can reorder anything: the order is serde_json's, and it was already that way before
//! this module existed. The tests below assert the bytes anyway, because a change to that feature
//! set would change the wire and something should say so out loud.
//!
//! WHAT IS ALSO NOT HERE, so a later reader does not "tidy" it in: `{"ok": false, "error": …}` with
//! NO code. Three sites answer it (`dispatch`'s not-found arms and the playwright ops), and giving
//! any of them a code would be a wire change wearing a refactor's clothes — the gateway and the
//! console branch on `ok === false` and the panel renders `error`, so a code that appeared where
//! there was none is a new field a client did not ask for.

use serde_json::Value;

/// The SUCCESS envelope: `{"ok": true}`, and nothing else.
///
/// It exists because the panel's `deviceRefused` (`panel-react/src/lib/api.ts`) refuses a body
/// WITHOUT `ok === true`, and a route that omits it goes quiet rather than red — see
/// `every_read_only_route_answers_the_envelope_the_panel_requires`, which walks the route table for
/// exactly that. This is the writer's half of that contract: the flag has one spelling.
pub(super) fn ok_body() -> Value {
    serde_json::json!({"ok": true})
}

/// The FAILURE envelope: `{"ok": false, "error": <message>, "code": <code>}`.
///
/// `message` is for a person and `code` is for a machine, and that split is the reason both
/// arguments are named rather than one being derived from the other. Both are `impl Into<String>`
/// rather than `impl Into<Value>` on purpose: a caller that handed over a number, an object or
/// `null` for either field would compile under `Into<Value>` and silently answer a client with a
/// shape the panel has never seen. `Into<String>` accepts the `String` and `&'static str` the call
/// sites already hold — and nothing else.
pub(super) fn error_body(message: impl Into<String>, code: impl Into<String>) -> Value {
    serde_json::json!({"ok": false, "error": message.into(), "code": code.into()})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The success envelope, byte for byte. The route-table walk in `web::tests` asserts the FLAG on
    /// every read-only route; this asserts the shape it arrives in.
    #[test]
    fn ok_body_serializes_to_the_bare_success_envelope() {
        let v = ok_body();
        assert_eq!(v.to_string(), r#"{"ok":true}"#);
        assert_eq!(
            v.as_object().map(|o| o.len()),
            Some(1),
            "the success envelope carries the flag and nothing else: {v}"
        );
    }

    /// The failure envelope, byte for byte — three keys, no more and no fewer. The order in the
    /// assertion is serde_json's (see the module docs), not this module's choice.
    #[test]
    fn error_body_serializes_the_three_keys_a_client_branches_on() {
        let v = error_body("boom", "invalid_params");
        assert_eq!(
            v.to_string(),
            r#"{"code":"invalid_params","error":"boom","ok":false}"#
        );
        assert_eq!(
            v.as_object().map(|o| o.len()),
            Some(3),
            "an extra key here is a wire change, not a convenience: {v}"
        );
    }

    /// The two argument types the call sites actually hold. A `String` message and a `&'static str`
    /// code are the common pair (`format!` + a literal); `invalid_params` hands over an owned code
    /// parameter instead. Both must land in the same three keys.
    #[test]
    fn error_body_takes_the_owned_and_borrowed_text_the_callers_hold() {
        let owned_message = format!("unknown tool: {}", "agent_status");
        let v = error_body(owned_message, "invalid_params");
        assert_eq!(v["ok"], serde_json::json!(false));
        assert_eq!(v["error"], "unknown tool: agent_status");
        assert_eq!(v["code"], "invalid_params");

        // The borrowed pair, including a code that arrives as a live parameter rather than a literal.
        let code: &str = "exec_timeout";
        let v = error_body("the command did not finish", code);
        assert_eq!(v["error"], "the command did not finish");
        assert_eq!(v["code"], "exec_timeout");
    }
}
