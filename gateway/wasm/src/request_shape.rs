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

    fn route_corpus() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/route-corpus.json");
        let text = std::fs::read_to_string(path).expect("the route corpus is committed");
        serde_json::from_str(&text).expect("the route corpus parses")
    }

    /// **THE ROUTE'S OWN DIFFERENTIAL, REPLAYED.** `fixtures/route-corpus.json` was produced by driving the
    /// SHIPPING `handleGateway` with a fake KV (`gateway/wasm/route-oracle.mjs`), so these expectations are
    /// what the deployed route ANSWERS rather than what a hand-written test believed.
    ///
    /// A case marked `beforeArm` is one where the handler answers ABOVE this arm — an OpenRouter key gate,
    /// or the default channel an empty body resolves to. Those are asserted as "not this function's
    /// answer", which is the honest thing for a differential to say about a gate that lives elsewhere.
    #[test]
    fn the_count_tokens_arm_matches_the_shipping_route() {
        let doc = route_corpus();
        let mut checked = 0;
        let mut skipped = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            if case.get("beforeArm").is_some() {
                skipped += 1;
                continue;
            }
            let byok = crate::byok::extract_byok_keys(&serde_json::json!(case["ukeys"]));
            let env = &case["env"];
            let raw = &case["rawText"];
            let got = count_tokens_response(case["kind"].as_str().unwrap_or(""), &byok, env, raw);
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
            checked += 1;
        }
        assert!(
            checked >= 5,
            "the route corpus shrank to {checked} replayable cases"
        );
        assert_eq!(skipped, 3, "the before-the-arm markers moved");
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
