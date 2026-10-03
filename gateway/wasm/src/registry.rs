//! THE MODEL REGISTRY, MOVED FROM `gateway/src/channels.ts`.
//!
//! WHY THIS ONE. Its own header calls it the fix for a measured failure: adding a model used to mean
//! remembering SIX separate tables, and "nothing FAILED if you forgot one — measured: adding an id to
//! MODELS alone left all 737 tests green while the model was half-wired". Every table in that file now
//! DERIVES from this registry, and **ORDER IS PART OF THE CONTRACT** — `/v1/models` serves it verbatim.
//!
//! IT ALSO UNBLOCKS THREE PORTS THAT WERE NAMED RATHER THAN APPROXIMATED, because each of them reads a
//! table DERIVED from this one: `wireModelName` (the og wire aliases), `searchTargetFor` (the
//! search-capable wire slugs) and `oxAlphaReasoningDefault` (the raw reasoning facet). They are the next
//! step, not this one.
//!
//! THE TABLE WAS GENERATED ONCE FROM THE TYPESCRIPT and is checked by the corpus thereafter: 24 records,
//! every facet the source sets. A transcription is exactly where a quiet typo hides, which is what the
//! oracle is for.

/// `ModelSpec`'s facets, as options — an absent facet and a false one are different things in this file
/// (`probe: false` MUST carry a reason, and the corpus asserts the records that do).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSpec {
    pub id: &'static str,
    pub owned_by: &'static str,
    pub wire: Option<&'static str>,
    pub us_egress: Option<bool>,
    pub search: Option<bool>,
    pub responses_only: Option<bool>,
    pub reasoning_max: Option<ReasoningMax>,
    pub reasoning_effort: Option<&'static str>,
    pub context_window: Option<i64>,
    pub max_tokens: Option<i64>,
    pub probe: Option<bool>,
    pub probe_why: Option<&'static str>,
}

/// `reasoningMax`'s mechanism — WHICH PATH injects the default, which is part of the facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningMax {
    /// Passthrough routes forward raw text (no parse, for CPU), so the default is injected textually.
    Raw,
    /// Translate routes already hold the parsed object, so it is set on that object.
    Parsed,
}

/// See the header. Order = the `/v1/models` order.
pub const MODEL_REGISTRY: [ModelSpec; 24] = [
    // 24 entries, generated once from gateway/src/channels.ts and then checked by the corpus.
    ModelSpec {
        id: "og/deepseek-v4.1-flash",
        owned_by: "opencode",
        wire: Some("deepseek-flash"),
        us_egress: None,
        search: Some(true),
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "og/minimax-m3",
        owned_by: "opencode",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(false),
        probe_why: Some("translate-only model; the og/ health card covers the channel"),
    },
    ModelSpec {
        id: "og/mimo-v2.5",
        owned_by: "opencode",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "og/mimo-v2.6-flash",
        owned_by: "opencode",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "og/ox-alpha-free",
        owned_by: "opencode",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: Some(ReasoningMax::Parsed),
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "og/muse-spark-1.3-contributor",
        owned_by: "opencode",
        wire: None,
        us_egress: Some(true),
        search: None,
        responses_only: Some(true),
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "og/muse-spark-1.2-contributor",
        owned_by: "opencode",
        wire: None,
        us_egress: Some(true),
        search: None,
        responses_only: Some(true),
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(false),
        probe_why: Some("superseded by 1.3; one probe per model family is enough"),
    },
    ModelSpec {
        id: "og/gpt-5.6-luna",
        owned_by: "opencode",
        wire: None,
        us_egress: Some(true),
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "og/openai/gpt-5.6-luna:floor[1m]",
        owned_by: "opencode",
        wire: None,
        us_egress: Some(true),
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(false),
        probe_why: Some("the or/ spelling below is the probed luna card"),
    },
    ModelSpec {
        id: "or/openai/gpt-5.6-luna:floor[1m]",
        owned_by: "openrouter",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "or/z-ai/glm-5.2:free",
        owned_by: "openrouter",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "or/nvidia/nemotron-3-ultra-550b-a55b:free",
        owned_by: "openrouter",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "nv/nvidia/nemotron-3-ultra-550b-a55b",
        owned_by: "nvidia",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "nv/moonshotai/kimi-k3",
        owned_by: "nvidia",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(false),
        probe_why: Some("the nv/ health card already probes nemotron"),
    },
    ModelSpec {
        id: "gmi/MiniMaxAI/MiniMax-M3",
        owned_by: "gmi",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "gmi/MiniMaxAI/MiniMax-M2.7",
        owned_by: "gmi",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "or/stealth/ox-alpha",
        owned_by: "openrouter",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: Some(ReasoningMax::Raw),
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "qw/qwen3.8-max-preview",
        owned_by: "qwen",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "qw/qwen3.8-flash",
        owned_by: "qwen",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "qw/deepseek-v4.1-flash",
        owned_by: "qwen",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "cm/meituan/LongCat-2.0:free",
        owned_by: "command-code",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "cm/poolside/laguna-s-2.1-free",
        owned_by: "command-code",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "cm/deepseek/deepseek-v4.1-flash",
        owned_by: "command-code",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
    ModelSpec {
        id: "r4/deepseek-v4.1-flash",
        owned_by: "r4",
        wire: None,
        us_egress: None,
        search: None,
        responses_only: None,
        reasoning_max: None,
        reasoning_effort: None,
        context_window: None,
        max_tokens: None,
        probe: Some(true),
        probe_why: None,
    },
];

/// The wire name a record is looked up by: the declared `wire`, else the advertised id with its channel
/// stripped. **THIS IS THE KEY THE TRANSLATE PATH HOLDS** — it has already resolved the route, so the
/// advertised prefix is gone by then.
fn wire_of(m: &ModelSpec) -> &str {
    match m.wire {
        Some(w) => w,
        None => match m.id.find('/') {
            Some(i) => &m.id[i + 1..],
            None => m.id,
        },
    }
}

/// `modelSpec(id)` — a record by advertised id.
pub fn model_spec(id: &str) -> Option<&'static ModelSpec> {
    MODEL_REGISTRY.iter().find(|m| m.id == id)
}

/// `wireSpec(wireName)` — a record by the name the UPSTREAM sees.
///
/// **AMBIGUOUS NAMES RESOLVE TO NOTHING.** Stripping the channel makes two channels that advertise the
/// same upstream slug collide (`og/openai/gpt-5.6-luna:floor[1m]` and `or/openai/gpt-5.6-luna:floor[1m]`
/// share one), and picking either would let one channel's model INHERIT the other's facet — silently wrong
/// the moment a facet is declared, which is the class of bug this registry exists to remove. So: exactly
/// one match, or none.
pub fn wire_spec(wire_name: &str) -> Option<&'static ModelSpec> {
    let mut hit: Option<&'static ModelSpec> = None;
    for m in MODEL_REGISTRY.iter() {
        if wire_of(m) != wire_name {
            continue;
        }
        if hit.is_some() {
            return None;
        }
        hit = Some(m);
    }
    hit
}

/// `reasoningMaxRawFor(wireName)` — does this model default its reasoning effort to `max` on the RAW-BODY
/// (passthrough) path?
pub fn reasoning_max_raw_for(wire_name: &str) -> bool {
    matches!(
        wire_spec(wire_name).and_then(|m| m.reasoning_max),
        Some(ReasoningMax::Raw)
    )
}

/// The same, for the PARSED-object (translate) path.
pub fn reasoning_max_parsed_for(wire_name: &str) -> bool {
    matches!(
        wire_spec(wire_name).and_then(|m| m.reasoning_max),
        Some(ReasoningMax::Parsed)
    )
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `registry` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them — INCLUDING THE WHOLE TABLE, dumped
    //! record by record, because a transcription is where a quiet typo hides.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    fn record_json(m: &ModelSpec) -> serde_json::Value {
        let opt_str = |v: Option<&str>| match v {
            Some(s) => serde_json::Value::String(s.to_string()),
            None => serde_json::Value::Null,
        };
        let opt_bool = |v: Option<bool>| match v {
            Some(b) => serde_json::Value::Bool(b),
            None => serde_json::Value::Null,
        };
        let opt_num = |v: Option<i64>| match v {
            Some(n) => serde_json::json!(n),
            None => serde_json::Value::Null,
        };
        serde_json::json!({
            "id": m.id,
            "ownedBy": m.owned_by,
            "wire": opt_str(m.wire),
            "usEgress": opt_bool(m.us_egress),
            "search": opt_bool(m.search),
            "responsesOnly": opt_bool(m.responses_only),
            "reasoningMax": opt_str(m.reasoning_max.map(|r| match r {
                ReasoningMax::Raw => "raw",
                ReasoningMax::Parsed => "parsed",
            })),
            "reasoningEffort": opt_str(m.reasoning_effort),
            "contextWindow": opt_num(m.context_window),
            "maxTokens": opt_num(m.max_tokens),
            "probe": opt_bool(m.probe),
            "probeWhy": opt_str(m.probe_why),
        })
    }

    #[test]
    fn every_registry_case_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "model_registry" => {
                    let got: Vec<serde_json::Value> =
                        MODEL_REGISTRY.iter().map(record_json).collect();
                    assert_eq!(&serde_json::Value::Array(got), want, "{func} / {name}");
                }
                "model_spec" => {
                    let hit = model_spec(input.as_str().unwrap_or(""));
                    let got = match hit {
                        Some(m) => serde_json::json!({ "id": m.id, "ownedBy": m.owned_by }),
                        None => serde_json::json!({ "null": true }),
                    };
                    assert_eq!(&got, want, "{func} / {name}");
                }
                "wire_spec" => {
                    let hit = wire_spec(input.as_str().unwrap_or(""));
                    let got = match hit {
                        Some(m) => serde_json::json!({ "id": m.id }),
                        None => serde_json::json!({ "null": true }),
                    };
                    assert_eq!(&got, want, "{func} / {name}");
                }
                "reasoning_max_raw_for" => {
                    let got = reasoning_max_raw_for(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                "reasoning_max_parsed_for" => {
                    let got = reasoning_max_parsed_for(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 30,
            "the registry corpus shrank to {checked} cases"
        );
    }
}
