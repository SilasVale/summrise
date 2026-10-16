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

/// The health cards, **MOVED HERE FROM `gateway/src/channels.ts` (2026-10-14)** — order included, because the
/// order is what the response bytes are.
///
/// **THIS TABLE WAS MIRRORED TWICE AND COMPARED NOWHERE.** `lib.rs` said of its copy "MIRRORING
/// `gateway/src/channels.ts`'s `HEALTH_CHANNELS` verbatim", and the console had the original; the only thing
/// standing between them was `/api/health`'s fixture, **which is a SNAPSHOT rather than a comparison** — so a
/// **THE CARD MODELS, IN ORDER — the list Task 2's ratchet reads.**
///
/// A card is a LABEL, and a label for a row `/v1/models` does not advertise is a lie the channel registry would
/// keep serving: `og/ox-alpha-free` answered 400 upstream and was still being probed as a health card until
/// 2026-10-15. The ratchet in `model_store.rs` asserts every id here is a CURATED row, so a card can only name
/// something the device actually offers.
pub fn health_model_ids() -> Vec<&'static str> {
    HEALTH_CHANNELS.iter().map(|(_, m)| *m).collect()
}

/// card edited on one side would have left the probe asking about a model the other side retired. It is the same
/// gap `CHANNEL_CARDS` closed, and it is closed the same way: one table, emitted back to the console.
///
/// **THE DUPLICATE IDS ARE DELIBERATE**: `build_health` asks the og circuit once per og card, and `recommended`
/// takes the FIRST card of a priority id.
pub const HEALTH_CHANNELS: [(&str, &str); 19] = [
    ("qw", "qw/qwen3.8-max-preview"),
    ("qw", "qw/qwen3.8-flash"),
    ("qw", "qw/deepseek-v4.1-flash"),
    ("og", "og/deepseek-v4.1-flash"),
    ("og", "og/gpt-5.6-luna"),
    ("og", "og/mimo-v2.5"),
    ("og", "og/mimo-v2.6-flash"),
    ("og", "og/muse-spark-1.3-contributor"),
    ("or", "or/openai/gpt-5.6-luna:floor[1m]"),
    ("or", "or/z-ai/glm-5.2:free"),
    ("or", "or/nvidia/nemotron-3-ultra-550b-a55b:free"),
    ("or", "or/stealth/ox-alpha"),
    ("nv", "nv/nvidia/nemotron-3-ultra-550b-a55b"),
    ("gmi", "gmi/MiniMaxAI/MiniMax-M3"),
    ("gmi", "gmi/MiniMaxAI/MiniMax-M2.7"),
    ("cm", "cm/deepseek/deepseek-v4.1-flash"),
    ("cm", "cm/meituan/LongCat-2.0:free"),
    ("cm", "cm/poolside/laguna-s-2.1-free"),
    ("r4", "r4/deepseek-v4.1-flash"),
];

/// The default channel leads (`HEALTH_PRIORITY`), so the "recommended" badge points where `auto` resolves.
pub const HEALTH_PRIORITY: [&str; 4] = ["cm", "qw", "og", "or"];

/// **THE CONSOLE'S CHANNEL CARDS — `ROUTE_INFO` — AND THEY USED TO BE THE TYPESCRIPT'S.**
///
/// They were `gateway/src/channels.ts`'s table, mirrored into the worker as a GENERATED JSON TWIN
/// (`wasm/route-info.json`) plus a script that wrote it plus a test that checked the twin. **A TWIN IS A
/// SECOND SOURCE WEARING A FRESHNESS TEST**: every prose edit had to be made in TypeScript, copied by a
/// script, committed, and only then could the worker serve it. `/api/admin/public` was the only reason the
/// worker needed them at all — so the table moved here, the cards are rendered BACK to the console by
/// [`render_typescript`], and the twin, its generator and its test are deleted.
///
/// **WHAT THE OPERATOR LOSES**: a prose edit is now a Rust edit plus the refresh command. **WHAT THE
/// REPOSITORY GAINS**: one source for nine cards, and no chance of the console's "model routing" section
/// describing a channel the worker does not dial.
///
/// The order is the console's own, and `none` is LAST — it is the default channel's card, not a prefix.
pub const CHANNEL_CARDS: [(&str, &str, &str); 8] = [
    (
        "og/",
        "OpenCode Go",
        "opencode.ai/zen/go — all models via chat/completions (OpenAI format); gpt-5.6-luna auto-routes via OpenRouter US exit (zen region-blocks it); muse-spark-* via /v1/responses forced through the US exit (Meta region policy)",
    ),
    (
        "or/",
        "OpenRouter",
        "openrouter.ai — user's own key (BYOK); dual-format passthrough, US-proxy switch decides direct vs exit",
    ),
    (
        "nv/",
        "NVIDIA NIM",
        "integrate.api.nvidia.com — official nemotron API, dedicated key capacity (build.nvidia.com), OpenAI format",
    ),
    (
        "gmi/",
        "GMI Cloud",
        "api.gmi-serving.com — MiniMax Week free tier (MiniMax-M3/M2.7 free 14 days, user's own GMI key), OpenAI format; any catalog model reachable as gmi/<id>",
    ),
    (
        "qw/",
        "Qwen MaaS (Aliyun)",
        "token-plan.ap-southeast-1.maas.aliyuncs.com — Anthropic passthrough",
    ),
    (
        "cm/",
        "Command Code (GOAT)",
        "api.commandcode.ai/provider — GOAT plan & up get Provider API access (Go plan excluded); Anthropic /v1/messages translated to chat/completions (the Anthropic endpoint only serves claude-*), OpenAI format passes through; any catalog model reachable as cm/<id>",
    ),
    (
        "r4/",
        "r4.codes",
        "api.r4.codes — Anthropic /v1/messages AND OpenAI /v1/chat/completions both native (no translation either way), user's own R4_API_KEY; no server-side tools — a web_search request is forwarded UNCHANGED and the tool is ignored (r4 is not on the parse gate that swaps to the og/ lane); any catalog model reachable as r4/<id>",
    ),
    (
        "none",
        "Command Code (default)",
        "no prefix → the default channel, Command Code (GOAT) with the model name passed through as-is; `auto` resolves to cm/deepseek/deepseek-v4.1-flash (per-user selection first, see model-route.ts). Note: cm/ rides Command Code's OpenAI endpoint, which rejects claude-* ids (those exist only on their Anthropic endpoint) — use a prefixed deepseek/OSS model there",
    ),
];

/// `routeModelsFor(prefix, noneModels)` — **THE DERIVATION, NOT A FIFTH COPY OF THE CATALOGUE.**
///
/// The console's own comment records what the hand-maintained version cost: `og/`'s list omitted
/// `openai/gpt-5.6-luna:floor[1m]` entirely while `/v1/models` advertised it, and two channels listed theirs in
/// a different order — "nothing caught it: the only ROUTE_INFO test checked that the PREFIXES were covered,
/// never the lists". The list is therefore DERIVED from [`MODEL_REGISTRY`] in catalogue order, and the `none`
/// card is the one exception: it carries an explicit list because a bare model name has no prefix to filter by.
pub fn route_models_for(prefix: &str, none_models: &[&str]) -> Vec<String> {
    if prefix == "none" {
        return none_models.iter().map(|s| (*s).to_string()).collect();
    }
    MODEL_REGISTRY
        .iter()
        .filter(|m| m.id.starts_with(prefix))
        .map(|m| m.id[prefix.len()..].to_string())
        .collect()
}

/// The `none` card's explicit list — **ONE ENTRY, AND IT IS THE MODEL `auto` RESOLVES TO**.
pub const NONE_CARD_MODELS: [&str; 1] = ["deepseek/deepseek-v4.1-flash"];

/// The cards as JSON, in the console's order — what `/api/admin/public` serves and what the emission renders.
pub fn channel_cards() -> Vec<serde_json::Value> {
    CHANNEL_CARDS
        .iter()
        .map(|(prefix, backend, desc)| {
            serde_json::json!({
                "prefix": prefix,
                "backend": backend,
                "desc": desc,
                "models": route_models_for(prefix, &NONE_CARD_MODELS),
            })
        })
        .collect()
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

/// **THE EMISSION: THE CARDS, RENDERED BACK TO THE CONSOLE.**
///
/// This is the file that replaces the twin. `channels.ts` re-exports it, so every consumer keeps its import
/// path and the console's "model routing" section is built from the same bytes the worker serves.
///
/// **IT RENDERS THE TABLES RATHER THAN QUOTING THEM**: a hand-written string literal for prose this long is a
/// transcription waiting to differ from [`CHANNEL_CARDS`], and the freshness test would then be checking the
/// literal against itself.
pub fn render_typescript() -> String {
    let mut out = String::from(
        "// channels-logic.ts — GENERATED. DO NOT EDIT BY HAND.\n\
         //\n\
         // Emitted by `gateway/wasm/src/registry.rs` (`render_typescript`). The freshness test in that module is a\n\
         // byte comparison against this file:\n\
         //\n\
         //     SUMMRISE_REFRESH_CHANNELS=1 cargo test --manifest-path gateway/wasm/Cargo.toml registry::emission\n\
         //\n\
         // `gateway/src/channels.ts` re-exports this module. **THE CHANNEL CARDS LIVE HERE NOW** — they were\n\
         // `channels.ts`'s table, mirrored into the worker as a generated JSON twin with a script and a freshness\n\
         // test of its own. A twin is a second source wearing a freshness test: the prose had to be edited in\n\
         // TypeScript, copied by the script, committed, and only then could the worker serve it. The table moved\n\
         // into the Rust, the cards are rendered back here, and the twin, its generator and its test are gone.\n\
         //\n\
         // **AN EDIT TO A CARD IS NOW A RUST EDIT PLUS**:\n\
         //\n\
         //     cd gateway && SUMMRISE_REFRESH_CHANNELS=1 cargo test --manifest-path wasm/Cargo.toml registry::emission\n\
         \n\
         /** The advertised models on one channel prefix, in catalogue order.  */\n\
         export const ROUTE_INFO: {\n\
         \x20 prefix: string;\n\
         \x20 backend: string;\n\
         \x20 desc: string;\n\
         \x20 models: string[];\n\
         }[] = [\n",
    );
    for (prefix, backend, desc, models) in card_rows() {
        out.push_str("  {\n");
        out.push_str(&format!("    prefix: {},\n", ts_string(prefix)));
        out.push_str(&format!("    backend: {},\n", ts_string(backend)));
        out.push_str(&format!("    desc: {},\n", ts_string(desc)));
        // **THE LIST'S LAYOUT IS PRETTIER'S, AND THAT IS A RULE RATHER THAN A STYLE**: a generated file that
        // fails `format:check` is a red CI on a tree nobody edited. Prettier keeps an array on ONE line when the
        // line fits `printWidth` (100, from gateway/.prettierrc) and wraps it one-per-line when it does not.
        let one_line = format!(
            "    models: [{}],",
            models
                .iter()
                .map(|m| ts_string(m))
                .collect::<Vec<_>>()
                .join(", ")
        );
        if one_line.chars().count() <= PRINT_WIDTH {
            out.push_str(&one_line);
            out.push('\n');
        } else {
            out.push_str("    models: [\n");
            for model in &models {
                out.push_str(&format!("      {},\n", ts_string(model)));
            }
            out.push_str("    ],\n");
        }
        out.push_str("  },\n");
    }
    out.push_str("];\n\n");

    // ── THE MODEL REGISTRY ───────────────────────────────────────────────────────────────────────
    //
    // **THE CONSOLE'S TABLE AND THE WORKER'S ARE ONE TABLE NOW.** Until this landing `channels.ts` held its own
    // 24 entries and `registry.rs` held a copy "generated once from gateway/src/channels.ts and then checked by
    // the corpus" — a snapshot, not a comparison. `model-catalogue-store.test.mjs` compared them field for field
    // from 2026-10-14; that comparison is GONE with the second copy, and this freshness test is what replaces it.
    out.push_str(
        "/** One catalogue entry. The three DISPLAY facets (`name`, `contextWindow`, `maxTokens`) and the\n\
         \x20* reasoning default may be overridden by a deployment; `wire`, `usEgress`, `search` and\n\
         \x20* `responsesOnly` change where a request GOES and stay pinned here. */\n\
         export interface ModelSpec {\n\
         \x20 id: string;\n\
         \x20 ownedBy: string;\n\
         \x20 wire?: string;\n\
         \x20 usEgress?: boolean;\n\
         \x20 search?: boolean;\n\
         \x20 responsesOnly?: boolean;\n\
         \x20 name?: string;\n\
         \x20 contextWindow?: number;\n\
         \x20 maxTokens?: number;\n\
         \x20 reasoningEffort?: \"low\" | \"medium\" | \"high\" | \"max\";\n\
         \x20 reasoningMax?: \"raw\" | \"parsed\";\n\
         \x20 probe?: boolean;\n\
         \x20 probeWhy?: string;\n\
         }\n\n\
         export const MODEL_REGISTRY: ModelSpec[] = [\n",
    );
    for spec in MODEL_REGISTRY.iter() {
        out.push_str("  {\n");
        out.push_str(&format!("    id: {},\n", ts_string(spec.id)));
        out.push_str(&format!("    ownedBy: {},\n", ts_string(spec.owned_by)));
        if let Some(wire) = spec.wire {
            out.push_str(&format!("    wire: {},\n", ts_string(wire)));
        }
        for (key, flag) in [
            ("usEgress", spec.us_egress),
            ("search", spec.search),
            ("responsesOnly", spec.responses_only),
        ] {
            if let Some(value) = flag {
                out.push_str(&format!("    {key}: {value},\n"));
            }
        }
        if let Some(effort) = spec.reasoning_effort {
            out.push_str(&format!("    reasoningEffort: {},\n", ts_string(effort)));
        }
        if let Some(max) = spec.reasoning_max {
            let spelling = match max {
                ReasoningMax::Raw => "raw",
                ReasoningMax::Parsed => "parsed",
            };
            out.push_str(&format!("    reasoningMax: {},\n", ts_string(spelling)));
        }
        if let Some(window) = spec.context_window {
            out.push_str(&format!("    contextWindow: {window},\n"));
        }
        if let Some(tokens) = spec.max_tokens {
            out.push_str(&format!("    maxTokens: {tokens},\n"));
        }
        if let Some(probe) = spec.probe {
            out.push_str(&format!("    probe: {probe},\n"));
        }
        if let Some(why) = spec.probe_why {
            out.push_str(&format!("    probeWhy: {},\n", ts_string(why)));
        }
        out.push_str("  },\n");
    }
    out.push_str(
        "];\n\
         \n\
         /** The advertised ids and their owners, in catalogue order — what `/v1/models` reports. */\n\
         export const MODELS: { id: string; owned_by: string }[] = MODEL_REGISTRY.map((m) => ({\n\
         \x20 id: m.id,\n\
         \x20 owned_by: m.ownedBy,\n\
         }));\n\
         \n\
         /** The `/api/health` cards, IN ORDER — the order is what the response bytes are. The duplicate ids are\n\
         \x20* deliberate: `buildHealth` asks the og circuit once per og card, and `recommended` takes the FIRST\n\
         \x20* card of a priority id. */\n\
         export const HEALTH_CHANNELS: { id: string; model: string }[] = [\n",
    );
    for (id, model) in HEALTH_CHANNELS.iter() {
        out.push_str(&format!(
            "  {{ id: {}, model: {} }},\n",
            ts_string(id),
            ts_string(model)
        ));
    }
    out.push_str("];\n\n");
    let wire_remap = og_wire_remap()
        .iter()
        .map(|(bare, wire)| format!("  {}: {},", ts_string(bare), ts_string(wire)))
        .collect::<Vec<_>>()
        .join("\n");
    // **ONE LINE, BECAUSE PRETTIER PUTS IT ON ONE LINE** (the list is one slug and the line fits `printWidth`).
    let search = search_capable_wire_models()
        .iter()
        .map(|wire| ts_string(wire))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "/** The default channel leads, so the \"recommended\" badge points where `auto` resolves. */\n\
         export const HEALTH_PRIORITY: string[] = [{}];\n\
         \n\
         /** The advertised models on one channel prefix, in catalogue order.\n\
         \x20*\n\
         \x20* **IT MOVED HERE WHEN THE REGISTRY DID.** It used to live in `channels.ts` because its input did —\n\
         \x20* an emitted helper closing over a table the emission did not own would have made the generated file\n\
         \x20* import back from its host. The registry is this module's table now, so the cycle is gone and the\n\
         \x20* derivation sits beside the list it walks.\n\
         \x20*\n\
         \x20* The history is worth keeping: `ROUTE_INFO[].models` used to be hand-maintained — the FIFTH copy of\n\
         \x20* the catalogue — and had silently drifted from MODELS: the `og/` list omitted\n\
         \x20* `openai/gpt-5.6-luna:floor[1m]` entirely (advertised by /v1/models, absent from the console's route\n\
         \x20* breakdown), and `og/` + `cm/` listed theirs in a different order. Nothing caught it: the only\n\
         \x20* ROUTE_INFO test checked that the PREFIXES were covered, never the lists. */\n\
         export function routeModelsFor(prefix: string, noneModels: string[] = []): string[] {{\n\
         \x20 if (prefix === \"none\") return noneModels;\n\
         \x20 return MODEL_REGISTRY.filter((m) => m.id.startsWith(prefix)).map((m) =>\n\
         \x20\x20\x20 m.id.slice(prefix.length),\n\
         \x20 );\n\
         }}\n\
         \n\
         /** One entry by ADVERTISED id. */\n\
         export function modelSpec(id: string): ModelSpec | undefined {{\n\
         \x20 return MODEL_REGISTRY.find((m) => m.id === id);\n\
         }}\n\
         \n\
         /** One entry by the name the UPSTREAM sees.\n\
         \x20*\n\
         \x20* **AMBIGUOUS NAMES RESOLVE TO NOTHING.** Stripping the channel makes two channels that advertise\n\
         \x20* the same slug indistinguishable, and guessing would apply one channel's wire remap or reasoning\n\
         \x20* mechanism to another's request. */\n\
         export function wireSpec(wireName: string): ModelSpec | undefined {{\n\
         \x20 let hit: ModelSpec | undefined;\n\
         \x20 for (const m of MODEL_REGISTRY) {{\n\
         \x20\x20\x20 const wire = m.wire ?? m.id.slice(m.id.indexOf(\"/\") + 1);\n\
         \x20\x20\x20 if (wire !== wireName) continue;\n\
         \x20\x20\x20 if (hit) return undefined; // ambiguous — refuse rather than guess\n\
         \x20\x20\x20 hit = m;\n\
         \x20 }}\n\
         \x20 return hit;\n\
         }}\n\
         \n\
         /** Does this wire name's model inject a `max` reasoning effort TEXTUALLY (passthrough routes)? */\n\
         export function reasoningMaxRawFor(wireName: string): boolean {{\n\
         \x20 return wireSpec(wireName)?.reasoningMax === \"raw\";\n\
         }}\n\
         \n\
         /** Does this wire name's model default to `max` on the PARSED object (translate routes)? */\n\
         export function reasoningMaxParsedFor(wireName: string): boolean {{\n\
         \x20 return wireSpec(wireName)?.reasoningMax === \"parsed\";\n\
         }}\n\
         \n\
         /** Is this model served ONLY by `/v1/responses`? */\n\
         export function isResponsesOnlyModel(wireName: string): boolean {{\n\
         \x20 return wireSpec(wireName)?.responsesOnly === true;\n\
         }}\n\
         \n\
         /** Advertised `og/` bare ids → the wire slug the upstream accepts. */\n\
         export const OG_WIRE_REMAP: Record<string, string> = {{\n@@WIRE_REMAP@@\n\
         }};\n\
         \n\
         /** The wire slugs that execute Anthropic `web_search` natively. */\n\
         export const SEARCH_CAPABLE_WIRE_MODELS: Set<string> = new Set([@@SEARCH@@]);\n",
        HEALTH_PRIORITY
            .iter()
            .map(|id| ts_string(id))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    // **THE SUBSTITUTION RUNS LAST**, after every push: the placeholders live in a block appended after the
    // derivation, so replacing them where the tables were BUILT left them in the output — which the formatter
    // caught (`@@WIRE_REMAP@@` is not TypeScript) rather than the freshness test, because the emission was
    // self-consistent and simply wrong.
    out = out
        .replace("@@WIRE_REMAP@@", &wire_remap)
        .replace("@@SEARCH@@", &search);
    out
}

/// The rows the renderer walks: the cards, joined to their derived model lists. **THE JOIN IS THE EMISSION'S
/// OWN SHAPE** — a card's `models` is derived, never typed, which is what makes the two sides unable to
/// disagree about a channel's list.
fn card_rows() -> Vec<(&'static str, &'static str, &'static str, Vec<String>)> {
    CHANNEL_CARDS
        .iter()
        .map(|(prefix, backend, desc)| {
            (
                *prefix,
                *backend,
                *desc,
                route_models_for(prefix, &NONE_CARD_MODELS),
            )
        })
        .collect()
}

/// `gateway/.prettierrc`'s `printWidth`, which the emission has to know: the arrays it writes are laid out by
/// Prettier's rule, not by one of mine.
const PRINT_WIDTH: usize = 100;

/// A TypeScript string literal for a Rust `&str`. The prose carries em-dashes and backticks; it does not carry
/// quotes or backslashes today, and the escape keeps that from being a requirement.
fn ts_string(raw: &str) -> String {
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/channels-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK** — the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_CHANNELS").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write channels-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_CHANNELS=1 cargo test registry::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/channels-logic.ts is not what registry.rs emits — \
             run SUMMRISE_REFRESH_CHANNELS=1 cargo test registry::emission"
        );
    }

    /// **THE CARDS ARE THE CONSOLE'S, IN THE CONSOLE'S ORDER, AND EVERY PREFIX HAS ONE.** The order matters:
    /// `none` is last because it is the DEFAULT channel's card rather than a prefix, and the console renders
    /// the list as it arrives.
    #[test]
    fn the_cards_are_the_consoles() {
        let cards = channel_cards();
        let prefixes: Vec<&str> = cards
            .iter()
            .map(|c| c["prefix"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            prefixes,
            vec!["og/", "or/", "nv/", "gmi/", "qw/", "cm/", "r4/", "none"]
        );
        // Every NON-default card's list is DERIVED, so it is exactly the registry's ids for that prefix —
        // bare, in catalogue order, and never empty (a prefix nobody uses would be a card nobody needs).
        for card in &cards {
            let prefix = card["prefix"].as_str().unwrap_or_default();
            let models = card["models"].as_array().expect("a list");
            if prefix == "none" {
                assert_eq!(models.len(), 1, "the default card carries the `auto` model");
                continue;
            }
            assert!(!models.is_empty(), "{prefix} has no models");
            for m in models {
                let bare = m.as_str().unwrap_or_default();
                assert!(
                    MODEL_REGISTRY
                        .iter()
                        .any(|spec| spec.id == format!("{prefix}{bare}")),
                    "{prefix}{bare} is not in the registry"
                );
            }
        }
    }
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
                "wire_model_name" => {
                    let got = wire_model_name(
                        input["prefix"].as_str().unwrap_or(""),
                        input["stripped"].as_str().unwrap_or(""),
                    );
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "search_target_for" => {
                    let got = search_target_for(
                        input["model"].as_str().unwrap_or(""),
                        input["upstreamModel"].as_str().unwrap_or(""),
                    );
                    let json = serde_json::json!({
                        "capable": got.capable,
                        "model": got.model,
                        "wireModel": got.wire_model,
                    });
                    assert_eq!(&json, want, "{func} / {name}");
                }
                "ox_alpha_reasoning_default" => {
                    let got = ox_alpha_reasoning_default(
                        input["routeKind"].as_str().unwrap_or(""),
                        input["upstreamModel"].as_str().unwrap_or(""),
                        input["body"].as_str().unwrap_or(""),
                    );
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
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

/// `OG_WIRE_REMAP` — DERIVED: the `og/` records that declare a wire slug, **KEYED BY THE PREFIX-STRIPPED
/// ADVERTISED ID** (`m.id.slice("og/".length)`) and not by the wire.
///
/// THAT KEY IS THE WHOLE POINT OF THE FUNCTION and I had it wrong first: `wireModelName(prefix, stripped)`
/// is handed the model name with its channel already removed, and answers the slug the upstream wants — so
/// the lookup key is `deepseek-v4.1-flash` and the VALUE is `deepseek-flash`. Keying by the wire would make
/// the map answer only for names that are already slugs, which is the case it exists to translate away.
///
/// **`Object.prototype.hasOwnProperty.call(...)` IS THE SEMANTICS, NOT A DEFENSIVE HABIT.** A name of
/// `"constructor"` finds something on the prototype of a plain object, and `wireModelName` would then
/// answer a FUNCTION where the map has no entry — the same prototype accident `keyMissingError` carries in
/// the opposite direction (there it is not guarded, and the corpus records the divergence). A table with no
/// prototype answers "not mine" for every name it does not hold.
pub fn og_wire_remap() -> Vec<(&'static str, &'static str)> {
    MODEL_REGISTRY
        .iter()
        .filter(|m| m.id.starts_with("og/"))
        .filter_map(|m| m.wire.map(|w| (&m.id["og/".len()..], w)))
        .collect()
}

/// `wireModelName(prefix, stripped)` — the upstream slug for a prefix-stripped model name.
///
/// ONLY `og/` HAS ALIASES. Every other prefix passes through unchanged, which is why the prefix test comes
/// first and why the remap is scoped to that channel.
pub fn wire_model_name(prefix: &str, stripped: &str) -> String {
    if prefix != "og" {
        return stripped.to_string();
    }
    og_wire_remap()
        .iter()
        .find(|(k, _)| *k == stripped)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| stripped.to_string())
}

/// `SEARCH_CAPABLE_WIRE_MODELS` — DERIVED: the wire slug of every record that can search natively.
pub fn search_capable_wire_models() -> Vec<&'static str> {
    MODEL_REGISTRY
        .iter()
        .filter(|m| m.search == Some(true))
        .map(wire_of)
        .collect()
}

/// What `searchTargetFor` answers — the three fields, named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTarget {
    pub capable: bool,
    pub model: String,
    pub wire_model: String,
}

/// `searchTargetFor(model, upstreamModel)` — which model serves a forced web-search request, and under
/// which WIRE name.
///
/// A caller that already names a search-capable model KEEPS it. Any other model is forced to the
/// version-less `deepseek-flash` lane, because the translate-only models (minimax/mimo/kimi/glm) fabricate
/// a query and return no `web_search_tool_result` (verified 2026-08-13).
pub fn search_target_for(model: &str, upstream_model: &str) -> SearchTarget {
    let capable = search_capable_wire_models().contains(&upstream_model);
    SearchTarget {
        capable,
        model: if capable {
            model.to_string()
        } else {
            "og/deepseek-v4.1-flash".to_string()
        },
        wire_model: if capable {
            upstream_model.to_string()
        } else {
            "deepseek-flash".to_string()
        },
    }
}

/// `oxAlphaReasoningDefault(routeKind, upstreamModel, body)` — respect a client-sent top-level `reasoning`;
/// only when absent default it to `effort: max`.
///
/// **TWO GATES, DELIBERATELY SEPARATE**: WHICH MODEL is a registry fact (the `reasoningMax: "raw"` facet),
/// and WHICH CHANNEL is a routing fact that stays a kind literal. The kind gate is redundant TODAY — one
/// record carries the raw facet and it rides this kind — and is kept rather than deleted, because removing
/// it would be a behaviour change if a second raw model ever lands on another channel. The corpus pins it
/// from both sides: a raw model on another kind is unchanged, and a non-raw model on this kind is too.
pub fn ox_alpha_reasoning_default(route_kind: &str, upstream_model: &str, body: &str) -> String {
    if reasoning_max_raw_for(upstream_model) && route_kind == "openrouter" {
        return crate::body_scan::raw_with_ox_alpha_reasoning_default(body);
    }
    body.to_string()
}
