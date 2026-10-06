//! THE RETRY, TIMEOUT AND FAILURE-CLASSIFICATION DECISIONS, MOVED FROM `gateway/src/reliability.ts`.
//!
//! WHY THESE. The timeout budgets are what stand between a stalled upstream and a client that waits for
//! ever, and the retry policy is a MEASURED table that used to be copy-pasted at two call sites "with
//! comments trying to keep them in sync". The failure classifier is the circuit breaker's input, and its
//! own comment says what happens if it is weakened: a channel that stalls and times out every time must
//! open the circuit, or every user burns the 120 s timeout indefinitely — the DoS-by-timeout class.
//!
//! **`Number(...)` IS THE HARD PART.** `Number.isFinite(v) && v > 0 ? v : DEFAULT` reads like a plain
//! validation and is not one: `Number("")` is 0, `Number("  12  ")` is 12, `Number("0x10")` is 16,
//! `Number("abc")` is NaN, `Number(null)` is 0 and `Number([])` is 0. The corpus carries all of those,
//! because a port that used a plain integer parse would answer a different budget for the same env.

/// The OpenRouter model whose free tier needs the longer retry policy.
pub const GLM_LOTTERY_MODEL: &str = "z-ai/glm-5.2:free";

/// The default upstream budget, and og's — the numbers the module's own comments justify.
pub const UPSTREAM_TIMEOUT_MS: i64 = 30_000;
pub const OG_TIMEOUT_MS: i64 = 120_000;

/// `Number(x)` for the shapes an env value can take — the engine's coercion, not a parse.
///
/// `NaN` is the answer for anything that is not a number, and the callers' `Number.isFinite` test is what
/// turns it into the default. An ARRAY coerces through its string form (`[]` -> `""` -> 0, `[5]` -> "5"),
/// which is why this handles it rather than refusing it.
pub fn js_number(v: &serde_json::Value) -> f64 {
    match v {
        serde_json::Value::Null => 0.0,
        serde_json::Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        serde_json::Value::String(s) => js_number_of_string(s),
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                0.0
            } else if items.len() == 1 {
                js_number(&items[0])
            } else {
                f64::NAN
            }
        }
        serde_json::Value::Object(_) => f64::NAN,
    }
}

/// `Number(str)`: trim, then the empty string is 0, the three prefixed bases are integers, and anything
/// else must be a complete decimal literal (or `Infinity`).
fn js_number_of_string(s: &str) -> f64 {
    let t =
        s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000c}' | '\u{000b}'));
    if t.is_empty() {
        return 0.0;
    }
    let lower = t.to_ascii_lowercase();
    let prefixed = |prefix: &str, radix: u32| -> Option<f64> {
        lower
            .strip_prefix(prefix)
            .filter(|rest| !rest.is_empty())
            .and_then(|rest| i64::from_str_radix(rest, radix).ok())
            .map(|n| n as f64)
    };
    if let Some(v) = prefixed("0x", 16) {
        return v;
    }
    if let Some(v) = prefixed("0o", 8) {
        return v;
    }
    if let Some(v) = prefixed("0b", 2) {
        return v;
    }
    match lower.as_str() {
        "infinity" | "+infinity" => f64::INFINITY,
        "-infinity" => f64::NEG_INFINITY,
        _ => lower.parse::<f64>().unwrap_or(f64::NAN),
    }
}

/// The shape of a retry policy. `None` is an ABSENT key, which is not the same as a false one: the
/// JavaScript builds each arm as an object literal, so `attempts`/`backoffMs`/`retry502`/
/// `ignoreRetryAfter` are present or missing per arm.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    pub timeout_ms: f64,
    pub attempts: Option<i64>,
    pub backoff_ms: Option<i64>,
    pub retry502: Option<bool>,
    pub ignore_retry_after: Option<bool>,
}

/// `retryPolicyFor(kind, upstreamModel, timeoutMs)` — three arms, and the middle one is two.
///
/// nv and gmi are the bursty pair (4 attempts, retry on 502). OpenRouter takes the same policy EXCEPT for
/// the GLM lottery model, which gets ten attempts, a 300 ms backoff and `ignoreRetryAfter`. Everything
/// else carries the timeout alone.
pub fn retry_policy_for(kind: &str, upstream_model: &str, timeout_ms: f64) -> RetryPolicy {
    let bursty = RetryPolicy {
        timeout_ms,
        attempts: Some(4),
        backoff_ms: None,
        retry502: Some(true),
        ignore_retry_after: None,
    };
    if kind == "nvidia" || kind == "gmi" {
        return bursty;
    }
    if kind == "openrouter" {
        if upstream_model == GLM_LOTTERY_MODEL {
            return RetryPolicy {
                timeout_ms,
                attempts: Some(10),
                backoff_ms: Some(300),
                retry502: Some(true),
                ignore_retry_after: Some(true),
            };
        }
        return bursty;
    }
    RetryPolicy {
        timeout_ms,
        attempts: None,
        backoff_ms: None,
        retry502: None,
        ignore_retry_after: None,
    }
}

/// `fetchWithRetry`'s own defaults — the destructuring defaults in its options object, not a table.
pub const DEFAULT_ATTEMPTS: i64 = 3;
pub const DEFAULT_BACKOFF_MS: f64 = 750.0;
pub const DEFAULT_MAX_WAIT_MS: f64 = 10_000.0;

/// `mayRetry5xx(status)`: "Treat 502/503 as retryable — caller asserts the upstream rejects BEFORE processing,
/// so re-sending cannot double-bill a BYOK key. Other 5xx stay gated by `idempotent`."
pub fn may_retry_5xx(status: u16, idempotent: bool, retry502: bool) -> bool {
    if status == 502 || status == 503 {
        idempotent || retry502
    } else {
        idempotent
    }
}

/// What `fetchWithRetry` does with ONE response — and the two details it can carry.
///
/// **THE DETAIL IS CLIENT-VISIBLE**: the arms put it in the error body when there is no response to normalize
/// (`${label}: ${detail}`), so "upstream 500 (not retried — POST may have been billed)" is a sentence a user
/// reads, and its exact spelling is part of the port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    /// Wait, then try again. The detail is what the source sets before continuing.
    Retry { detail: String },
    /// Hand this response back. An empty detail is the source's `{ response: last, detail: "" }`.
    Stop { detail: String },
}

/// The loop's decision for a response, in the source's own order — and the ORDER is the behaviour.
///
/// `ok || !(status >= 500 || status === 429)` stops on everything the caller must see (a 4xx that is not 429,
/// and every 2xx); then the retryable branch (429 always, 5xx per `mayRetry5xx`); then the "billing guard"
/// detail for a 5xx that may have been processed already.
pub fn retry_decision(
    status: u16,
    attempt: i64,
    attempts: i64,
    idempotent: bool,
    retry502: bool,
) -> RetryDecision {
    let ok = (200..=299).contains(&status);
    if ok || !(status >= 500 || status == 429) {
        return RetryDecision::Stop {
            detail: String::new(),
        };
    }
    if status == 429 || (status >= 500 && may_retry_5xx(status, idempotent, retry502)) {
        let detail = format!("upstream {status} (retried {attempt}/{attempts})");
        return if attempt < attempts {
            RetryDecision::Retry { detail }
        } else {
            RetryDecision::Stop { detail }
        };
    }
    if attempt >= attempts {
        return RetryDecision::Stop {
            detail: format!("upstream {status} (retried {attempt}/{attempts})"),
        };
    }
    RetryDecision::Stop {
        detail: format!("upstream {status} (not retried — POST may have been billed)"),
    }
}

/// `retryWaitMs()` — with the clock's two inputs as ARGUMENTS: `jitter` (`Math.floor(Math.random() * 200)`)
/// and the upstream's `retry-after`, which is read off the response the loop just got.
///
/// `Number(last?.headers?.get?.("retry-after"))` is a COERCION: an absent header is `Number(undefined)` = NaN,
/// and `Number.isFinite(ra) && ra > 0` then refuses it — so a header of `"0"`, `"-5"` or `"later"` all fall
/// back to the ladder. Everything is clamped to `maxWaitMs` so "a huge header can't stall the request".
pub fn retry_wait_ms(
    ignore_retry_after: bool,
    retry_after: Option<&str>,
    backoff_ms: f64,
    attempt: i64,
    jitter: f64,
    max_wait_ms: f64,
) -> f64 {
    if ignore_retry_after {
        return (backoff_ms + jitter).min(max_wait_ms);
    }
    let ra = match retry_after {
        Some(raw) => crate::reliability::js_number(&serde_json::Value::String(raw.to_string())),
        None => f64::NAN,
    };
    let ra_ms = if ra.is_finite() && ra > 0.0 { ra * 1000.0 } else { 0.0 };
    (ra_ms.max(backoff_ms * attempt as f64) + jitter).min(max_wait_ms)
}

/// The shared body of `upstreamTimeoutMs` and `ogTimeoutMs`: `Number.isFinite(v) && v > 0 ? v : DEFAULT`.
///
/// **THE ORDER IS THE SEMANTICS.** `Number.isFinite` refuses `Infinity` and `NaN` alike, and `v > 0`
/// refuses 0 and the negatives — so `""`, `null`, `"abc"`, `Infinity`, `"0"` and `"-5"` ALL take the
/// default, while `"  12  "` and `"0x10"` do not.
pub fn timeout_from(env: Option<&serde_json::Value>, key: &str, default_ms: i64) -> f64 {
    let raw = env.and_then(|e| e.get(key));
    let v = match raw {
        Some(value) => js_number(value),
        None => f64::NAN,
    };
    // **THE ANSWER IS A NUMBER, NOT A COUNT.** `1500.7` is what the JavaScript returns for that env, so
    // this returns the float rather than truncating it — the corpus has the case.
    if v.is_finite() && v > 0.0 {
        v
    } else {
        default_ms as f64
    }
}

/// `upstreamTimeoutMs(env)` — the generic 30 s budget, `UPSTREAM_TIMEOUT_MS` over it.
pub fn upstream_timeout_ms(env: Option<&serde_json::Value>) -> f64 {
    timeout_from(env, "UPSTREAM_TIMEOUT_MS", UPSTREAM_TIMEOUT_MS)
}

/// `ogTimeoutMs(env)` — the 120 s budget for zen, whose latency intermittently spikes past 30 s and whose
/// real max-thinking requests run 40–54 s to first byte. It gates TIME-TO-HEADERS only: once the SSE
/// stream starts it runs untimed.
pub fn og_timeout_ms(env: Option<&serde_json::Value>) -> f64 {
    timeout_from(env, "OG_TIMEOUT_MS", OG_TIMEOUT_MS)
}

/// `passthroughTimeoutMs(env, kind)` — which budget a passthrough channel gets.
///
/// Three kinds share og's: `opencode` (zen itself), `commandgoat` (a third-party gateway with the same
/// long-thinking profile) and `amd` (a cold-starting self-deploy router whose GLM backend measured >40 s
/// to first byte under load). Every other passthrough keeps the generic budget.
pub fn passthrough_timeout_ms(env: Option<&serde_json::Value>, kind: &str) -> f64 {
    if kind == "opencode" || kind == "commandgoat" || kind == "amd" {
        og_timeout_ms(env)
    } else {
        upstream_timeout_ms(env)
    }
}

/// `isChannelDownFailure(detail)` — the circuit breaker's input, and its semantics are deliberate.
///
/// HARD network errors and FULL TIMEOUTS both count, because a channel that stalls and times out every
/// time must open the circuit or every user burns the 120 s timeout indefinitely. Fast 5xx/429 do NOT
/// count (the retry layer absorbs those), and neither do short latency spikes. The `!!detail` is first,
/// so an empty string is not a failure.
pub fn is_channel_down_failure(detail: Option<&str>) -> bool {
    match detail {
        Some(d) if !d.is_empty() => d.starts_with("network error") || d.starts_with("timeout"),
        _ => false,
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `reliability` cases were produced by the SHIPPING TypeScript
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

    /// **A WHOLE NUMBER IS NOT A FLOAT ON THE WIRE.** `JSON.stringify(30000)` is `30000` while
    /// `serde_json::json!(30000.0)` is `30000.0`, so both sides are compared as `f64` — what a JS number
    /// IS — rather than as two Values that differ in a way the JavaScript does not.
    fn same_number(got: f64, want: &serde_json::Value) -> bool {
        match want.as_f64() {
            Some(w) => got == w,
            None => false,
        }
    }

    /// `JSON.stringify` writes `30000` for a whole number and `1500.7` for the other one, so the fixture's
    /// shape is reproduced rather than a float that means the same thing.
    fn num(v: f64) -> serde_json::Value {
        if v.fract() == 0.0 && v.abs() < 9e15 {
            serde_json::json!(v as i64)
        } else {
            serde_json::json!(v)
        }
    }

    fn policy_to_json(p: &RetryPolicy) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        m.insert("timeoutMs".into(), num(p.timeout_ms));
        if let Some(v) = p.attempts {
            m.insert("attempts".into(), serde_json::json!(v));
        }
        if let Some(v) = p.backoff_ms {
            m.insert("backoffMs".into(), serde_json::json!(v));
        }
        if let Some(v) = p.retry502 {
            m.insert("retry502".into(), serde_json::json!(v));
        }
        if let Some(v) = p.ignore_retry_after {
            m.insert("ignoreRetryAfter".into(), serde_json::json!(v));
        }
        serde_json::Value::Object(m)
    }

    /// **THE RETRY LOOP'S TWO DECISIONS, PINNED AGAINST THE MEASURED BEHAVIOUR OF THE SHIPPING ROUTE.**
    ///
    /// MUTATION: swap the two arms' policies back (`translate_retry_policy` answering `attempts: Some(4),
    ///           retry502: Some(true)`).
    /// RESULT:   the 503 case below stops being "not retried" and the 429 case's last attempt becomes 4 — which
    ///           is exactly what the divergence sweep measured on 2026-10-06 before the swap was fixed: a 503
    ///           made the shipping route dial ONCE and this worker FOUR times, a 429 three against four.
    #[test]
    fn the_retry_decision_is_the_sources_order() {
        use RetryDecision::{Retry, Stop};
        // The translate arm's policy: `{ timeoutMs }` alone, so 3 attempts and no 502 retry.
        let translate = crate::request_shape::translate_retry_policy(None);
        let attempts = translate.attempts.unwrap_or(DEFAULT_ATTEMPTS);
        let retry502 = translate.retry502.unwrap_or(false);
        assert_eq!(attempts, 3, "the source's default");
        assert!(!retry502, "and no 502 retry");

        // A 2xx stops with an empty detail.
        assert_eq!(
            retry_decision(200, 1, attempts, false, retry502),
            Stop {
                detail: String::new()
            }
        );
        // A 4xx that is not 429 stops the same way.
        assert_eq!(
            retry_decision(404, 1, attempts, false, retry502),
            Stop {
                detail: String::new()
            }
        );
        // A 429 retries, and the detail names the attempt.
        assert_eq!(
            retry_decision(429, 1, attempts, false, retry502),
            Retry {
                detail: "upstream 429 (retried 1/3)".to_string()
            }
        );
        // Its LAST attempt stops with the same sentence.
        assert_eq!(
            retry_decision(429, 3, attempts, false, retry502),
            Stop {
                detail: "upstream 429 (retried 3/3)".to_string()
            }
        );
        // **A 5xx IS NOT RETRIED — "a 5xx AFTER the upstream processed the request may have billed the user's
        // BYOK key" — and the sentence says so.**
        for status in [500, 502, 503] {
            assert_eq!(
                retry_decision(status, 1, attempts, false, retry502),
                Stop {
                    detail: format!("upstream {status} (not retried — POST may have been billed)")
                },
                "status {status}"
            );
        }
        // The CHAT arm's policy is the other one, and there a 503 IS retried.
        let chat = crate::request_shape::chat_retry_policy(None);
        assert_eq!(chat.attempts, Some(4));
        assert_eq!(
            retry_decision(503, 1, 4, false, true),
            Retry {
                detail: "upstream 503 (retried 1/4)".to_string()
            }
        );
        // And `may_retry_5xx`'s own edge: 502/503 follow the flag, every other 5xx follows `idempotent`.
        assert!(may_retry_5xx(502, false, true));
        assert!(!may_retry_5xx(502, false, false));
        assert!(may_retry_5xx(500, true, false));
        assert!(!may_retry_5xx(500, false, true));
    }

    /// `retryWaitMs()` — the ladder, the header, the clamp and the two `Number.isFinite` refusals.
    #[test]
    fn the_retry_wait_is_the_sources_arithmetic() {
        // The legacy ladder: `backoffMs * attempt`, plus jitter, clamped.
        assert_eq!(retry_wait_ms(false, None, 750.0, 1, 0.0, 10_000.0), 750.0);
        assert_eq!(retry_wait_ms(false, None, 750.0, 2, 199.0, 10_000.0), 1699.0);
        // A `Retry-After` LARGER than the ladder wins...
        assert_eq!(retry_wait_ms(false, Some("5"), 750.0, 1, 0.0, 10_000.0), 5000.0);
        // ...and a smaller one loses to it.
        assert_eq!(retry_wait_ms(false, Some("1"), 750.0, 3, 0.0, 10_000.0), 2250.0);
        // **`Number.isFinite(ra) && ra > 0` REFUSES THESE**, so each falls back to the ladder.
        for bad in ["0", "-5", "later", ""] {
            assert_eq!(
                retry_wait_ms(false, Some(bad), 750.0, 1, 0.0, 10_000.0),
                750.0,
                "retry-after {bad:?}"
            );
        }
        // A huge header cannot stall the request: `Math.min(..., maxWaitMs)`.
        assert_eq!(retry_wait_ms(false, Some("600"), 750.0, 1, 0.0, 10_000.0), 10_000.0);
        // `ignoreRetryAfter` — the OpenRouter lottery's pacing — ignores the header entirely.
        assert_eq!(retry_wait_ms(true, Some("5"), 300.0, 1, 10.0, 10_000.0), 310.0);
    }

    #[test]
    fn every_reliability_decision_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            let env = input.get("env");
            match func {
                "retry_policy_for" => {
                    let got = retry_policy_for(
                        input["kind"].as_str().unwrap_or(""),
                        input["upstreamModel"].as_str().unwrap_or(""),
                        input["timeoutMs"].as_f64().unwrap_or(0.0),
                    );
                    assert_eq!(&policy_to_json(&got), want, "{func} / {name}");
                }
                "upstream_timeout_ms" => {
                    let got = upstream_timeout_ms(env);
                    assert!(
                        same_number(got, want),
                        "{func} / {name}: {got} against {want}"
                    );
                }
                "og_timeout_ms" => {
                    let got = og_timeout_ms(env);
                    assert!(
                        same_number(got, want),
                        "{func} / {name}: {got} against {want}"
                    );
                }
                "passthrough_timeout_ms" => {
                    let got = passthrough_timeout_ms(env, input["kind"].as_str().unwrap_or(""));
                    assert!(
                        same_number(got, want),
                        "{func} / {name}: {got} against {want}"
                    );
                }
                "is_channel_down_failure" => {
                    let got = is_channel_down_failure(input["detail"].as_str());
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 25,
            "the reliability corpus shrank to {checked} cases"
        );
    }
}
