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
