//! THE PURE RULES OF `gateway/src/store/cache.ts` AND `gateway/src/session.ts`.
//!
//! WHY THESE. The cache's TTL rule is what DECOUPLES KV READ VOLUME FROM REQUEST VOLUME — the module's own
//! comment measures it: "each key costs at most one read per day per isolate, instead of one read per
//! request", and the short-TTL prefix list exists because "a deleted or re-pointed provider must stop being
//! dialled within a minute on every isolate, not within a day". The session pair is the FAIL-CLOSED rule for
//! issuing new sessions. The caches, the locks and the KV reads stay in TypeScript: they are state and I/O.

use std::sync::Mutex;

use serde_json::Value;
use worker::Env;

/// The two TTLs, verbatim.
pub const CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;
pub const AUTH_CACHE_TTL_MS: i64 = 60 * 1000;

/// The per-isolate cache's size bound.
pub const CACHE_MAX_ENTRIES: usize = 512;

/// `AUTH_PREFIXES` — the key prefixes that get the SHORT TTL.
///
/// **`"providers:"` IS IN THIS LIST, AND THE FIRST VERSION OF THIS PORT SAID THE OPPOSITE — THAT WAS THE BUG.**
/// Its comment read *"the source keeps it as a COMMENTED entry"* and the test pinned `providers:x` at the 24 h
/// TTL, so the two agreed with each other and neither agreed with `gateway/src/store/cache.ts`, which has the
/// entry **ACTIVE** with its own dated reasoning (2026-09-13): *"a custom provider record carries a credential
/// and an egress DESTINATION. A deleted or re-pointed provider must stop being dialled within a minute on every
/// isolate, not within a day."* The port was written from a reading of the file that had gone stale, and the
/// emission is what forced the two sides to be written down beside each other — **the third live divergence
/// this migration has found that way** (the mask's UTF-16 arm, `advertisedModelId`'s null wire, and this).
pub const AUTH_PREFIXES: [&str; 10] = [
    "settings:",
    "token:",
    "user:",
    "ukeys:",
    "auth:",
    "route:",
    "devices:",
    "plugins:",
    "cf:",
    "providers:",
];

/// `cset`'s TTL decision: the short TTL for an auth-shaped key, the 24 h one otherwise.
pub fn ttl_for(key: &str) -> i64 {
    if AUTH_PREFIXES.iter().any(|p| key.starts_with(p)) {
        AUTH_CACHE_TTL_MS
    } else {
        CACHE_TTL_MS
    }
}

/// `withKeyLock`'s table bound — the source's own hard bound for pathological distinct-key bursts. Named here
/// because the EMISSION publishes it, and a number in two languages is a number that drifts.
pub const LOCK_MAX_ENTRIES: usize = 512;

/// `cset`'s eviction: when the cache is at its bound, the OLDEST entry goes.
///
/// **"OLDEST" IS THE INSERTION ORDER OF A `Map`, WHICH IS WHY THIS ANSWERS A POSITION.** The same lesson the
/// relay's queue learned from a flaky test on 2026-10-02: a structure without insertion order cannot answer
/// "the oldest", and `Map` promises it. What this returns is the index to drop — 0 when the bound is
/// reached, and nothing otherwise.
pub fn cache_eviction_index(size: usize) -> Option<usize> {
    if cache_at_bound(size) {
        Some(0)
    } else {
        None
    }
}

/// The comparison itself, split out because it is what the EMISSION can express: the console's cache asks
/// "must I evict?" and answers it with a boolean, while this crate's `Vec`-backed cache asks "which index?".
/// One rule, two shapes, and the shape belongs to the host.
pub fn cache_at_bound(size: usize) -> bool {
    size >= CACHE_MAX_ENTRIES
}

/// **THE PER-ISOLATE KV CACHE — `store/cache.ts`'s `__c`, AND ITS ABSENCE WAS A MEASURED COST.**
///
/// Every decision this cache makes was ALREADY ported (`ttl_for`, `cache_eviction_index`, `AUTH_PREFIXES`); what
/// was missing is the state. Measured 2026-10-06 with a KV stub that records every read, the same request
/// against both workers:
///
/// ```text
///     shipping:  1 request -> 22 reads (the admin seed),  10 requests ->  8 reads  (0.8/request)
///     wasm:      1 request ->  6 reads,                   10 requests -> 60 reads  (6.0/request)
/// ```
///
/// The source's own comment says what that buys: "KV read volume is thus decoupled from request volume — each
/// key costs at most one read per day per isolate, instead of one read per request." The responses are
/// byte-identical either way, which is why no case in the divergence sweep could see it: it is latency and cost,
/// not bytes.
///
/// **A CACHED MISS IS NOT A MISS.** `cget` answers `undefined` for "not in the cache" and `null` for a value
/// that WAS read and was absent — the source caches the absence too ("caches null too — no zombie lookups"), so
/// a token that does not exist is not re-read on every request that presents it. That is why this is
/// `Option<Value>` and not `Option<String>`.
static KV_CACHE: Mutex<Vec<(String, serde_json::Value, i64)>> = Mutex::new(Vec::new());

/// **A TEST-ONLY GUARD OVER THE PROCESS-GLOBAL CACHE, AND THE MEASUREMENT THAT ASKED FOR IT.**
///
/// `KV_CACHE` is one `static` for the whole test binary, and cargo runs a binary's tests on PARALLEL
/// THREADS — so two tests that touch it interleave. Measured 2026-10-08, first in CI on the merge commit
/// `7111cdcb` and then reproduced here (2 of 5 local runs failed):
///
/// ```text
///     thread 'store::cache_tests::the_eviction_takes_the_oldest_at_the_bound' panicked at src/store.rs:164:
///     and only the oldest
/// ```
///
/// `the_cache_is_the_sources` was inserting and deleting keys while the eviction test filled the cache to
/// `CACHE_MAX_ENTRIES`, so the interleaved `put` is what the bound evicted — or what became "the oldest".
/// **THE GUARD ALSO EMPTIES THE CACHE**, because a test that inherits another test's leftovers is the same
/// defect one run later. Any test that touches the cache must take it; this is the crate's one piece of
/// shared mutable state, and the crate has exactly two such tests.
///
/// (It is not a fix to the cache: the cache is per-isolate by design, and one isolate is one thread of
/// requests. What was wrong was the TESTS sharing it across threads.)
#[cfg(test)]
pub(crate) fn cache_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static CACHE_TESTS: Mutex<()> = Mutex::new(());
    let guard = CACHE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut cache) = KV_CACHE.lock() {
        cache.clear();
    }
    guard
}

/// `cget(k)`: the cached value when it is still fresh, `None` when it is absent OR expired.
pub fn cached_get(key: &str, now_ms: i64) -> Option<serde_json::Value> {
    let Ok(mut cache) = KV_CACHE.lock() else {
        return None;
    };
    let at = cache.iter().position(|(k, _, _)| k == key)?;
    let (_, value, exp) = cache[at].clone();
    if exp <= now_ms {
        cache.remove(at);
        return None;
    }
    Some(value)
}

/// `cset(k, v)`: store with the key's TTL, evicting the OLDEST entry when the bound is reached.
pub fn cache_put(key: &str, value: serde_json::Value, now_ms: i64) {
    let Ok(mut cache) = KV_CACHE.lock() else {
        return;
    };
    if let Some(index) = cache_eviction_index(cache.len()) {
        cache.remove(index);
    }
    let exp = now_ms + ttl_for(key);
    match cache.iter_mut().find(|(k, _, _)| k == key) {
        Some(entry) => *entry = (key.to_string(), value, exp),
        None => cache.push((key.to_string(), value, exp)),
    }
}

/// `cdel(...ks)` — the write-through half: every write in the source refreshes the cache immediately so an admin
/// change takes effect on the hot isolate at once.
pub fn cache_del(key: &str) {
    if let Ok(mut cache) = KV_CACHE.lock() {
        cache.retain(|(k, _, _)| k != key);
    }
}

/// The emitted module's header, and then the module.
const TS_MODULE: &str = r#"/**
 * cache-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/store.rs` (`render_typescript`), which is now the single source for the rules
 * below. The freshness test in that module is a byte comparison against this file, so a hand edit fails there
 * with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_CACHE=1 cargo test --manifest-path gateway/wasm/Cargo.toml store::emission
 *
 * `gateway/src/store/cache.ts` re-exports this module and keeps the HOST: the two `Map`s and the pruning of the
 * per-key lock chain. **THE RULES ARE HERE** — the two TTLs, the prefix list that chooses between them, the two
 * bounds, and the comparison the cache asks before it evicts.
 *
 * **AND THE PREFIX LIST IS WHY THIS EMISSION EXISTS**: the worker's port of it was missing `"providers:"` —
 * with a comment claiming the source kept that entry commented out, and a test pinning the same wrong answer —
 * so a deleted or re-pointed provider stayed dialled for up to 24 h on a warm isolate where the console
 * propagates it within a minute.
 */

/** The backstop TTL: after 24 h a cached value is re-read from KV even if no write touched this isolate. */
export const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

/** Security-critical keys get the short TTL, so an admin change propagates across isolates within a minute. */
export const AUTH_CACHE_TTL_MS = 60 * 1000;

/** The per-isolate cache's bound: at it, the OLDEST entry goes (a `Map`'s insertion order). */
export const CACHE_MAX_ENTRIES = 512;

/** The per-key lock table's hard bound, for pathological distinct-key bursts. */
export const LOCK_MAX_ENTRIES = 512;

/** The key prefixes that get the SHORT TTL. `"providers:"` is on it: a custom provider record carries a
 *  credential and an egress DESTINATION, so a deleted or re-pointed one must stop being dialled within a
 *  minute, not within a day. */
export const AUTH_PREFIXES: string[] = [
  "settings:",
  "token:",
  "user:",
  "ukeys:",
  "auth:",
  "route:",
  "devices:",
  "plugins:",
  "cf:",
  "providers:",
];

/** `cset`'s TTL decision: the short TTL for an auth-shaped key, the 24 h one otherwise. */
export function ttlFor(key: string): number {
  return AUTH_PREFIXES.some((p) => key.startsWith(p)) ? AUTH_CACHE_TTL_MS : CACHE_TTL_MS;
}

/** `cset`'s eviction test — the comparison, with the eviction itself left to the host that owns the `Map`. */
export function cacheAtBound(size: number): boolean {
  return size >= CACHE_MAX_ENTRIES;
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_MODULE.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/cache-logic.ts")
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    /// **THE CACHE'S FOUR PROPERTIES, EACH ONE A WAY IT COULD BE WRONG.**
    ///
    /// MUTATION: make `cached_get` ignore the expiry (`if false && exp <= now_ms`).
    /// RESULT:   `the_ttl_is_the_keys_own` fails — a `token:` entry outlives its 60 s window, so a disabled
    ///           user or a re-pointed provider keeps being served for up to a day. That is the security half of
    ///           the short-TTL list, not a performance detail.
    #[test]
    fn the_cache_is_the_sources() {
        let _exclusive = cache_test_guard();
        // A MISS IS `None`; A CACHED MISS IS `Some(Null)` — the distinction the source spells "caches null too".
        assert_eq!(cached_get("token:t", 1000), None);
        cache_put("token:t", serde_json::Value::Null, 1000);
        assert_eq!(cached_get("token:t", 1000), Some(serde_json::Value::Null));
        // THE TTL IS THE KEY'S OWN: `token:` is on the auth list (60 s), a model key is not (24 h).
        assert_eq!(ttl_for("token:t"), AUTH_CACHE_TTL_MS);
        assert_eq!(ttl_for("models:custom"), CACHE_TTL_MS);
        // ...and the expiry is enforced, not merely recorded.
        assert!(cached_get("token:t", 1000 + AUTH_CACHE_TTL_MS - 1).is_some());
        assert_eq!(cached_get("token:t", 1000 + AUTH_CACHE_TTL_MS), None);
        // A 24 h key is still there at 60 s.
        cache_put("models:custom", serde_json::json!([1]), 1000);
        assert!(cached_get("models:custom", 1000 + AUTH_CACHE_TTL_MS).is_some());
        // `cdel` is the write-through half.
        cache_del("models:custom");
        assert_eq!(cached_get("models:custom", 1000), None);
        // A RE-PUT REPLACES rather than duplicating (the source's `Map.set`).
        cache_put("token:t", serde_json::json!("v1"), 2000);
        cache_put("token:t", serde_json::json!("v2"), 2000);
        assert_eq!(cached_get("token:t", 2000), Some(serde_json::json!("v2")));
    }

    /// The eviction is at the BOUND and takes the OLDEST — `Map`'s insertion order, which is why the state is a
    /// `Vec` and not a `HashMap`.
    ///
    /// **THE `cache_test_guard` LINE IS THE FIX FOR A FLAKE, NOT A FORMALITY**: without it this test
    /// failed roughly two runs in five, because the test above it was filling the same global cache at the
    /// same time (see the guard's own comment for the measurement and the CI run that caught it).
    #[test]
    fn the_eviction_takes_the_oldest_at_the_bound() {
        let _exclusive = cache_test_guard();
        for i in 0..CACHE_MAX_ENTRIES {
            cache_put(&format!("k{i}"), serde_json::json!(i), 1000);
        }
        // At the bound the NEXT insert drops `k0`.
        cache_put("fresh", serde_json::json!("x"), 1000);
        assert_eq!(cached_get("k0", 1000), None, "the oldest went");
        assert!(cached_get("k1", 1000).is_some(), "and only the oldest");
        assert!(cached_get("fresh", 1000).is_some());
    }
}

/// `sessionSecret(env, adminPassword)` — the VERIFY path's key.
///
/// It prefers the dedicated `SESSION_SECRET` and still accepts cookies signed with the admin password, so a
/// rotation or a first-time rollout does not log everyone out. `||` is a TRUTHINESS test, so an empty
/// `SESSION_SECRET` falls back to the password.
pub fn session_secret(session_secret: Option<&str>, admin_password: &str) -> String {
    match session_secret {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => admin_password.to_string(),
    }
}

/// `issueSessionSecret(env)` — the ISSUANCE key, **FAIL CLOSED**.
///
/// Without a dedicated high-entropy secret the HMAC key would fall back to the admin password, letting any
/// invited user offline-brute-force it from their own signed cookie (HMAC-SHA256 is not memory-hard). So
/// this answers `None` when unconfigured and the caller must refuse to issue a session rather than silently
/// fall back. The `console.error` stays at the edge — it is I/O, not the decision.
pub fn issue_session_secret(session_secret: Option<&str>) -> Option<String> {
    match session_secret {
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

/// `normalizeSetting(v)` — **AN EXPLICIT OFF IS `null`, NOT THE STRING.** The source persists OFF as `"0"`
/// (which shadows the Worker var of the same name), so every read normalizes: `"0"` and `"false"` become
/// `null`, and the caller's fallback chain sees "unset" rather than a truthy string.
pub fn normalize_setting(v: Option<&str>) -> Option<String> {
    match v {
        Some("0") | Some("false") => None,
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

/// `globalSettingEnabled(v)` — and its list is NOT `normalizeSetting`'s: an EMPTY string is disabled here and
/// passes through there. Two functions, two rules, both pinned.
pub fn global_setting_enabled(v: Option<&str>) -> bool {
    !matches!(v, None | Some("") | Some("0") | Some("false"))
}

/// `getGlobalSetting(env, name)` — the KV key, the Worker-var fallback, and the normalization, in order.
///
/// `kv` IS THE READ (`env.KEYS.get("settings:<name>")`) and `env_var` is `env[name]`, because both are I/O.
pub fn get_global_setting(kv: Option<&str>, env_var: Option<&str>) -> Option<String> {
    match kv {
        Some(v) => normalize_setting(Some(v)),
        None => normalize_setting(env_var),
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! The session pair is replayed from `fixtures/translate-corpus.json` (produced by the SHIPPING
    //! TypeScript). **THE TTL RULE IS PINNED BY HAND INSTEAD**, and the reason is worth stating: the TTL is
    //! chosen inside `cset` and is invisible from outside — a cache read cannot tell 60 s from 24 h without
    //! moving the clock — so the oracle has nothing to record. The values below are the source's own
    //! constants and prefix list, and the module header of this file carries the decision they encode.
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
    fn every_session_secret_rule_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "session_secret" => {
                    let got = session_secret(
                        input["secret"].as_str(),
                        input["password"].as_str().unwrap_or(""),
                    );
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "issue_session_secret" => {
                    let got = issue_session_secret(input["secret"].as_str());
                    let as_json = match got {
                        Some(s) => serde_json::Value::String(s),
                        None => serde_json::Value::Null,
                    };
                    assert_eq!(&as_json, want, "{func} / {name}");
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(checked >= 7, "the session corpus shrank to {checked} cases");
    }

    #[test]
    fn the_ttl_rule_is_the_sources_own_list_and_constants() {
        // **THE LIST IS PINNED, NOT LOOPED OVER.** The first version of this test iterated `AUTH_PREFIXES` and
        // asserted each entry takes the short TTL — a test that reads its own subject, which passes for any list
        // including the one that was missing `providers:`. The contents are written out here so a missing entry
        // fails, and the emission's own floor test reads this same list.
        assert_eq!(
            AUTH_PREFIXES,
            [
                "settings:",
                "token:",
                "user:",
                "ukeys:",
                "auth:",
                "route:",
                "devices:",
                "plugins:",
                "cf:",
                "providers:",
            ]
        );
        // The ten prefixes take the SHORT ttl; everything else takes the 24 h one.
        for prefix in AUTH_PREFIXES {
            assert_eq!(
                ttl_for(&format!("{prefix}x")),
                AUTH_CACHE_TTL_MS,
                "{prefix}"
            );
        }
        assert_eq!(ttl_for("something-else"), CACHE_TTL_MS);
        assert_eq!(ttl_for(""), CACHE_TTL_MS);
        // `startsWith`, not `contains`: a prefix in the MIDDLE is not a prefix.
        assert_eq!(ttl_for("xsettings:y"), CACHE_TTL_MS);
        // **`providers:` IS ON THE LIST, AND THIS ROW USED TO ASSERT THE OPPOSITE.** A deleted or re-pointed
        // provider carries a credential and an egress destination, so its record must stop being dialled within
        // a minute on every isolate — the source's own dated reason, and the behaviour the console ships.
        assert_eq!(ttl_for("providers:x"), AUTH_CACHE_TTL_MS);
        assert_eq!(ttl_for("providers:custom"), AUTH_CACHE_TTL_MS);
        assert_eq!(AUTH_CACHE_TTL_MS, 60 * 1000);
        assert_eq!(CACHE_TTL_MS, 24 * 60 * 60 * 1000);
    }

    #[test]
    fn the_eviction_bound_is_the_sources_own() {
        assert_eq!(cache_eviction_index(511), None);
        assert_eq!(
            cache_eviction_index(512),
            Some(0),
            "at the bound, the OLDEST goes"
        );
        assert_eq!(cache_eviction_index(513), Some(0));
        assert_eq!(CACHE_MAX_ENTRIES, 512);
        assert!(!cache_at_bound(511));
        assert!(cache_at_bound(512));
        assert_eq!(LOCK_MAX_ENTRIES, 512);
    }
}

/// **`barePrefix` AND `advertisedModelId` LIVE IN `summrise-core::provider_spec` NOW (2026-10-13)**, because
/// the console's emission ships them beside the parser that uses them — and these two names are the RE-EXPORTS
/// every call site here already reads.
///
/// `?? ""` IS THE ARM THAT MOVED WITH THEM, and it is a COERCION rather than a null check: `String(prefix ??
/// "")` maps `null` and an absent key to the EMPTY string, where `String(null)` is the text `"null"` — measured,
/// a provider whose `prefix` was absent once advertised its models as `null/foo`.
pub use summrise_core::provider_spec::{advertised_model_id, bare_prefix};

/// **`providerForPrefix(env, prefix)` — THE CUSTOM-PROVIDER LOOKUP, AND THE HALF THAT WAS MISSING.**
///
/// `store/providers.ts` is the console's custom-provider feature, and `upstream.ts`'s `resolveRoute` consults it
/// BETWEEN the built-in table and the default route:
///
/// ```text
///     isBuiltInPrefix(prefix)          -> pickRoute(…)          a built-in always wins
///     providerForPrefix(env, prefix)   -> providerRoute(provider)
///     otherwise                        -> pickRoute(…)          the default channel
/// ```
///
/// **THIS CRATE HAD THE LISTING HALF AND NOT THE ROUTING HALF** — `advertised_provider_models` below made
/// `/v1/models` advertise `acme/acme-chat` while `routing.rs`'s `resolve_model` consulted the built-in table
/// only. Measured 2026-10-06 on the built worker: a request for a custom provider's model answered
/// **`502 config_error — "CMD_API_KEY not configured — add your Command Code key"`** where the shipping route
/// dialled `POST https://acme.test/v1/chat/completions` with `Bearer sk-acme-inline`. That is the exact failure
/// `providerRoute`'s own comment refuses: *"AN UNROUTABLE RECORD IS AN ERROR ROUTE, NEVER THE DEFAULT CHANNEL …
/// it would dial a built-in upstream under a different provider's name."*
///
/// The match is on the BARE prefix (trailing slashes off both sides), and the FIRST record wins — the same
/// `new URL(raw).host` — the HOST, or `None` when the string is not a URL (the caller keeps the raw text, which
/// is the console's own arm: "a hand-edited record: show the raw string rather than dropping the card").
pub fn url_host(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    url.host_str().map(|h| h.to_string())
}

/// `publicProvider(p, env)` — the record view: the key is MASKED, never returned, and `keyReady` says whether one
/// resolved at all. The projection itself is the DECISION ROUTE's (`decisions::public_provider`), called here
/// rather than restated: this route and `/__decision` must not be able to disagree about a masked credential.
pub fn public_provider_view(env: &Env, spec: &Value) -> Value {
    // The binding the record NAMES, read off the real environment — exactly what the source's `env?.[name]`
    // does. An absent binding and an explicit null are the same answer (`?? ""`), which `provider_key` owns.
    let mut bindings = serde_json::Map::new();
    if let Some(name) = spec.get("apiKeyEnv").and_then(Value::as_str) {
        if !name.is_empty() {
            if let Ok(value) = env.var(name) {
                bindings.insert(name.to_string(), Value::String(value.to_string()));
            }
        }
    }
    let resolved = provider_key(&Value::Object(bindings), Some(spec));
    crate::decisions::public_provider(&serde_json::json!({ "p": spec, "resolved": resolved }))
        .unwrap_or(Value::Null)
}

/// `setCfToken(env, value)` — a NON-EMPTY value is written and cached; an EMPTY one DELETES the key and drops it
/// from the cache. That asymmetry is the route's "clear the token" arm.
pub async fn set_cf_token(env: &Env, value: &str) {
    let v = value.trim();
    if v.is_empty() {
        crate::device_store::kv_delete(env, "cf:api_token").await;
    } else {
        crate::device_store::kv_put(env, "cf:api_token", v, None).await;
    }
}

/// linear scan the source does.
pub fn provider_for_prefix(
    providers: &[serde_json::Value],
    prefix: &str,
) -> Option<serde_json::Value> {
    let bare = bare_prefix(&serde_json::Value::String(prefix.to_string()));
    if bare.is_empty() {
        return None;
    }
    providers
        .iter()
        .find(|p| bare_prefix(p.get("prefix").unwrap_or(&serde_json::Value::Null)) == bare)
        .cloned()
}

/* ─────────────────── the provider store's writes ─────────────────── */

/// `providers:custom` — the KV key the custom provider records live under.
pub const CUSTOM_PROVIDERS_KEY: &str = "providers:custom";

/// `readList(env)` — the KV read with the source's two silent arms, exactly like the model store's
/// ([`crate::model_store::read_list`]): a non-array cache hit and a malformed blob both answer `[]`, and the cache
/// is refreshed with what was read.
pub async fn custom_provider_list(env: &Env) -> Vec<Value> {
    crate::model_store::read_list(env, CUSTOM_PROVIDERS_KEY).await
}

/// `putCustomProvider(env, spec)` — **THE UPSERT IS BY BARE PREFIX, UNDER A KEYED LOCK.** Re-posting a prefix is
/// how an operator edits the provider they own, and the lock is the source's own lesson ("two concurrent POSTs
/// used to be able to read the same pre-write list and have the second clobber the first").
pub async fn put_custom_provider(env: &Env, spec: &Value) -> Vec<Value> {
    crate::key_lock::with_key_lock(&PROVIDER_LOCKS, CUSTOM_PROVIDERS_KEY, async {
        let cur = custom_provider_list(env).await;
        let bare =
            summrise_core::provider_spec::bare_prefix(spec.get("prefix").unwrap_or(&Value::Null));
        let mut next: Vec<Value> = cur
            .into_iter()
            .filter(|p| {
                summrise_core::provider_spec::bare_prefix(p.get("prefix").unwrap_or(&Value::Null))
                    != bare
            })
            .collect();
        next.push(spec.clone());
        crate::model_store::write_list(env, CUSTOM_PROVIDERS_KEY, &next).await;
        next
    })
    .await
}

/// `deleteCustomProvider(env, prefix)` — the same lock, and `false` when the prefix is not one this deployment
/// owns (the route turns that into its 404).
pub async fn delete_custom_provider(env: &Env, prefix: &str) -> bool {
    crate::key_lock::with_key_lock(&PROVIDER_LOCKS, CUSTOM_PROVIDERS_KEY, async {
        let cur = custom_provider_list(env).await;
        let bare = summrise_core::provider_spec::bare_prefix(&Value::String(prefix.to_string()));
        let next: Vec<Value> = cur
            .iter()
            .filter(|p| {
                summrise_core::provider_spec::bare_prefix(p.get("prefix").unwrap_or(&Value::Null))
                    != bare
            })
            .cloned()
            .collect();
        if next.len() == cur.len() {
            return false;
        }
        crate::model_store::write_list(env, CUSTOM_PROVIDERS_KEY, &next).await;
        true
    })
    .await
}

/// The keyed locks for the provider store — one map, because both writers serialize on the same key.
static PROVIDER_LOCKS: crate::key_lock::KeyedLocks = crate::key_lock::KeyedLocks::new();

/// `new URL(rawBase)` — **THE PARSE THE PURE PARSER CANNOT DO, SO THE HOST DOES IT AND HANDS IT OVER.**
///
/// The core's `parse_provider_spec` takes `base: Option<&ParsedBase>` precisely because a pure module cannot
/// reach a URL parser (`core` has no `url` dependency); the WORKER can, and `parse_provider_spec` without it
/// answers "baseURL is not a URL" for every record — which is what this route did on its first replay, for three
/// cases in a row.
///
/// The eight fields are `URL`'s own (`protocol`, `username`, `password`, `search`, `hash`, `hostname`, `origin`,
/// `pathname`), and `worker::Url` is the WHATWG parser, so `https://x.test/v1` answers `https:` + `x.test` +
/// `https://x.test` + `/v1`. An unparsable base answers `None`, which the parser turns into the source's own
/// refusal with the raw text quoted.
pub fn parsed_base(raw: &str) -> Option<summrise_core::provider_spec::ParsedBase> {
    let url = url::Url::parse(raw).ok()?;
    Some(summrise_core::provider_spec::ParsedBase {
        protocol: format!("{}:", url.scheme()),
        username: url.username().to_string(),
        password: url.password().unwrap_or_default().to_string(),
        search: url.query().map(|q| format!("?{q}")).unwrap_or_default(),
        hash: url.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
        hostname: url.host_str().unwrap_or_default().to_string(),
        origin: url.origin().ascii_serialization(),
        pathname: url.path().to_string(),
    })
}

/// `providerKey(env, p)` — the provider's OWN credential: the inline `apiKey`, else the named Worker binding.
///
/// **THE TRUTHINESS IS THE SOURCE'S, AND IT IS TWO DIFFERENT OPERATORS**: `if (p.apiKey)` skips an empty
/// string, and `env?.[p.apiKeyEnv] ?? ""` maps BOTH `undefined` and `null` to the empty string — so an
/// `apiKeyEnv` naming a binding this deployment does not carry answers `""`, which the caller turns into the
/// "no provider key" 502 rather than sending a headerless request.
pub fn provider_key(env: &serde_json::Value, provider: Option<&serde_json::Value>) -> String {
    let Some(p) = provider else {
        return String::new();
    };
    let inline = p.get("apiKey").unwrap_or(&serde_json::Value::Null);
    if crate::translate::truthy_js(inline) {
        return crate::stream::js_text(inline);
    }
    let name = p.get("apiKeyEnv").unwrap_or(&serde_json::Value::Null);
    if crate::translate::truthy_js(name) {
        let binding = crate::stream::js_text(name);
        let value = env.get(&binding).unwrap_or(&serde_json::Value::Null);
        // `?? ""` — an absent binding and an explicit null are the same answer.
        return if value.is_null() {
            String::new()
        } else {
            crate::stream::js_text(value)
        };
    }
    String::new()
}

/// `providerModelVision(p, wire)` — does the record declare this WIRE model as seeing images itself?
/// (DSH's `input: [text, image]`.) `===` on both sides: an id of another type never matches, and `vision`
/// must be the boolean `true`.
pub fn provider_model_vision(provider: Option<&serde_json::Value>, wire: &str) -> bool {
    let Some(models) = provider
        .and_then(|p| p.get("models"))
        .and_then(|m| m.as_array())
    else {
        return false;
    };
    models.iter().any(|m| {
        m.get("id")
            .map(|id| id.as_str() == Some(wire))
            .unwrap_or(false)
            && m.get("vision") == Some(&serde_json::Value::Bool(true))
    })
}

/// **THE BINDING NAMES THE RECORDS POINT AT — AND WHY THEY CANNOT LIVE IN A FIXED LIST.**
///
/// `providerKey` reads `env[p.apiKeyEnv]`: an ARBITRARY binding name chosen by whoever registered the provider,
/// so it is not one of the names `v1.rs`'s `ENV_KEYS` carries. **MEASURED, AND IT WAS THIS PORT'S OWN FIRST
/// BUG**: the built worker answered `502 — "acme/: no provider key"` for a record whose `apiKeyEnv` named a
/// binding the worker DID carry, because the env object handed to the plan had been built from the fixed list
/// alone. `verify.mjs`'s "a NAMED binding's value rides Bearer" is the case that caught it.
///
/// The names are returned in order, deduplicated, and only when truthy — the same `if (p.apiKeyEnv)` the key
/// resolution makes.
pub fn provider_key_env_names(providers: &[serde_json::Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in providers {
        let raw = p.get("apiKeyEnv").unwrap_or(&serde_json::Value::Null);
        if !crate::translate::truthy_js(raw) {
            continue;
        }
        let name = crate::stream::js_text(raw);
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// `advertisedProviderModels` — every model of every CUSTOM PROVIDER, as the listing sees them.
///
/// A provider with no usable prefix is skipped, and so is a model whose wire id is empty after trimming. The
/// returned records carry the PROVIDER and the MODEL too, because `extraModelEntries` reads `provider.label`
/// and `model.contextWindow` off them.
pub fn advertised_provider_models(providers: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for p in providers {
        if !p.is_object() {
            continue;
        }
        let prefix = bare_prefix(p.get("prefix").unwrap_or(&serde_json::Value::Null));
        if prefix.is_empty() {
            continue;
        }
        let models = match p.get("models") {
            Some(serde_json::Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        for m in models {
            let wire = crate::stream::js_text(m.get("id").unwrap_or(&serde_json::Value::Null))
                .trim()
                .trim_start_matches('/')
                .to_string();
            if wire.is_empty() {
                continue;
            }
            out.push(serde_json::json!({
                "id": advertised_model_id(&prefix, &serde_json::Value::String(wire.clone())),
                "wire": wire,
                "provider": p,
                "model": m,
            }));
        }
    }
    out
}

/// `advertisedIds(env)` — the built-ins minus the disabled ones, then the console-added, then the providers'.
///
/// **THE ORDER IS THE LISTING'S**, and the three sources are arguments here because each is a KV read.
pub fn advertised_ids(
    registry_ids: &[&str],
    disabled: &[String],
    custom_ids: &[String],
    provider_ids: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = registry_ids
        .iter()
        .filter(|id| !disabled.iter().any(|d| d == *id))
        .map(|id| (*id).to_string())
        .collect();
    out.extend(custom_ids.iter().cloned());
    out.extend(provider_ids.iter().cloned());
    out
}

/// `extraModelEntries(env)` — the NON-BUILT-IN listing entries, in the source's two groups.
///
/// `owned_by` for a console-added model is its own `ownedBy`; for a provider model it is
/// `provider.label || barePrefix(provider.prefix)`. The optional three are TRUTHINESS-guarded, so a `0`
/// context window is dropped rather than emitted.
pub fn extra_model_entries(
    custom: &[serde_json::Value],
    provided: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for m in custom {
        let mut e = serde_json::Map::new();
        if let Some(id) = m.get("id") {
            e.insert("id".into(), id.clone());
        }
        if let Some(owner) = m.get("ownedBy") {
            e.insert("owned_by".into(), owner.clone());
        }
        for (from, to) in [
            ("name", "name"),
            ("contextWindow", "context_window"),
            ("maxTokens", "max_tokens"),
        ] {
            if let Some(v) = m.get(from).filter(|v| truthy(v)) {
                e.insert(to.into(), v.clone());
            }
        }
        out.push(serde_json::Value::Object(e));
    }
    for entry in provided {
        let provider = entry.get("provider").unwrap_or(&serde_json::Value::Null);
        let model = entry.get("model").unwrap_or(&serde_json::Value::Null);
        let label = provider
            .get("label")
            .filter(|v| truthy(v))
            .map(crate::stream::js_text)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                bare_prefix(provider.get("prefix").unwrap_or(&serde_json::Value::Null))
            });
        let mut e = serde_json::Map::new();
        if let Some(id) = entry.get("id") {
            e.insert("id".into(), id.clone());
        }
        e.insert("owned_by".into(), serde_json::json!(label));
        for (from, to) in [
            ("name", "name"),
            ("contextWindow", "context_window"),
            ("maxTokens", "max_tokens"),
        ] {
            if let Some(v) = model.get(from).filter(|v| truthy(v)) {
                e.insert(to.into(), v.clone());
            }
        }
        out.push(serde_json::Value::Object(e));
    }
    out
}

/// JS truthiness, for the guards above: `0`, `""`, `null` and `false` are all falsy.
fn truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        serde_json::Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod settings_tests {
    //! The two settings rules, which are DIFFERENT and both pinned: `normalizeSetting` turns an explicit OFF
    //! into `null` so a caller's fallback chain sees "unset", while `globalSettingEnabled` treats an EMPTY
    //! string as disabled — a value `normalizeSetting` passes straight through.
    use super::*;

    #[test]
    fn an_explicit_off_normalizes_to_nothing() {
        assert_eq!(normalize_setting(Some("0")), None);
        assert_eq!(normalize_setting(Some("false")), None);
        assert_eq!(normalize_setting(Some("1")), Some("1".to_string()));
        // NOT in the list: the empty string survives normalization...
        assert_eq!(normalize_setting(Some("")), Some(String::new()));
        assert_eq!(normalize_setting(None), None);
        // ...and it is disabled by the OTHER rule, which is the point of having two.
        assert!(!global_setting_enabled(Some("")));
        assert!(!global_setting_enabled(Some("0")));
        assert!(!global_setting_enabled(Some("false")));
        assert!(!global_setting_enabled(None));
        assert!(global_setting_enabled(Some("1")));
        assert!(global_setting_enabled(Some("true")));
    }

    fn models_corpus() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/models-corpus.json");
        let text = std::fs::read_to_string(path).expect("the models corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    /// **THE CATALOGUE CHAIN, REPLAYED FROM THE SHIPPING STORE.** Seven cases, and the two that matter are the
    /// custom providers: one has a trailing slash on its prefix and a model whose id starts with one, the
    /// other has NO prefix at all (whose models are skipped) and a wire that already carries its prefix (which
    /// must not be prepended twice).
    #[test]
    fn the_catalogue_chain_matches_the_shipping_store() {
        let doc = models_corpus();
        let mut checked = 0;
        let mut with_providers = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let disabled: Vec<String> = case["disabled"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let custom = case["custom"].as_array().cloned().unwrap_or_default();
            let providers = case["providers"].as_array().cloned().unwrap_or_default();

            // 1. THE PROVIDER MODELS, then the two lists that consume them.
            let advertised = advertised_provider_models(&providers);
            let provider_ids: Vec<String> = advertised
                .iter()
                .filter_map(|e| e.get("id").and_then(|i| i.as_str()).map(str::to_string))
                .collect();
            let custom_ids: Vec<String> = custom
                .iter()
                .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_string))
                .collect();
            let registry_ids: Vec<&str> = crate::registry::MODEL_REGISTRY
                .iter()
                .map(|m| m.id)
                .collect();
            let got_ids = advertised_ids(&registry_ids, &disabled, &custom_ids, &provider_ids);
            let want_ids: Vec<String> = case["liveIds"]
                .as_array()
                .expect("liveIds")
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            assert_eq!(got_ids, want_ids, "{name}: the advertised ids");

            // 2. THE EXTRA ENTRIES.
            let got_extra = extra_model_entries(&custom, &advertised);
            assert_eq!(
                serde_json::Value::Array(got_extra),
                case["extraEntries"],
                "{name}: the extra entries"
            );
            if !providers.is_empty() {
                with_providers += 1;
                assert!(
                    !advertised.is_empty(),
                    "{name}: a provider with models must advertise some"
                );
            }
            checked += 1;
        }
        assert_eq!(checked, 7, "the models corpus changed size");
        // **TWO OF THE SEVEN HAVE PROVIDERS, AND WITHOUT THEM `advertisedProviderModels` WOULD BE UNEXERCISED**
        // — which is the vacuous-corpus shape this suite keeps finding.
        assert_eq!(with_providers, 2, "the provider cases changed size");
    }

    #[test]
    fn a_provider_with_no_usable_prefix_advertises_nothing() {
        // **THE CASE THAT CAUGHT `js_text(null)`.** `String(x ?? "")` maps null to the EMPTY string, while
        // `js_text` maps a JSON null to the TEXT `"null"` — so a provider whose prefix was absent advertised
        // its models as `null/foo` before this was fixed. An empty prefix skips the provider entirely.
        let none = serde_json::json!([{ "models": [{ "id": "orphan" }] }]);
        assert!(advertised_provider_models(none.as_array().unwrap()).is_empty());
        let empty = serde_json::json!([{ "prefix": "", "models": [{ "id": "orphan" }] }]);
        assert!(advertised_provider_models(empty.as_array().unwrap()).is_empty());
        let slashes = serde_json::json!([{ "prefix": "///", "models": [{ "id": "m" }] }]);
        assert!(advertised_provider_models(slashes.as_array().unwrap()).is_empty());
        // And the "prefix ONCE" rule, stated directly.
        assert_eq!(
            advertised_model_id("acme", &serde_json::json!("acme/chat")),
            "acme/chat"
        );
        assert_eq!(
            advertised_model_id("acme", &serde_json::json!("chat")),
            "acme/chat"
        );
        assert_eq!(
            advertised_model_id("acme/", &serde_json::json!("/chat")),
            "acme/chat",
            "both slashes come off and the prefix lands once"
        );
    }

    #[test]
    fn the_kv_read_wins_and_the_worker_var_is_only_a_fallback() {
        // `getGlobalSetting`: the KV key first, and the Worker var of the same name only when it is absent.
        assert_eq!(
            get_global_setting(Some("1"), Some("0")),
            Some("1".to_string())
        );
        assert_eq!(get_global_setting(None, Some("1")), Some("1".to_string()));
        assert_eq!(get_global_setting(None, None), None);
        // **AND AN EXPLICIT OFF IN KV SHADOWS AN ON IN THE VAR** — the source's own round-94/95 notes: OFF is
        // persisted as "0" precisely so it can shadow the var, and normalization is what makes that work.
        assert_eq!(get_global_setting(Some("0"), Some("1")), None);
    }
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK** — the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_CACHE").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write cache-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_CACHE=1 cargo test store::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/cache-logic.ts is not what store.rs emits — \
             run SUMMRISE_REFRESH_CACHE=1 cargo test store::emission"
        );
    }

    /// **THE EMISSION'S PREFIX LIST IS THE RUST'S, ELEMENT FOR ELEMENT** — the check that would have caught the
    /// missing `"providers:"` on the day it was written, rather than three landings later.
    ///
    /// MUTATION: drop `"providers:"` from `AUTH_PREFIXES` in this file.
    /// RESULT:   this test fails naming the difference — and the emitted module would put a deleted provider
    ///           back on a 24 h cache, which is the behaviour the source's dated comment exists to prevent.
    ///
    /// **AND THE FIRST VERSION OF THIS CHECK COUNTED QUOTES**, `ts.matches("\":\",").count()`, which the
    /// heredoc that wrote it mangled into a two-character pattern: it answered `right: 0` against a list of ten
    /// and would have answered the same for a list of one. The array is PARSED here instead, so the comparison
    /// is between two lists rather than between a list and a guess about its punctuation.
    #[test]
    fn the_emission_carries_the_prefix_list_verbatim() {
        let ts = render_typescript();
        let start = ts
            .find("export const AUTH_PREFIXES: string[] = [")
            .expect("the emitted prefix array");
        let body = &ts[start..];
        let end = body.find("];").expect("a closed array");
        let emitted: Vec<&str> = body[..end].split('"').skip(1).step_by(2).collect();
        assert_eq!(
            emitted,
            AUTH_PREFIXES.to_vec(),
            "the emission's prefix list is not the rule's"
        );
    }
}
