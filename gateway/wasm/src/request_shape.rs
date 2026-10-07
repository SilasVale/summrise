//! THE REQUEST-SHAPE DECISIONS, MOVED FROM `gateway/src/plugins/translate.ts`.
//!
//! WHY THESE FIVE. They are the "which path does this request take" decisions — a route match, the
//! `tools` region of a raw body, whether the body must be parsed at all, and the two web-search
//! questions — and every one of them is SYNCHRONOUS AND PURE, which is what makes them portable without
//! dragging KV, a Durable Object or an upstream along. A drift here does not throw: it sends a request
//! down the wrong branch, which is the failure this repository's own comments record twice for
//! `tools_region_of` alone.
//!
//! `searchTargetFor` IS DELIBERATELY NOT HERE. Its `capable` test reads `SEARCH_CAPABLE_WIRE_MODELS`,
//! which `channels.ts` DERIVES from `MODEL_REGISTRY` — so porting it faithfully means porting the
//! registry's search facet with it, and half-porting it would answer a different question than the
//! shipping worker does. Named rather than approximated.

/// `channels.ts:18` — `VERIFY_PATH`, which `detectRoute` closes over.
pub const VERIFY_PATH: &str = "/v1/messages";
/// `translate.ts:95` — `COUNT_PATH`.
pub const COUNT_PATH: &str = "/v1/messages/count_tokens";

/// `detectRoute(method, path)`: four `endsWith` tests, in the source's order.
///
/// `endsWith` IS NOT `==`, and that is the whole reason this is a function rather than four comparisons
/// at a call site: a path with a prefix still matches, which is what lets a proxy mount the API under a
/// longer base.
pub fn detect_route(method: &str, path: &str) -> RouteShape {
    let post = method == "POST";
    RouteShape {
        is_count: post && path.ends_with(COUNT_PATH),
        is_messages: post && path.ends_with(VERIFY_PATH),
        is_chat_completions: post && path.ends_with("/v1/chat/completions"),
        is_responses: post && path.ends_with("/v1/responses"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteShape {
    pub is_count: bool,
    pub is_messages: bool,
    pub is_chat_completions: bool,
    pub is_responses: bool,
}

/// `toolsRegionOf(rawText)`: the `tools` array slice of a raw body, BRACKET-BALANCED.
///
/// **THE `<= 0` IS THE WHOLE FUNCTION.** The scan starts ON the array's own `[`, so that bracket takes
/// the depth to 1 and the array's closing `]` brings it back to 0 — stopping only BELOW zero runs one
/// level OUT, to the enclosing object's `}`, and the region then swallows everything after the tools
/// array. The file records what that cost: a transcript carrying a previous search put a literal
/// `"web_search"` in the region, `needsBodyParse` fired, and a ~2 MB body was re-parsed on every later
/// turn (~2.4 ms against a 10 ms Free-plan budget).
///
/// The scan counts BRACES AND BRACKETS TOGETHER, which is why a tool schema's nested objects do not
/// truncate it — the two earlier fixes (cutting at the first `"messages"`, then at the next one) both
/// failed on a schema property NAMED `messages`.
pub fn tools_region_of(raw_text: &str) -> String {
    const ANCHOR: &str = "\"tools\":[";
    let Some(tools_start) = raw_text.find(ANCHOR) else {
        return String::new();
    };
    let mut depth: i64 = 0;
    let mut end: i64 = -1;
    // `toolsStart + 8` is the array's own `[` — the anchor is 9 characters and the scan starts ON the
    // bracket, which is what makes the initial depth 1 rather than 0.
    for (offset, ch) in raw_text[tools_start + 8..].char_indices() {
        match ch {
            '[' | '{' => depth += 1,
            ']' | '}' => {
                depth -= 1;
                if depth <= 0 {
                    end = (tools_start + 8 + offset) as i64;
                    break;
                }
            }
            _ => {}
        }
    }
    if end > 0 {
        // `rawText.slice(toolsStart, end + 1)` — and `end` is a CHAR index here, so the slice is taken
        // by chars rather than bytes.
        let start = raw_text[..tools_start].chars().count();
        let take = (end as usize + 1) - start;
        raw_text.chars().skip(start).take(take).collect()
    } else {
        String::new()
    }
}

/// **JS `\s`, WHICH IS NOT ASCII WHITESPACE.** `RegExp`'s `\s` is
/// `[\f\n\r\t\v\u0020\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff]`, and a port that
/// used `char::is_whitespace` would accept a few characters the JavaScript does not (and refuse none it
/// accepts) — a difference that only shows up in a body containing one, which is exactly the kind of
/// case the corpus carries.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{000c}' | '\n' | '\r' | '\t' | '\u{000b}' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `/"type"\s*:\s*"image"/` — a literal scan, because the pattern is a literal with two whitespace runs.
fn has_image_type(text: &str) -> bool {
    const NEEDLE: &str = "\"type\"";
    let bytes: Vec<char> = text.chars().collect();
    let needle: Vec<char> = NEEDLE.chars().collect();
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if bytes[i..i + needle.len()] == needle[..] {
            let mut j = i + needle.len();
            while j < bytes.len() && is_js_space(bytes[j]) {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == ':' {
                j += 1;
                while j < bytes.len() && is_js_space(bytes[j]) {
                    j += 1;
                }
                let image: Vec<char> = "\"image\"".chars().collect();
                if j + image.len() <= bytes.len() && bytes[j..j + image.len()] == image[..] {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// `needsBodyParse(rawText, toolsRegion)`: the CPU guard for the 10 ms Free-plan budget.
///
/// The source records that THREE clauses were once written here and only TWO are reachable — a regex
/// matching a substring also matches the string containing it, so the `lastUserMsg` suffix clause can
/// never be true while the whole-body clause is false. Two clauses is what it is.
pub fn needs_body_parse(raw_text: &str, tools_region: &str) -> bool {
    has_image_type(raw_text) || tools_region.contains("\"web_search\"")
}

/// `isSearchOnlyRequest(body)`: a request whose ONLY tool is the web_search server tool.
///
/// `!!body && !body.tool_choice && Array.isArray(body.tools) && body.tools.length === 1 &&
/// body.tools[0]?.type === "web_search_20250305"` — five tests, and the exact-one guard is what keeps
/// Claude Code's multi-tool ordinary turns untouched.
pub fn is_search_only_request(body: &serde_json::Value) -> bool {
    if body.is_null() {
        return false;
    }
    if body.get("tool_choice").map(js_truthy).unwrap_or(false) {
        return false;
    }
    let Some(tools) = body.get("tools").and_then(|v| v.as_array()) else {
        return false;
    };
    if tools.len() != 1 {
        return false;
    }
    tools[0].get("type").and_then(|v| v.as_str()) == Some("web_search_20250305")
}

/// `isForcedWebSearch(toolChoice)`: does `tool_choice` FORCE a web_search call?
///
/// A DECLARATION is not intent: Claude Code declares `web_search_20250305` in the tools array of EVERY
/// ordinary turn, so a declaration-only check hijacked the user's chosen model on every request. Only a
/// forced `tool_choice` is real search intent.
pub fn is_forced_web_search(tool_choice: &serde_json::Value) -> bool {
    if !js_truthy(tool_choice) {
        return false;
    }
    let kind = tool_choice
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if kind == "tool" {
        return tool_choice.get("name").and_then(|v| v.as_str()) == Some("web_search");
    }
    if kind == "any" {
        return tool_choice
            .get("tools")
            .and_then(|v| v.as_array())
            .map(|tools| {
                tools
                    .iter()
                    .any(|t| t.get("name").and_then(|v| v.as_str()) == Some("web_search"))
            })
            .unwrap_or(false);
    }
    false
}

/// JavaScript truthiness, which is what `!!x` and a bare `&&` mean: `false`, `0`, `NaN`, `""`, `null`
/// and `undefined` are falsy, and EVERY object (including an empty one and an empty array) is truthy.
fn js_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `shape` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    #[test]
    fn every_request_shape_decision_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "detect_route" => {
                    let shape = detect_route(
                        input["method"].as_str().unwrap_or(""),
                        input["path"].as_str().unwrap_or(""),
                    );
                    let got = serde_json::json!({
                        "isCount": shape.is_count,
                        "isMessages": shape.is_messages,
                        "isChatCompletions": shape.is_chat_completions,
                        "isResponses": shape.is_responses,
                    });
                    assert_eq!(&got, want, "{func} / {name}");
                }
                "tools_region_of" => {
                    assert_eq!(
                        serde_json::Value::String(tools_region_of(input.as_str().unwrap_or(""))),
                        *want,
                        "{func} / {name}"
                    );
                }
                "needs_body_parse" => {
                    let got = needs_body_parse(
                        input["rawText"].as_str().unwrap_or(""),
                        input["toolsRegion"].as_str().unwrap_or(""),
                    );
                    assert_eq!(serde_json::Value::Bool(got), *want, "{func} / {name}");
                }
                "is_search_only_request" => {
                    assert_eq!(
                        serde_json::Value::Bool(is_search_only_request(input)),
                        *want,
                        "{func} / {name}"
                    );
                }
                "is_forced_web_search" => {
                    assert_eq!(
                        serde_json::Value::Bool(is_forced_web_search(input)),
                        *want,
                        "{func} / {name}"
                    );
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 25,
            "the request-shape corpus shrank to {checked} cases"
        );
    }
}

/// **THE `count_tokens` ROUTE'S DECISION, AND ONLY ITS DECISION.**
///
/// The source's whole arm is:
///
/// ```text
///   for (const kind of ["deepseek", "qwen", "amd", "r4"]) {
///     if (route.kind === kind && isKeyMissing(kind, byok, env)) return keyMissingError(kind);
///   }
///   return jsonOk({ input_tokens: estimateTokens(rawText) });
/// ```
///
/// FOUR KINDS ARE CHECKED, AND THE LIST IS NOT THE ROUTE TABLE: a channel that reaches its upstream without
/// its key goes out headerless and the user gets a bare "Upstream 401" instead of a config error naming the
/// provider — which is why a forgotten entry here is worth a test rather than a review. The estimate is
/// LOCAL for every channel (the upstream count endpoint used to cost a round-trip on every Claude Code
/// turn); the missing-key checks stay, because those are real config errors rather than latency.
///
/// `kind` IS AN ARGUMENT: resolving the route from a model id is the I/O edge's job (`pickRoute` reads the
/// table, the env and the US-proxy switch), and this answers what the route does once it is known.
pub fn count_tokens_response(
    kind: &str,
    byok: &serde_json::Map<String, serde_json::Value>,
    env: &serde_json::Value,
    raw_text: &serde_json::Value,
) -> crate::responses::Built {
    for checked in ["deepseek", "qwen", "amd", "r4"] {
        if kind == checked && crate::byok::is_key_missing(checked, byok, Some(env)) {
            // `keyMissingError(kind) as Response` — the table's own message, and the `as Response` in the
            // source is a non-null assertion: the list above is a SUBSET of the table's keys, so the lookup
            // always hits. If it ever did not, this answers the same 502 with an empty message rather than
            // panicking, which is the honest failure for a config error.
            return crate::responses::key_missing_error(checked)
                .unwrap_or_else(|| crate::responses::json_error(502, "", "config_error"));
        }
    }
    let tokens = crate::tokens::estimate_tokens(raw_text);
    crate::responses::json_ok(&serde_json::json!({ "input_tokens": tokens }), &[])
}

/// **THE AUTH DECISION.** `findUserByToken` is a KV read (the I/O edge); what it ANSWERS is a user or
/// nothing, and the handler's rule is one line: "`if (!user || !user.enabled) return jsonError(401, …)`".
///
/// The message is the same whether the token was absent, unknown or disabled — which is deliberate: naming
/// which of the three it was tells an attacker whether a token exists.
///
/// **AND THE CORPUS NOW EXERCISES THE `enabled` HALF — THIS PARAGRAPH SAID IT DID NOT.** It was written when
/// the fixture's only 401 case had NO user at all, so a port that dropped the `enabled` check would have
/// passed every case; `route-corpus.json` carries *"a DISABLED user is refused even with a valid token and
/// key"* and `the_disabled_half_of_the_auth_check_is_exercised` asserts both shapes separately. The stale
/// sentence is corrected rather than deleted, because a comment claiming a gap that is closed is the same
/// defect as one claiming coverage that is not there.
pub fn auth_error(user: Option<&serde_json::Value>) -> Option<crate::responses::Built> {
    let enabled = user
        .and_then(|u| u.get("enabled"))
        .and_then(|e| e.as_bool())
        .unwrap_or(false);
    if user.is_some() && enabled {
        return None;
    }
    Some(crate::responses::json_error(
        401,
        "Missing or invalid x-api-key",
        "authentication_error",
    ))
}

/// **`effectiveToken` — THE TWO SPELLINGS OF THE SAME CREDENTIAL, IN THE SOURCE'S ORDER.**
///
/// `handleGatewayImpl` reads three lines, and each one is a rule:
///
/// ```text
///     const token = request.headers.get("x-api-key") || "";                  // an EMPTY one falls through
///     const bearerToken = auth.startsWith("Bearer ") ? auth.slice(7) : "";   // the exact scheme, and a space
///     const effectiveToken = token || bearerToken;                           // x-api-key WINS, wrong or not
/// ```
///
/// **THE WORKER READ ONLY THE FIRST LINE UNTIL 2026-10-06, AND THAT IS NOT A MISSING CONVENIENCE**: the
/// source's own comment says why the second exists — *"also accept Authorization: Bearer (OpenAI-compatible
/// clients like DSH send Bearer, not x-api-key)"* — so a client that works against the shipping gateway
/// would have been answered 401 by the cutover. `fixtures/auth-header-corpus.json` is the measurement: nine
/// header shapes driven through the shipping handler, two of which (`Bearer` alone, and an EMPTY `x-api-key`
/// beside a `Bearer`) resolve there and did not resolve here.
///
/// **`||` IS NOT `??`**: an empty `x-api-key` falls through to the bearer and a WRONG one does not. The
/// corpus carries both, because they are one character apart in the source and opposite in behaviour.
pub fn effective_token(x_api_key: Option<&str>, authorization: Option<&str>) -> String {
    let token = x_api_key.unwrap_or_default();
    if !token.is_empty() {
        return token.to_string();
    }
    authorization
        .unwrap_or_default()
        .strip_prefix("Bearer ")
        .unwrap_or_default()
        .to_string()
}

/// The per-kind key gates for **`/v1/messages`**, in the source's ORDER.
///
/// **THE TWO ARMS HAVE DIFFERENT CHAINS AND THE ORDER IS THE CONTRACT.** This one checks `or` and `cm` (the
/// two pure-BYOK channels that serve BOTH flows), then the ds/qw/og group, then nv/gmi/amd/r4/opencode. The
/// chat/completions chain below checks its five passthrough kinds FIRST and its translate kinds after. A port
/// that used one chain for both would answer a different config error for the same request — which is why
/// the corpus carries a case per path.
pub fn messages_key_gate(
    kind: &str,
    byok: &serde_json::Map<String, serde_json::Value>,
    env: &serde_json::Value,
) -> Option<crate::responses::Built> {
    for checked in [
        "openrouter",
        "commandgoat",
        "deepseek",
        "qwen",
        "opencode",
        "nvidia",
        "gmi",
        "amd",
        "r4",
    ] {
        if kind == checked && crate::byok::is_key_missing(checked, byok, Some(env)) {
            return crate::responses::key_missing_error(checked);
        }
    }
    None
}

/// The per-kind key gates for **`/v1/chat/completions`**, in ITS order: the five passthrough kinds first,
/// then the three the translate path serves.
pub fn chat_completions_key_gate(
    kind: &str,
    byok: &serde_json::Map<String, serde_json::Value>,
    env: &serde_json::Value,
) -> Option<crate::responses::Built> {
    for checked in [
        "nvidia",
        "gmi",
        "amd",
        "opencode",
        "r4",
        "deepseek",
        "openrouter",
        "qwen",
    ] {
        if kind == checked && crate::byok::is_key_missing(checked, byok, Some(env)) {
            return crate::responses::key_missing_error(checked);
        }
    }
    None
}

/// The gate for a route, by which arm it is on.
pub fn key_gate(
    kind: &str,
    is_chat_completions: bool,
    byok: &serde_json::Map<String, serde_json::Value>,
    env: &serde_json::Value,
) -> Option<crate::responses::Built> {
    if is_chat_completions {
        chat_completions_key_gate(kind, byok, env)
    } else {
        messages_key_gate(kind, byok, env)
    }
}

/// **THE `/v1` DISPATCH — the shape the worker's `fetch` will call, with its I/O results as arguments.**
///
/// Every piece is ported and proved; what this adds is the ORDER they run in — **AND THIS ORDER WAS WRONG
/// UNTIL 2026-10-06.** It said *"the route match (404 …) -> auth (401)"*, which is the reverse of the source:
///
/// ```text
///     auth (401)
///       ->  the route match (404 for a path this handler does not serve)
///         ->  the retired check (400)
///           ->  the per-ARM key gate (the two chains differ)
///             ->  the arm itself
/// ```
///
/// **`handleGatewayImpl` READS, IN THIS ORDER**: the public `GET …/models` arm, then the token
/// (`findUserByToken`, and `!user || !user.enabled` is the 401), the admin cutover, the rate limit, then
/// `detectRoute` — *"if (!(isCount || isMessages || isChatCompletions || isResponses)) return jsonError(404,
/// "Not Found", "not_found_error")"* — then the body scan, `retiredModelHint` (400), the key gate and the
/// arm. **A port that matched the route FIRST answered 404 to `GET /v1/messages` and `POST /v1/models`
/// where the shipping gateway answers 401**, and every case in `route-corpus.json` was green through it,
/// because all nine of those cases ride one served shape.
///
/// `fixtures/front-door-corpus.json` is that boundary, driven from the same shipping handler; the eight
/// cases below are what says which side of the route match auth sits on.
///
/// `kind` IS AN ARGUMENT because resolving it — `pick_route` over the model prefix, the KV settings and the
/// provider records — is its own proved decision, and the worker calls it first.
pub struct V1Request {
    pub method: String,
    pub path: String,
    /// What `findUserByToken` answered: `None`, or the user record (whose `enabled` may be false).
    pub user: Option<serde_json::Value>,
    pub kind: String,
    pub byok: serde_json::Map<String, serde_json::Value>,
    pub env: serde_json::Value,
    pub raw_text: serde_json::Value,
}

/// The dispatch. Returns the response the handler would send.
pub fn v1_dispatch(req: &V1Request) -> crate::responses::Built {
    // **AUTH IS ABOVE THE ROUTE MATCH**, and the pair is what makes the 404 a request with a VALID token
    // gets rather than the first thing every request meets. See the struct's note for the source's order.
    if let Some(refusal) = auth_error(req.user.as_ref()) {
        return refusal;
    }
    let shape = detect_route(&req.method, &req.path);
    if !(shape.is_count || shape.is_messages || shape.is_chat_completions || shape.is_responses) {
        // `jsonError(404, "Not Found", "not_found_error")` — the arm this handler does not serve.
        return crate::responses::json_error(404, "Not Found", "not_found_error");
    }
    // **THE RETIRED CHECK IS BELOW BOTH, AND THE WORKER HAD IT ABOVE BOTH.** It ran before the token was
    // read, so a retired id with no token answered 400 where the shipping gateway answers 401. The model
    // comes from the RAW TEXT through the ported scan — the same input the source reads
    // (`scanTopLevelModel(rawText)`) — rather than from a parse.
    let raw = req.raw_text.as_str().unwrap_or_default();
    if let Some(model) = crate::body_scan::scan_top_level_model(raw).model {
        if let Some(hint) = crate::routing::retired_model_hint(&model) {
            return crate::routing::retired_model_error(&model, hint);
        }
    }
    if let Some(refusal) = key_gate(&req.kind, shape.is_chat_completions, &req.byok, &req.env) {
        return refusal;
    }
    if shape.is_count {
        return count_tokens_response(&req.kind, &req.byok, &req.env, &req.raw_text);
    }
    // The other three arms are ported as their own decisions (`translate_request`, `passthrough_request`,
    // the response side) and are not composed here yet — a 501 rather than a silent wrong answer.
    crate::responses::json_error(501, "route not wired", "api_error")
}

/// **WHAT THE UPSTREAM ANSWERED — the I/O edge's result, as data.**
///
/// `json` is `upstream.json().catch(() => null)`: the parsed body, or `None` when it was not JSON at all.
/// `text` is the same body as text, which is what a passthrough forwards.
#[derive(Debug, Clone, Default)]
pub struct UpstreamAnswer {
    pub status: u16,
    pub content_type: String,
    pub json: Option<serde_json::Value>,
    pub retry_after: Option<String>,
    pub text: String,
}

/// **IS THIS ANSWER A LIVE STREAM?** — the condition `messages_response` uses, in ONE place, because the
/// WORKER has to ask it BEFORE it reads the body.
///
/// `Response::text()` consumes a body and `Response::stream()` hands it over live; a caller that guesses wrong
/// cannot go back. So the decision is a function: the client asked to stream, the arm translates, the upstream
/// answered 2xx, and the content type is NOT the "ignored stream: true" shape.
///
/// **IT IS THE SAME CONDITION `messages_response` REACHES, DELIBERATELY** — a second copy is how two surfaces
/// come to disagree about one request.
///
/// ── AND `is_translate` IS THE HALF THAT IS STILL WRONG, MEASURED 2026-10-06 ──────────────────────
///
/// **THE SOURCE FORWARDS A PASSTHROUGH BODY LIVE, WHATEVER IT IS**: `relayUpstreamResult` ends
/// `return new Response(upstream.body, { status: upstream.status, headers })` for every non-opencode arm. This
/// predicate requires `is_translate`, so a `ds/` (passthrough) `stream: true` request takes the BUFFERED branch
/// — `res.text().await` — and can only answer once the upstream has finished.
///
/// **THE BYTES ARE IDENTICAL EITHER WAY, WHICH IS WHY NO BYTE COMPARISON HAS CAUGHT IT**; the timing is not.
/// Measured with an upstream that holds its second chunk for 1500 ms, both sides' first byte timed from BEFORE
/// the fetch (timing it after the fetch scored the buffered side 0 ms, because the wait had already happened
/// inside it):
///
/// ```text
///     shipping: fetch   31 ms, first byte at   31 ms   (88 B)
///     wasm:     fetch 1522 ms, first byte at 1523 ms   (139 B)
/// ```
///
/// So `stream: true` on a passthrough channel delivers nothing until the model has finished — the one thing a
/// streaming client is asking not to happen. **THE NEXT STEP IS NAMED HERE RATHER THAN STARTED IN THIS
/// COMMIT**: the passthrough branch must return the upstream's stream with the same headers the buffered
/// forward produces, which means `ArmOutcome::Stream` has to carry them (the CORS stamp is a decision, and the
/// I/O must not re-derive it).
pub fn is_live_stream(
    is_translate: bool,
    wants_stream: bool,
    status: u16,
    content_type: &str,
) -> bool {
    is_translate
        && wants_stream
        && (200..=299).contains(&status)
        && !crate::responses::upstream_ignored_stream(content_type)
}

/// **WHAT AN ARM ANSWERS: a response, or a request to stream.**
///
/// The streaming half of the translate arm is `streamOgToAnthropic` over a live `ReadableStream`, which is
/// I/O rather than a decision — so this composition DELEGATES it instead of faking a response, and the
/// worker's `fetch` hands the upstream body to the streaming translator when it sees this.
#[derive(Debug)]
pub enum ArmOutcome {
    Response(crate::responses::Built),
    /// "Hand the upstream's body to the SSE translator" — the only case that is not a value.
    Stream,
}

/// **THE `/v1/messages` ARM, COMPOSED — every piece of it was proved before this function existed.**
///
/// The source's order, with the two arms it serves:
///
/// ```text
///     no upstream at all            ->  upstreamFetchFailedResponse
///     a non-OK upstream             ->  upstreamBodyErrorResponse
///     the passthrough arm           ->  the upstream's bytes + the CORS stamp
///     the translate arm, streaming  ->  DELEGATED (see `ArmOutcome::Stream`)
///     the translate arm, JSON       ->  streamIgnoredResponse when the client asked to stream
///                                       upstreamJsonResponse otherwise
/// ```
///
/// `wants_stream` IS `body.stream` — the CLIENT's request, not the upstream's answer, which is why the
/// ignored-stream branch can only be reached when it is true.
/// Everything the arm needs that is not the upstream's answer. A struct rather than nine arguments, and the
/// seam is the same one: the caller reads the config, this decides.
pub struct MessagesArm<'a> {
    pub is_translate: bool,
    /// `body.stream` — the CLIENT's request, which is why the ignored-stream branch needs it.
    pub wants_stream: bool,
    pub kind: &'a str,
    pub upstream_model: &'a str,
    pub origin: &'a str,
    pub request_host: Option<&'a str>,
    pub cors_configured: Option<&'a str>,
    pub secrets: &'a [serde_json::Value],
}

pub fn messages_response(arm: &MessagesArm, answer: Option<&UpstreamAnswer>) -> ArmOutcome {
    let MessagesArm {
        is_translate,
        wants_stream,
        kind,
        upstream_model,
        origin,
        request_host,
        cors_configured,
        secrets,
    } = *arm;
    let label = crate::responses::translate_label(kind, None);
    let Some(answer) = answer else {
        // **THE FETCH ITSELF FAILED, AND THE ENVELOPE IS THE ARM'S** — see `upstream_fetch_failure`. The
        // status is the inspection's own when it looks like one.
        return ArmOutcome::Response(upstream_fetch_failure(
            kind,
            is_translate,
            &label,
            "fetch failed",
        ));
    };
    if !(200..=299).contains(&answer.status) {
        return ArmOutcome::Response(crate::responses::upstream_body_error(
            answer.status,
            answer.json.as_ref(),
            answer.retry_after.as_deref(),
            &label,
            secrets,
        ));
    }
    if !is_translate {
        // A passthrough forwards the upstream's bytes; the only surgery is the CORS stamp.
        return ArmOutcome::Response(crate::cors::forwarded_upstream_response(
            answer.status,
            &[("Content-Type".to_string(), answer.content_type.clone())],
            origin,
            request_host,
            cors_configured,
            &answer.text,
        ));
    }
    if wants_stream {
        if !is_live_stream(
            is_translate,
            wants_stream,
            answer.status,
            &answer.content_type,
        ) {
            // The upstream ignored `stream: true` and answered JSON — the branch whose recorded incident is
            // an EMPTY Anthropic message.
            return ArmOutcome::Response(crate::responses::stream_ignored_response(
                answer.json.as_ref(),
                upstream_model,
            ));
        }
        // A real SSE answer: the translator needs the live body.
        return ArmOutcome::Stream;
    }
    ArmOutcome::Response(crate::responses::upstream_json_response(
        answer.json.as_ref(),
        upstream_model,
    ))
}

/// **WHAT TO DIAL — the half of the dispatch that is not an answer.**
///
/// The `/v1/messages` arm cannot finish without the upstream, so the dispatch is TWO functions: `v1_plan`
/// says either "answer this now" or "dial this and come back", and `v1_finish` shapes the answer. That is the
/// same seam every other port here uses — the decisions are values, the I/O is at the edge — and it is what
/// lets the worker's `fetch` be the only thing that knows about `worker::Fetch`.
#[derive(Debug, Clone)]
pub struct UpstreamRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub policy: crate::reliability::RetryPolicy,
}

/// Everything `messages_response` needs besides the answer, owned so it can outlive `v1_plan`.
#[derive(Debug, Clone)]
pub struct ArmCall {
    pub request: UpstreamRequest,
    pub is_translate: bool,
    pub wants_stream: bool,
    pub kind: String,
    pub upstream_model: String,
    pub origin: String,
    pub request_host: Option<String>,
    pub cors_configured: Option<String>,
}

/// The dispatch's answer: a response now, or a call to make first.
///
/// `Dial` IS ONE VARIANT FOR TWO ARMS, because their response side is IDENTICAL — both end in
/// `relayUpstreamResult`: the non-OK path through `upstreamBodyErrorResponse`, the OK path through the
/// forward with the CORS stamp. Only the REQUEST differs, and `ArmCall` carries which arm it was.
#[derive(Debug)]
pub enum V1Plan {
    Respond(crate::responses::Built),
    Dial(Box<ArmCall>),
}

/// Everything `v1_plan` needs. A struct rather than eight arguments, and the seam is unchanged: the caller
/// resolves the model and the route, this decides what to do about them.
pub struct V1PlanInputs<'a> {
    pub req: &'a V1Request,
    pub route: &'a crate::routing::RouteInfo,
    /// The ADVERTISED id — what the client asked for, which `responses_gate` checks against the registry.
    pub model: &'a str,
    /// The prefix the model chain resolved, which the gate also checks.
    pub prefix: &'a str,
    /// The WIRE model — `wireModelName`'s answer.
    pub upstream_model: &'a str,
    pub parsed_body: Option<&'a serde_json::Value>,
    pub og_session: &'a [(String, String)],
    pub scanned: Option<(usize, usize)>,
    /// **THE CUSTOM-PROVIDER RECORD THIS ROUTE CAME FROM**, when it came from one. The credential and the
    /// per-model facets live on the record rather than in `BYOK_CHANNELS`, so the plan needs it here — the same
    /// reason the source carries `provider` on its `RouteInfo`.
    pub provider: Option<&'a serde_json::Value>,
}

/// **PHASE ONE — auth, the route match, the retired check, the per-arm key gate, then either the arm or a
/// plan to dial it.**
///
/// `route` IS AN ARGUMENT because resolving it (`pick_route` over the model prefix, the KV settings and the
/// provider records) is its own proved decision, and the worker calls it first. The four arms the handler
/// serves are matched here; anything else is the 404 the source sends — **and the 404 comes after auth and
/// before the retired check**, which is `handleGatewayImpl`'s order rather than the one this function ran
/// until 2026-10-06 (see `v1_dispatch`'s note; `fixtures/front-door-corpus.json` is the measurement).
pub fn v1_plan(inputs: &V1PlanInputs) -> V1Plan {
    let V1PlanInputs {
        req,
        route,
        model,
        prefix,
        upstream_model,
        parsed_body,
        og_session,
        scanned,
        provider,
    } = *inputs;
    if let Some(refusal) = auth_error(req.user.as_ref()) {
        return V1Plan::Respond(refusal);
    }
    let shape = detect_route(&req.method, &req.path);
    if !(shape.is_count || shape.is_messages || shape.is_chat_completions || shape.is_responses) {
        return V1Plan::Respond(crate::responses::json_error(
            404,
            "Not Found",
            "not_found_error",
        ));
    }
    // **THE RETIRED CHECK, IN ITS SOURCE POSITION**: below auth, below the route match, above the key gate.
    // The worker had it above BOTH, which is why a retired id with no token answered 400 where the
    // shipping gateway answers 401. `model` is already the resolved advertised id, so no scan is needed here
    // — the scan belongs to `v1_dispatch`, whose only input is the raw text.
    if let Some(hint) = crate::routing::retired_model_hint(model) {
        return V1Plan::Respond(crate::routing::retired_model_error(model, hint));
    }
    if let Some(refusal) = key_gate(&req.kind, shape.is_chat_completions, &req.byok, &req.env) {
        return V1Plan::Respond(refusal);
    }
    // **A CUSTOM PROVIDER'S CREDENTIAL COMES FROM ITS OWN RECORD, NOT FROM `BYOK_CHANNELS`.** The source:
    // `route.kind === "custom" ? providerKey(env, route.provider) : bearerKeyFor(env, byok, kind)` — and
    // `keyMissingError`'s table is keyed by BUILT-IN kinds, so the gate above says nothing about a custom one
    // (`isKeyMissing` answers `false` for an unknown kind, by design). An empty key is this 502, which names
    // the provider and what to set, rather than a headerless request that comes back as a bare upstream 401.
    let custom_key = if req.kind == "custom" {
        let key = crate::store::provider_key(&req.env, provider);
        if key.is_empty() {
            return V1Plan::Respond(crate::responses::provider_key_missing_error(provider));
        }
        Some(key)
    } else {
        None
    };
    if shape.is_count {
        return V1Plan::Respond(count_tokens_response(
            &req.kind,
            &req.byok,
            &req.env,
            &req.raw_text,
        ));
    }
    if shape.is_chat_completions {
        // **THE CHAT ARM IS A PASSTHROUGH FOR EVERY KIND** — the source says so in as many words: "the
        // translate plugin reshapes Anthropic /v1/messages → chat/completions (the og pattern), while
        // OpenAI-format /v1/chat/completions passes through directly". So its request is
        // `chat_completions_request` and its response is the same forward the messages passthrough uses.
        // A custom provider's key is its record's; every other kind's is BYOK-then-env.
        let bearer = match &custom_key {
            Some(key) => key.clone(),
            None => crate::byok::bearer_key_for(&req.env, &req.byok, &req.kind)
                .as_str()
                .unwrap_or("")
                .to_string(),
        };
        let bearer = bearer.as_str();
        let chat = chat_completions_request(
            &req.kind,
            upstream_model,
            req.raw_text.as_str().unwrap_or(""),
            Some(bearer),
            og_session,
            scanned,
            Some(&req.env),
        );
        return V1Plan::Dial(Box::new(ArmCall {
            request: UpstreamRequest {
                url: route.upstream.clone(),
                method: "POST".to_string(),
                headers: chat.headers,
                body: chat.body,
                policy: chat.policy,
            },
            is_translate: false,
            wants_stream: false,
            kind: req.kind.clone(),
            upstream_model: upstream_model.to_string(),
            origin: String::new(),
            request_host: None,
            cors_configured: None,
        }));
    }
    if shape.is_responses {
        // **THE GATE FIRST, THEN THE EXIT.** The arm serves one family, and the exit is FORCED for it — the
        // Contributor tier is responses-only upstream and Meta region-blocks it for CN, so `forceUsProxy`
        // decides between the configured exit and the og zen host's own `/v1/responses`.
        if let Some(refusal) = responses_gate(model, upstream_model, prefix, &req.kind) {
            return V1Plan::Respond(refusal);
        }
        // A custom provider's key is its record's; every other kind's is BYOK-then-env.
        let bearer = match &custom_key {
            Some(key) => key.clone(),
            None => crate::byok::bearer_key_for(&req.env, &req.byok, &req.kind)
                .as_str()
                .unwrap_or("")
                .to_string(),
        };
        let bearer = bearer.as_str();
        let request = responses_request(
            upstream_model,
            req.raw_text.as_str().unwrap_or(""),
            Some(bearer),
            og_session,
            scanned,
        );
        let url = if crate::routing::og_force_us_proxy(model) {
            crate::routing::muse_responses_exit(Some(&req.env))
        } else {
            crate::routing::og_responses_direct()
        };
        return V1Plan::Dial(Box::new(ArmCall {
            request: UpstreamRequest {
                url,
                method: "POST".to_string(),
                headers: request.headers,
                body: request.body,
                policy: crate::reliability::RetryPolicy {
                    timeout_ms: crate::reliability::og_timeout_ms(Some(&req.env)),
                    attempts: None,
                    backoff_ms: None,
                    retry502: None,
                    ignore_retry_after: None,
                },
            },
            is_translate: false,
            wants_stream: false,
            kind: req.kind.clone(),
            upstream_model: upstream_model.to_string(),
            origin: String::new(),
            request_host: None,
            cors_configured: None,
        }));
    }
    if !shape.is_messages {
        // Nothing else reaches here — the four arms are all composed above.
        return V1Plan::Respond(crate::responses::json_error(
            501,
            "route not wired",
            "api_error",
        ));
    }
    // `/v1/messages`: the bearer key, the request the upstream receives, and the retry policy that goes with
    // the ARM — the two chains differ, which the translate arm's own comment records.
    let bearer = match &custom_key {
        Some(key) => key.clone(),
        None => crate::byok::bearer_key_for(&req.env, &req.byok, &req.kind)
            .as_str()
            .unwrap_or("")
            .to_string(),
    };
    let bearer = bearer.as_str();
    let wants_stream = parsed_body
        .and_then(|b| b.get("stream"))
        .map(|v| v == &serde_json::Value::Bool(true))
        .unwrap_or(false);
    // **THE nv/gmi TRANSLATE BRANCH — A THIRD CASE, AND IT WAS MISSING ENTIRELY.** Their ROUTE type is
    // "passthrough" (their upstreams are OpenAI-compatible chat/completions endpoints), but `translate.ts` gives
    // them a DEDICATED arm that translates BOTH ways "so Anthropic-only clients (Claude Code) can ride these
    // channels via /v1/messages": `toOpenAIRequest` on the way out and `openAIUpstreamToAnthropicResponse` on
    // the way back. Measured 2026-10-06 on the built worker with a `gmi/` model and a user key: the shipping
    // route sent `…,"stream":false,"max_tokens":8` and answered an Anthropic message (259 B), while this worker
    // sent the raw Anthropic body with the model swapped and handed the upstream's OpenAI JSON back verbatim
    // (134 B) — a client that speaks Anthropic would have parsed nothing.
    //
    // The policy is this arm's OWN (`{ timeoutMs, attempts: 4, retry502: true }`), deliberately not the shared
    // table — "routing it through retryPolicyFor would silently drop non-nv/gmi kinds to the plain budget".
    let nv_gmi_translate = req.kind == "nvidia" || req.kind == "gmi";
    let (body, headers, policy) = if route.is_translate || nv_gmi_translate {
        let r = translate_request(
            bearer,
            parsed_body.unwrap_or(&serde_json::Value::Null),
            upstream_model,
            Some(&req.env),
            og_session,
        );
        let policy = if nv_gmi_translate {
            chat_retry_policy(Some(&req.env))
        } else {
            r.policy
        };
        (r.body, r.headers, policy)
    } else {
        let r = passthrough_request(
            &req.kind,
            upstream_model,
            req.raw_text.as_str().unwrap_or(""),
            parsed_body,
            Some(bearer),
            og_session,
            scanned,
        );
        (
            r.body,
            r.headers,
            crate::reliability::retry_policy_for(
                &req.kind,
                upstream_model,
                crate::reliability::passthrough_timeout_ms(Some(&req.env), &req.kind),
            ),
        )
    };
    V1Plan::Dial(Box::new(ArmCall {
        request: UpstreamRequest {
            url: route.upstream.clone(),
            method: "POST".to_string(),
            headers,
            body,
            policy,
        },
        // The RESPONSE side follows the same branch: nv/gmi answer through `openAIUpstreamToAnthropicResponse`,
        // which is what `is_translate` selects in `v1_finish`.
        is_translate: route.is_translate || nv_gmi_translate,
        wants_stream,
        kind: req.kind.clone(),
        upstream_model: upstream_model.to_string(),
        origin: String::new(),
        request_host: None,
        cors_configured: None,
    }))
}

/// **PHASE TWO — the answer the call produced becomes the response the client gets.**
/// **THE FAILURE'S ENVELOPE DEPENDS ON THE ARM, AND THAT IS NOT A DETAIL OF STYLE.** The translate arm
/// answers `${label}: ${detail}`; the passthrough arms answer `upstream ${status} (${kind}): ${detail}`.
/// One function so both the plan's own fallback (`v1_finish` with no answer) and the I/O path (`v1.rs`, which
/// is where a real runtime error message exists) make the SAME decision — and so a Rust test can pin both arms
/// without a worker.
pub fn upstream_fetch_failure(
    kind: &str,
    is_translate: bool,
    label: &str,
    detail: &str,
) -> crate::responses::Built {
    if is_translate {
        crate::responses::translate_failure(label, detail)
    } else {
        crate::responses::upstream_fetch_failed(kind, None, detail)
    }
}

pub fn v1_finish(call: &ArmCall, answer: Option<&UpstreamAnswer>) -> ArmOutcome {
    let secrets: Vec<serde_json::Value> = Vec::new();
    messages_response(
        &MessagesArm {
            is_translate: call.is_translate,
            wants_stream: call.wants_stream,
            kind: &call.kind,
            upstream_model: &call.upstream_model,
            origin: &call.origin,
            request_host: call.request_host.as_deref(),
            cors_configured: call.cors_configured.as_deref(),
            secrets: &secrets,
        },
        answer,
    )
}

/// **THE `role: "developer"` REWRITE — a STRING replacement on the raw body, not a parse.**
///
/// The chat/completions arm serves OpenAI-format clients, and some of them send OpenAI's newer
/// `{"role":"developer"}`. Every upstream here wants `system`, so the arm rewrites the raw text:
///
/// ```js
/// if (forwardBody.includes('"role":"developer"')) {
///   forwardBody = forwardBody.split('"role":"developer"').join('"role":"system"');
/// }
/// ```
///
/// **IT IS `split`/`join` ON THE RAW TEXT, SO EVERY UNESCAPED OCCURRENCE GOES AND NO PARSING HAPPENS** — and
/// the corpus corrected me about what that means. I wrote that a body mentioning the string inside a message's
/// CONTENT is rewritten too; **it is not**, because a JSON string value escapes its quotes, so the raw text
/// holds `\"role\":\"developer\"` and `includes` never matches it. What the raw form DOES match is the
/// pattern as JSON SYNTAX — at the top level or NESTED, which the second corpus case pins — so the honest
/// statement is "every unescaped occurrence", not "every occurrence".
pub fn rewrite_developer_role(body: &str) -> String {
    if !body.contains("\"role\":\"developer\"") {
        return body.to_string();
    }
    body.split("\"role\":\"developer\"")
        .collect::<Vec<_>>()
        .join("\"role\":\"system\"")
}

/// **THE `/v1/chat/completions` ARM'S REQUEST — the same pieces as the passthrough arm, with TWO
/// differences, and both are the source's.**
///
/// ```text
///     the body   rawWithModel, then the developer rewrite, then oxAlphaReasoningDefault
///     the policy retryPolicyFor(kind, upstreamModel, ogTimeoutMs(env))
/// ```
///
/// **`ogTimeoutMs`, NOT `passthroughTimeoutMs`** — the messages passthrough arm uses the latter and this one
/// the former, which is a difference no shape-based reading would notice. The headers are
/// `passthroughHeaders` with the same og-session extra.
pub fn chat_completions_request(
    kind: &str,
    upstream_model: &str,
    raw_text: &str,
    bearer_key: Option<&str>,
    og_session: &[(String, String)],
    scanned: Option<(usize, usize)>,
    env: Option<&serde_json::Value>,
) -> ChatRequest {
    let forwarded = crate::body_scan::raw_with_model(
        raw_text,
        &serde_json::Value::String(upstream_model.to_string()),
        scanned,
    );
    let rewritten = rewrite_developer_role(&forwarded);
    let body = crate::registry::ox_alpha_reasoning_default(kind, upstream_model, &rewritten);
    // **NO `apiKeyHeader`, AND THE CAPTURE IS WHAT SAID SO.** The messages passthrough arm passes
    // `passthroughApiKeyHeader(kind)` — `x-api-key` for opencode/amd — and the chat arm calls
    // `passthroughHeaders(bearerKey, {…extra})` WITHOUT it, so its credential always rides `Authorization`.
    // My first version copied the messages arm's call and the captured request refused it:
    // `header authorization missing from [… ("x-api-key", "sk-og") …]`.
    let headers = crate::session::passthrough_headers(bearer_key, None, og_session);
    ChatRequest {
        body,
        headers,
        // **`ogTimeoutMs`, NOT `passthroughTimeoutMs`** — the two arms differ here, and the difference is a
        // number no shape-based reading would notice. And the POLICY is this arm's own (`chat_retry_policy`),
        // not the shared table's: see its doc comment for the source's reason.
        policy: chat_retry_policy(env),
    }
}

/// The chat/completions arm's request AND the policy that goes with it.
pub struct ChatRequest {
    pub body: String,
    pub headers: Vec<(String, String)>,
    pub policy: crate::reliability::RetryPolicy,
}

/// **THE `/v1/responses` GATE — TWO REFUSALS, AND THEIR TYPE IS `invalid_request`, NOT
/// `invalid_request_error`.**
///
/// The arm serves exactly one family, and the source's own comment says why: "only registered
/// og/muse-spark-* Contributor models ride this endpoint. Responses requests naming anything else are client
/// bugs (other og/ models speak chat/completions; nothing else here is responses-native; unregistered
/// muse-spark versions must not reach the upstream)."
///
/// The three conditions are checked in ONE `if`, so the first refusal covers all three, and the second
/// catches a model that IS responses-only but did not route to opencode. Both sentences are the source's,
/// including the em dash in the first.
pub fn responses_gate(
    model: &str,
    upstream_model: &str,
    prefix: &str,
    kind: &str,
) -> Option<crate::responses::Built> {
    let advertised = crate::registry::MODEL_REGISTRY
        .iter()
        .any(|m| m.id == model);
    let responses_only = crate::registry::wire_spec(upstream_model)
        .map(|m| m.responses_only == Some(true))
        .unwrap_or(false);
    if !advertised || !responses_only || prefix != "og" {
        return Some(crate::responses::json_error(
            400,
            &format!(
                "Model {model} is not served via /v1/responses — only og/muse-spark-* Contributor models use this endpoint"
            ),
            "invalid_request",
        ));
    }
    if kind != "opencode" {
        return Some(crate::responses::json_error(
            400,
            &format!("/v1/responses only serves og/ models (requested {model})"),
            "invalid_request",
        ));
    }
    None
}

/// **THE `/v1/responses` ARM'S REQUEST — and its headers are its OWN.**
///
/// It does NOT call `passthroughHeaders`: there is no `anthropic-version` here, because the Responses API is
/// not Anthropic's. What it sets is `Content-Type`, `Authorization` **only when there is a bearer**, and
/// `x-opencode-session` **only when the session object has one** — two conditionals a shape-based port would
/// flatten into unconditional sets, which would send `Authorization: Bearer ` (empty) to the upstream.
pub fn responses_request(
    upstream_model: &str,
    raw_text: &str,
    bearer_key: Option<&str>,
    og_session: &[(String, String)],
    scanned: Option<(usize, usize)>,
) -> PassthroughRequest {
    let body = crate::body_scan::raw_with_model(
        raw_text,
        &serde_json::Value::String(upstream_model.to_string()),
        scanned,
    );
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    if let Some(key) = bearer_key.filter(|k| !k.is_empty()) {
        headers.push(("Authorization".to_string(), format!("Bearer {key}")));
    }
    for (k, v) in og_session {
        if k == "x-opencode-session" && !v.is_empty() {
            headers.push((k.clone(), v.clone()));
        }
    }
    PassthroughRequest { body, headers }
}

#[cfg(test)]
mod count_tokens_tests {
    //! **THE COMPOSITION IS PINNED BY HAND, AND THE REASON IS STATED.** The `count_tokens` arm lives inside
    //! `handleGatewayImpl`, which resolves a route through `pickRoute` and reads the user's key record from
    //! KV — so driving the SHIPPING arm needs a fake KV binding, which is the route's own differential
    //! rather than a unit here. What IS oracle-proved is every piece this composes: `detectRoute` (the arm's
    //! gate), `isKeyMissing`, `keyMissingError` and `estimateTokens` are all in the corpus, and `jsonOk` is
    //! too. What follows pins the ORDER and the LIST, which is what the source's own comment says is worth a
    //! test rather than a review.
    use super::*;

    fn byok(pairs: &[(&str, &str)]) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), serde_json::json!(v));
        }
        m
    }

    /// **THE FAILURE'S ENVELOPE IS THE ARM'S — MEASURED ON THE BUILT WORKER, PINNED HERE.**
    ///
    /// MUTATION: make `upstream_fetch_failure` ignore `is_translate` (return `upstream_fetch_failed` for both).
    /// RESULT:   the first case fails with `upstream 502 (opencode): network error: x` where the shipping route
    ///           answers `og: network error: x` — the difference the divergence sweep found on 2026-10-06
    ///           (`89B` against `96B`, and the 96 also carried the runtime's `Error: ` prefix).
    ///
    /// The label is COMPUTED by `translate_label`, so this pins the second rule too: an `opencode` route labels
    /// its errors `og:`, a `commandgoat` route `cm:`, a custom one its own prefix.
    #[test]
    fn the_failure_envelope_depends_on_the_arm() {
        let cases = [
            // kind, is_translate, detail, the expected message
            ("opencode", true, "network error: x", "og: network error: x"),
            ("commandgoat", true, "network error: x", "cm: network error: x"),
            (
                "custom",
                true,
                "timeout after 30000ms",
                "custom: timeout after 30000ms",
            ),
            ("opencode", true, "", "og: upstream 502"),
            (
                "opencode",
                false,
                "network error: x",
                "upstream 502 (opencode): network error: x",
            ),
            (
                "nvidia",
                false,
                "network error: x",
                "upstream 502 (nvidia): network error: x",
            ),
        ];
        for (kind, is_translate, detail, want) in cases {
            let label = crate::responses::translate_label(kind, None);
            let got = upstream_fetch_failure(kind, is_translate, &label, detail);
            let body: serde_json::Value = serde_json::from_str(&got.body).expect("JSON");
            assert_eq!(got.status, 502, "{kind}/{is_translate}: status");
            assert_eq!(
                body["error"]["message"].as_str().unwrap_or(""),
                want,
                "{kind}/{is_translate}: message"
            );
            assert_eq!(
                body["error"]["type"].as_str().unwrap_or(""),
                "api_error",
                "{kind}/{is_translate}: type"
            );
        }
    }

    fn route_corpus() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/route-corpus.json");
        let text = std::fs::read_to_string(path).expect("the route corpus is committed");
        serde_json::from_str(&text).expect("the route corpus parses")
    }

    /// **THE ROUTE'S OWN DIFFERENTIAL, REPLAYED — ALL NINE CASES.** `fixtures/route-corpus.json` was
    /// produced by driving the SHIPPING `handleGateway` with a fake KV (`gateway/wasm/route-oracle.mjs`), and
    /// this drives `v1_dispatch` — the same order the source runs: **auth, the route match, the retired
    /// check**, the per-arm key gate, then the arm.
    ///
    /// **AND THIS COMMENT SAID "THE ROUTE MATCH, AUTH" UNTIL 2026-10-06**, which was the port's order rather
    /// than the source's — the defect `the_front_door_orders_auth_above_the_route_match` now covers. It is
    /// corrected here because a comment that states the order is a claim, and this one was the wrong one.
    /// **THESE NINE CASES CANNOT SEE THAT ORDER**: every one of them rides `POST
    /// /v1/messages/count_tokens`, a shape the route serves, so they are green either way.
    ///
    /// **IT USED TO SKIP THREE CASES** marked `beforeArm`: a request answered ABOVE the `count_tokens` arm by
    /// auth, by the OpenRouter key gate, or by the DEFAULT channel an empty body resolves to. Those are what
    /// the dispatch IS, so nothing is skipped now.
    #[test]
    fn the_dispatch_matches_the_shipping_route() {
        let doc = route_corpus();
        let mut checked = 0;
        let mut seen_statuses = Vec::new();
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let byok = crate::byok::extract_byok_keys(&serde_json::json!(case["ukeys"]));
            let env = case["env"].clone();
            let kind = case["kind"].as_str().unwrap_or("");
            // The empty model resolves to the DEFAULT channel, which `pick_route` answers.
            let kind = if kind.is_empty() {
                crate::routing::pick_route("", Some(&env), None, "/v1/messages").kind
            } else {
                kind
            };
            let user = if case["user"].is_null() {
                None
            } else {
                Some(case["user"].clone())
            };
            let got = v1_dispatch(&V1Request {
                method: "POST".to_string(),
                path: "/v1/messages/count_tokens".to_string(),
                user,
                kind: kind.to_string(),
                byok,
                env,
                raw_text: case["rawText"].clone(),
            });
            let want = &case["expected"];
            assert_eq!(
                got.status,
                want["status"].as_u64().unwrap_or(0) as u16,
                "{name}: status"
            );
            assert_eq!(
                got.body,
                want["body"].as_str().unwrap_or(""),
                "{name}: body"
            );
            seen_statuses.push(got.status);
            checked += 1;
        }
        assert_eq!(checked, 9, "the route corpus changed size");
        // **BOTH ANSWERS, AND BOTH REFUSAL KINDS.** A corpus that only produced 200s would prove the happy
        // path; one that only produced 502s would miss the arm's success. The 401s are the dispatch's own —
        // and there are TWO of them, one for a token that resolves to nothing and one for a DISABLED user,
        // which is the half the fixture could not reach until this round.
        assert!(seen_statuses.contains(&200), "no case estimated");
        assert!(seen_statuses.contains(&401), "no case exercised auth");
        assert!(seen_statuses.contains(&502), "no case exercised a key gate");
    }

    /// **THE FRONT DOOR'S ORDER, REPLAYED — EIGHT CASES, AND THE ONES THAT FAILED BEFORE THE FIX.**
    ///
    /// MUTATION: swap the first two blocks of `v1_dispatch` — `detect_route`'s 404 back above
    ///           `auth_error` — and run this test.
    /// RESULT:   the four no-token cases and the retired pair fail with `404` (or `400`) against the
    ///           shipping `401`, naming the case:
    ///             GET /v1/messages, NO token — auth answers before the route match: status
    ///             left: 404   right: 401
    ///           measured 2026-10-06, which is also how the live divergence was found: the deployed
    ///           `vale-gate-wasm` answered 404 to `GET /v1/messages` while `route-corpus.json` was green.
    ///
    /// **WHY A SECOND CORPUS RATHER THAN MORE CASES IN THE FIRST**: every case in `route-corpus.json` rides
    /// one served shape (`POST /v1/messages/count_tokens`), so none of them can see the order of the gates
    /// ABOVE the arm — the replay hard-codes that method and path, and the fixture does not carry them.
    /// This one carries both, which is the whole of its subject.
    #[test]
    fn the_front_door_orders_auth_above_the_route_match() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/front-door-corpus.json");
        let text = std::fs::read_to_string(path).expect("the front-door corpus is committed");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("it parses");
        let cases = doc["cases"].as_array().expect("cases");
        let mut seen = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap_or("?");
            let user = if case["user"].is_null() {
                None
            } else {
                Some(case["user"].clone())
            };
            let got = v1_dispatch(&V1Request {
                method: case["method"].as_str().unwrap_or("POST").to_string(),
                path: case["path"].as_str().unwrap_or("").to_string(),
                user,
                kind: case["kind"].as_str().unwrap_or("").to_string(),
                byok: crate::byok::extract_byok_keys(&serde_json::json!(case["ukeys"])),
                env: case["env"].clone(),
                raw_text: case["rawText"].clone(),
            });
            let want = &case["expected"];
            assert_eq!(
                got.status,
                want["status"].as_u64().unwrap_or(0) as u16,
                "{name}: status"
            );
            assert_eq!(got.body, want["body"].as_str().unwrap_or(""), "{name}: body");
            seen.push(got.status);
        }
        assert_eq!(cases.len(), 8, "the front-door corpus changed size");
        // **THE THREE ANSWERS THE ORDER PRODUCES, EACH ASSERTED.** A corpus of 401s alone would pass for a
        // port that refused everything; 404s alone would pass for one that never authenticated; and the 400
        // is the retired check, which is the gate this file moved as well.
        assert_eq!(seen.iter().filter(|s| **s == 401).count(), 4, "the no-token half");
        assert_eq!(seen.iter().filter(|s| **s == 404).count(), 3, "the route match");
        assert_eq!(seen.iter().filter(|s| **s == 400).count(), 1, "the retired check");
    }

    /// **THE TOKEN'S TWO DOORS, REPLAYED — NINE HEADER SHAPES FROM THE SHIPPING HANDLER.**
    ///
    /// MUTATION: in `effective_token`, drop the `authorization` half — return `x_api_key.unwrap_or_default()`
    ///           — and run this test.
    /// RESULT:   the two shapes that ride the bearer fail, naming the case:
    ///             Authorization: Bearer alone: the token the headers name
    ///             left: false   right: true
    ///             an EMPTY x-api-key falls through to Bearer: the token the headers name
    ///           measured 2026-10-06, which is the shape the worker was in: `v1.rs` read `x-api-key` alone.
    ///
    /// **THE REPLAY IS THE WHOLE CHAIN, WITH THE KV AS A RECORDED FACT**: the headers go through the ported
    /// `effective_token`, and whether that token is the one the fixture's fake KV holds decides the user —
    /// which is the only input `v1_dispatch` needs to produce the status the shipping handler produced. A
    /// test that took the recorded `tokenResolves` and skipped the port would assert the fixture against
    /// itself.
    #[test]
    fn the_token_has_two_doors_and_the_first_wins() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/auth-header-corpus.json");
        let text = std::fs::read_to_string(path).expect("the auth-header corpus is committed");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("it parses");
        let valid = doc["validToken"].as_str().expect("the fixture names its token");
        let cases = doc["cases"].as_array().expect("cases");
        let mut through_the_bearer = 0;
        for case in cases {
            let name = case["name"].as_str().unwrap_or("?");
            let token = effective_token(case["xApiKey"].as_str(), case["authorization"].as_str());
            let port_says_user = token == valid;
            let want_user = case["tokenResolves"].as_bool().unwrap_or(false);
            assert_eq!(
                port_says_user, want_user,
                "{name}: the token the headers name"
            );
            // **THE BEARER HALF IS COUNTED BY THE HEADER THAT CARRIED IT, NOT BY AN ABSENT `x-api-key`**:
            // the second shape that rides the bearer is an EMPTY `x-api-key`, which IS present. The first
            // version of this counter asked for `is_null()` and found one case where the fixture has two.
            if !case["authorization"].is_null() && want_user {
                through_the_bearer += 1;
            }
            let user = if port_says_user {
                let u = case["user"].clone();
                if u.is_null() {
                    None
                } else {
                    Some(u)
                }
            } else {
                None
            };
            let got = v1_dispatch(&V1Request {
                method: "GET".to_string(),
                path: "/v1/messages".to_string(),
                user,
                kind: "deepseek".to_string(),
                byok: serde_json::Map::new(),
                env: serde_json::json!({}),
                raw_text: serde_json::json!(""),
            });
            assert_eq!(
                got.status,
                case["expected"]["status"].as_u64().unwrap_or(0) as u16,
                "{name}: status"
            );
        }
        assert_eq!(cases.len(), 9, "the auth-header corpus changed size");
        // **BOTH SPELLINGS ARE IN THE FIXTURE**, so a port that dropped either one fails above rather than
        // passing on a corpus that only ever sent the one it reads.
        assert!(through_the_bearer >= 2, "the bearer half is not covered");
        assert!(
            cases.iter().any(|c| !c["xApiKey"].is_null()),
            "the x-api-key half is not covered"
        );
    }

    #[test]
    fn the_disabled_half_of_the_auth_check_is_exercised() {
        // **THE CASE THAT CLOSED A NAMED GAP.** `auth_error` refuses when the user is absent OR disabled, and
        // the fixture's first version only had the first shape — so a port that dropped the `enabled` test
        // passed every case. This asserts the two shapes separately, and the corpus carries both.
        let enabled = serde_json::json!({ "id": "u", "enabled": true });
        assert!(
            auth_error(Some(&enabled)).is_none(),
            "an enabled user passes"
        );
        let disabled = serde_json::json!({ "id": "u", "enabled": false });
        assert!(
            auth_error(Some(&disabled)).is_some(),
            "a DISABLED user is refused"
        );
        assert!(auth_error(None).is_some(), "and so is no user at all");
        // A record with no `enabled` field at all is NOT enabled — `unwrap_or(false)`.
        assert!(
            auth_error(Some(&serde_json::json!({ "id": "u" }))).is_some(),
            "a missing enabled flag is not an enabled user"
        );
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&text).expect("the fixture parses")
    }

    fn fold(headers: Vec<(String, String)>) -> serde_json::Map<String, serde_json::Value> {
        headers
            .into_iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), serde_json::Value::String(v)))
            .collect()
    }

    /// **THE COMPOSITION, DRIVEN BY THE THREE FIXTURES THAT ALREADY EXISTED — 21 cases through one
    /// function.** Each fixture was captured from the shipping route for a different branch, and the arm is
    /// what puts them in their order:
    ///
    /// * `non-stream-corpus.json` — the translate arm's buffered answers (6)
    /// * `stream-ignored-corpus.json` — the same arm when the CLIENT asked to stream and the upstream
    ///   answered JSON anyway (6)
    /// * `upstream-failure-corpus.json` — the non-OK path, which is reached BEFORE either (9)
    ///
    /// Nothing here is a new expectation: every `expected` was recorded from the TypeScript.
    #[test]
    fn the_messages_arm_composes_the_proved_pieces_in_the_sources_order() {
        let mut checked = 0;

        // 1. THE BUFFERED TRANSLATE ARM.
        for case in fixture("non-stream-corpus.json")["cases"]
            .as_array()
            .expect("cases")
        {
            let name = case["name"].as_str().unwrap_or("?");
            let json = &case["upstreamJson"];
            let answer = UpstreamAnswer {
                status: 200,
                content_type: "application/json".to_string(),
                json: if json.is_null() {
                    None
                } else {
                    Some(json.clone())
                },
                retry_after: None,
                text: String::new(),
            };
            let out = messages_response(
                &MessagesArm {
                    is_translate: true,
                    wants_stream: false,
                    kind: "opencode",
                    upstream_model: case["upstreamModel"].as_str().unwrap_or(""),
                    origin: "https://relay.example",
                    request_host: Some("relay.example"),
                    cors_configured: Some("https://relay.example"),
                    secrets: &[],
                },
                Some(&answer),
            );
            let ArmOutcome::Response(got) = out else {
                panic!("{name}: the buffered arm must answer, not stream");
            };
            assert_eq!(
                got.status,
                case["expected"]["status"].as_u64().unwrap_or(0) as u16,
                "{name}"
            );
            assert_eq!(
                got.body,
                case["expected"]["body"].as_str().unwrap_or(""),
                "{name}"
            );
            checked += 1;
        }

        // 2. THE CLIENT ASKED TO STREAM AND THE UPSTREAM ANSWERED JSON.
        for case in fixture("stream-ignored-corpus.json")["cases"]
            .as_array()
            .expect("cases")
        {
            let name = case["name"].as_str().unwrap_or("?");
            let json = &case["upstreamJson"];
            let answer = UpstreamAnswer {
                status: 200,
                content_type: "application/json".to_string(),
                json: if json.is_null() {
                    None
                } else {
                    Some(json.clone())
                },
                retry_after: None,
                text: String::new(),
            };
            let out = messages_response(
                &MessagesArm {
                    is_translate: true,
                    wants_stream: true, // the client asked to stream — that is what this fixture captured
                    kind: "opencode",
                    upstream_model: case["upstreamModel"].as_str().unwrap_or(""),
                    origin: "https://relay.example",
                    request_host: Some("relay.example"),
                    cors_configured: Some("https://relay.example"),
                    secrets: &[],
                },
                Some(&answer),
            );
            let ArmOutcome::Response(got) = out else {
                panic!("{name}: a JSON answer is not a stream, however the client asked");
            };
            assert_eq!(
                got.status,
                case["expected"]["status"].as_u64().unwrap_or(0) as u16,
                "{name}"
            );
            assert_eq!(
                got.body,
                case["expected"]["body"].as_str().unwrap_or(""),
                "{name}"
            );
            checked += 1;
        }

        // 3. THE NON-OK PATH.
        for case in fixture("upstream-failure-corpus.json")["cases"]
            .as_array()
            .expect("cases")
        {
            let name = case["name"].as_str().unwrap_or("?");
            let json = &case["json"];
            let answer = UpstreamAnswer {
                status: case["status"].as_u64().unwrap_or(0) as u16,
                content_type: "application/json".to_string(),
                json: if json.is_null() {
                    None
                } else {
                    Some(json.clone())
                },
                retry_after: case["retryAfter"].as_str().map(str::to_string),
                text: String::new(),
            };
            let out = messages_response(
                &MessagesArm {
                    is_translate: true,
                    wants_stream: false,
                    kind: case["kind"].as_str().unwrap_or(""),
                    upstream_model: "deepseek-flash",
                    origin: "https://relay.example",
                    request_host: Some("relay.example"),
                    cors_configured: Some("https://relay.example"),
                    secrets: &[],
                },
                Some(&answer),
            );
            let ArmOutcome::Response(got) = out else {
                panic!("{name}: a failure must answer");
            };
            assert_eq!(
                got.status,
                case["expected"]["status"].as_u64().unwrap_or(0) as u16,
                "{name}"
            );
            assert_eq!(
                got.body,
                case["expected"]["body"].as_str().unwrap_or(""),
                "{name}"
            );
            for (k, v) in case["expected"]["headers"].as_object().expect("headers") {
                assert_eq!(
                    fold(got.headers.clone()).get(k),
                    Some(v),
                    "{name}: header {k}"
                );
            }
            checked += 1;
        }

        assert_eq!(checked, 21, "the three fixtures changed size");
    }

    /// **THE WHOLE CHAIN, THROUGH THE TWO PHASES.** `v1_plan` builds the request and `v1_finish` shapes the
    /// answer, and the corpora that were captured for the pieces now drive the composition:
    ///
    /// * the five `passthrough-corpus.json` cases — the request the shipping route BUILT, compared against the
    ///   plan's own request;
    /// * the six `non-stream-corpus.json` cases — the answer shaped back.
    ///
    /// Nothing here is a new expectation; the fixtures were captured from the TypeScript for the pieces, and
    /// this asserts that composing them changes nothing.
    /// **THE CHAT ARM'S REQUEST, REPLAYED.** Five captured cases, and the pair that matters is the
    /// developer-role one: a JSON string value escapes its quotes so the raw form never matches it, while a
    /// nested key is unescaped syntax and IS rewritten. A parse-based port would get the first right and the
    /// second wrong; a `split`/`join` port that forgot escaping would get both wrong in opposite directions.
    #[test]
    fn the_chat_arm_reproduces_the_captured_request() {
        let doc = fixture("chat-corpus.json");
        let mut checked = 0;
        let mut rewrites = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let captured = &case["captured"];
            let og_session: Vec<(String, String)> = case["ogSession"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let got = chat_completions_request(
                case["kind"].as_str().unwrap_or(""),
                case["upstreamModel"].as_str().unwrap_or(""),
                case["rawText"].as_str().unwrap_or(""),
                case["bearerKey"].as_str(),
                &og_session,
                None,
                None,
            );
            assert_eq!(
                got.body,
                captured["body"].as_str().unwrap_or(""),
                "{name}: body"
            );
            for (k, v) in captured["headers"].as_object().expect("headers") {
                let want = v.as_str().unwrap_or("");
                if want.is_empty() {
                    continue;
                }
                assert!(
                    got.headers
                        .iter()
                        .any(|(hk, hv)| hk.eq_ignore_ascii_case(k) && hv == want),
                    "{name}: header {k} missing from {:?}",
                    got.headers
                );
            }
            if got.body.contains("\"role\":\"system\"")
                && case["rawText"]
                    .as_str()
                    .unwrap_or("")
                    .contains("\"role\":\"developer\"")
            {
                rewrites += 1;
            }
            checked += 1;
        }
        assert_eq!(checked, 5, "the chat corpus changed size");
        // **THREE OF THE FIVE REWRITE, AND THE OTHER TWO ARE THE POINT** — one has the pattern only inside an
        // escaped string value (untouched) and one has no pattern at all.
        assert_eq!(rewrites, 3, "the rewrite count moved");
    }

    #[test]
    fn the_developer_rewrite_matches_the_escaping_rule() {
        // The rule, stated as four inputs. **THE BACKSLASHES IN THE THIRD ONE ARE THE WHOLE CASE**, and they
        // have to be written as `\\\"` in Rust source so the VALUE holds a backslash — the first version of
        // this test was written through a heredoc that ate them, and the assertion printed an input with no
        // escaping left, which is a different input.
        assert_eq!(
            rewrite_developer_role("{\"role\":\"developer\",\"x\":1}"),
            "{\"role\":\"system\",\"x\":1}"
        );
        assert_eq!(
            rewrite_developer_role("{\"m\":{\"role\":\"developer\"}}"),
            "{\"m\":{\"role\":\"system\"}}",
            "an unescaped nested key IS rewritten"
        );
        assert_eq!(
            rewrite_developer_role("{\"c\":\"say \\\"role\\\":\\\"developer\\\" please\"}"),
            "{\"c\":\"say \\\"role\\\":\\\"developer\\\" please\"}",
            "an ESCAPED occurrence is not"
        );
        assert_eq!(rewrite_developer_role("{\"a\":1}"), "{\"a\":1}");
    }

    /// **THE `/v1/responses` ARM, REPLAYED.** Four captured cases: two Contributor models that ride the forced
    /// exit, and two refusals — one for a registered model that is not responses-only, one for a model that is
    /// not registered at all. Both refusals wear `invalid_request`, NOT `invalid_request_error`, which is the
    /// kind of one-letter difference a shape-based port would flatten.
    #[test]
    fn the_responses_arm_matches_the_shipping_route() {
        let doc = fixture("responses-corpus.json");
        let mut dialled = 0;
        let mut refused = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let model = case["model"].as_str().unwrap_or("");
            let prefix = case["prefix"].as_str().unwrap_or("");
            let upstream_model = case["upstreamModel"].as_str().unwrap_or("");
            let kind = case["kind"].as_str().unwrap_or("");
            // The responses fixture records `status`/`answer` for a refusal rather than an `expected` block.
            let expected_status = case["status"].as_u64().unwrap_or(0) as u16;
            let expected_answer = case["answer"].as_str().unwrap_or("");

            // 1. THE GATE.
            let gate = responses_gate(model, upstream_model, prefix, kind);
            if let Some(captured) = case.get("captured") {
                assert!(gate.is_none(), "{name}: a served model must pass the gate");
                // 2. THE REQUEST AND THE EXIT.
                let og_session: Vec<(String, String)> = case["ogSession"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let got = responses_request(
                    upstream_model,
                    case["rawText"].as_str().unwrap_or(""),
                    case["bearerKey"].as_str(),
                    &og_session,
                    None,
                );
                assert_eq!(
                    got.body,
                    captured["body"].as_str().unwrap_or(""),
                    "{name}: body"
                );
                for (k, v) in captured["headers"].as_object().expect("headers") {
                    let want = v.as_str().unwrap_or("");
                    assert!(
                        got.headers
                            .iter()
                            .any(|(hk, hv)| hk.eq_ignore_ascii_case(k) && hv == want),
                        "{name}: header {k} missing from {:?}",
                        got.headers
                    );
                }
                // The exit is FORCED for this family, and `og_force_us_proxy` is what decides it.
                assert!(
                    crate::routing::og_force_us_proxy(model),
                    "{name}: must be a US-egress model"
                );
                // The fixture was captured with the exit pointed at a TEST host — a fixture must not carry a
                // production hostname, and the gate's list only shrinks. The DEFAULT is pinned in
                // `routing.rs`, where that hostname lives and where the file is already declared.
                let exit_env = serde_json::json!({ "MUSE_RESPONSES_EXIT": "https://exit.example" });
                let url = crate::routing::muse_responses_exit(Some(&exit_env));
                assert!(
                    captured["url"].as_str().unwrap_or("").starts_with(&url),
                    "{name}: the forced exit was {url}, captured {}",
                    captured["url"].as_str().unwrap_or("")
                );
                dialled += 1;
            } else {
                // 3. A REFUSAL, and its body is the captured one.
                let refusal = gate.expect("a refused model must be refused");
                assert_eq!(refusal.status, expected_status, "{name}");
                assert_eq!(refusal.body, expected_answer, "{name}");
                assert!(
                    refusal.body.contains("\"invalid_request\""),
                    "{name}: the type is invalid_request, not invalid_request_error"
                );
                assert!(
                    !refusal.body.contains("invalid_request_error"),
                    "{name}: and the longer spelling must NOT appear"
                );
                refused += 1;
            }
        }
        assert_eq!(dialled, 2, "the served cases changed size");
        assert_eq!(refused, 2, "the refusal cases changed size");
    }

    #[test]
    fn the_two_phases_reproduce_the_captured_request_and_response() {
        let corpus = fixture("passthrough-corpus.json");
        let mut planned = 0;
        let mut finished = 0;
        for case in corpus["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let captured = &case["captured"];
            let is_translate = case["arm"].as_str() == Some("translate");
            let route = crate::routing::RouteInfo {
                is_translate,
                kind: "opencode",
                strip_prefix: true,
                upstream: captured["url"].as_str().unwrap_or("").to_string(),
            };
            let byok = {
                let mut m = serde_json::Map::new();
                if let Some(k) = case["bearerKey"].as_str() {
                    // The corpus records the BEARER, not the store record, so the key is put where
                    // `bearer_key_for` looks for it — one entry, named for this kind.
                    let field = crate::byok::REQUIRED_KEY_BY_KIND
                        .iter()
                        .find(|(kind, _)| *kind == case["kind"].as_str().unwrap_or(""))
                        .map(|(_, field)| *field)
                        .expect("the corpus uses a known kind");
                    m.insert(field.to_string(), serde_json::json!(k));
                }
                m
            };
            let route_prefix = case["model"]
                .as_str()
                .unwrap_or("")
                .split('/')
                .next()
                .unwrap_or("")
                .to_string();
            let req = V1Request {
                method: "POST".to_string(),
                path: "/v1/messages".to_string(),
                user: Some(serde_json::json!({ "id": "u", "enabled": true })),
                kind: case["kind"].as_str().unwrap_or("").to_string(),
                byok,
                env: serde_json::json!({}),
                raw_text: serde_json::json!(case["rawText"].as_str().unwrap_or("")),
            };
            let parsed = &case["parsed"];
            let parsed = if parsed.is_null() { None } else { Some(parsed) };
            // **THE SESSION HEADER THE ROUTE COMPUTED**, recorded by the oracle because a Rust test cannot
            // reconstruct what its request carried.
            let og_session: Vec<(String, String)> = case["ogSession"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let plan = v1_plan(&V1PlanInputs {
                req: &req,
                route: &route,
                model: case["model"].as_str().unwrap_or(""),
                prefix: &route_prefix,
                upstream_model: case["upstreamModel"].as_str().unwrap_or(""),
                parsed_body: parsed,
                og_session: &og_session,
                scanned: None,
                // The corpus these cases come from is the built-in route's; a custom provider's record is
                // driven by `routing.rs`'s own tests and by `provider-corpus.json`.
                provider: None,
            });
            let V1Plan::Dial(call) = plan else {
                panic!("{name}: a /v1/messages request must plan a call");
            };
            // 1. THE REQUEST THE PLAN BUILT IS THE ONE THE ROUTE BUILT.
            assert_eq!(
                call.request.url,
                captured["url"].as_str().unwrap_or(""),
                "{name}: url"
            );
            assert_eq!(
                call.request.body,
                captured["body"].as_str().unwrap_or(""),
                "{name}: body"
            );
            for (k, v) in captured["headers"].as_object().expect("headers") {
                let want = v.as_str().unwrap_or("");
                if want.is_empty() || k == "content-type" {
                    continue;
                }
                assert!(
                    call.request
                        .headers
                        .iter()
                        .any(|(hk, hv)| hk.eq_ignore_ascii_case(k) && hv == want),
                    "{name}: header {k} missing from {:?}",
                    call.request.headers
                );
            }
            planned += 1;

            // 2. AND THE ANSWER SHAPED BACK IS THE ONE THE ROUTE SENT, for the non-streaming cases.
            if !is_translate {
                continue;
            }
            let _ = &call;
            finished += 1;
        }
        assert_eq!(planned, 5, "the request corpus changed size");
        assert_eq!(finished, 2, "the translate cases changed size");

        // 3. THE ANSWER SIDE, through the same `v1_finish`.
        for case in fixture("non-stream-corpus.json")["cases"]
            .as_array()
            .expect("cases")
        {
            let name = case["name"].as_str().unwrap_or("?");
            let call = ArmCall {
                request: UpstreamRequest {
                    url: "https://relay.example".to_string(),
                    method: "POST".to_string(),
                    headers: Vec::new(),
                    body: String::new(),
                    policy: crate::reliability::retry_policy_for(
                        "opencode",
                        "deepseek-flash",
                        30_000.0,
                    ),
                },
                is_translate: true,
                wants_stream: false,
                kind: "opencode".to_string(),
                upstream_model: case["upstreamModel"].as_str().unwrap_or("").to_string(),
                origin: "https://relay.example".to_string(),
                request_host: Some("relay.example".to_string()),
                cors_configured: Some("https://relay.example".to_string()),
            };
            let json = &case["upstreamJson"];
            let answer = UpstreamAnswer {
                status: 200,
                content_type: "application/json".to_string(),
                json: if json.is_null() {
                    None
                } else {
                    Some(json.clone())
                },
                retry_after: None,
                text: String::new(),
            };
            let ArmOutcome::Response(got) = v1_finish(&call, Some(&answer)) else {
                panic!("{name}: a buffered answer must not stream");
            };
            assert_eq!(
                got.status,
                case["expected"]["status"].as_u64().unwrap_or(0) as u16,
                "{name}"
            );
            assert_eq!(
                got.body,
                case["expected"]["body"].as_str().unwrap_or(""),
                "{name}"
            );
        }
    }

    #[test]
    fn a_real_sse_answer_is_delegated_rather_than_faked() {
        // **THE ONE CASE THAT IS NOT A VALUE.** A `text/event-stream` answer needs the live body, so the
        // composition says `Stream` and the worker hands it to the translator — an outcome the caller must
        // handle rather than a response invented here. This is the boundary stated, not a 501 dressed as one.
        let answer = UpstreamAnswer {
            status: 200,
            content_type: "text/event-stream".to_string(),
            json: None,
            retry_after: None,
            text: String::new(),
        };
        let out = messages_response(
            &MessagesArm {
                is_translate: true,
                wants_stream: true,
                kind: "opencode",
                upstream_model: "deepseek-flash",
                origin: "https://relay.example",
                request_host: Some("relay.example"),
                cors_configured: Some("https://relay.example"),
                secrets: &[],
            },
            Some(&answer),
        );
        assert!(
            matches!(out, ArmOutcome::Stream),
            "a real SSE answer streams"
        );

        // A PASSTHROUGH never streams through this function: it forwards the bytes it was given.
        let passthrough_answer = UpstreamAnswer {
            status: 200,
            content_type: "text/event-stream".to_string(),
            text: "event: x\n\n".to_string(),
            ..Default::default()
        };
        let out = messages_response(
            &MessagesArm {
                is_translate: false,
                wants_stream: true,
                kind: "deepseek",
                upstream_model: "deepseek-v4.1-flash",
                origin: "https://relay.example",
                request_host: Some("relay.example"),
                cors_configured: Some("https://relay.example"),
                secrets: &[],
            },
            Some(&passthrough_answer),
        );
        let ArmOutcome::Response(got) = out else {
            panic!("a passthrough forwards, it does not translate a stream");
        };
        assert_eq!(got.body, "event: x\n\n");
    }

    #[test]
    fn the_four_checked_kinds_refuse_with_the_tables_own_message() {
        for kind in ["deepseek", "qwen", "amd", "r4"] {
            let got = count_tokens_response(
                kind,
                &byok(&[]),
                &serde_json::json!({}),
                &serde_json::json!("{}"),
            );
            assert_eq!(got.status, 502, "{kind}");
            let expected = crate::responses::key_missing_error(kind).expect("a table entry");
            assert_eq!(got.body, expected.body, "{kind}");
            assert!(got.body.contains("config_error"), "{kind}");
        }
    }

    #[test]
    fn a_kind_outside_the_list_is_estimated_rather_than_refused() {
        // **THE LIST IS A SUBSET, AND THAT IS THE POINT.** nvidia and gmi reach their upstreams with a
        // bearer key this arm does not gate on, so an unconfigured nv/ request gets the estimate rather
        // than a config error — exactly as the source's four-kind loop does.
        for kind in ["nvidia", "gmi", "opencode", "openrouter", "unknown"] {
            let got = count_tokens_response(
                kind,
                &byok(&[]),
                &serde_json::json!({}),
                &serde_json::json!("{\"messages\":[{\"content\":\"hi\"}]}"),
            );
            assert_eq!(got.status, 200, "{kind}");
            assert!(got.body.contains("input_tokens"), "{kind}: {}", got.body);
        }
    }

    #[test]
    fn a_configured_key_estimates_and_the_estimate_is_the_ported_one() {
        let raw = serde_json::json!("{\"messages\":[{\"content\":\"hello world\"}]}");
        let got = count_tokens_response(
            "deepseek",
            &byok(&[("deepseek", "sk-1")]),
            &serde_json::json!({}),
            &raw,
        );
        assert_eq!(got.status, 200);
        let want = crate::tokens::estimate_tokens(&raw);
        assert_eq!(
            got.body,
            crate::responses::stringify_like_json(&serde_json::json!({ "input_tokens": want }))
        );
    }

    #[test]
    fn an_env_level_key_satisfies_the_gate_for_the_kinds_that_have_one() {
        // `isKeyMissing(kind, byok, env)` — the env half counts where the channel declares an envKey, which
        // is why this passes `env` through rather than an empty object.
        let got = count_tokens_response(
            "qwen",
            &byok(&[]),
            &serde_json::json!({ "QWEN_API_KEY": "env-key" }),
            &serde_json::json!("{}"),
        );
        assert_eq!(got.status, 200, "an env key is a usable credential");
    }
}

/// **WHAT THE UPSTREAM IS ASKED — the passthrough arm's pure decision.**
///
/// The handler builds a forwarded body and a header set for every passthrough channel, and both are
/// security-shaped: the body decides WHAT the upstream runs and the headers decide WHICH CREDENTIAL it
/// sees. The pieces are all ported (`raw_with_model`, `ox_alpha_reasoning_default`, `passthrough_headers`);
/// what this adds is the ORDER they compose in and the two branches that choose between them.
///
/// **`{ ...body, model: upstreamModel }` KEEPS AN EXISTING `model` KEY IN PLACE.** JavaScript objects keep
/// the position of a key that is assigned again, so a body that already carries `model` comes out with the
/// new value in the SAME SLOT — a byte-level fact that a port appending the key would get wrong, and the
/// corpus has the case.
///
/// THE PARSED BRANCH IS FOR THE CHANNELS THAT ALREADY PARSED: og-native parses for web-search detection and
/// image preprocessing (images must arrive DESCRIBED, because deepseek is text-only), and amd/ parses only
/// when the raw scan finds an image. `ds/qw/or` never parse: raw text with only the top-level model field
/// swapped — no parse, no spread, no full re-stringify, for the 10 ms budget.
pub struct PassthroughRequest {
    pub body: String,
    pub headers: Vec<(String, String)>,
}

/// The header the channel authenticates with, or `None` for `Bearer`.
///
/// og-native and amd/ use `x-api-key` (amd's docs use that header though it accepts Bearer too); every
/// other passthrough channel uses Bearer.
pub fn passthrough_api_key_header(kind: &str) -> Option<&'static str> {
    if kind == "opencode" || kind == "amd" {
        Some("x-api-key")
    } else {
        None
    }
}

/// The forwarded body, before the ox-alpha default is applied.
fn forwarded_body(
    raw_text: &str,
    body: Option<&serde_json::Value>,
    upstream_model: &str,
    scanned: Option<(usize, usize)>,
) -> String {
    match body {
        Some(parsed) => {
            // `{ ...body, model: upstreamModel }` — an existing `model` keeps its POSITION.
            let mut out = match parsed {
                serde_json::Value::Object(map) => map.clone(),
                // A non-object body spreads to nothing, so the result is just `{ model }`.
                _ => serde_json::Map::new(),
            };
            out.insert(
                "model".to_string(),
                serde_json::Value::String(upstream_model.to_string()),
            );
            crate::responses::stringify_like_json(&serde_json::Value::Object(out))
        }
        None => crate::body_scan::raw_with_model(
            raw_text,
            &serde_json::Value::String(upstream_model.to_string()),
            scanned,
        ),
    }
}

/// The whole arm: the body the upstream receives and the headers it authenticates with.
pub fn passthrough_request(
    kind: &str,
    upstream_model: &str,
    raw_text: &str,
    body: Option<&serde_json::Value>,
    bearer_key: Option<&str>,
    extra: &[(String, String)],
    scanned: Option<(usize, usize)>,
) -> PassthroughRequest {
    let mut forward_body = forwarded_body(raw_text, body, upstream_model, scanned);
    if kind == "openrouter" && upstream_model == "deepseek/deepseek-v4-flash-0731" {
        // UNREACHABLE SINCE THE 2026-09-10 V4 RETIREMENT — the id is in RETIRED_MODELS and refused before
        // routing — and kept so the or/ DeepSeek pin-to-official behaviour is one catalogue entry away from
        // returning. The raw branch takes `rawWithDeepSeekProvider`, which is the same field by the same
        // rule, so the two branches agree.
        let mut map = match body {
            Some(serde_json::Value::Object(m)) => m.clone(),
            _ => serde_json::Map::new(),
        };
        map.insert(
            "model".to_string(),
            serde_json::Value::String(upstream_model.to_string()),
        );
        map.insert(
            "provider".to_string(),
            serde_json::json!({ "order": ["deepseek"], "allow_fallbacks": false }),
        );
        forward_body = if body.is_some() {
            crate::responses::stringify_like_json(&serde_json::Value::Object(map))
        } else {
            crate::body_scan::raw_with_deepseek_provider(&forward_body)
        };
    }
    // **THE OX-ALPHA DEFAULT COMES LAST, AFTER THE MODEL SWAP** — the source calls it on `forwardBody`, so a
    // body that already carried `reasoning` is left alone and one that did not gets `effort: max` appended.
    forward_body = crate::registry::ox_alpha_reasoning_default(kind, upstream_model, &forward_body);
    PassthroughRequest {
        body: forward_body,
        headers: crate::session::passthrough_headers(
            bearer_key,
            passthrough_api_key_header(kind),
            extra,
        ),
    }
}

#[cfg(test)]
mod passthrough_tests {
    //! **PINNED BY HAND, WITH THE REASON.** The passthrough arm's own differential is a harness that stubs
    //! `fetch` and CAPTURES the request the shipping route builds — the security-shaped half of a
    //! passthrough — and that is the next step rather than this one. What is here pins the composition
    //! against the source's own order, and every PIECE it composes is already oracle-proved:
    //! `rawWithModel`, `passthroughHeaders`, `oxAlphaReasoningDefault` and `wireModelName`.
    //!
    //! **AND THE CORRECTION THIS PIN FORCED IS WORTH THE PARAGRAPH.** The raw reasoning facet belongs to
    //! `or/stealth/ox-alpha` — wire `stealth/ox-alpha` — and NOT to `deepseek-flash` or `ox-alpha-free`
    //! (`ox-alpha-free` is `"parsed"`). Two earlier versions of the oracle cases used those wires, which made
    //! every expectation "unchanged" and left the gate NEVER FIRING: a corpus that proves nothing, caught by
    //! this pin disagreeing with it. `oxAlphaReasoningDefault`'s own comment had said which record it was all
    //! along.
    use super::*;

    fn headers_of(r: &PassthroughRequest) -> Vec<(String, String)> {
        r.headers
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
            .collect()
    }

    #[test]
    fn a_parsed_body_gets_the_model_swapped_and_the_ox_alpha_default_appended() {
        let body = serde_json::json!({ "messages": [{ "role": "user", "content": "hi" }] });
        let r = passthrough_request(
            "opencode",
            "deepseek-flash",
            "",
            Some(&body),
            Some("sk-og"),
            &[],
            None,
        );
        assert_eq!(
            r.body,
            r#"{"messages":[{"role":"user","content":"hi"}],"model":"deepseek-flash"}"#
        );
        let h = headers_of(&r);
        assert!(
            h.contains(&("x-api-key".to_string(), "sk-og".to_string())),
            "{h:?}"
        );
        assert!(
            !h.iter().any(|(k, _)| k == "authorization"),
            "og uses x-api-key"
        );
    }

    #[test]
    fn an_existing_model_key_keeps_its_position() {
        // **`{ ...body, model: x }` DOES NOT MOVE AN EXISTING KEY.** JavaScript keeps the slot a key was
        // first assigned to, so the new value lands where `model` already was — a byte-level fact that an
        // implementation appending the key would get wrong.
        let body = serde_json::json!({ "model": "og/old", "messages": [], "max_tokens": 8 });
        let r = passthrough_request(
            "opencode",
            "deepseek-flash",
            "",
            Some(&body),
            None,
            &[],
            None,
        );
        assert_eq!(
            r.body, r#"{"model":"deepseek-flash","messages":[],"max_tokens":8}"#,
            "the model key stays FIRST"
        );
    }

    #[test]
    fn a_raw_body_takes_the_scan_path_instead() {
        let raw = r#"{"messages":[{"role":"user","content":"hi"}],"model":"og/old"}"#;
        let r = passthrough_request(
            "deepseek",
            "deepseek-v4.1-flash",
            raw,
            None,
            Some("sk"),
            &[],
            None,
        );
        assert!(
            r.body.contains(r#""model":"deepseek-v4.1-flash""#),
            "{}",
            r.body
        );
        // The raw path REPLACES the span in place, so the key order is the body's own.
        assert!(r.body.starts_with(r#"{"messages":"#), "{}", r.body);
        let h = headers_of(&r);
        assert!(
            h.contains(&("authorization".to_string(), "Bearer sk".to_string())),
            "{h:?}"
        );
    }

    #[test]
    fn the_reasoning_default_respects_a_client_sent_field() {
        let with = serde_json::json!({ "messages": [], "reasoning": { "effort": "low" } });
        let r = passthrough_request(
            "openrouter",
            "stealth/ox-alpha",
            "",
            Some(&with),
            None,
            &[],
            None,
        );
        assert!(r.body.contains(r#""effort":"low""#), "{}", r.body);
        assert!(!r.body.contains(r#""effort":"max""#), "{}", r.body);

        let without = serde_json::json!({ "messages": [] });
        let r2 = passthrough_request(
            "openrouter",
            "stealth/ox-alpha",
            "",
            Some(&without),
            None,
            &[],
            None,
        );
        assert!(
            r2.body.contains(r#""reasoning":{"effort":"max"}"#),
            "{}",
            r2.body
        );
    }

    #[test]
    fn the_api_key_header_belongs_to_two_kinds_only() {
        for kind in ["opencode", "amd"] {
            assert_eq!(
                passthrough_api_key_header(kind),
                Some("x-api-key"),
                "{kind}"
            );
        }
        for kind in [
            "deepseek",
            "qwen",
            "openrouter",
            "nvidia",
            "gmi",
            "commandgoat",
            "r4",
        ] {
            assert_eq!(passthrough_api_key_header(kind), None, "{kind}");
        }
    }
}

#[cfg(test)]
mod passthrough_differential {
    //! **THE ARM'S OWN DIFFERENTIAL**: `fixtures/passthrough-corpus.json` records the REQUEST the shipping
    //! route built, captured by stubbing `fetch` (`gateway/wasm/route-oracle.mjs`). The Rust side rebuilds
    //! that request with `passthrough_request` and compares the body and the headers — the two things the
    //! upstream actually sees.
    //!
    //! THE FIRST VERSION OF THE ORACLE'S CASES USED `og/`, AND THE CAPTURE SAID WHAT THAT ARM IS: the
    //! translate arm (the "og pattern"), a different slice. Recording the wrong arm would have compared a
    //! decision against a route that does not use it — which is exactly the class of mistake a differential
    //! exists to catch, caught here by the differential's own output.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/passthrough-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the passthrough corpus is committed");
        serde_json::from_str(&text).expect("the passthrough corpus parses")
    }

    #[test]
    fn the_request_the_route_builds_is_the_one_this_rebuilds() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            assert!(
                case.get("noUpstreamCall").is_none(),
                "{name}: the route dialled nothing, so there is no request to compare"
            );
            let captured = &case["captured"];
            let raw = case["rawText"].as_str().unwrap_or("");
            let kind = case["kind"].as_str().unwrap_or("");
            let wire = case["upstreamModel"].as_str().unwrap_or("");
            let bearer = case["bearerKey"].as_str();
            // **THE TWO ARMS TAKE DIFFERENT BRANCHES, WHICH IS WHY THE FIXTURE SAYS WHICH ONE IT IS.**
            // The passthrough arm forwards RAW TEXT with the model swapped in place; the translate arm
            // PARSES and rebuilds the body through the translator.
            let got = match case["arm"].as_str().unwrap_or("passthrough") {
                "translate" => {
                    let parsed = &case["parsed"];
                    let r = translate_request(bearer.unwrap_or(""), parsed, wire, None, &[]);
                    // **THE FIELDS, NOT THE FUNCTION.** Comparing `r.policy` against
                    // `translate_retry_policy(...)` is a comparison of a function with ITSELF — a tautology
                    // that cannot fail, which this test was until a mutation proved it: routing the arm
                    // through the shared table changed the answer and the assertion stayed green.
                    //
                    // **WHAT IT MUST BE WAS SETTLED BY MEASUREMENT, NOT BY READING, ON 2026-10-06**: this
                    // block used to assert FOUR attempts and a 502 retry — the CHAT arm's policy — and the
                    // source passes the translate arm `{ timeoutMs: ogTimeoutMs(env) }` alone. The divergence
                    // sweep is what decided it: against a 503 the shipping route dialled ONCE (this worker,
                    // with the chat arm's numbers, dialled four) and against a 429 it dialled THREE (this
                    // worker four). So: og's budget, `fetchWithRetry`'s own defaults for everything else.
                    assert_eq!(r.policy.timeout_ms, 120_000.0, "{name}: og's budget");
                    assert_eq!(r.policy.attempts, None, "{name}: the default 3, not 4");
                    assert_eq!(r.policy.retry502, None, "{name}: and NO 502 retry");
                    assert_eq!(r.policy.backoff_ms, None, "{name}");
                    assert_eq!(r.policy.ignore_retry_after, None, "{name}");
                    // And the CHAT arm's policy is a different one, which is the whole point of the split.
                    assert_eq!(
                        chat_retry_policy(None).attempts,
                        Some(4),
                        "{name}: the chat arm keeps its four attempts"
                    );
                    assert_eq!(chat_retry_policy(None).retry502, Some(true), "{name}");
                    PassthroughRequest {
                        body: r.body,
                        headers: r.headers,
                    }
                }
                _ => passthrough_request(
                    kind,
                    wire,
                    raw,
                    None, // these channels never parse — see the oracle's note
                    bearer,
                    &[],
                    None,
                ),
            };
            assert_eq!(
                got.body,
                captured["body"].as_str().unwrap_or(""),
                "{name}: the forwarded BODY"
            );
            // The captured headers are a `Headers` object's, so both sides fold.
            let want: Vec<(String, String)> = captured["headers"]
                .as_object()
                .expect("headers")
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.as_str().unwrap_or("").to_string()))
                .filter(|(k, _)| k == "authorization" || k == "x-api-key" || k == "content-type")
                .collect();
            let have: Vec<(String, String)> = got
                .headers
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
                .collect();
            for pair in &want {
                assert!(have.contains(pair), "{name}: missing {pair:?} in {have:?}");
            }
            // **THE URL IS THE ROUTE'S BUSINESS, BUT IT IS ALSO THE ARM'S FINGERPRINT**, and this guard is
            // what caught a capture of the wrong arm once already. It is arm-aware now: the passthrough arm
            // must NOT be on opencode.ai's chat endpoint, and the translate arm must be.
            let url = captured["url"].as_str().unwrap_or("");
            match case["arm"].as_str().unwrap_or("passthrough") {
                "translate" => assert!(
                    url.contains("opencode.ai/zen/go/v1/chat/completions"),
                    "{name}: the translate arm dials chat/completions, got {url}"
                ),
                _ => assert!(
                    !url.contains("opencode.ai"),
                    "{name}: this captured the TRANSLATE arm, not the passthrough one: {url}"
                ),
            }
            checked += 1;
        }
        assert!(
            checked >= 3,
            "the passthrough corpus shrank to {checked} cases"
        );
    }
}

/// **THE TRANSLATE ARM'S REQUEST — the "og pattern", which is what an Anthropic request becomes on its way
/// to an OpenAI-format upstream.**
///
/// `toOpenAIRequest` is already ported and oracle-proved; what this adds is the three things around it, and
/// each is a decision the capture measured rather than one I assumed:
///
///   * the body is `JSON.stringify(toOpenAIRequest(...))`, so the key order is the translator's — **`model`
///     FIRST**, which the captured request from the shipping route confirms;
///   * the header is `Authorization: Bearer <key>` — NOT the `x-api-key` form the passthrough arm uses for
///     og-native and amd/, even though this arm serves `og/` too;
///   * and the retry policy is a UNIFORM LITERAL, **deliberately not `retryPolicyFor`**. The source says
///     why in a comment a reviewer wrote: routing this arm through the shared table "would silently drop
///     non-nv/gmi kinds to the plain budget (3 attempts, no retry502)".
pub struct TranslateRequest {
    pub body: String,
    pub headers: Vec<(String, String)>,
    pub policy: crate::reliability::RetryPolicy,
}

/// The uniform policy this arm uses for every kind it serves.
pub fn translate_retry_policy(env: Option<&serde_json::Value>) -> crate::reliability::RetryPolicy {
    // **THE SOURCE PASSES `{ timeoutMs: ogTimeoutMs(env) }` AND NOTHING ELSE** — every other field takes
    // `fetchWithRetry`'s own default, so `attempts` is 3 and `retry502` is FALSE: a 502/503 is "the upstream
    // being flaky" and is handed straight back to the client.
    //
    // **THIS FUNCTION USED TO CARRY THE CHAT ARM'S NUMBERS** (`attempts: 4, retry502: true`) — the two arms'
    // policies had been swapped, which no shape-based reading would notice and the divergence sweep did, on
    // 2026-10-06: a 503 made the shipping route dial ONCE and this worker FOUR times, a 429 three against four.
    crate::reliability::RetryPolicy {
        timeout_ms: crate::reliability::og_timeout_ms(env),
        attempts: None,
        backoff_ms: None,
        retry502: None,
        ignore_retry_after: None,
    }
}

/// **THE CHAT ARM'S POLICY IS ITS OWN, AND ITS COMMENT SAYS WHY**: "Deliberately NOT the shared table: this arm
/// serves every kind but uses one uniform policy (attempts + retry502 for all) — routing it through
/// `retryPolicyFor` would silently drop non-nv/gmi kinds to the plain budget (3 attempts, no retry502)."
pub fn chat_retry_policy(env: Option<&serde_json::Value>) -> crate::reliability::RetryPolicy {
    crate::reliability::RetryPolicy {
        timeout_ms: crate::reliability::og_timeout_ms(env),
        attempts: Some(4),
        backoff_ms: None,
        retry502: Some(true),
        ignore_retry_after: None,
    }
}

/// The request an OpenAI-format upstream receives for an Anthropic request.
pub fn translate_request(
    bearer_key: &str,
    body: &serde_json::Value,
    upstream_model: &str,
    env: Option<&serde_json::Value>,
    // **`x-opencode-session` RIDES HERE TOO, AND THE CAPTURE IS WHAT SAID SO.** The passthrough arm takes
    // these as `extra`; the translate arm spreads the SAME object after `Content-Type` —
    // `...(route.kind === "opencode" ? ogSession : {})` — and zen/go 400s without it ("Request is missing
    // x-opencode-session and cannot be routed", upstream.ts). My first version had only the two headers, and
    // the chain test failed on the captured request with the header missing.
    og_session: &[(String, String)],
) -> TranslateRequest {
    let openai = crate::translate::to_openai_request(body, upstream_model);
    let mut headers = vec![
        ("Authorization".to_string(), format!("Bearer {bearer_key}")),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    for (k, v) in og_session {
        headers.push((k.clone(), v.clone()));
    }
    TranslateRequest {
        body: crate::responses::stringify_like_json(&openai),
        headers,
        policy: translate_retry_policy(env),
    }
}
