//! THE PER-IP RATE LIMIT, MOVED FROM `gateway/src/lib/ratelimit.ts` — the gate the devices plugin puts in front
//! of its three PUBLIC routes.
//!
//! WHY IT MOVED WITH THE FAMILY. `/api/register`, `/api/devices/self-register` and `/api/install/tunnel-token`
//! are the routes an unauthenticated caller can make WRITE KV (`regclaim` put + delete, `regkey` read, the
//! device record) and the source's own comment measures the attack they were written for: "an attacker firing
//! random keys otherwise burned 2 KV writes per attempt … exhausting the daily KV write quota". A port that
//! answered identically but dropped this gate would look identical in every response case and cost the
//! deployment its KV budget — which is exactly the class of change no byte comparison can see.
//!
//! THE DECISION IS PURE AND THE STATE IS A VALUE: the source keeps `counters` in module scope and reads
//! `Date.now()`; here the counter table is passed in and the clock is a parameter, so the window arithmetic is
//! testable without waiting a minute for a bucket to turn over.
//!
//! **`kvSeed` IS NOT PORTED, AND THAT IS A DECISION RATHER THAN AN OMISSION.** The factory has two modes and the
//! devices plugin instantiates the memory-only one (`createIpRateLimiter({name:"pub-rate", limit:10,
//! windowMs:60_000})` — no `kvSeed`, whose comment says why: "these endpoints cost 2-3 KV writes per attempt
//! themselves; a per-request KV write HERE would let an attacker exhaust the Free-plan daily KV write quota").
//! In that mode the KV arms of the source are unreachable, so porting them would be porting dead code.

/// `createIpRateLimiter({name, limit, windowMs})` — the three parameters, as a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpRateLimiter {
    pub name: &'static str,
    pub limit: u32,
    pub window_ms: i64,
}

/// **THE PROBE'S OWN INSTANCE, VERBATIM FROM `gateway/src/tooling.ts`:**
/// `createIpRateLimiter({ name: "probe-rate", limit: 60, windowMs: 60_000, kvSeed: true })`.
/// It is the KV-SEEDED one, and the seed is Task 2's; what matters here is that the numbers
/// live in Rust with the algorithm rather than beside the call site.
pub const PROBE_RATE: IpRateLimiter = IpRateLimiter {
    name: "probe-rate",
    limit: 60,
    window_ms: 60_000,
};

/// The route family this limiter guards: `index.ts:298` calls `probeRateLimited` for
/// `/api/summrise-probe` and nothing else.
pub fn probe_in_family(path: &str) -> bool {
    path == "/api/summrise-probe"
}

/// The devices plugin's own instance, verbatim: `name: "pub-rate", limit: 10, windowMs: 60_000`.
pub const PUBLIC_RATE: IpRateLimiter = IpRateLimiter {
    name: "pub-rate",
    limit: 10,
    window_ms: 60_000,
};

/// The source's `counters` — insertion-ordered, because the 4096-key bound evicts the OLDEST.
pub type Counters = Vec<(String, u32)>;

/// `Math.floor(Date.now() / windowMs)` — `div_euclid`, not `/`, for the same reason `rate_limit.rs` uses it:
/// `Math.floor` is not truncation for a negative number.
pub fn bucket_of(now_ms: i64, window_ms: i64) -> i64 {
    now_ms.div_euclid(window_ms)
}

/// `${name}:${ip}:${bucket}` — `cf-connecting-ip`, or the literal `"unknown"` when the header is absent.
pub fn counter_key(limiter: IpRateLimiter, ip: &str, now_ms: i64) -> String {
    format!(
        "{}:{}:{}",
        limiter.name,
        ip,
        bucket_of(now_ms, limiter.window_ms)
    )
}

/// The limiter's whole decision: mutate the table and answer whether THIS request is over budget.
///
/// THE TWO ARMS DIFFER, and the asymmetry is the source's: an existing counter that is already at the limit
/// answers `true` WITHOUT incrementing, while a fresh key inserts `cur + 1` and answers `cur >= limit` — which
/// for the memory-only mode is always `false` on the first hit of a bucket. A port that "simplified" this into
/// one arm would either let the 11th request through or refuse the 10th.
pub fn check(limiter: IpRateLimiter, counters: &mut Counters, ip: &str, now_ms: i64) -> bool {
    let key = counter_key(limiter, ip, now_ms);
    if let Some(entry) = counters.iter_mut().find(|(k, _)| *k == key) {
        if entry.1 >= limiter.limit {
            return true;
        }
        entry.1 += 1;
        return false;
    }
    let cur = 0;
    counters.push((key, cur + 1));
    // `if (counters.size > 4096) { delete the first key }` — the insertion-order eviction, and it happens only
    // on this arm because only this arm can grow the table.
    if counters.len() > 4096 {
        counters.remove(0);
    }
    cur >= limiter.limit
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE BUDGET IS TEN, AND THE ELEVENTH IS THE ONE THAT MUST BE REFUSED.**
    ///
    /// MUTATION: change the existing-counter arm's `>=` to `>`.
    /// RESULT:   `the_eleventh_request_is_refused` fails — the eleventh request is allowed and the twelve-month
    ///           KV-write exhaustion the gate exists for is back, with every response case still green.
    #[test]
    fn the_eleventh_request_is_refused() {
        let mut counters = Counters::new();
        for n in 1..=10 {
            assert!(
                !check(PUBLIC_RATE, &mut counters, "1.2.3.4", 0),
                "request {n} must pass"
            );
        }
        assert!(
            check(PUBLIC_RATE, &mut counters, "1.2.3.4", 0),
            "the 11th must be refused"
        );
        // ...and the refusal does not consume anything: the 12th is refused too.
        assert!(check(PUBLIC_RATE, &mut counters, "1.2.3.4", 0));
        assert_eq!(counters[0].1, 10, "a refused request does not increment");
    }

    /// A DIFFERENT IP IS A DIFFERENT BUDGET, and a different WINDOW is a fresh one — the two ways the gate
    /// could be too strict (a shared bucket would lock the console out of its own install flow).
    #[test]
    fn the_budget_is_per_ip_and_per_window() {
        let mut counters = Counters::new();
        for _ in 0..10 {
            assert!(!check(PUBLIC_RATE, &mut counters, "1.2.3.4", 0));
        }
        assert!(check(PUBLIC_RATE, &mut counters, "1.2.3.4", 0));
        assert!(
            !check(PUBLIC_RATE, &mut counters, "5.6.7.8", 0),
            "another IP has its own budget"
        );
        assert!(
            !check(PUBLIC_RATE, &mut counters, "1.2.3.4", 60_000),
            "the next window is fresh"
        );
        assert_eq!(counter_key(PUBLIC_RATE, "unknown", 0), "pub-rate:unknown:0");
        assert_eq!(
            counter_key(PUBLIC_RATE, "1.2.3.4", 59_999),
            "pub-rate:1.2.3.4:0"
        );
        assert_eq!(
            counter_key(PUBLIC_RATE, "1.2.3.4", 60_000),
            "pub-rate:1.2.3.4:1"
        );
    }

    /// The 4096 bound evicts the OLDEST key, which is the source's `counters.keys().next().value`.
    #[test]
    fn the_table_evicts_the_oldest_at_the_bound() {
        let mut counters = Counters::new();
        for i in 0..4096 {
            check(
                PUBLIC_RATE,
                &mut counters,
                &format!("10.0.{}.{}", i / 256, i % 256),
                0,
            );
        }
        assert_eq!(counters.len(), 4096);
        check(PUBLIC_RATE, &mut counters, "9.9.9.9", 0);
        assert_eq!(counters.len(), 4096);
        assert_eq!(counters[0].0, "pub-rate:10.0.0.1:0", "the oldest went");
    }
}

    /// **THE PROBE'S BUDGET IS SIXTY, AND THE SIXTY-FIRST IS THE ONE THAT MUST BE REFUSED** — the same
    /// assertion as the devices instance, against the OTHER constant, because a port that carries the
    /// algorithm but the wrong numbers is a limiter with the wrong budget and every case above still passes.
    ///
    /// MUTATION: change `PROBE_RATE.limit` to 10.
    /// RESULT:   this test fails — the eleventh probe is refused while the corpus is green.
    #[test]
    fn the_sixty_first_probe_is_refused() {
        // **SIXTY IS WRITTEN OUT, NOT READ FROM THE CONSTANT — AND THAT IS THE WHOLE POINT.**
        // The first version looped `0..PROBE_RATE.limit`, so it asserted "the (limit+1)th call is
        // refused" for ANY limit, and the mutation below passed: `limit: 10` made the loop ten long
        // and the eleventh call refused, exactly as written. **A test that reads its own subject
        // cannot fail when the subject changes.**
        let mut counters = Counters::new();
        for n in 1..=60 {
            assert!(
                !check(PROBE_RATE, &mut counters, "203.0.113.7", 1_700_000_000_000),
                "probe {n} of 60 was refused"
            );
        }
        assert!(check(PROBE_RATE, &mut counters, "203.0.113.7", 1_700_000_000_000));
    }

    /// And the family is the probe route, not a prefix that would swallow a neighbour.
    #[test]
    fn the_family_is_the_probe_route() {
        assert!(probe_in_family("/api/summrise-probe"));
        assert!(!probe_in_family("/api/summrise-probe/extra"));
        assert!(!probe_in_family("/api/health"));
    }
