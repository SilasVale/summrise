//! The standard MCP transport's pure half: the bearer (a grant envelope or a permanent token), the
//! media types, the one local answer and the tool filter.
//!
//! The route itself (`POST /mcp/:id`) lives in `http.rs`, beside the custom envelope the page uses.
//! Everything here is a function of bytes the route has already decided are worth parsing: no state,
//! no I/O, no logging. That is what makes the protocol shape testable without a device channel or a
//! socket, and it keeps the handler's decisions readable as an order of checks rather than a body of
//! parsing.

use axum::http::{header, HeaderMap};
use serde_json::{json, Value};
use summrise_core::grant::GrantEnvelope;

/// The MCP protocol version this endpoint speaks.
///
/// It is the version the console answers, not the version a client asks for: a client that cannot
/// speak it must disconnect rather than be told a version the console does not implement.
pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";

/// What a standard MCP bearer is: a one-off grant, or a permanent token the console mints a grant
/// for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BearerKind {
    /// The grant envelope itself, in hex. Verified exactly as before.
    Grant(GrantEnvelope),
    /// A permanent token (`srm_` plus 64 lowercase hex). The route looks it up and mints a grant.
    Token(String),
}

/// Classify the `Authorization` header: a permanent token, or a grant.
///
/// The token is recognized by its prefix, which is why the prefix exists: a bearer that is not a
/// token goes to [`grant_from_headers`] unchanged, so the grant path keeps its exact parsing and
/// every existing client behaves as before. A malformed token falls through to the grant parser and
/// fails there (hex has no `s`, `r`, `m` or `_`), which is one `None` for every bad credential.
pub(crate) fn bearer_from_headers(headers: &HeaderMap) -> Option<BearerKind> {
    if let Some(token) = token_from_headers(headers) {
        return Some(BearerKind::Token(token));
    }
    grant_from_headers(headers).map(BearerKind::Grant)
}

/// The permanent token this header carries, when it is one.
fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    let body = token.strip_prefix(crate::tokens::TOKEN_PREFIX)?;
    let well_formed = token.len() == crate::tokens::TOKEN_LEN
        && body
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    well_formed.then(|| token.to_owned())
}

/// The `Authorization: Bearer <lowercase hex of the GrantEnvelope JSON>` a grant arrives as.
///
/// The scheme is case-insensitive because HTTP says it is. The token is the envelope's own JSON
/// bytes in hex, the console's one wire encoding for secrets and keys, so a client does not need a
/// second serializer for this credential.
pub(crate) fn grant_from_headers(headers: &HeaderMap) -> Option<GrantEnvelope> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let bytes = crate::hex::decode_bytes(token.trim())?;
    serde_json::from_slice(&bytes).ok()
}

/// Whether the request body is JSON. A missing `Content-Type` is not JSON: the endpoint does not
/// guess what an unlabelled body means.
pub(crate) fn content_type_is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
        .unwrap_or(false)
}

/// Whether the client accepts a JSON answer.
///
/// An absent `Accept` is `*/*` by HTTP's rule. `text/event-stream` alone is refused: this endpoint
/// has no server-initiated messages to put on a stream, and answering a stream-only client with
/// JSON would be a contract it did not ask for.
pub(crate) fn accept_allows_json(headers: &HeaderMap) -> bool {
    let Some(accept) = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    accept.split(',').any(|media| {
        let media = media.split(';').next().unwrap_or("").trim();
        media.eq_ignore_ascii_case("application/json")
            || media == "*/*"
            || media.eq_ignore_ascii_case("application/*")
    })
}

/// The `initialize` result: what this endpoint is and what it will answer.
///
/// The instructions name the one device, because an MCP client may talk to several servers and the
/// device id is the only fact that distinguishes this one from the same registry behind another
/// path.
pub(crate) fn initialize_result(device_id: &str, version: &str) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "summrise-console", "version": version },
        "instructions": format!(
            "This endpoint reaches Summrise device {device_id}. The tools it lists are the grant's \
             tools; every call is audited."
        ),
    })
}

/// Retain only the tools the grant names, in the order the device returned them.
///
/// The device's `tools/list` is the authority on names, descriptions and input schemas; the grant is
/// the authority on which of them this client may see. A response without a `result.tools` array is
/// left alone, whatever it is.
pub(crate) fn filter_tools(response: &mut Value, allowed: &[String]) {
    let Some(tools) = response
        .get_mut("result")
        .and_then(|result| result.get_mut("tools"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    tools.retain(|tool| {
        tool.get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| allowed.iter().any(|allowed| allowed == name))
    });
}

/// A JSON-RPC 2.0 result envelope.
pub(crate) fn jsonrpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A JSON-RPC 2.0 error envelope, as a value: the route decides the HTTP status.
pub(crate) fn jsonrpc_error_body(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use summrise_core::grant::Grant;

    fn envelope() -> GrantEnvelope {
        GrantEnvelope {
            grant: Grant {
                device_id: "ab".repeat(32),
                tools: vec!["files.read".to_owned(), "terminal.list".to_owned()],
                expires_at_ms: 1_000,
                allow_destructive: false,
            },
            signature: "cd".repeat(64),
        }
    }

    #[test]
    fn a_grant_header_round_trips_through_hex() {
        let envelope = envelope();
        let bytes = serde_json::to_vec(&envelope).expect("the envelope serializes");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", crate::hex::encode(&bytes))
                .parse()
                .expect("a header value"),
        );

        assert_eq!(grant_from_headers(&headers), Some(envelope));
    }

    #[test]
    fn a_bearer_is_either_a_grant_envelope_or_a_permanent_token() {
        let envelope = envelope();
        let bytes = serde_json::to_vec(&envelope).expect("the envelope serializes");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", crate::hex::encode(&bytes))
                .parse()
                .expect("a header value"),
        );
        assert_eq!(
            bearer_from_headers(&headers),
            Some(BearerKind::Grant(envelope))
        );

        let token = format!("srm_{}", "ab".repeat(32));
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("a header value"),
        );
        assert_eq!(
            bearer_from_headers(&headers),
            Some(BearerKind::Token(token)),
            "an srm_ bearer is the token, never an attempt to hex-decode a grant"
        );
    }

    #[test]
    fn a_bearer_with_a_malformed_token_is_no_bearer() {
        for value in [
            // Not 64 hex after the prefix.
            format!("Bearer srm_{}", "ab".repeat(31)),
            format!("Bearer srm_{}", "ab".repeat(33)),
            // Uppercase is not the shape the console mints.
            format!("Bearer srm_{}", "AB".repeat(32)),
            // Not hex at all.
            format!("Bearer srm_{}", "zz".repeat(32)),
            "Bearer srm_".to_owned(),
            "Basic srm_ababababababababababababababababababababababababababababababababab"
                .to_owned(),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                value.parse().expect("a header value"),
            );
            assert_eq!(bearer_from_headers(&headers), None, "{value}");
        }
    }

    #[test]
    fn a_wrong_scheme_or_odd_hex_is_not_a_grant() {
        let bytes = serde_json::to_vec(&envelope()).expect("the envelope serializes");
        let token = crate::hex::encode(&bytes);

        for value in [
            format!("Basic {token}"),
            "Bearer 0".to_owned(),
            "Bearer zz".to_owned(),
            "Bearer".to_owned(),
            String::new(),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                value.parse().expect("a header value"),
            );
            assert_eq!(grant_from_headers(&headers), None, "{value}");
        }
    }

    #[test]
    fn only_the_allowed_tools_survive_the_filter() {
        let mut response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "tools": [
                { "name": "files.read", "description": "read" },
                { "name": "files.write", "description": "write" },
            ]},
        });

        filter_tools(&mut response, &["files.read".to_owned()]);

        assert_eq!(
            response["result"]["tools"],
            json!([{ "name": "files.read", "description": "read" }])
        );
    }

    #[test]
    fn a_response_without_a_tool_array_is_left_alone() {
        let mut error = json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32601 } });
        let before = error.clone();
        filter_tools(&mut error, &["files.read".to_owned()]);
        assert_eq!(error, before);
    }

    #[test]
    fn a_client_that_wants_only_sse_is_not_served_json() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, "text/event-stream".parse().unwrap());
        assert!(!accept_allows_json(&headers));

        headers.insert(
            header::ACCEPT,
            "application/json, text/event-stream".parse().unwrap(),
        );
        assert!(accept_allows_json(&headers));

        headers.remove(header::ACCEPT);
        assert!(accept_allows_json(&headers));
    }

    #[test]
    fn json_is_the_only_content_type_the_endpoint_reads() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        assert!(content_type_is_json(&headers));

        headers.insert(
            header::CONTENT_TYPE,
            "application/json; charset=utf-8".parse().unwrap(),
        );
        assert!(content_type_is_json(&headers));

        headers.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
        assert!(!content_type_is_json(&headers));

        headers.remove(header::CONTENT_TYPE);
        assert!(!content_type_is_json(&headers));
    }

    #[test]
    fn the_initialize_answer_names_the_device_and_the_console() {
        let answer = initialize_result("ab".repeat(32).as_str(), "0.3.40");

        assert_eq!(answer["protocolVersion"], json!(PROTOCOL_VERSION));
        assert_eq!(answer["capabilities"]["tools"]["listChanged"], json!(false));
        assert_eq!(answer["serverInfo"]["name"], json!("summrise-console"));
        assert_eq!(answer["serverInfo"]["version"], json!("0.3.40"));
        assert!(answer["instructions"]
            .as_str()
            .expect("an instructions string")
            .contains(&"ab".repeat(32)));
    }
}
