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

// OpenCode Zen/Go endpoints. All og/ models (including deepseek-v4-flash)
// route through /v1/chat/completions (OpenAI format) — zen natively supports
// OpenAI format for all models. No Anthropic passthrough needed.
export const VERIFY_PATH: string = "/v1/messages";
export const OG_ZEN_ANTHROPIC: string = "https://opencode.ai/zen/go" + VERIFY_PATH;
export const OG_ZEN_CHAT: string = "https://opencode.ai/zen/go/v1/chat/completions";
// Command Code Provider API (api.commandcode.ai/provider). The Anthropic
// /v1/messages endpoint serves claude-* models ONLY ("Use
// /provider/v1/chat/completions for OpenAI and OSS models" — verified against
// the live API); cm/ models therefore ride the OpenAI endpoint, and
// /v1/messages is Anthropic→OpenAI translated (the og pattern).
export const CMD_CHAT: string = "https://api.commandcode.ai/provider/v1/chat/completions";
// Qwen MaaS (Aliyun Token Plan) OpenAI-compatible endpoint — the Anthropic
// /apps/anthropic/v1/messages endpoint rejects OpenAI-format bodies (400
// "Request body format invalid"), so chat/completions requests (DSH & co.)
// must ride the compatible-mode endpoint instead.
export const QWEN_COMPAT_CHAT: string =
  "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions";
// AMD Radeon Cloud (developer.amd.com.cn/radeon) — self-deploy inference pool
// ("Dynamic sglang/vllm-router service managed by Model Ops"), free tier with
// a per-model global concurrency cap. BOTH formats are native: /api/v1/messages
// really speaks Anthropic (thinking blocks + tool_use + SSE, verified against
// the live API 2026-09-02) and /api/v1/chat/completions speaks OpenAI — no
// translation needed on either path.
export const AMD_ANTHROPIC: string = "https://developer.amd.com.cn/radeon/api" + VERIFY_PATH;
export const AMD_CHAT: string = "https://developer.amd.com.cn/radeon/api/v1/chat/completions";
// r4.codes — a coding-agent gateway serving its whole catalog on ONE host under
// BOTH protocols natively, like AMD (verified against the live API 2026-09-20):
// /v1/messages returns a real Anthropic message (thinking blocks included) and
// accepts x-api-key OR Bearer, and /v1/chat/completions returns a normal
// chat.completion. No translation on either path, so the route is picked by
// requestPath — see r4Route in upstream.ts.
export const R4_ANTHROPIC: string = "https://api.r4.codes" + VERIFY_PATH;
export const R4_CHAT: string = "https://api.r4.codes/v1/chat/completions";
// Reserved for future use — currently empty. Models listed here would bypass
// the OpenAI translate path and use native Anthropic /v1/messages passthrough.
export const OG_NATIVE_ANTHROPIC: Set<string> = new Set();

// zen/go wire-slug aliases: advertised og/ names that differ from the slug
// the upstream actually accepts (applied after prefix-strip, og/ only — see
// wireModelName in upstream.ts). deepseek-v4.1-flash → deepseek-flash: zen/go
// runs the Flash line as a version-less lane (slug = family, version lives in
// the display name) and has NO deepseek-v4.1-flash slug; we advertise the
// clear name and rewrite on the wire. The raw lane name stays reachable by
// exact spelling too (it strips to a slug with no remap entry → passthrough).

/** A record by advertised id. */
export function modelSpec(id: string): ModelSpec | undefined {
  return MODEL_REGISTRY.find((m) => m.id === id);
}

/** A record by the name the UPSTREAM sees (prefix stripped, wire applied).
 *
 * This is the key the translate path holds — it has already resolved the
 * route, so the advertised prefix is gone by then. `responsesOnly` and
 * `reasoningMax` are consulted this way.
 *
 * AMBIGUOUS NAMES RESOLVE TO NOTHING. Stripping the channel makes two
 * channels that advertise the same upstream slug collide:
 * `og/openai/gpt-5.6-luna:floor[1m]` and `or/openai/gpt-5.6-luna:floor[1m]`
 * share the wire name `openai/gpt-5.6-luna:floor[1m]`. Picking either would
 * let one channel's model INHERIT the other's facet — silently wrong the
 * moment a facet is declared, which is exactly the class of bug this
 * registry exists to remove. So: exactly one match, or none.
 *
 * `model-registry.test.mjs` asserts that every model whose facets are
 * consulted BY WIRE NAME (responsesOnly, reasoningMax) has a unique wire
 * name, so the refusal can never silently disable a live rule. Facets
 * consulted by ADVERTISED id (usEgress via OG_FORCE_US_PROXY) are unaffected
 * by the collision. */
export function wireSpec(wireName: string): ModelSpec | undefined {
  let hit: ModelSpec | undefined;
  for (const m of MODEL_REGISTRY) {
    const wire = m.wire ?? m.id.slice(m.id.indexOf("/") + 1);
    if (wire !== wireName) continue;
    if (hit) return undefined; // ambiguous — refuse rather than guess
    hit = m;
  }
  return hit;
}

/** Does this model default its reasoning effort to `max`, on the RAW-BODY
 *  (passthrough) path? Scoped by mechanism — see ModelSpec.reasoningMax. */
export function reasoningMaxRawFor(wireName: string): boolean {
  return wireSpec(wireName)?.reasoningMax === "raw";
}

/** Same, for the PARSED-object (translate) path. */
export function reasoningMaxParsedFor(wireName: string): boolean {
  return wireSpec(wireName)?.reasoningMax === "parsed";
}

/** Is this model served ONLY by /v1/responses? (muse-spark Contributor.)
 *  Anything NOT in the registry is refused too — an unadvertised id must not
 *  reach the upstream through a route the catalogue does not describe. */
export function isResponsesOnlyModel(wireName: string): boolean {
  return wireSpec(wireName)?.responsesOnly === true;
}

export const OG_WIRE_REMAP: Record<string, string> = Object.fromEntries(
  MODEL_REGISTRY.filter((m) => m.wire && m.id.startsWith("og/")).map((m) => [
    m.id.slice("og/".length),
    m.wire as string,
  ]),
);

// zen/go server-side web_search capability, keyed by WIRE slug (i.e. after
// OG_WIRE_REMAP — the search-model swap in plugins/translate.ts checks the wire
// name). "deepseek-flash" is the version-less lane = V4.1, the remap target of
// og/deepseek-v4.1-flash: it executes Anthropic's web_search_20250305 natively
// and answers with server_tool_use + web_search_tool_result (live-verified
// 2026-09-10 against zen/go/v1/messages with a forced
// tool_choice:{type:"tool",name:"web_search"}: HTTP 200 + 4 searches). The V4
// slug `deepseek-v4-flash` was search-capable too but retired with the V4 line
// on 2026-09-10 (RETIRED_MODELS) and is no longer a swap target.
// Every other og/ model on the translate path (minimax-m3, mimo-v2.5, kimi,
// glm) fabricates a query and returns NO web_search_tool_result, so a search
// request naming one of those still falls back to the lane slug.
export const SEARCH_CAPABLE_WIRE_MODELS: Set<string> = new Set(
  MODEL_REGISTRY.filter((m) => m.search).map((m) => m.wire ?? m.id.slice(m.id.indexOf("/") + 1)),
);

// Retired models — the whole DeepSeek V4 Flash line, taken down 2026-09-10 in
// favour of V4.1 Flash. DeepSeek official retired V4 first (its docs: the old
// names `deepseek-v4-flash` / `deepseek-v4-flash-vision-exp` still answer but
// are served BY V4.1), AMD's pool never got a V4.1, and every other channel we
// advertise serves V4.1. Requests naming a retired id (or its bare upstream
// wire spelling) are REJECTED with the replacement in the message instead of
// silently riding an alias: a silent alias hides which model actually
// answered and lets dead names live in client configs forever.
// Keys are lowercase — the lookup lowercases + trims the requested model, so
// AMD's case-sensitive catalogue ids are covered by one entry.
export const RETIRED_MODELS: Record<string, string> = {
  // Advertised ids that left the catalog.
  "ds/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "og/deepseek-v4-flash": "og/deepseek-v4.1-flash",
  "cm/deepseek/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "cm/deepseek/deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  "or/deepseek/deepseek-v4-flash-0731": "cm/deepseek/deepseek-v4.1-flash",
  "amd/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "amd/deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  // Bare upstream spellings (unprefixed names and wire names), for clients
  // that never used a prefix.
  "deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek-v4-flash-0731": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek/deepseek-v4-flash": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek/deepseek-v4-flash-vision-exp": "cm/deepseek/deepseek-v4.1-flash",
  "deepseek/deepseek-v4-flash-0731": "cm/deepseek/deepseek-v4.1-flash",
};

/** Replacement id for a retired model, or null when the model is not retired.
 *  Lookup is case-insensitive on the trimmed name (AMD ids are mixed-case). */
export function retiredModelHint(model: string): string | null {
  const key = String(model || "")
    .trim()
    .toLowerCase();
  return Object.prototype.hasOwnProperty.call(RETIRED_MODELS, key)
    ? (RETIRED_MODELS[key] as string)
    : null;
}

// Model-level forced US egress. These og/ models are region-blocked when zen
// is reached directly from CN clients, so requests ALWAYS ride a US exit
// regardless of the global US_PROXY switch:
//   - og/gpt-5.6-luna — zen region-blocks it for CN (translate.ts remaps the
//     og/ spellings to the or/ route via OpenRouter's US exit).
//   - og/muse-spark-1.2/1.3-contributor — Meta Geographic Use Policy blocks
//     the contributor tier outside permitted regions; the US exit clears the
//     RegionError (verified 2026-09-04: direct = 403 RegionError, via US exit
//     = 200).
export const OG_FORCE_US_PROXY: Set<string> = new Set(
  MODEL_REGISTRY.filter((m) => m.usEgress).map((m) => m.id),
);

export function usProxyBase(env: any): string {
  return env?.US_PROXY_BASE || "https://v.saisi.online";
}

// US exit for og/muse-spark-* via POST /v1/responses (translate.ts). The
// muse Contributor tier is responses-only upstream AND Meta region-blocks it
// for CN, so it is FORCED through a US exit. The default exit is the Oracle
// Cloud relay: oracle.saisi.online (grey-cloud DNS + Let's Encrypt cert) → an
// Always-Free ARM VM in US West (Phoenix) whose nginx forwards /v1/responses
// to opencode zen (US egress clears the Meta RegionError — verified live
// 2026-09-08: full chain gateway→oracle→zen returns completed responses).
// The previous default, the Vercel relay (v.saisi.online/api/zen), is now
// UNAVAILABLE: the free team exceeded the 10 GB Fast Origin Transfer cap
// (304%) and the account was paused (402 DEPLOYMENT_DISABLED, 2026-09-07).
// MUSE_RESPONSES_EXIT selects the exit:
//   - "vercel"        → legacy NAME: routes muse through the generic zen
//                       relay ({US_PROXY_BASE}/api/zen?target=og&path=…).
//                       Since the 2026-09-08 Vercel retirement that endpoint
//                       is served by the SAME Oracle box (vrelay) — a
//                       distinct code path (BYOK gate + path allowlist),
//                       not a distinct machine.
//   - "zen-us"        → the zen-us Cloudflare worker (zen-us.saisi.online/
//                       v1/responses). US-pinned via WNAM D1 + placement,
//                       but the CF egress still hits 403 RegionError live
//                       (verified 2026-09-08) — experimental, do not set.
//   - a http(s) URL   → used verbatim (any US exit speaking the same
//                       BYOK /v1/responses contract)
//   - unset / other   → the Oracle relay default (oracle.saisi.online)
export function museResponsesExit(env: any): string {
  const v = env?.MUSE_RESPONSES_EXIT;
  if (v === "vercel")
    return `${usProxyBase(env)}/api/zen?target=og&path=${encodeURIComponent("/v1/responses")}`;
  if (v === "zen-us") return "https://zen-us.saisi.online/v1/responses";
  if (typeof v === "string" && /^https?:\/\//.test(v)) return v;
  return "https://oracle.saisi.online/v1/responses";
}

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
  HEALTH_PRIORITY,
  MODEL_REGISTRY,
  MODELS,
  ROUTE_INFO,
  type ModelSpec,
} from "./channels-logic.ts";
// The reserved prefixes are the PROVIDER vocabulary's (`provider_spec.rs`, emitted as `providers-logic.ts`) and
// this file is where every consumer imports them from — a re-export that survived the migration of the four
// tables above.
export { RESERVED_PREFIXES } from "./store/providers-logic.ts";

/** The advertised models on one channel prefix, in catalogue order.
 *
 * **IT LIVES HERE BECAUSE ITS INPUT DOES** — `MODEL_REGISTRY` is now the EMISSION's, but this helper is the
 * console's own derivation and `channels-logic.ts` bakes the Rust's answer into each card. The two are compared
 * card by card (`CARDS: the emitted ROUTE_INFO is routeModelsFor's answer`), which is what keeps one derivation
 * with two spellings from drifting apart quietly.
 *
 * The history is worth keeping: `ROUTE_INFO[].models` used to be hand-maintained — the FIFTH copy of the
 * catalogue — and had silently drifted from MODELS: the `og/` list omitted `openai/gpt-5.6-luna:floor[1m]`
 * entirely (advertised by /v1/models, absent from the console's route breakdown), and `og/` + `cm/` listed
 * theirs in a different order. Nothing caught it: the only ROUTE_INFO test checked that the PREFIXES were
 * covered, never the lists. */
export function routeModelsFor(prefix: string, noneModels: string[] = []): string[] {
  if (prefix === "none") return noneModels;
  return MODEL_REGISTRY.filter((m) => m.id.startsWith(prefix)).map((m) =>
    m.id.slice(prefix.length),
  );
}

import { MODEL_REGISTRY, type ModelSpec } from "./channels-logic.ts";

// The default channel leads: `auto`/no-prefix now resolve to Command Code
// V4.1 (model-route.ts / upstream.ts defaultRoute), so the console's
// "recommended" badge points at the same place the gateway defaults to.
