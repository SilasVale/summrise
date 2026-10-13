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
use worker::Env;
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

/// **THE SEED'S TWO DECISIONS, PURE — BECAUSE THE EXECUTION IS A PLATFORM CALL.**
///
/// The TypeScript's source is `gateway/src/lib/ratelimit.ts` and it is one function doing three things: read KV, write
/// KV, and decide WHEN. The reading and the writing are `device_store::kv_get`/`kv_put`; the deciding is here, as values,
/// so a test can pin it without a Cloudflare binding. **A decision that can be tested belongs where the decision
/// already lives** — the same split `rate_limit.rs` makes for its clock and counters.
/// "This key has never been seen by this isolate" — the condition the source guards with
/// `if (kvSeed && env?.KEYS)` on BOTH the read and the write arms, and with `hit === undefined` between them.
///
/// The read and the write are ONE event, not two: the read happens on this arm only, and so does the write.
/// An implementation that seeds per-request reads KV on every probe, which is the quota exhaustion the
/// memory-only arms exist to avoid.
pub fn needs_seed(counters: &Counters, key: &str) -> bool {
    !counters.iter().any(|(k, _)| k == key)
}

/// `Math.ceil((windowMs / 1000) * 2) + 10` — the TTL on the persist write. For the probe's 60_000 ms window that
/// is **130 seconds**: the bucket plus its successor, plus ten, so a key that outlives its window cannot be
/// inherited by the next one.
pub fn seed_ttl_secs(window_ms: i64) -> u64 {
    ((window_ms as f64 / 1000.0) * 2.0).ceil() as u64 + 10
}

/// The devices plugin's own instance, verbatim: `name: "pub-rate", limit: 10, windowMs: 60_000`.
pub const PUBLIC_RATE: IpRateLimiter = IpRateLimiter {
    name: "pub-rate",
    limit: 10,
    window_ms: 60_000,
};

/// `cf-connecting-ip`, or this literal when the header is absent — **ONE NAME, THREE SPELLINGS UNTIL THIS
/// LANDING** (two Rust call sites and the TypeScript's own default).
pub const UNKNOWN_IP: &str = "unknown";

/// The per-isolate table's bound: `counters.size > 4096` evicts the OLDEST key, and the eviction happens only on
/// the fresh-key arm because only that arm can grow the table.
pub const MAX_COUNTERS: usize = 4096;

/// The posture on an INTERNAL error: NOT over budget — **FAIL OPEN**, the same posture the breaker takes. The
/// emission is this constant's caller: the TypeScript's outer `catch` returns it, so the value has one home.
pub const FAIL_OPEN_VERDICT: bool = false;

/// The budget test, and **THE ASYMMETRY BETWEEN THE ARMS IS THE SOURCE'S**: an existing counter that is already AT
/// the limit is refused WITHOUT incrementing, while a fresh key inserts `cur + 1` and answers `cur >= limit` —
/// always false on a bucket's first hit in memory-only mode. A port that "simplified" this into one arm would
/// either let the 11th request through or refuse the 10th.
pub fn over_budget(count: u32, limit: u32) -> bool {
    count >= limit
}

/// The eviction test, on the same arm that can grow the table.
pub fn needs_eviction(size: usize) -> bool {
    size > MAX_COUNTERS
}

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
/// **THE SEED, EXECUTED — AND THE READ AND THE WRITE ARE ONE EVENT, NOT TWO.**
///
/// The TypeScript is `ratelimit.ts`'s first-sight arm, in order: one KV read, the counter starts from it, then one KV
/// write carrying the TTL. `kv_text` answers `None` for both a binding failure and an absent key, which is exactly
/// the source's `catch { /* KV read failed — isolate-local count from zero */ }` — a KV that is down must not
/// fail OPEN the limiter.
///
/// The counter is seeded with `cur` (not `cur + 1`) so `check` performs the increment; when the request is already
/// over budget `check` refuses without incrementing, so the in-isolate count is `cur` where the source's is
/// `cur + 1`. That is not observable: the next call from the same isolate is refused either way.
pub async fn seeded_check(env: &Env, counters: &mut Counters, ip: &str, now_ms: i64) -> bool {
    let key = counter_key(PROBE_RATE, ip, now_ms);
    let seeded = if needs_seed(counters, &key) {
        let cur = crate::device_store::kv_text(env, &key)
            .await
            .and_then(|raw| raw.parse::<u32>().ok())
            .unwrap_or(0);
        counters.push((key.clone(), cur));
        Some(cur)
    } else {
        None
    };
    let refused = check(PROBE_RATE, counters, ip, now_ms);
    if let Some(cur) = seeded {
        crate::device_store::kv_put(
            env,
            &key,
            &(cur + 1).to_string(),
            Some(seed_ttl_secs(PROBE_RATE.window_ms)),
        )
        .await;
    }
    refused
}

pub fn check(limiter: IpRateLimiter, counters: &mut Counters, ip: &str, now_ms: i64) -> bool {
    let key = counter_key(limiter, ip, now_ms);
    if let Some(entry) = counters.iter_mut().find(|(k, _)| *k == key) {
        if over_budget(entry.1, limiter.limit) {
            return true;
        }
        entry.1 += 1;
        return false;
    }
    let cur = 0;
    counters.push((key, cur + 1));
    // `if (counters.size > 4096) { delete the first key }` — the insertion-order eviction, and it happens only
    // on this arm because only this arm can grow the table.
    if needs_eviction(counters.len()) {
        counters.remove(0);
    }
    over_budget(cur, limiter.limit)
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
    assert!(check(
        PROBE_RATE,
        &mut counters,
        "203.0.113.7",
        1_700_000_000_000
    ));
}

/// And the family is the probe route, not a prefix that would swallow a neighbour.
#[test]
fn the_family_is_the_probe_route() {
    assert!(probe_in_family("/api/summrise-probe"));
    assert!(!probe_in_family("/api/summrise-probe/extra"));
    assert!(!probe_in_family("/api/health"));
}

/// **THE SEED HAPPENS ONCE PER KEY, NOT ONCE PER REQUEST.**
///
/// MUTATION: make `needs_seed` return `true` unconditionally.
/// RESULT:   this test fails — the third request of an established key is reported as needing a seed, which is
///           a KV READ PER REQUEST, and the suite stays green because nothing else reads the predicate.
#[test]
fn the_seed_happens_once_per_key() {
    let mut counters = Counters::new();
    let key = counter_key(PROBE_RATE, "203.0.113.7", 1_700_000_000_000);
    assert!(needs_seed(&counters, &key), "an unseen key needs a seed");
    check(PROBE_RATE, &mut counters, "203.0.113.7", 1_700_000_000_000);
    assert!(!needs_seed(&counters, &key), "a seen key does not");
}

/// **THE TTL IS THE BUCKET PLUS ITS SUCCESSOR PLUS TEN — 130 FOR THE PROBE.**
///
/// MUTATION: change the `+ 10` to `+ 60`.
/// RESULT:   this test fails on the probe's window while every behavioural case stays green.
#[test]
fn the_seed_ttl_is_the_window_doubled_plus_ten() {
    assert_eq!(seed_ttl_secs(60_000), 130);
    assert_eq!(seed_ttl_secs(30_000), 70);
    assert_eq!(seed_ttl_secs(1), 11);
}

/// The emitted module's header — **THE SOURCE FILE'S OWN COMMENTS, CARRIED VERBATIM, PLUS THE GENERATED NOTICE.**
const TS_HEADER: &str = r#"/**
 * ratelimit-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/ip_rate_limit.rs` (`render_typescript`), which is now the single source. The
 * freshness test in that module is a byte comparison against this file, so a hand edit fails there with the text and
 * one command to fix it:
 *
 *     SUMMRISE_REFRESH_RATELIMIT=1 cargo test --manifest-path gateway/wasm/Cargo.toml ip_rate_limit::emission
 *
 * `gateway/src/lib/ratelimit.ts` re-exports this module and keeps the HOST: the per-isolate `Map`, the KV read and
 * write, and the `try` that makes a KV failure best-effort. **The RULES are here** — the bucket, the key, the budget
 * test, the eviction bound, the persist TTL, and the fail-open verdict — and `UNKNOWN_IP` is one name now where the
 * literal was written three times (twice in Rust, once in TypeScript).
 */

/** `cf-connecting-ip`, or this literal when the header is absent. */
export const UNKNOWN_IP = "unknown";

/** The per-isolate table's bound: exceeding it evicts the OLDEST key (insertion order), on the fresh-key arm only. */
export const MAX_COUNTERS = 4096;

/** The posture on an INTERNAL error: NOT over budget — **FAIL OPEN**, the posture the breaker takes too. */
export const FAIL_OPEN_VERDICT = false;

/** `Math.floor(Date.now() / windowMs)` — the fixed window a request counts against. */
export function bucketOf(nowMs: number, windowMs: number): number {
  return Math.floor(nowMs / windowMs);
}

/** `${name}:${ip}:${bucket}`. */
export function counterKey(name: string, ip: string, bucket: number): string {
  return `${name}:${ip}:${bucket}`;
}

/** The budget test. **THE ASYMMETRY BETWEEN THE ARMS IS THE SOURCE'S**: an existing counter already AT the limit is
 *  refused WITHOUT incrementing, while a fresh key inserts `cur + 1` and answers `cur >= limit` — always false on a
 *  bucket's first hit in memory-only mode. */
export function overBudget(count: number, limit: number): boolean {
  return count >= limit;
}

/** The eviction test, on the same arm that can grow the table. */
export function needsEviction(size: number): boolean {
  return size > MAX_COUNTERS;
}

/** `Math.ceil((windowMs / 1000) * 2) + 10` — the TTL on the persist write. For the probe's 60 000 ms window that is
 *  **130 seconds**: the bucket plus its successor, plus ten, so a key that outlives its window cannot be inherited
 *  by the next one. */
export function seedTtlSecs(windowMs: number): number {
  return Math.ceil((windowMs / 1000) * 2) + 10;
}

/** Seeding needs BOTH the option and a store bound. */
export function shouldSeed(kvSeed: boolean, hasKeys: boolean): boolean {
  return kvSeed && hasKeys;
}

/** A persisted count read back: a missing or non-numeric value is ZERO, which is the source's `Number(v) || 0`. */
export function seededCount(raw: unknown): number {
  return Number(raw) || 0;
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_HEADER.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/ratelimit-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.**
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_RATELIMIT").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write ratelimit-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_RATELIMIT=1 cargo test ip_rate_limit::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/lib/ratelimit-logic.ts is not what ip_rate_limit.rs emits — \
             run SUMMRISE_REFRESH_RATELIMIT=1 cargo test ip_rate_limit::emission"
        );
    }

    /// **AND THE LITERAL IS ONE NAME NOW, WHICH IS CHECKED RATHER THAN INTENDED**: the two routes that read
    /// `cf-connecting-ip` must NAME the fallback, not spell it — three spellings is how a wire drifts.
    #[test]
    fn the_unknown_ip_is_one_name() {
        for file in ["src/auth_routes.rs", "src/devices.rs"] {
            let text = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file),
            )
            .expect("a committed source file");
            assert!(
                text.contains("UNKNOWN_IP"),
                "{file} must NAME the unknown-ip fallback"
            );
            // **PROSE MAY NAME THE LITERAL; ONLY CODE CAN SPELL IT** — the doc comment at `auth_routes.rs`'s ip
            // read quotes the source's expression on purpose, and a `!contains` here would fail on the RECORD of
            // the thing this test is checking. (`proxy_cors_parity.rs` learned the same way: anchor on the
            // DECLARATION, never on a mention.)
            let spelled_in_code = text
                .lines()
                .any(|line| !line.trim_start().starts_with("//") && line.contains("\"unknown\""));
            assert!(!spelled_in_code, "{file} still spells the literal in code");
        }
        assert!(render_typescript().contains(UNKNOWN_IP));
    }
}

#[cfg(test)]
mod emitted_rules {
    use super::*;

    /// **THE RULES THE TYPESCRIPT NOW READS**, pinned where they live.
    ///
    /// MUTATION: `over_budget`'s `>=` becomes `>`.
    /// RESULT:   `the_budget_and_the_bound` fails, and so does this file's own
    ///           `the_eleventh_request_is_refused` — the eleventh request is allowed again.
    #[test]
    fn the_budget_and_the_bound() {
        assert!(over_budget(10, 10), "AT the limit is over");
        assert!(!over_budget(9, 10));
        assert!(!over_budget(0, 10));
        assert!(over_budget(0, 0), "a zero budget refuses the first request");
        assert!(needs_eviction(MAX_COUNTERS + 1));
        assert!(!needs_eviction(MAX_COUNTERS), "AT the bound is not over it");
        assert_eq!(UNKNOWN_IP, "unknown");
        // The posture is a VALUE rather than a bare literal in a catch — and asserting on the CONSTANT would be an
        // assertion on a constant (clippy), so this pins what the EMISSION carries instead.
        assert!(render_typescript().contains("export const FAIL_OPEN_VERDICT = false;"));
    }

    /// **THE BUCKET, THE KEY AND THE PERSIST TTL** — `Math.floor` by `div_euclid` (it is not truncation for a
    /// negative number), and the TTL the source's own comment works out to 130 s for the probe's window.
    #[test]
    fn the_bucket_the_key_and_the_ttl() {
        assert_eq!(bucket_of(0, 60_000), 0);
        assert_eq!(bucket_of(59_999, 60_000), 0);
        assert_eq!(bucket_of(60_000, 60_000), 1);
        assert_eq!(bucket_of(-1, 60_000), -1, "floor, not truncation");
        assert_eq!(
            counter_key(PROBE_RATE, "1.2.3.4", 60_000),
            "probe-rate:1.2.3.4:1"
        );
        assert_eq!(
            counter_key(PROBE_RATE, UNKNOWN_IP, 0),
            "probe-rate:unknown:0"
        );
        assert_eq!(seed_ttl_secs(60_000), 130);
        assert_eq!(seed_ttl_secs(1_000), 12);
    }
}
