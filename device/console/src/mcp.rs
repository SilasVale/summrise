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
    let token = bearer_value(headers)?;
    if let Some(permanent) = permanent_token(token) {
        return Some(BearerKind::Token(permanent));
    }
    grant_from_value(token).map(BearerKind::Grant)
}

/// The credential in `Authorization: Bearer …`, parsed once for both bearer types.
fn bearer_value(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(token.trim())
}

/// The permanent token this value is, when it has the prefix and the exact shape.
fn permanent_token(token: &str) -> Option<String> {
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
/// second serializer for this credential. Test-only: the route goes through [`bearer_from_headers`],
/// which uses the same single parse and the same [`grant_from_value`].
#[cfg(test)]
pub(crate) fn grant_from_headers(headers: &HeaderMap) -> Option<GrantEnvelope> {
    grant_from_value(bearer_value(headers)?)
}

fn grant_from_value(token: &str) -> Option<GrantEnvelope> {
    let bytes = crate::hex::decode_bytes(token)?;
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

/// One device an aggregate token may reach: its registry id and the optional operator label.
///
/// The label is a convenience; the id is the identity. Keeping the pair in one type is what stops
/// the `(String, String)` shape from being rebuilt by hand at every call site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeviceRef {
    pub id: String,
    pub name: Option<String>,
}

/// The `initialize` result for the aggregate endpoint: the devices its token may reach and the one
/// argument every tool takes.
///
/// The list is `id (name)` so a client can pass either form; the console, not the schema, is the
/// authority that a name is unique and inside the token's scope.
pub(crate) fn aggregate_initialize_result(devices: &[DeviceRef], version: &str) -> Value {
    let list = device_list(devices);
    let reach = if devices.is_empty() {
        "no devices are currently registered in this token's scope".to_owned()
    } else {
        format!("{} Summrise device(s): {list}", devices.len())
    };
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "summrise-console", "version": version },
        "instructions": format!(
            "This endpoint reaches {reach}. Every tool takes a required `device` argument: the device \
             id, or a unique device name. The tools it lists are the token's tools; every call is \
             audited."
        ),
    })
}

/// Add the required `device` argument to every tool in a `tools/list` result.
///
/// The device's own schema is preserved; the new property is described with the token's
/// `id (name)` pairs and added to `required`. A tool that arrived without a schema still gets one
/// that admits the argument, rather than being dropped from the surface.
pub(crate) fn add_device_argument(result: &mut Value, devices: &[DeviceRef]) {
    let description = format!(
        "The device to act on: an id, or a name unique in this token's scope ({}).",
        device_list(devices)
    );
    let Some(tools) = result
        .get_mut("result")
        .and_then(|result| result.get_mut("tools"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for tool in tools {
        let Some(tool) = tool.as_object_mut() else {
            continue;
        };
        let schema = tool
            .entry("inputSchema")
            .or_insert_with(|| json!({ "type": "object" }));
        let Some(schema) = schema.as_object_mut() else {
            continue;
        };
        schema.entry("type").or_insert_with(|| json!("object"));
        let properties = schema.entry("properties").or_insert_with(|| json!({}));
        if let Some(properties) = properties.as_object_mut() {
            properties.insert(
                "device".to_owned(),
                json!({ "type": "string", "description": description }),
            );
        }
        let required = schema.entry("required").or_insert_with(|| json!([]));
        if let Some(required) = required.as_array_mut() {
            if !required.iter().any(|value| value == "device") {
                required.push(json!("device"));
            }
        }
    }
}

/// `id (name)` pairs for instructions and tool descriptions.
fn device_list(devices: &[DeviceRef]) -> String {
    if devices.is_empty() {
        return "none".to_owned();
    }
    devices
        .iter()
        .map(|device| match device.name.as_deref() {
            Some(name) if !name.is_empty() => format!("{} ({name})", device.id),
            _ => device.id.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
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
                nonce: String::new(),
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

    fn device(id: &str, name: Option<&str>) -> DeviceRef {
        DeviceRef {
            id: id.to_owned(),
            name: name.map(str::to_owned),
        }
    }

    #[test]
    fn the_aggregate_initialize_names_every_device() {
        let value = aggregate_initialize_result(
            &[device("a", Some("ALPHA")), device("b", Some("BETA"))],
            "0.3.43",
        );
        assert_eq!(value["serverInfo"]["name"], json!("summrise-console"));
        assert_eq!(value["serverInfo"]["version"], json!("0.3.43"));
        let text = value["instructions"].as_str().expect("instructions");
        assert!(text.contains("a (ALPHA)"), "{text}");
        assert!(text.contains("b (BETA)"), "{text}");
        assert!(text.contains("device"), "{text}");
    }

    #[test]
    fn an_unlabelled_device_is_listed_by_id_only() {
        let value = aggregate_initialize_result(&[device("a", None)], "0.3.43");
        let text = value["instructions"].as_str().expect("instructions");
        assert!(text.contains("a"), "{text}");
        assert!(!text.contains("unnamed"), "{text}");
    }

    #[test]
    fn the_aggregate_instructions_have_no_source_indentation() {
        let value = aggregate_initialize_result(
            &[device("a", Some("ALPHA")), device("b", Some("BETA"))],
            "0.3.43",
        );
        let text = value["instructions"].as_str().expect("instructions");
        assert!(!text.contains("  "), "{text}");
    }

    #[test]
    fn the_aggregate_initialize_with_no_devices_says_so() {
        let value = aggregate_initialize_result(&[], "0.3.43");
        let text = value["instructions"].as_str().expect("instructions");
        assert!(text.contains("no devices"), "{text}");
    }

    #[test]
    fn every_tool_gets_a_required_device_argument() {
        let mut result = json!({"result": {"tools": [
            {"name": "files.read", "inputSchema": {
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }}
        ]}});
        add_device_argument(&mut result, &[device("a", Some("ALPHA"))]);
        let schema = &result["result"]["tools"][0]["inputSchema"];
        assert_eq!(schema["properties"]["device"]["type"], json!("string"));
        assert!(schema["properties"]["device"]["description"]
            .as_str()
            .unwrap()
            .contains("a (ALPHA)"));
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "device"));
        assert_eq!(schema["properties"]["path"]["type"], json!("string"));
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "path"));
    }

    #[test]
    fn injection_tolerates_a_tool_without_a_schema() {
        let mut result = json!({"result": {"tools": [{"name": "x"}]}});
        add_device_argument(&mut result, &[device("a", Some("ALPHA"))]);
        assert_eq!(
            result["result"]["tools"][0]["inputSchema"]["properties"]["device"]["type"],
            json!("string")
        );
    }

    #[test]
    fn injection_is_a_no_op_on_a_result_without_tools() {
        let mut result = json!({"result": {"tools": "not an array"}});
        let before = result.clone();
        add_device_argument(&mut result, &[device("a", Some("ALPHA"))]);
        assert_eq!(result, before);
    }
}
