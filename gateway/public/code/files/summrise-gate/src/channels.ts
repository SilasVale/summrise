/**
 * Channel registry — the single source of truth for backend channels.
 * MODELS / ROUTE_INFO / HEALTH_CHANNELS / HEALTH_PRIORITY and the og endpoints
 * derive from here; adding a channel touches only this file. Extracted from
 * index.js (2026-08-12).
 *
 * Boundary (architecture review 2026-09-06): this module is the DATA registry
 * — endpoints, whitelists, display/health metadata, and the two env-derived
 * exit helpers (usProxyBase/museResponsesExit). The routing DECISION layer
 * (pickRoute/passthroughHeaders) lives in upstream.ts, one-way upstream ->
 * channels. Reviewed as correctly layered; no split warranted — the muse
 * exit policy stays beside its registry data for documentation coherence.
 */

// **THE UPSTREAM URLS AND THE ROUTING PREDICATES ARE THE EMISSION'S NOW (2026-10-14)** — `routing-logic.ts`,
// GENERATED from `gateway/wasm/src/routing.rs`, where `pick_route` decides by these very URLs. The console's
// copies existed because this is where its consumers import them from, and they are re-exported below.
// zen/go wire-slug aliases: advertised og/ names that differ from the slug
// the upstream actually accepts (applied after prefix-strip, og/ only — see
// wireModelName in upstream.ts). deepseek-v4.1-flash → deepseek-flash: zen/go
// runs the Flash line as a version-less lane (slug = family, version lives in
// the display name) and has NO deepseek-v4.1-flash slug; we advertise the
// clear name and rewrite on the wire. The raw lane name stays reachable by
// exact spelling too (it strips to a slug with no remap entry → passthrough).

/** The advertised catalogue, DERIVED from MODEL_REGISTRY (SOLID R120).
 *
 * The 22 hand-written entries that used to live here moved into the registry
 * verbatim — same ids, same order, same `owned_by` — so /v1/models is
 * byte-identical. The per-model rationale (why a model exists, what upstream
 * serves it, which region policy applies) moved with them; see MODEL_REGISTRY.
 *
 * Adding a model is now ONE record there instead of choosing among six
 * tables, and `model-registry.test.mjs` fails the build if a facet is
 * forgotten. */
// **THE CHANNEL CARDS ARE AN EMISSION NOW (2026-10-14).** They were this file's table, mirrored into the worker
// as a generated JSON twin (`wasm/route-info.json`) by a script, with a freshness test of its own. **A twin is a
// second source wearing a freshness test**: editing a description meant editing TypeScript, running the script,
// committing the twin, and only then could the worker serve it — and `/api/admin/public` was the only reason the
// worker needed them at all. The table is `gateway/wasm/src/registry.rs`'s `CHANNEL_CARDS` now; `channels-logic.ts`
// renders it back, and the twin, its generator and its test are deleted.
//
// AN EDIT TO A CARD IS A RUST EDIT, THEN:
//   cd gateway && SUMMRISE_REFRESH_CHANNELS=1 cargo test --manifest-path wasm/Cargo.toml registry::emission
// **FOUR MORE TABLES ARE THE EMISSION'S NOW (2026-10-14).** `MODEL_REGISTRY` (24 entries), `ModelSpec`, `MODELS`
// and the `/api/health` cards were this file's, and `registry.rs` held copies — the registry compared field for
// field by a test, the health cards mirrored "verbatim" and compared by NOTHING but a fixture snapshot. Both are
// one table now, rendered back here by the same emission that carries the channel cards.
//
// The helpers BELOW still live in this file: `modelSpec`, `wireSpec`, the two reasoning lookups,
// `isResponsesOnlyModel`, `OG_WIRE_REMAP`, `SEARCH_CAPABLE_WIRE_MODELS`, the retired-model table, the US-proxy
// predicates and `routeModelsFor`. They read the emitted registry, and moving them is the next landing.
export {
  HEALTH_CHANNELS,
  isResponsesOnlyModel,
  modelSpec,
  OG_WIRE_REMAP,
  reasoningMaxParsedFor,
  reasoningMaxRawFor,
  SEARCH_CAPABLE_WIRE_MODELS,
  wireSpec,
  HEALTH_PRIORITY,
  MODEL_REGISTRY,
  MODELS,
  ROUTE_INFO,
  routeModelsFor,
  type ModelSpec,
} from "./channels-logic.ts";
// The reserved prefixes are the PROVIDER vocabulary's (`provider_spec.rs`, emitted as `providers-logic.ts`) and
// this file is where every consumer imports them from — a re-export that survived the migration of the four
// tables above.
export {
  AMD_ANTHROPIC,
  AMD_CHAT,
  CMD_CHAT,
  museResponsesExit,
  OG_FORCE_US_PROXY,
  OG_NATIVE_ANTHROPIC,
  OG_ZEN_ANTHROPIC,
  OG_ZEN_CHAT,
  QWEN_COMPAT_CHAT,
  R4_ANTHROPIC,
  R4_CHAT,
  RETIRED_MODELS,
  retiredModelHint,
  usProxyBase,
  VERIFY_PATH,
} from "./routing-logic.ts";
export { RESERVED_PREFIXES } from "./store/providers-logic.ts";

// The default channel leads: `auto`/no-prefix now resolve to Command Code
// V4.1 (model-route.ts / upstream.ts defaultRoute), so the console's
// "recommended" badge points at the same place the gateway defaults to.
