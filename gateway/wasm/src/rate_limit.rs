//! THE RATE-LIMIT GUARD, MOVED FROM `gateway/src/plugins/translate.ts`.
//!
//! WHY THIS ONE. It is the only thing standing between a token and an unbounded bill, and its shape is
//! three separate things that a port can get wrong independently: WHICH requests it applies to, WHICH
//! counters it reads, and WHAT it answers when one is over. The source keeps them in one function because
//! the state lives in module-level maps; here the DECISION is pure and the STATE is a value, so the clock
//! and the counters are arguments rather than globals.
//!
//! THE CLOCK IS AN ARGUMENT FOR A REASON: `Math.floor(Date.now() / 60000)` is the minute bucket and
//! `Math.floor(Date.now() / 86400000)` is the day bucket, so a test that cannot move the clock can only
//! ever exercise one bucket — and the buckets are what the limits are counted in.

/// `checkRateLimit`'s gate: does the guard apply to this request AT ALL?
///
/// Four tests, and the LAST one is REDUNDANT — MEASURED, NOT ASSUMED. The clause exists so that a
/// token-count request cannot consume a generation's budget, but `/v1/messages/count_tokens` does not end
/// with `/messages`, so the suffix test already excludes it: replacing the clause with `true` changes NO
/// case in the corpus (the mutation was planted and the count_tokens case still passed with `gate=false`).
/// It is kept rather than deleted, because deleting a guard whose redundancy rests on a path spelling is a
/// behaviour change the day that spelling moves — the same reasoning `oxAlphaReasoningDefault` records for
/// its own redundant gate. What the mutation DID prove is where the guard really lives: in the suffix list.
///
/// The suffix tests are `endsWith`, not equality, for the same reason `detectRoute`'s are.
pub fn rate_limit_gate(has_keys: bool, method: &str, path: &str) -> bool {
    has_keys
        && method == "POST"
        && (path.ends_with("/messages")
            || path.ends_with("/chat/completions")
            || path.ends_with("/responses"))
        && !path.ends_with(super::request_shape::COUNT_PATH)
}

/// The two bucket keys, `min:<token>:<bucket>` and `day:<token>:<bucket>`.
///
/// **`Math.floor` IS NOT TRUNCATION FOR A NEGATIVE NUMBER**, which is why this is `div_euclid` rather
/// than `/`: `Date.now()` is not negative in practice, but a port that says `Math.floor` and means
/// truncation is a port that is wrong about the function it copied. The corpus pins the arithmetic with a
/// fixed clock.
pub fn rate_limit_keys(token: &str, now_ms: i64) -> (String, String) {
    let minute = now_ms.div_euclid(60_000);
    let day = now_ms.div_euclid(86_400_000);
    (
        format!("min:{token}:{minute}"),
        format!("day:{token}:{day}"),
    )
}

/// The limits themselves — 48/minute and 4000/day, both checked BEFORE the counters move.
pub fn rate_limit_decision(minute: u32, day: u32) -> Option<super::responses::Built> {
    if minute >= 48 {
        return Some(super::responses::json_error(
            429,
            "Rate limit: ~60 requests/minute per token",
            "rate_limit_error",
        ));
    }
    if day >= 4000 {
        return Some(super::responses::json_error(
            429,
            "Rate limit: ~5000 requests/day per token",
            "rate_limit_error",
        ));
    }
    None
}

/// The counters, IN INSERTION ORDER — and that is not an implementation detail.
///
/// The JavaScript's `__rlMin`/`__rlDay` are `Map`s, and the eviction is
/// `__rlMin.delete(__rlMin.keys().next().value)` — "drop the OLDEST key". A `HashMap` here would drop
/// whatever the hasher felt like, which is the bug a flaky test caught in the relay's queue on
/// 2026-10-02. A `Vec` of pairs keeps the insertion order the `Map` promises, and the queues are tiny.
#[derive(Default)]
pub struct RateLimitState {
    minute: Vec<(String, u32)>,
    day: Vec<(String, u32)>,
}

/// The `Map.size > 4096` ceiling. Both maps carry it separately in the source.
const MAX_ENTRIES: usize = 4096;

impl RateLimitState {
    fn bump(map: &mut Vec<(String, u32)>, key: &str) {
        match map.iter_mut().find(|(k, _)| k == key) {
            Some((_, count)) => *count += 1,
            None => map.push((key.to_string(), 1)),
        }
        // `if (map.size > 4096) map.delete(map.keys().next().value)` — the OLDEST entry, which is the
        // FIRST in a Map's iteration order.
        if map.len() > MAX_ENTRIES {
            map.remove(0);
        }
    }

    /// The whole guard, with the clock and the counters as state: `None` means "allowed, and counted".
    pub fn check(
        &mut self,
        has_keys: bool,
        method: &str,
        path: &str,
        token: &str,
        now_ms: i64,
    ) -> Option<super::responses::Built> {
        if !rate_limit_gate(has_keys, method, path) {
            return None;
        }
        let (mk, dk) = rate_limit_keys(token, now_ms);
        let minute = self
            .minute
            .iter()
            .find(|(k, _)| *k == mk)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        let day = self
            .day
            .iter()
            .find(|(k, _)| *k == dk)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        if let Some(refusal) = rate_limit_decision(minute, day) {
            // **A REFUSED REQUEST DOES NOT COUNT.** Both `return`s sit ABOVE the two `set` calls, so a
            // token that is over its limit does not push the counter further over it.
            return Some(refusal);
        }
        Self::bump(&mut self.minute, &mk);
        Self::bump(&mut self.day, &dk);
        None
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `rate_limit` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`) with `Date.now` pinned, and this replays them.
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
    fn every_rate_limit_case_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            if case["fn"].as_str() != Some("rate_limit") {
                continue;
            }
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let has_keys = input["hasKeys"].as_bool().unwrap_or(false);
            let method = input["method"].as_str().unwrap_or("");
            let path = input["path"].as_str().unwrap_or("");
            let token = input["token"].as_str().unwrap_or("");
            let now = input["now"].as_i64().unwrap_or(0);
            let mut state = RateLimitState::default();
            // `preload` primes the counters the way the shipping worker's maps would already be primed —
            // the limits are reached by COUNTING, not by a magic value.
            for _ in 0..input["preload"].as_u64().unwrap_or(0) {
                state.check(true, "POST", "/v1/messages", token, now);
            }
            // `spread` reaches the DAY counter, and it has to advance the clock the same way the oracle
            // did: the minute limit is checked first, so 4000 counts in one minute are refused at 48.
            let spread = input["spread"].as_u64().unwrap_or(0);
            if spread > 0 {
                let mut done = 0u64;
                let mut minute = 0i64;
                while done < spread {
                    let at = now + minute * 60_000;
                    let mut i = 0;
                    while i < 48 && done < spread {
                        state.check(true, "POST", "/v1/messages", token, at);
                        i += 1;
                        done += 1;
                    }
                    minute += 1;
                }
            }
            let got = state.check(has_keys, method, path, token, now);
            let want = &case["expected"];
            if want.get("null").is_some() {
                assert!(
                    got.is_none(),
                    "rate_limit / {name}: the TypeScript allows it"
                );
            } else {
                let got = got.unwrap_or_else(|| panic!("rate_limit / {name}: this allowed it"));
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
            }
            checked += 1;
        }
        assert!(
            checked >= 8,
            "the rate-limit corpus shrank to {checked} cases"
        );
    }
}
