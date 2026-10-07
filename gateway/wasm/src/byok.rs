//! THE BYOK DECISIONS, MOVED FROM `gateway/src/plugins/translate.ts` AND `gateway/src/store/byok.ts`.
//!
//! WHY THESE. `REQUIRED_KEY_BY_KIND` is described in its own comment as "the ONE canonical copy of a
//! mapping that was previously written out FIVE times", and the irregular entries are the whole point of
//! centralising it — three of the nine do NOT match their kind (`nvidia -> nv`, `opencode -> opencodeGo`,
//! `commandgoat -> cmd`). `bearerKeyFor` is the single source for the bearer key every flow uses, and the
//! comment records what its absence cost: a request that ROUTED to a channel because env had a key, then
//! 502'd at the bearer site because only the user half was read.
//!
//! THEY TAKE THEIR FACTS AS ARGUMENTS. `env` and the user's key record are `serde_json::Value` here rather
//! than a binding, which is what lets the oracle drive the shipping TypeScript and this port over the same
//! corpus. The DECISIONS are pure; the KV read that produces `ukeys` stays at the edge, where it belongs.

/// `store/byok.ts`'s `BYOK_CHANNELS`, as the two things this module needs from it: the kind -> user-key
/// field map, and the kind -> ENV-KEY map where `nv` and `gmi` deliberately have NO deployment fallback.
/// A shared source that cannot say "this channel has no env key" would be a loss of information, which is
/// why the `null` is carried rather than inferred.
pub(crate) const BYOK_CHANNELS: [(&str, &str, Option<&str>); 9] = [
    (
        "opencode",
        "OPENCODE_GO_API_KEY",
        Some("OPENCODE_GO_API_KEY"),
    ),
    ("deepseek", "DEEPSEEK_API_KEY", Some("DEEPSEEK_API_KEY")),
    ("qwen", "QWEN_API_KEY", Some("QWEN_API_KEY")),
    (
        "openrouter",
        "OPENROUTER_API_KEY",
        Some("OPENROUTER_API_KEY"),
    ),
    ("nvidia", "NVAPI_KEY", None),
    ("gmi", "GMI_API_KEY", None),
    ("commandgoat", "CMD_API_KEY", Some("CMD_API_KEY")),
    ("amd", "AMD_API_KEY", Some("AMD_API_KEY")),
    ("r4", "R4_API_KEY", Some("R4_API_KEY")),
];

/// `REQUIRED_KEY_BY_KIND` — route kind -> the field `extractByokKeys` spells it as. The three irregular
/// ones are `nvidia -> nv`, `opencode -> opencodeGo`, `commandgoat -> cmd`.
// `pub(crate)` so the dispatch's own test can build a store record without copying the table — a second copy
// is how two surfaces come to disagree about one kind.
pub(crate) const REQUIRED_KEY_BY_KIND: [(&str, &str); 9] = [
    ("deepseek", "deepseek"),
    ("opencode", "opencodeGo"),
    ("openrouter", "openRouter"),
    ("qwen", "qwen"),
    ("nvidia", "nv"),
    ("gmi", "gmi"),
    ("commandgoat", "cmd"),
    ("amd", "amd"),
    ("r4", "r4"),
];

/// JavaScript truthiness — `x || null` keeps a truthy value AS IT IS, whatever its type, and answers
/// `null` for `""`, `0`, `NaN`, `false`, `null` and `undefined`. A NUMBER in a key record is nonsense,
/// but it is nonsense the shipping worker CARRIES, so the value is a `Value` here rather than a `String`.
fn truthy_or_null(v: Option<&serde_json::Value>) -> serde_json::Value {
    match v {
        Some(value) => {
            let keep = match value {
                serde_json::Value::Null => false,
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
                serde_json::Value::String(s) => !s.is_empty(),
                serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
            };
            if keep {
                value.clone()
            } else {
                serde_json::Value::Null
            }
        }
        None => serde_json::Value::Null,
    }
}

/// `extractByokKeys(ukeys)` — a FIXED nine-field object, each field the record's value or `null`.
///
/// The fixed shape is the contract: an unknown field in the record is not carried, and a field the record
/// omits is present as `null` rather than absent.
pub fn extract_byok_keys(ukeys: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    const FIELDS: [(&str, &str); 9] = [
        ("deepseek", "DEEPSEEK_API_KEY"),
        ("opencodeGo", "OPENCODE_GO_API_KEY"),
        ("openRouter", "OPENROUTER_API_KEY"),
        ("qwen", "QWEN_API_KEY"),
        ("nv", "NVAPI_KEY"),
        ("gmi", "GMI_API_KEY"),
        ("cmd", "CMD_API_KEY"),
        ("amd", "AMD_API_KEY"),
        ("r4", "R4_API_KEY"),
    ];
    let mut out = serde_json::Map::new();
    for (field, source) in FIELDS {
        out.insert(field.to_string(), truthy_or_null(ukeys.get(source)));
    }
    out
}

/// `bearerKeyFor(env, byok, kind)`: the user's BYOK key first, then the deployment's Worker secret.
///
/// **`envKey: null` IS NOT `envKey: ""`.** For `nvidia` and `gmi` the channel declares NO deployment
/// fallback, so this answers `null` even when the env carries a value of that name — the contract's own
/// words are that those two are "pure BYOK". An unknown kind answers `null` too, because it has no entry
/// in either table.
pub fn bearer_key_for(
    env: &serde_json::Value,
    byok: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
) -> serde_json::Value {
    let Some((_, field)) = REQUIRED_KEY_BY_KIND.iter().find(|(k, _)| *k == kind) else {
        return serde_json::Value::Null;
    };
    let user = truthy_or_null(byok.get(*field));
    if !user.is_null() {
        return user;
    }
    let Some((_, _, env_key)) = BYOK_CHANNELS.iter().find(|(k, _, _)| *k == kind) else {
        return serde_json::Value::Null;
    };
    match env_key {
        Some(name) => truthy_or_null(env.get(*name)),
        None => serde_json::Value::Null,
    }
}

/// `isKeyMissing(routeKind, byok, env?)`: is the required key absent from BOTH places?
///
/// **AN UNKNOWN KIND IS NOT MISSING.** A route with no entry in the table is a programming error the
/// routing layer or the upstream will surface; inventing a "missing key" for it here would mask that with
/// a config error. And `env` is optional in the source — omitted, this is the historical "BYOK only"
/// semantics — which is `env ?? {}` here.
pub fn is_key_missing(
    route_kind: &str,
    byok: &serde_json::Map<String, serde_json::Value>,
    env: Option<&serde_json::Value>,
) -> bool {
    if !REQUIRED_KEY_BY_KIND.iter().any(|(k, _)| *k == route_kind) {
        return false;
    }
    let empty = serde_json::json!({});
    bearer_key_for(env.unwrap_or(&empty), byok, route_kind).is_null()
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `byok` cases were produced by the SHIPPING TypeScript
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

    fn byok_of(v: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        extract_byok_keys(v)
    }

    #[test]
    fn every_byok_decision_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "extract_byok_keys" => {
                    let got = serde_json::Value::Object(byok_of(input));
                    assert_eq!(&got, want, "{func} / {name}");
                }
                "bearer_key_for" => {
                    let byok = byok_of(&input["ukeys"]);
                    let env = input.get("env").cloned().unwrap_or(serde_json::json!({}));
                    let got = bearer_key_for(&env, &byok, input["kind"].as_str().unwrap_or(""));
                    assert_eq!(&got, want, "{func} / {name}");
                }
                "is_key_missing" => {
                    let byok = byok_of(&input["ukeys"]);
                    let env = input.get("env");
                    let got = is_key_missing(input["kind"].as_str().unwrap_or(""), &byok, env);
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(checked >= 20, "the byok corpus shrank to {checked} cases");
    }
}
