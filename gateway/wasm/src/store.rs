//! THE PURE RULES OF `gateway/src/store/cache.ts` AND `gateway/src/session.ts`.
//!
//! WHY THESE. The cache's TTL rule is what DECOUPLES KV READ VOLUME FROM REQUEST VOLUME — the module's own
//! comment measures it: "each key costs at most one read per day per isolate, instead of one read per
//! request", and the short-TTL prefix list exists because "a deleted or re-pointed provider must stop being
//! dialled within a minute on every isolate, not within a day". The session pair is the FAIL-CLOSED rule for
//! issuing new sessions. The caches, the locks and the KV reads stay in TypeScript: they are state and I/O.

/// The two TTLs, verbatim.
pub const CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;
pub const AUTH_CACHE_TTL_MS: i64 = 60 * 1000;

/// The per-isolate cache's size bound.
pub const CACHE_MAX_ENTRIES: usize = 512;

/// `AUTH_PREFIXES` — the key prefixes that get the SHORT TTL.
///
/// **`"providers:"` IS NOT IN THIS LIST, AND THAT IS A DECISION WITH A DATE.** The source keeps it as a
/// commented entry with its reasoning (a custom provider record carries a credential and an egress
/// DESTINATION, so a deleted or re-pointed one must stop being dialled within a minute) and the read cost is
/// bounded because `resolveRoute` consults that key only for prefixes the built-in route table does not
/// cover. A port that "restored" the commented line would be changing a decision rather than transcribing
/// one, so the list is exactly what the code holds.
pub const AUTH_PREFIXES: [&str; 9] = [
    "settings:",
    "token:",
    "user:",
    "ukeys:",
    "auth:",
    "route:",
    "devices:",
    "plugins:",
    "cf:",
];

/// `cset`'s TTL decision: the short TTL for an auth-shaped key, the 24 h one otherwise.
pub fn ttl_for(key: &str) -> i64 {
    if AUTH_PREFIXES.iter().any(|p| key.starts_with(p)) {
        AUTH_CACHE_TTL_MS
    } else {
        CACHE_TTL_MS
    }
}

/// `cset`'s eviction: when the cache is at its bound, the OLDEST entry goes.
///
/// **"OLDEST" IS THE INSERTION ORDER OF A `Map`, WHICH IS WHY THIS ANSWERS A POSITION.** The same lesson the
/// relay's queue learned from a flaky test on 2026-10-02: a structure without insertion order cannot answer
/// "the oldest", and `Map` promises it. What this returns is the index to drop — 0 when the bound is
/// reached, and nothing otherwise.
pub fn cache_eviction_index(size: usize) -> Option<usize> {
    if size >= CACHE_MAX_ENTRIES {
        Some(0)
    } else {
        None
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
        // The nine prefixes take the SHORT ttl; everything else takes the 24 h one.
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
        // **`providers:` IS NOT ON THE LIST** — the source keeps it as a COMMENTED entry with its dated
        // reasoning, and "restoring" it would be changing a decision rather than transcribing one.
        assert_eq!(ttl_for("providers:x"), CACHE_TTL_MS);
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
