/**
 * store/providers.ts — custom model PROVIDERS as DATA — the layer above
 * store/models.ts, and the same gap closed one level up.
 *
 * WHY THIS EXISTS. `store/models.ts` closed "adding a MODEL means editing
 * channels.ts and redeploying": a model is now a POST. It did not close the
 * layer those models sit on — the seven CHANNELS are still code (`ROUTE_INFO` in
 * channels.ts, `ROUTE_TABLE` in upstream.ts). An operator with a new
 * OpenAI-compatible endpoint (their own vLLM, a company gateway, another
 * aggregator) had no way in short of a rebuild. DSH — the harness this gateway
 * serves — closes it in `settings.yaml`:
 *
 *     providers:
 *       summrise:
 *         api: openai-completions
 *         baseURL: https://api.saisi.online
 *         apiKeyEnv: SUMMRISE_API_KEY
 *         models: [{ id: og/mimo-v2.5, contextWindow: 1000000, maxTokens: 128000, input: [text, image] }]
 *
 * This module is that, in KV, under the key `providers:custom`.
 *
 * WHERE A PROVIDER IS CONSULTED. `resolveRoute` (upstream.ts) is the only
 * reader, and the order is the contract:
 *
 *     1. ROUTE_TABLE[prefix]        — a BUILT-IN channel always wins
 *     2. providers:custom[prefix]   — a custom provider (this file)
 *     3. defaultRoute               — no prefix / genuinely unknown prefix
 *
 * Built-ins win because a record that shadowed `og/` (hand-edited KV, or written
 * before a built-in channel existed) would silently re-point every existing
 * route — and the user's key with it — at a third party. Creation-time
 * validation refuses RESERVED_PREFIXES for operator feedback; step 1 is the
 * guard that does not depend on any write path. A provider record that exists
 * but cannot be routed (unknown dialect, no baseURL) is an ERROR route, never
 * the default channel: falling through would dial a built-in upstream under a
 * different provider's key.
 *
 * WHAT IS REPORTED WHERE (the rest of the integration):
 *   /v1/models            — yes, `prefix + wire` per model, `owned_by` = label,
 *                           plus `context_window`/`max_tokens`/`name` when the
 *                           record declares them (DSH's model discovery reads
 *                           exactly those).
 *   console catalogue     — yes: `catalogue()` merges one route card per
 *                           provider (backend = label, desc names host+protocol).
 *   /api/health           — NO card, deliberately. HEALTH_CHANNELS is a
 *                           compile-time probe list and every card costs an
 *                           upstream call on a PUBLIC, unauthenticated endpoint;
 *                           adding operator-declared third parties there would
 *                           turn /api/health into an amplifier aimed wherever a
 *                           provider record points. Reachability of a custom
 *                           provider is the operator's to check.
 *
 * THE RECORD STORES WIRE NAMES. `models[].id` is what the UPSTREAM is asked for;
 * the advertised id is `prefix + id` (see advertisedModelId). A model id that
 * already carries the prefix — the full-id spelling an operator may copy from a
 * client config — is normalized on the way in, so the record has exactly ONE
 * representation and cannot advertise `my/my/llama`.
 *
 * THE KEY. `apiKeyEnv` is DSH's spelling: the NAME of a Worker env binding /
 * secret, resolved against `env` at request time. `apiKey` is the inline
 * alternative, stored in the same KV namespace that already holds the BYOK user
 * keys and the Cloudflare API token (store/devices.ts) — a secret-binding-only
 * design would make a registry whose entire point is "no rebuild" require a
 * rebuild. Exactly one of the two may be set; the inline value is NEVER returned
 * by the admin API (see publicProvider, which reuses maskKey like the model
 * handlers do).
 */
import { cget, cset, cdel, withKeyLock, type Env } from "./cache.ts";
import { decide } from "../gate.ts";
import { RESERVED_PREFIXES } from "../channels.ts";
import { loadCatalogueFile, mergeLayers } from "./file-config.ts";
import { maskKey } from "./users.ts";
// THE EMISSION (2026-10-13): the vocabulary, the prefix helpers, the record shapes and the parser. The host
// reads `parseProviderSpec` under a DECISION name so this file's own exported wrapper — the one with the URL
// parse in it — is the only `parseProviderSpec` a caller sees.
import {
  advertisedModelId,
  barePrefix,
  parseProviderSpec as parseProviderSpecDecision,
  providerBaseText,
  type ParsedBase,
  type ProviderModel,
  type ProviderParseError,
  type ProviderSpec,
} from "./providers-logic.ts";

// …AND RE-EXPORTED, so every existing import (`admin.ts`, `models-probe.ts`, `models.ts`) keeps its path. The
// names are listed rather than `export *`, because a star export of `parseProviderSpec` would collide with the
// wrapper below and TS resolves that silently in the local export's favour — which is correct and invisible.
export {
  advertisedModelId,
  barePrefix,
  providerBaseText,
  parseProviderModel,
  KNOWN_MODALITIES,
  KNOWN_UNSUPPORTED_APIS,
  MODEL_FIELDS,
  REASONING_EFFORTS,
  RESERVED_PREFIXES,
  SUPPORTED_PROVIDER_APIS,
  type ParsedBase,
  type ProviderModel,
  type ProviderParseError,
  type ProviderSpec,
} from "./providers-logic.ts";

const CUSTOM_KEY = "providers:custom";

/**
 * **EVERY DECISION IN THIS FILE THAT IS PURE IS AN EMISSION NOW (2026-10-13).**
 * `gateway/src/store/providers-logic.ts` is GENERATED from `core/summrise-core/src/provider_spec.rs`, and it
 * carries the vocabulary (`SUPPORTED_PROVIDER_APIS`, `KNOWN_UNSUPPORTED_APIS`, `RESERVED_PREFIXES`,
 * `MODEL_FIELDS`, `KNOWN_MODALITIES`, `REASONING_EFFORTS`), the two prefix helpers (`barePrefix`,
 * `advertisedModelId`), the record shapes (`ProviderModel`, `ProviderSpec`, `ProviderParseError`) and the
 * parser (`parseProviderModel`, `parseProviderSpec`). What is left HERE is the HOST: the KV reads and writes,
 * the per-isolate cache, the key resolution, the record views, and the `new URL` call the parser is handed.
 *
 * WHY AN EMISSION AND NOT A DELETION, for the fourth time: `npm test` cannot reach the decision route
 * (`helpers.mjs`'s `WASM_GATE` stub answers a fixed model list and knows nothing about `/__decision`), so a
 * deleted twin would remove the only implementation the suite can run. The emitted module is JavaScript, so the
 * suite runs the Rust's own output.
 */

/** Read a JSON array from KV, tolerating anything malformed — a registry that
 *  cannot be parsed must degrade to "no custom providers", never to a 500 that
 *  takes the whole gateway down (same discipline as store/models.ts). */
async function readList(env: Env): Promise<ProviderSpec[]> {
  const hit = cget(CUSTOM_KEY);
  if (hit !== undefined) return Array.isArray(hit) ? (hit as ProviderSpec[]) : [];
  let out: ProviderSpec[] = [];
  try {
    const raw = await env.KEYS.get(CUSTOM_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) out = parsed as ProviderSpec[];
    }
  } catch {
    // Malformed or unreadable: no custom providers. Silent to the CALLER,
    // visible to the operator — the prefix then resolves as unknown.
  }
  cset(CUSTOM_KEY, out);
  return out;
}

async function writeList(env: Env, value: unknown[]): Promise<void> {
  await env.KEYS.put(CUSTOM_KEY, JSON.stringify(value));
  cset(CUSTOM_KEY, value);
}

/** Every custom provider record. */
export async function customProviders(env: Env): Promise<ProviderSpec[]> {
  const kv = await readList(env);
  // THE FILE LAYER, merged here because this is the ONE read every consumer goes through
  // (resolveRoute, the catalogue, the admin API). The merge is a pure function so the
  // precedence — file wins — is tested with both sides supplied.
  const file = loadCatalogueFile({
    knownPrefixes: RESERVED_PREFIXES,
    // BARE reference, not `(raw) => parseProviderSpec(raw)`: `loadCatalogueFile`'s
    // shared cache now VERIFIES that every caller passes the same validators, and a
    // fresh wrapper has a fresh identity — so the wrapper was the one shape that could
    // not be checked (round-178, D14).
    parseProvider: parseProviderSpec,
  }).providers;
  return mergeLayers(file, kv, (p) => String(p.prefix ?? "").replace(/\/$/, ""));
}

/** The record for a routing prefix (bare or slashed), or null. */
export async function providerForPrefix(env: Env, prefix: string): Promise<ProviderSpec | null> {
  const bare = barePrefix(prefix);
  if (!bare) return null;
  for (const p of await readList(env)) {
    if (barePrefix(p?.prefix) === bare) return p;
  }
  return null;
}

/** Add or REPLACE a provider, keyed by prefix (the upsert the admin POST is).
 *  Serialized per key: two concurrent POSTs used to be able to read the same
 *  pre-write list and have the second clobber the first (the withKeyLock
 *  discipline store/devices.ts uses for the same reason). */
export async function putCustomProvider(env: Env, spec: ProviderSpec): Promise<ProviderSpec[]> {
  return withKeyLock(CUSTOM_KEY, async () => {
    const cur = await readList(env);
    const next = [...cur.filter((p) => barePrefix(p?.prefix) !== barePrefix(spec.prefix)), spec];
    await writeList(env, next);
    return next;
  });
}

/** Remove a provider this store owns. Returns false when the prefix is not one. */
export async function deleteCustomProvider(env: Env, prefix: string): Promise<boolean> {
  return withKeyLock(CUSTOM_KEY, async () => {
    const cur = await readList(env);
    const bare = barePrefix(prefix);
    const next = cur.filter((p) => barePrefix(p?.prefix) !== bare);
    if (next.length === cur.length) return false;
    await writeList(env, next);
    return true;
  });
}

/**
 * Every advertised custom-provider model, in record order.
 *
 * Malformed records are skipped HERE rather than at each call site: a
 * hand-edited KV blob (a `models` that is not an array, an entry without an id)
 * must not be able to break /v1/models for every client.
 */
export async function advertisedProviderModels(env: Env): Promise<AdvertisedProviderModel[]> {
  const out: AdvertisedProviderModel[] = [];
  for (const p of await readList(env)) {
    if (!p || typeof p !== "object") continue;
    const prefix = barePrefix(p.prefix);
    if (!prefix) continue;
    const models = Array.isArray(p.models) ? p.models : [];
    for (const m of models) {
      const wire = String(m?.id ?? "")
        .trim()
        .replace(/^\/+/, "");
      if (!wire) continue;
      out.push({ id: advertisedModelId(prefix, wire), wire, provider: p, model: m });
    }
  }
  return out;
}

/** The key this deployment would send for a provider: the inline value, else the
 *  named env binding/secret, else "". */
export function providerKey(env: any, p: ProviderSpec | undefined | null): string {
  if (!p) return "";
  if (p.apiKey) return String(p.apiKey);
  if (p.apiKeyEnv) return String(env?.[p.apiKeyEnv] ?? "");
  return "";
}

/** Does the provider's own record declare this WIRE model as seeing images?
 *  (DSH's `input: [text, image]`.) */
export function providerModelVision(p: ProviderSpec | undefined | null, wire: string): boolean {
  const models = Array.isArray(p?.models) ? (p?.models as ProviderModel[]) : [];
  return models.some((m) => m?.id === wire && m?.vision === true);
}

/** The admin/console view. An inline key leaves this function ONLY as
 *  maskKey(value) — the same rule adminListUsers and adminGetCfToken follow. */
/**
 * **THE SHAPE IS RUST'S NOW, AND THE RESOLUTION STAYS HERE.** `providerKey(env, p)` reads a Worker secret or an
 * inline key, so IT cannot be a decision — but everything after it can. Splitting the two is what made the rest
 * portable: this resolves, `POST /__decision` builds the public shape.
 *
 * **`decide` ANSWERS `undefined` WHEN THE GATE DID NOT ANSWER** (absent, refused, unreachable) and the local
 * function below is the fallback for that case only. A wrong answer from Rust cannot be silently corrected here,
 * which is the one failure this route must not hide.
 */
export async function publicProvider(p: ProviderSpec, env: any): Promise<PublicProviderSpec> {
  const resolved = providerKey(env, p);
  const fromRust = await decide<PublicProviderSpec>(env, "public_provider", { p, resolved });
  if (fromRust !== undefined) return fromRust;
  return publicProviderLocal(p, resolved);
}

/** The fallback shape, kept byte-identical to the source. */
function publicProviderLocal(p: ProviderSpec, resolved: string): PublicProviderSpec {
  const models = Array.isArray(p.models) ? p.models : [];
  return {
    prefix: p.prefix,
    label: p.label,
    baseURL: p.baseURL,
    api: p.api,
    models: models.map((m) => ({ ...m })),
    advertised: models.map((m) => advertisedModelId(p.prefix, String(m?.id ?? ""))),
    keyEnv: p.apiKeyEnv || "",
    keyMasked: resolved ? maskKey(resolved) : "",
    keyReady: !!resolved,
  };
}

/* ---------------- Validation ----------------
 *
 * Creation is the ONLY place an operator gets to hear about a mistake: a record
 * that is accepted here is dialled on the next request. So every field is
 * checked here — including the SSRF host check, which REUSES device-fetch's
 * guard stack (deviceHostError) instead of a second copy of the private-IP
 * rules that would drift from it.
 */

/** The admin/console view of a record — an inline key is replaced by maskKey(). */
export interface PublicProviderSpec {
  prefix: string;
  label: string;
  baseURL: string;
  api: string;
  /** As STORED (wire names), so a POST body round-trips. */
  models: ProviderModel[];
  /** What clients are advertised — `prefix + wire`, the ids /v1/models serves. */
  advertised: string[];
  /** The env NAME (never a value). "" when the key is inline. */
  keyEnv: string;
  /** maskKey() of the key this deployment would send — never the key. */
  keyMasked: string;
  /** Whether a key resolves in THIS deployment right now. */
  keyReady: boolean;
}

/** One advertised model, with the record it belongs to. The single shape every consumer reads
 *  (advertisedIds, catalogue, /v1/models, vision lookup), so the prefix+wire join exists once. */
export interface AdvertisedProviderModel {
  /** prefix + wire — the id clients send and /v1/models serves. */
  id: string;
  /** The wire name the upstream is asked for. */
  wire: string;
  provider: ProviderSpec;
  model: ProviderModel;
}

/**
 * `parseProviderSpec(body, opts)` — **THE HOST HALF**: read the base text the decision asks for, PARSE it, and
 * hand the parts over. The decisions — every refusal, in the source's order — are the emission's.
 *
 * **THE PARSE IS BEFORE THE CALL AND THAT IS NOT AN ORDERING BUG**: the source parses `baseURL` after its
 * prefix/label refusals, and so does the decision — it reads `base` only at that point in its own sequence, so a
 * body that is wrong twice is refused for the same reason the shipping console refuses it. The `catch` is the
 * source's own: an unparseable `baseURL` is not a throw here, it is a refusal the decision words.
 */
function parseBase(rawBase: string): ParsedBase | null {
  try {
    const url = new URL(rawBase);
    return {
      protocol: url.protocol,
      username: url.username,
      password: url.password,
      search: url.search,
      hash: url.hash,
      hostname: url.hostname,
      origin: url.origin,
      pathname: url.pathname,
    };
  } catch {
    return null;
  }
}

export function parseProviderSpec(
  body: any,
  opts: { keepKeyFrom?: { apiKey?: string; apiKeyEnv?: string } } = {},
): { spec?: ProviderSpec } & Partial<ProviderParseError> {
  return parseProviderSpecDecision(body, parseBase(providerBaseText(body)), opts);
}

/** Forget the per-isolate cache (tests, and any explicit resync). */
export function dropProviderCache(): void {
  cdel(CUSTOM_KEY);
}
