/**
 * store/models.ts — the model catalogue as DATA, not compiled code.
 *
 * WHY THIS EXISTS. Until now the catalogue was `MODEL_REGISTRY` in
 * `src/channels.ts`: a TypeScript constant, so adding or retiring a model meant
 * editing source, rebuilding and redeploying the worker. DSH — the harness this
 * gateway serves — does it by editing `~/.dsh/settings.yaml` and restarting: a
 * CONFIG FILE, no rebuild. That is the real difference, and it is the one worth
 * closing. (DSH has no "add model" UI either; it has no runtime catalogue API at
 * all. The advantage is data-vs-code, not a screen.)
 *
 * TWO THINGS ARE STORED HERE, both in the KEYS namespace:
 *
 *   models:custom    ModelSpec[]  — models added from the console
 *   models:disabled  string[]     — BUILT-IN ids switched off from the console
 *
 * WHY DISABLING IS SEPARATE FROM DELETING. A built-in record carries six facets
 * (advertised id, wire slug, US-egress policy, web-search capability, health
 * card, vision) that no form should have to re-derive — and a record DELETED
 * from KV could not be brought back. Disabling keeps the record and its facets
 * and simply stops advertising and routing it, which is what "retire this model"
 * actually means. Deleting is for models this store owns.
 *
 * WHAT A CUSTOM MODEL INHERITS. A new record names its CHANNEL by prefix, and
 * inherits `ownedBy` from it. `wire` stays unset unless the admin supplies one,
 * because a wire slug on a non-`og/` prefix is silently ignored by
 * `wireModelName` — a trap `channels.ts` pins in a comment. Everything else
 * defaults to the conservative value.
 */
import { cget, cset, cdel, type Env } from "./cache.ts";
import { MODEL_REGISTRY, RESERVED_PREFIXES, ROUTE_INFO, type ModelSpec } from "../channels.ts";
import { loadCatalogueFile, mergeLayers } from "./file-config.ts";
import {
  advertisedProviderModels,
  barePrefix,
  customProviders,
  parseProviderSpec,
  providerForPrefix,
} from "./providers.ts";
// THE EMISSION (2026-10-14): the three key names, the advertised-id merge, the /v1/models extras, the sort, the
// two upserts, the override rule, the reasoning chain and the catalogue projection. The host reads the pure ones
// under a suffix so its own async wrappers keep the names every caller already imports.
import {
  CUSTOM_MODELS_KEY,
  DISABLED_MODELS_KEY,
  MODEL_OVERRIDES_KEY,
  advertisedIds as advertisedIdsOf,
  catalogueRoutes,
  declaredFromCustom,
  declaredFromOverride,
  declaredFromProvider,
  extraModelEntries as extraModelEntriesOf,
  isBuiltIn as isBuiltInOf,
  jsSorted,
  putOverride,
  removeById,
  upsertById,
} from "./models-logic.ts";

// …AND RE-EXPORTED, so a caller that wants the RULES (a test, the admin route) can read them without going
// through a KV read. The names are listed rather than `export *`: two of them collide with this file's own
// async wrappers, and a star export would resolve that silently.
export {
  CUSTOM_MODELS_KEY,
  DISABLED_MODELS_KEY,
  MODEL_OVERRIDES_KEY,
  catalogueRoutes,
  declaredFromCustom,
  declaredFromOverride,
  declaredFromProvider,
  jsSorted,
  putOverride,
  removeById,
  upsertById,
} from "./models-logic.ts";

/** The registry's ids, hoisted: the emitted `isBuiltIn` takes the TABLE as an argument, because the table's
 *  home is `channels.ts` and the RULE (membership) is the emission's. */
const REGISTRY_IDS = MODEL_REGISTRY.map((m) => m.id);

/** Read a JSON array from KV, tolerating anything malformed — a catalogue that
 *  cannot be parsed must degrade to "nothing extra", never to a 500 that takes
 *  the whole gateway down. */
async function readList<T>(env: Env, key: string): Promise<T[]> {
  const hit = cget(key);
  if (hit !== undefined) return Array.isArray(hit) ? (hit as T[]) : [];
  let out: T[] = [];
  try {
    const raw = await env.KEYS.get(key);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) out = parsed as T[];
    }
  } catch {
    // Malformed or unreadable: no custom entries. Deliberately silent to the
    // CALLER, but not to the operator — the console shows what it can serve.
  }
  cset(key, out);
  return out;
}

async function writeList(env: Env, key: string, value: unknown[]): Promise<void> {
  await env.KEYS.put(key, JSON.stringify(value));
  cset(key, value);
}

/** Built-in ids switched off from the console. */
export async function disabledModels(env: Env): Promise<Set<string>> {
  return new Set(await readList<string>(env, DISABLED_MODELS_KEY));
}

/** Models added from the console. */
export async function customModels(env: Env): Promise<ModelSpec[]> {
  const kv = await readList<ModelSpec>(env, CUSTOM_MODELS_KEY);
  const file = loadCatalogueFile({
    knownPrefixes: RESERVED_PREFIXES,
    parseProvider: parseProviderSpec,
  }).models;
  return mergeLayers(file, kv, (m) => m.id);
}

/** Every id the gateway currently advertises — built-ins minus disabled, plus
 *  console-added models, plus every CUSTOM PROVIDER's models (a provider's
 *  models are advertised by its record; `isAdvertised` gates setting a route to
 *  one exactly as it does for any other id). */
export async function advertisedIds(env: Env): Promise<string[]> {
  const off = await disabledModels(env);
  const custom = await customModels(env);
  const provided = await advertisedProviderModels(env);
  // The ORDER — built-ins minus disabled, then console-added, then the providers' — is the emission's.
  return advertisedIdsOf(
    REGISTRY_IDS,
    [...off],
    custom.map((m) => m.id),
    provided.map((m) => m.id),
  );
}

/**
 * The NON-BUILT-IN `/v1/models` entries — console-added models and custom
 * provider models — with the facets a provider model may declare. ONE merge
 * point, so the model list a client reads and the catalogue the console renders
 * cannot drift apart (the failure mode ROUTE_INFO's own header records).
 *
 * `context_window`/`max_tokens` are emitted for provider models that declare
 * them because a client can USE them: DSH's model discovery reads exactly those
 * keys (`contextWindow`/`context_window`, `maxTokens`/`max_tokens`) off a
 * `GET {baseURL}/models` listing. Built-in entries are unchanged.
 */
export async function extraModelEntries(env: Env): Promise<
  {
    id: string;
    owned_by: string;
    name?: string;
    context_window?: number;
    max_tokens?: number;
  }[]
> {
  return extraModelEntriesOf(await customModels(env), await advertisedProviderModels(env));
}

/**
 * FORM-OWNED DISPLAY OVERRIDES FOR A BUILT-IN MODEL.
 *
 * A built-in record carries six ROUTING facets pinned in `channels.ts` — and none of the
 * display ones, because a compile-time registry has no reason to hold a human name. This
 * is the layer that lets an operator add what a client can USE (a name, a context window,
 * a max-output, a reasoning default) WITHOUT moving anything that changes where a request
 * goes: a stored record cannot contradict the registry on `wire` or `usEgress`, because
 * the admin route refuses those fields for a built-in id outright.
 *
 * Keyed by advertised id, stored as a list so it reuses the same read/write helpers (and
 * their cache invalidation) as every other store here.
 */
export interface FacetOverride {
  id: string;
  name?: string;
  contextWindow?: number;
  maxTokens?: number;
  reasoningEffort?: "low" | "medium" | "high" | "max";
}

export async function facetOverrides(env: Env): Promise<FacetOverride[]> {
  const kv = await readList<FacetOverride>(env, MODEL_OVERRIDES_KEY);
  const file = loadCatalogueFile({
    knownPrefixes: RESERVED_PREFIXES,
    parseProvider: parseProviderSpec,
  }).overrides;
  return mergeLayers(file, kv, (o) => o.id);
}

/** Add or replace one built-in's override, dropping fields the caller emptied. */
export async function putFacetOverride(env: Env, o: FacetOverride): Promise<FacetOverride[]> {
  const cur = await facetOverrides(env);
  // EMPTYING EVERY FIELD REMOVES IT — the rule, and the reason, are the emission's.
  const next = putOverride(cur, o);
  await writeList(env, MODEL_OVERRIDES_KEY, next);
  return next;
}

/** Is this id advertised right now? The gate for BOTH setting a route and using one. */
export async function isAdvertised(env: Env, id: string): Promise<boolean> {
  return (await advertisedIds(env)).includes(id);
}

/** The card's `host`: `new URL(baseURL).host`, or the RAW string when that throws — a hand-edited record must
 *  still show a card rather than dropping out of the console. */
function cardHost(baseURL: unknown): string {
  const raw = String(baseURL || "");
  try {
    return new URL(raw).host;
  } catch {
    return raw;
  }
}

/**
 * The catalogue as the console and `/api/admin/public` need it: the static routes
 * with their per-channel model lists filtered, plus one synthetic entry per
 * prefix that only custom models use.
 */
export async function catalogue(env: Env): Promise<{
  models: string[];
  routes: { prefix: string; backend: string; desc: string; models: string[] }[];
}> {
  const custom = await customModels(env);
  const ids = await advertisedIds(env);
  const providers = await customProviders(env);
  const provided = await advertisedProviderModels(env);

  /**
   * **THE PROJECTION IS THE EMISSION'S** — the bare-vs-prefixed join, the synthetic cards for custom models, and
   * the provider cards. The HOST does the two things only it can: flattening the advertised provider models to
   * `{wire, provider}`, and parsing each record's `baseURL` for the card's `host`.
   *
   * **THE BARE-vs-PREFIXED TRAP IS WHY THIS IS ONE FUNCTION** (`catalogueRoutes`): `ROUTE_INFO[].models` holds
   * BARE names while the disabled set holds FULL advertised ids, and comparing them directly matches nothing —
   * the mismatch that made this page set the wrong channel (round 58) and left its default card permanently
   * empty (round 65).
   */
  const routes = catalogueRoutes(
    ROUTE_INFO,
    ids,
    custom,
    provided.map((m) => ({ wire: m.wire, provider: barePrefix(m.provider?.prefix) })),
    providers
      .filter((p) => !!p && typeof p === "object" && !!barePrefix(p.prefix))
      .map((p) => ({
        prefix: barePrefix(p.prefix),
        label: String(p.label ?? ""),
        baseURL: String(p.baseURL ?? ""),
        api: String(p.api ?? ""),
        host: cardHost(p.baseURL),
      })),
  );
  return { models: ids, routes };
}

/** Switch a BUILT-IN model off (or back on). Custom models are deleted, not disabled. */
export async function setModelDisabled(env: Env, id: string, disabled: boolean): Promise<string[]> {
  const cur = await disabledModels(env);
  if (disabled) cur.add(id);
  else cur.delete(id);
  const next = jsSorted([...cur]);
  await writeList(env, DISABLED_MODELS_KEY, next);
  return next;
}

/** Add (or replace) a model the console owns. */
export async function putCustomModel(env: Env, spec: ModelSpec): Promise<ModelSpec[]> {
  const cur = await customModels(env);
  const next = upsertById(cur, spec);
  await writeList(env, CUSTOM_MODELS_KEY, next);
  return next;
}

/** Remove a model this store owns. Returns false when the id is not custom. */
export async function deleteCustomModel(env: Env, id: string): Promise<boolean> {
  const cur = await customModels(env);
  const { next, removed } = removeById(cur, id);
  if (!removed) return false;
  await writeList(env, CUSTOM_MODELS_KEY, next);
  return true;
}

/** Where a model comes from — the console needs this to offer the right action. */
export function isBuiltIn(id: string): boolean {
  return isBuiltInOf(REGISTRY_IDS, id);
}

/** Forget the per-isolate cache (tests, and any explicit resync). */
export function dropModelCache(): void {
  cdel(CUSTOM_MODELS_KEY, DISABLED_MODELS_KEY);
}

/**
 * The reasoning default a model DECLARES, looked up BY RECORD.
 *
 * Two stores can own a model's declaration: a console-added record on a built-in
 * channel, or a model inside a custom provider's record. Both are form-owned, so both
 * are consulted here rather than through `wireSpec` (which is the built-in registry's
 * by-wire lookup and cannot see either).
 *
 * Returns null when nothing declares one — the caller then falls back to the registry's
 * built-in behaviour, which is what keeps every existing model unchanged.
 */
export async function declaredReasoningEffort(env: Env, id: string): Promise<string | null> {
  // **THE ORDER IS THIS CALL SITE'S AND THE THREE TESTS ARE THE EMISSION'S** — the source short-circuits, so
  // passing all three results in would read KV three times where it reads once.
  const fromCustom = declaredFromCustom(await customModels(env), id);
  if (fromCustom) return fromCustom;
  const slash = id.indexOf("/");
  if (slash < 0) return null;
  const provider = await providerForPrefix(env, id.slice(0, slash + 1));
  const fromProvider = declaredFromProvider(provider, id);
  if (fromProvider) return fromProvider;
  // LAST, because an override DECORATES a built-in: it may set the reasoning default the
  // registry never had, and it cannot contradict the registry about anything else.
  return declaredFromOverride(await facetOverrides(env), id);
}
