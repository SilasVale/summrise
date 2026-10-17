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

/// **THE SINGLE SOURCE NOW — AND THE TYPESCRIPT IS ITS EMISSION.** This was a projection of
/// `store/byok.ts` (`kind`, `userKey`, `envKey`, "the two things this module needs from it"). It carries all five
/// fields because the file it renders needs all five, and `store/byok.ts` is GENERATED from it — the shape
/// `mcp_tools.rs` already has, where the freshness test is a byte comparison rather than a second table.
///
/// **`nv` AND `gmi` DELIBERATELY HAVE NO DEPLOYMENT FALLBACK** and the `None` is carried rather than inferred: a
/// shared source that cannot say "this channel has no env key" would be a loss of information.
pub(crate) const BYOK_CHANNELS: [(&str, &str, Option<&str>, &str, &str); 9] = [
    (
        "opencode",
        "OPENCODE_GO_API_KEY",
        Some("OPENCODE_GO_API_KEY"),
        "og",
        "openai",
    ),
    (
        "deepseek",
        "DEEPSEEK_API_KEY",
        Some("DEEPSEEK_API_KEY"),
        "ds",
        "anthropic",
    ),
    (
        "qwen",
        "QWEN_API_KEY",
        Some("QWEN_API_KEY"),
        "qw",
        "anthropic",
    ),
    (
        "openrouter",
        "OPENROUTER_API_KEY",
        Some("OPENROUTER_API_KEY"),
        "or",
        "anthropic",
    ),
    ("nvidia", "NVAPI_KEY", None, "nv", "openai"),
    ("gmi", "GMI_API_KEY", None, "gmi", "openai"),
    (
        "commandgoat",
        "CMD_API_KEY",
        Some("CMD_API_KEY"),
        "cm",
        "openai",
    ),
    (
        "amd",
        "AMD_API_KEY",
        Some("AMD_API_KEY"),
        "amd",
        "anthropic",
    ),
    ("r4", "R4_API_KEY", Some("R4_API_KEY"), "r4", "anthropic"),
];

/// The emitted module's header — **THE ORIGINAL FILE'S OWN COMMENT, CARRIED VERBATIM, PLUS THE GENERATED NOTICE.**
///
/// **AN EMISSION THAT LOSES THE FILE'S DOCUMENTED REASON IS A LOSS, NOT A SIMPLIFICATION.** That comment is the
/// record of why the table exists at all — the four independently-typed tables, the author who wrote the risk down
/// and then named the wrong file, and the three facets each earned — and none of it is recoverable from the rows.
const TS_HEADER: &str = "\
/**
 * byok.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/byok.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command
 * to fix it:
 *
 *     SUMMRISE_REFRESH_BYOK=1 cargo test --manifest-path gateway/wasm/Cargo.toml byok
 *
 * The comment below is the file's own, carried verbatim: it is the record of why this table exists, and it is not
 * recoverable from the rows.
 *
 * byok — THE single source for the BYOK channel vocabulary (round 190).
 *
 * WHY THIS EXISTS. Until this file, the same eight channels and their env-var names lived
 * in FOUR independently-typed tables: `model-route.ts`'s CHANNEL_KEY_RULES (keyed by routing
 * PREFIX), `models-probe.ts`'s BYOK_KEY_FOR_KIND (keyed by route KIND), `translate-vision.ts`'s
 * per-key `{key, shape}` table, and `store/users.ts`'s bare USER_KEY_NAMES list. Nothing
 * compared them; the values agreed only because whoever typed them was careful. The author of
 * the second one wrote the risk down — \"a second naming of channels is how two tables drift
 * apart\" — and then named the wrong file in the pointer, so a reader who followed it found a
 * CONSUMER (translate.ts) and no table at all.
 *
 * WHAT IT IS NOT. It is not a merge of those four tables. `model-route.ts` keys by prefix and
 * the other two key by kind, and NOTHING in the repository mapped one vocabulary to the other —
 * so the shared thing is the table all four can DERIVE from, and the prefix<->kind pairing is
 * the part that did not exist anywhere until this file.
 *
 * THREE FACETS, EACH ONE EARNED: `userKey`/`envKey` (round 187's measurement) and `shape`
 * (found by reading the FOURTH table rather than assuming it from the three that agreed — a
 * shared source with only the first two would have forced `translate-vision.ts` to keep its own
 * table and the drift class would have survived the fix). `envKey: null` is load-bearing: it
 * means \"this channel has no environment fallback\", which was expressible in exactly one of the
 * four tables, so a merge that flattened to `kind -> keyName` would have DELETED a distinction.
 *
 * PURE DATA, ZERO IMPORTS, deliberately: every one of the four consumers can import it without
 * creating a cycle, which is the property that makes the derivation possible at all.
 */

export interface ByokChannel {
  prefix: string;
  kind: string;
  userKey: string;
  envKey: string | null;
  shape: \"anthropic\" | \"openai\";
}

export const BYOK_CHANNELS: ByokChannel[] = [
";
/// **THE PRODUCER.** `store/byok.ts` is exactly this function's output.
pub fn render_typescript() -> String {
    let mut out = String::from(TS_HEADER);
    for &(kind, user_key, env_key, prefix, shape) in BYOK_CHANNELS.iter() {
        // The source's own formatting: every row breaks, EXCEPT the two whose line fits — and those two are
        // the ones with `envKey: null`, which is why they are short. **The emitted bytes are the file's bytes**,
        // so the layout is part of the emission and not a prettier decision made later.
        let env = match env_key {
            Some(v) => format!("\"{v}\""),
            None => "null".to_string(),
        };
        let short = format!(
            "  {{ prefix: \"{prefix}\", kind: \"{kind}\", userKey: \"{user_key}\", envKey: {env}, shape: \"{shape}\" }},\n"
        );
        // **THE THRESHOLD IS PRETTIER S PRETTIER S RULE, MEASURED**: at 100 the `amd` row broke and the
        // emitted bytes stopped matching the source. The rule is the repo s configured printWidth.
        if short.chars().count() <= 101 {
            out.push_str(&short);
        } else {
            out.push_str(&format!(
                "  {{\n    prefix: \"{prefix}\",\n    kind: \"{kind}\",\n    userKey: \"{user_key}\",\n    envKey: {env},\n    shape: \"{shape}\",\n  }},\n"
            ));
        }
    }
    out.push_str(
        "];

export function byPrefix(prefix: string): ByokChannel | undefined {
  return BYOK_CHANNELS.find((c) => c.prefix === prefix);
}

export function byKind(kind: string): ByokChannel | undefined {
  return BYOK_CHANNELS.find((c) => c.kind === kind);
}
",
    );
    out
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/byok.ts")
}

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
/// `CHANNEL_KEY_RULES[prefix]` — the channel's `(userKey, envKey)` for a model's prefix, or `None`.
///
/// **THE TABLE IS `BYOK_CHANNELS`, THE SAME ONE `store/byok.ts` IS EMITTED FROM**, so the route selection's
/// key matrix and `/v1`'s BYOK decisions cannot drift: `model-route.ts` carried its own projection of this
/// table until this lookup existed.
pub fn rule_for_prefix(prefix: &str) -> Option<(&'static str, Option<&'static str>)> {
    BYOK_CHANNELS
        .iter()
        .find(|(_, _, _, row_prefix, _)| *row_prefix == prefix)
        .map(|(_, user_key, env_key, _, _)| (*user_key, *env_key))
}

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
    let Some((_, _, env_key, _, _)) = BYOK_CHANNELS.iter().find(|(k, ..)| *k == kind) else {
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

#[cfg(test)]
mod emission_tests {
    use super::*;

    /// **THE FRESHNESS CHECK — THE SAME SHAPE `mcp_tools.rs` AND `settings_logic.rs` USE.** A hand edit to
    /// `gateway/src/store/byok.ts` (or a table change that was not re-emitted) fails here with the text and the one
    /// command that fixes it.
    #[test]
    fn the_typescript_table_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_BYOK").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write byok.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_BYOK=1 cargo test byok",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/store/byok.ts is not what byok.rs emits — run SUMMRISE_REFRESH_BYOK=1 cargo test byok"
        );
    }

    /// **THE TABLE IS THE SOURCE NOW, SO ITS SHAPE IS WORTH PINNING** — and so is the ORDER, because the emitted
    /// rows are the file's bytes.
    ///
    /// MUTATION: swap `nv` and `gmi`'s rows.
    /// RESULT:   the order assertion fails, and the emitted file would reorder two rows the console reads in order.
    #[test]
    fn the_table_is_nine_rows_in_the_sources_order() {
        assert_eq!(BYOK_CHANNELS.len(), 9);
        let kinds: Vec<&str> = BYOK_CHANNELS.iter().map(|(k, ..)| *k).collect();
        assert_eq!(
            kinds,
            vec![
                "opencode",
                "deepseek",
                "qwen",
                "openrouter",
                "nvidia",
                "gmi",
                "commandgoat",
                "amd",
                "r4"
            ]
        );
        // **`nv` AND `gmi` HAVE NO DEPLOYMENT FALLBACK** and the `None` is carried, not inferred.
        for (kind, _, env_key, _, _) in BYOK_CHANNELS {
            let expected_none = kind == "nvidia" || kind == "gmi";
            assert_eq!(env_key.is_none(), expected_none, "{kind}'s envKey");
        }
    }
}
