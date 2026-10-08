//! THE ZEN-GO DIFFERENTIAL, JUDGED IN RUST — the cases, the upstream answers and the byte comparison.
//!
//! ── WHAT MOVED HERE, AND WHAT DID NOT ───────────────────────────────────────────────────────────
//!
//! `worker/verify.mjs` used to hold all three jobs: run the built wasm module, hold the cases, and judge
//! the bytes. **RUNNING THE MODULE NEEDS A JS ENGINE** — `worker-build`'s output is wasm-bindgen glue that
//! imports `cloudflare:workers` — so that one job is `gateway/wasm/run-cases.mjs`, a runner that takes a
//! spec and prints what it saw. **THE OTHER TWO ARE DATA AND LOGIC**, and they are here. The expected
//! bytes are `shipping-answers.json`: what `src/index.js` answered, recorded by the harness on a run where
//! both sides agreed on all 17 cases, before that file was deleted.
//!
//! ── MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────────
//!
//! MUTATION: make the translated arm answer with `jsonError` where the shipping worker answers the
//!           provider's own error envelope — the structural difference this corpus was built around.
//! RESULT:   the case fails on `body DIFFERS`, with the recorded bytes on one side and the port's on the
//!           other. **AND IT IS A ONE-WORD CHANGE THAT NO STATUS CHECK WOULD SEE** (both are 4xx).
//!
//! MUTATION (the judge itself): make `judge` compare only status and bytes.
//! RESULT:   `a_missing_upstream_header_is_not_a_pass` fails — that case differs on the upstream row alone.

use std::path::PathBuf;

use summrise_differential_harness::{
    build, judge, recorded, repo_root, run, verdict, Answer, Case, Observed, Recorded, Spec,
    Upstream,
};

const WORKER_KEY: &str = "worker-key-1234567890";
const CLIENT_KEY: &str = "client-key-1234567890";

const HOST: &str = "https://zen-go.test";

const ENV: &[(&str, &str)] = &[
    ("CLIENT_KEY", CLIENT_KEY),
    ("OPENCODE_GO_API_KEY", WORKER_KEY),
];

/// What the stubbed upstream answers, per path — `/v1/messages` serves both shapes, so it is the SSE one.
const ANSWERS: &[(&str, Answer)] = &[
    (
        "/zen/go/v1/models",
        Answer {
            status: 200,
            headers: &[("content-type", "application/json")],
            body: r#"{"data":[{"id":"m"}]}"#,
        },
    ),
    (
        "/zen/go/v1/chat/completions",
        Answer {
            status: 200,
            headers: &[("content-type", "application/json")],
            body: r#"{"id":"chatcmpl-1","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#,
        },
    ),
    (
        "/zen/go/v1/messages",
        Answer {
            status: 200,
            headers: &[("content-type", "text/event-stream")],
            body: "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
        },
    ),
];

/// The `x-api-key` every gated case carries.
const KEY: &[(&str, &str)] = &[("x-api-key", CLIENT_KEY)];
const KEY_JSON: &[(&str, &str)] = &[
    ("x-api-key", CLIENT_KEY),
    ("content-type", "application/json"),
];

const fn case(
    label: &'static str,
    method: &'static str,
    path: &'static str,
    headers: &'static [(&'static str, &'static str)],
    body: Option<&'static str>,
    upstream: Option<Answer>,
) -> Case {
    Case {
        label,
        method,
        path,
        headers,
        body,
        upstream,
    }
}

/// The 17 cases, in the order the fixture recorded them — the order is checked, not assumed.
const CASES: &[Case] = &[
    // **THE ONE THAT PROVES THE GATE'S POSITION.** No key, and a path NO ROUTE NAMES: the shipping worker
    // answers 401 because the gate runs before it looks at the path.
    case(
        "no key, on a path NO ROUTE NAMES — the gate runs first",
        "GET",
        "/nope",
        &[],
        None,
        None,
    ),
    case(
        "the preflight — answered BEFORE the gate",
        "OPTIONS",
        "/v1/messages",
        &[],
        None,
        None,
    ),
    case(
        "the preflight, on a path no route names",
        "OPTIONS",
        "/anything",
        &[],
        None,
        None,
    ),
    case(
        "GET /v1/models with no key",
        "GET",
        "/v1/models",
        &[],
        None,
        None,
    ),
    case(
        "GET /v1/models, wrong key",
        "GET",
        "/v1/models",
        &[("x-api-key", "wrong")],
        None,
        None,
    ),
    case(
        "a path no route names, WITH the key",
        "POST",
        "/nope",
        KEY,
        None,
        None,
    ),
    case(
        "GET on the messages path — not a POST",
        "GET",
        "/v1/messages",
        KEY,
        None,
        None,
    ),
    // The upstream arms.
    case(
        "GET /v1/models, gated and served",
        "GET",
        "/v1/models",
        KEY,
        None,
        None,
    ),
    case(
        "count_tokens — system + tools + messages, all three counted",
        "POST",
        "/v1/messages/count_tokens",
        KEY_JSON,
        Some(r#"{"system":"abc","tools":[{"a":1}],"messages":[{"role":"user"}]}"#),
        None,
    ),
    case(
        "count_tokens — null parts are dropped by the loose comparison",
        "POST",
        "/v1/messages/count_tokens",
        KEY_JSON,
        Some(r#"{"system":null,"messages":null}"#),
        None,
    ),
    // The native arm: the Flash line, streamed and not.
    case(
        "messages NATIVE + stream — the raw request goes up, SSE comes back",
        "POST",
        "/v1/messages",
        KEY_JSON,
        Some(r#"{"model":"deepseek-flash","stream":true,"messages":[{"role":"user"}]}"#),
        None,
    ),
    case(
        "messages NATIVE, the retired alias, not streamed — JSON, and no Cache-Control",
        "POST",
        "/v1/messages",
        KEY_JSON,
        Some(r#"{"model":"deepseek-v4-flash","messages":[{"role":"user"}]}"#),
        None,
    ),
    // The translated arm: every other model.
    case(
        "messages TRANSLATED + stream — toOpenAIRequest up, toSSE back",
        "POST",
        "/v1/messages",
        KEY_JSON,
        Some(
            r#"{"model":"other-model","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
        ),
        None,
    ),
    case(
        "messages TRANSLATED, not streamed — toAnthropicResponse, jsonOk",
        "POST",
        "/v1/messages",
        KEY_JSON,
        Some(r#"{"model":"other-model","messages":[{"role":"user","content":"hi"}]}"#),
        None,
    ),
    case(
        "messages with NO model — the deepseek-flash default",
        "POST",
        "/v1/messages",
        KEY_JSON,
        Some(r#"{"messages":[{"role":"user"}]}"#),
        None,
    ),
    // The failure paths, where the provider's words reach the client and must not carry a credential.
    case(
        "a 4xx that ECHOES THE WORKER'S KEY — it must be redacted",
        "POST",
        "/v1/messages",
        KEY_JSON,
        Some(r#"{"model":"deepseek-flash","messages":[]}"#),
        Some(Answer {
            status: 400,
            headers: &[("content-type", "application/json")],
            body: r#"{"error":{"message":"bad key worker-key-1234567890"}}"#,
        }),
    ),
    case(
        "a 5xx — generic client text, the detail stays server-side",
        "POST",
        "/v1/messages",
        KEY_JSON,
        Some(r#"{"model":"deepseek-flash","messages":[]}"#),
        Some(Answer {
            status: 503,
            headers: &[("content-type", "application/json")],
            body: r#"{"error":{"message":"upstream exploded"}}"#,
        }),
    ),
];

#[test]
fn the_port_is_byte_identical_to_the_recorded_shipping_answers() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    build(&crate_dir);

    let spec = Spec {
        host: HOST,
        env: ENV,
        answers: ANSWERS,
        cases: CASES,
    };
    let observed = run(
        &spec,
        &crate_dir.join("build/index.js"),
        &root.join("gateway/wasm/run-cases.mjs"),
    );
    let expected = recorded(&crate_dir.join("shipping-answers.json"));
    let bad = judge("zen-go", CASES, &expected, &observed);
    verdict("zen-go", CASES.len(), bad);
}

// ── the judge's own proof: it must be able to say NO, and the row that matters must be one of them ───

fn recorded_case(status: u16, body: &str, upstream: Option<Upstream>) -> Recorded {
    Recorded {
        method: "POST".into(),
        path: "/v1/messages".into(),
        status,
        headers: vec!["content-type: application/json".into()],
        bytes: body.len(),
        body: body.to_string(),
        upstream,
    }
}

fn observed_case(status: u16, body: &str, upstream: Option<Upstream>) -> Observed {
    Observed {
        label: "a case".into(),
        status,
        headers: vec!["content-type: application/json".into()],
        bytes: body.len(),
        body: body.to_string(),
        upstream,
    }
}

const ONE_CASE: &[Case] = &[case("a case", "POST", "/v1/messages", &[], None, None)];

#[test]
fn a_missing_upstream_header_is_not_a_pass() {
    let full = Some(Upstream {
        method: "POST".into(),
        path: "/zen/go/v1/messages".into(),
        headers: vec!["x-api-key: worker-key-1234567890".into()],
    });
    let without = Some(Upstream {
        method: "POST".into(),
        path: "/zen/go/v1/messages".into(),
        headers: vec![],
    });
    let expected = [recorded_case(200, "same bytes", full)];
    let observed = [observed_case(200, "same bytes", without)];
    assert_eq!(
        judge("mutation", ONE_CASE, &expected, &observed),
        1,
        "a dropped upstream header must be a finding"
    );
}

#[test]
fn a_body_that_moved_is_not_a_pass_even_at_the_same_status() {
    // The structural difference this corpus was built around: two error envelopes, both 4xx.
    let expected = [recorded_case(
        400,
        r#"{"type":"error","error":{"type":"authentication_error"}}"#,
        None,
    )];
    let observed = [observed_case(
        400,
        r#"{"type":"error","error":{"type":"not_found_error"}}"#,
        None,
    )];
    assert_eq!(judge("mutation", ONE_CASE, &expected, &observed), 1);
}

#[test]
#[should_panic(expected = "recorded from a different")]
fn a_fixture_from_another_case_list_is_refused() {
    let expected = [recorded_case(200, "a", None), recorded_case(200, "b", None)];
    let observed = [observed_case(200, "a", None)];
    judge("stale fixture", ONE_CASE, &expected, &observed);
}
