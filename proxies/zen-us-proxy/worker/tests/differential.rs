//! THE ZEN-US DIFFERENTIAL, JUDGED IN RUST — the cases, the upstream answers and the byte comparison.
//!
//! ── WHAT MOVED HERE, AND WHAT DID NOT ───────────────────────────────────────────────────────────
//!
//! `worker/verify.mjs` used to hold all three jobs: run the built wasm module, hold the cases, and judge
//! the bytes. **RUNNING THE MODULE NEEDS A JS ENGINE** — `worker-build`'s output is wasm-bindgen glue that
//! imports `cloudflare:workers` — so that one job is `gateway/wasm/run-cases.mjs`, a runner that takes a
//! spec and prints what it saw. **THE OTHER TWO ARE DATA AND LOGIC**, and they are here, next to the
//! policy they check. The expected bytes are `shipping-answers.json`: what `src/index.js` answered,
//! recorded by the harness on a run where both sides agreed on all 17 cases, before that file was deleted.
//!
//! ── MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────────
//!
//! MUTATION: in `worker.rs`, drop the `sessionHeader` push from the `/responses` arm — one line, and the
//!           response bytes do not change at all, because the session header goes UPSTREAM.
//! RESULT:   `zen-us verify: 2 of 17 case(s) DIFFER`, and the two are the `/responses` upstream cases:
//!             FAIL POST /v1/responses — 29 bytes, status 200
//!             upstream request DIFFERS
//!               recorded: "POST /zen/go/v1/responses | authorization: Bearer caller-key-1234567890 | content-type: application/json | x-opencode-session: s-1"
//!               rust:     "POST /zen/go/v1/responses | authorization: Bearer caller-key-1234567890 | content-type: application/json"
//!           **AND THE `29 bytes, status 200` ON BOTH SIDES IS THE POINT**: a response-only check would
//!           have called it green. The row is what makes this gate real, and
//!           `a_missing_upstream_header_is_not_a_pass` below is that mutation in-process, so the proof
//!           does not depend on the runner.
//!
//! MUTATION (the judge's own first version, measured the hard way): make `build()` skip the build when
//!           `build/index.js` already exists.
//! RESULT:   **THE MUTATION ABOVE PASSES — `4 passed`** — because the judge drove the PREVIOUS build and
//!           never compiled the edit. That is why `build()` always builds; the rule it is an instance of
//!           is already in this repository ("when a test reads a built artifact, `build` must run before
//!           `test`"), and `rust-byte-checks.bash` carries the same lesson from 2026-10-05.
//!
//! MUTATION (the judge itself): make `judge` compare only status and bytes.
//! RESULT:   `a_missing_upstream_header_is_not_a_pass` fails, because the two rows it drops are the ones
//!           that case differs on.

use std::path::PathBuf;

use summrise_differential_harness::{
    build, judge, recorded, repo_root, run, verdict, Answer, Case, Observed, Recorded, Spec,
    Upstream,
};

const WORKER_KEY: &str = "worker-key-1234567890";
const CLIENT_KEY: &str = "client-key-1234567890";
// The caller key appears INLINE in the case headers below rather than as a constant: a `Case`'s headers are
// `&'static str`, and `concat!` is the only way to build `Bearer …` at compile time, which would hide the
// value in a macro instead of showing it where the request is described.

const HOST: &str = "https://zen.test";

const ENV: &[(&str, &str)] = &[
    ("CLIENT_KEY", CLIENT_KEY),
    ("OPENCODE_GO_API_KEY", WORKER_KEY),
];

/// What the stubbed upstream answers, per path.
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
        "/zen/go/v1/responses",
        Answer {
            status: 200,
            headers: &[("content-type", "text/event-stream")],
            body: "data: {\"a\":1}\n\ndata: [DONE]\n\n",
        },
    ),
    (
        "/zen/go/v1/messages",
        Answer {
            status: 200,
            headers: &[("content-type", "application/json")],
            body: r#"{"content":[{"type":"text","text":"hi"}]}"#,
        },
    ),
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
    // The arms that need no upstream.
    case("the preflight", "OPTIONS", "/v1/messages", &[], None, None),
    case(
        "the preflight, on a path no route names",
        "OPTIONS",
        "/anything",
        &[],
        None,
        None,
    ),
    case(
        "GET /v1/models with no x-api-key",
        "GET",
        "/v1/models",
        &[],
        None,
        None,
    ),
    case(
        "GET /v1/models with the WRONG key",
        "GET",
        "/v1/models",
        &[("x-api-key", "wrong-key-1234567890")],
        None,
        None,
    ),
    case(
        "POST /v1/responses with no Authorization",
        "POST",
        "/v1/responses",
        &[],
        None,
        None,
    ),
    case(
        "POST /v1/responses with a non-Bearer Authorization",
        "POST",
        "/v1/responses",
        &[("authorization", "Basic zzz")],
        None,
        None,
    ),
    case(
        "POST /v1/messages with no x-api-key",
        "POST",
        "/v1/messages",
        &[],
        None,
        None,
    ),
    case("a path no route names", "GET", "/nope", &[], None, None),
    // The upstream arms.
    case(
        "GET /v1/models, gated and served",
        "GET",
        "/v1/models",
        &[("x-api-key", CLIENT_KEY)],
        None,
        None,
    ),
    case(
        "POST /v1/responses — BYOK, and the session header goes UPSTREAM",
        "POST",
        "/v1/responses",
        &[
            ("authorization", "Bearer caller-key-1234567890"),
            ("content-type", "application/json"),
            ("x-opencode-session", "s-1"),
        ],
        Some(r#"{"model":"m"}"#),
        None,
    ),
    case(
        "POST /v1/responses — the SECOND source header, when the first is absent",
        "POST",
        "/v1/responses",
        &[
            ("authorization", "Bearer caller-key-1234567890"),
            ("x-session-id", "s-2"),
        ],
        Some("{}"),
        None,
    ),
    case(
        "POST /v1/responses — a WHITESPACE-ONLY session header is not forwarded",
        "POST",
        "/v1/responses",
        &[
            ("authorization", "Bearer caller-key-1234567890"),
            ("x-opencode-session", "   "),
        ],
        Some("{}"),
        None,
    ),
    case(
        "POST /v1/messages, gated and served",
        "POST",
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        Some(r#"{"model":"m"}"#),
        None,
    ),
    // The failure paths, where the provider's own words reach the client — and must not carry a key.
    case(
        "a 4xx whose message ECHOES THE WORKER'S KEY — it must be redacted",
        "POST",
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        Some("{}"),
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
        &[("x-api-key", CLIENT_KEY)],
        Some("{}"),
        Some(Answer {
            status: 503,
            headers: &[("content-type", "application/json")],
            body: r#"{"error":{"message":"upstream exploded"}}"#,
        }),
    ),
    case(
        "a 429 on /responses — the `err.message` branch of the relay",
        "POST",
        "/v1/responses",
        &[
            ("authorization", "Bearer caller-key-1234567890"),
            ("x-api-key", "caller-api-key-1234"),
        ],
        Some("{}"),
        Some(Answer {
            status: 429,
            headers: &[("content-type", "application/json")],
            body: r#"{"message":"nope"}"#,
        }),
    ),
    case(
        "a 4xx whose body is NOT JSON — the default message",
        "POST",
        "/v1/responses",
        &[("authorization", "Bearer caller-key-1234567890")],
        Some("{}"),
        Some(Answer {
            status: 400,
            headers: &[],
            body: "not json at all",
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
    let bad = judge("zen-us", CASES, &expected, &observed);
    verdict("zen-us", CASES.len(), bad);
}

// ── the judge's own proof: it must be able to say NO, and the row that matters must be one of them ───

fn recorded_case(status: u16, body: &str, upstream: Option<Upstream>) -> Recorded {
    Recorded {
        method: "POST".into(),
        path: "/v1/responses".into(),
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

const ONE_CASE: &[Case] = &[case("a case", "POST", "/v1/responses", &[], None, None)];

#[test]
fn a_missing_upstream_header_is_not_a_pass() {
    // **THE MUTATION THE HEADER DESCRIBES, IN PROCESS.** Same status, same headers, same bytes — the
    // session header only goes upstream — so a judge comparing responses alone would call this equal.
    let full = Some(Upstream {
        method: "POST".into(),
        path: "/zen/go/v1/responses".into(),
        headers: vec![
            "authorization: Bearer …".into(),
            "x-opencode-session: s-1".into(),
        ],
    });
    let without = Some(Upstream {
        method: "POST".into(),
        path: "/zen/go/v1/responses".into(),
        headers: vec!["authorization: Bearer …".into()],
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
fn a_response_byte_that_moved_is_not_a_pass() {
    let expected = [recorded_case(200, "the shipping bytes", None)];
    let observed = [observed_case(200, "the rust bytes", None)];
    assert_eq!(judge("mutation", ONE_CASE, &expected, &observed), 1);
    // ...and a status that moved, with the bytes equal.
    let observed = [observed_case(500, "the shipping bytes", None)];
    assert_eq!(judge("mutation", ONE_CASE, &expected, &observed), 1);
}

#[test]
#[should_panic(expected = "recorded from a different")]
fn a_fixture_from_another_case_list_is_refused() {
    let expected = [recorded_case(200, "a", None), recorded_case(200, "b", None)];
    let observed = [observed_case(200, "a", None)];
    judge("stale fixture", ONE_CASE, &expected, &observed);
}
