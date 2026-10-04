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

/// `barePrefix(prefix)` — `String(prefix ?? "").replace(/\/+$/, "")`: the TRAILING slashes come off.
///
/// `String(... ?? "")` IS A COERCION, not a null check: a number becomes its digits, an object becomes
/// `[object Object]`, and an array becomes its joined elements. The corpus carries a number.
pub fn bare_prefix(prefix: &serde_json::Value) -> String {
    // **`?? ""` IS NOT DECORATION, AND `js_text` IS NOT A SUBSTITUTE FOR IT.** `String(x ?? "")` maps `null`
    // and `undefined` to the EMPTY string; `js_text` maps a JSON null to the TEXT `"null"` (it mirrors
    // `String(x)`, which is a different expression). Measured: with the direct call, a provider whose `prefix`
    // was absent advertised its models as `null/foo`. The corpus carries a provider with no prefix.
    let text = if prefix.is_null() {
        String::new()
    } else {
        crate::stream::js_text(prefix)
    };
    text.trim_end_matches('/').to_string()
}

/// `advertisedModelId(prefix, wire)` — **"prefix ONCE, never twice"**, in the source's own words.
///
/// The leading slashes come off the wire first, and then the prefix is prepended ONLY IF the wire does not
/// already start with it. A provider whose models are spelled `acme/foo` under the prefix `acme` would
/// otherwise be advertised as `acme/acme/foo`.
pub fn advertised_model_id(prefix: &str, wire: &serde_json::Value) -> String {
    let bare = bare_prefix(&serde_json::Value::String(prefix.to_string()));
    let w = crate::stream::js_text(wire);
    let w = w.trim_start_matches('/');
    if w.starts_with(&format!("{bare}/")) {
        w.to_string()
    } else {
        format!("{bare}/{w}")
    }
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
