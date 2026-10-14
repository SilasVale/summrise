/**
 * models-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/model_store.rs` (`render_typescript`), which is now the single source for the
 * rules below. The freshness test in that module is a byte comparison against this file, so a hand edit fails
 * there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_MODELS=1 cargo test --manifest-path gateway/wasm/Cargo.toml model_store::emission
 *
 * `gateway/src/store/models.ts` re-exports this module and keeps the HOST: the three KV reads and writes, the
 * per-isolate cache, the file-layer merge and the writes' write-through. **THE RULES ARE HERE** — the three key
 * names, the advertised-id merge, the /v1/models extras, the disabled set's sort, the two upserts, the
 * facet-override rule, the reasoning-effort chain and the CATALOGUE PROJECTION.
 *
 * **AND THE PROJECTION IS THE ONE THAT HAS COST THIS REPOSITORY TWICE**: `ROUTE_INFO` holds BARE names while the
 * advertised set holds FULL ids, and comparing them directly matches NOTHING — the mismatch that made the
 * console's Models page set the wrong channel (round 58) and left its default card permanently empty (round 65).
 *
 * `barePrefix` and `SUPPORTED_PROVIDER_APIS` are IMPORTED from the provider emission rather than restated: one
 * rule, one owner, and the provider store already publishes both.
 */

import { SUPPORTED_PROVIDER_APIS, barePrefix } from "./providers-logic.ts";

/** `models:custom` — the console's own model records. */
export const CUSTOM_MODELS_KEY = "models:custom";

/** `models:overrides` — display-only decoration for a built-in. */
export const MODEL_OVERRIDES_KEY = "models:overrides";

/** `models:disabled` — built-in ids the console switched off. */
export const DISABLED_MODELS_KEY = "models:disabled";

/** `[...ids].sort()` — **JAVASCRIPT'S DEFAULT SORT COMPARES UTF-16 CODE UNITS**, and a model id is ASCII today.
 *  The comparison is written the engine's way anyway: a sort that agrees only on today's data is a sort that
 *  changes behaviour on tomorrow's. */
export function jsSorted(ids: string[]): string[] {
  return [...ids].sort((a, b) => {
    const au = a.split("").map((c) => c.charCodeAt(0));
    const bu = b.split("").map((c) => c.charCodeAt(0));
    for (let i = 0; i < Math.min(au.length, bu.length); i++) {
      if (au[i] !== bu[i]) return au[i]! - bu[i]!;
    }
    return au.length - bu.length;
  });
}

/** The advertised ids: built-ins minus the disabled ones, then the console-added, then the providers'. **THE
 *  ORDER IS THE LISTING'S**, which is why this is a function rather than three spreads at a call site. */
export function advertisedIds(
  registryIds: string[],
  disabled: string[],
  customIds: string[],
  providerIds: string[],
): string[] {
  return [...registryIds.filter((id) => !disabled.includes(id)), ...customIds, ...providerIds];
}

/** The NON-BUILT-IN `/v1/models` entries — console-added models and custom provider models — with the facets a
 *  provider model may declare. ONE merge point, so the list a client reads and the catalogue the console renders
 *  cannot drift apart. */
export function extraModelEntries(
  custom: any[],
  provided: any[],
): {
  id: string;
  owned_by: string;
  name?: string;
  context_window?: number;
  max_tokens?: number;
}[] {
  const out: {
    id: string;
    owned_by: string;
    name?: string;
    context_window?: number;
    max_tokens?: number;
  }[] = [];
  for (const m of custom) {
    const e: any = { id: m.id, owned_by: m.ownedBy };
    if (m.name) e.name = m.name;
    if (m.contextWindow) e.context_window = m.contextWindow;
    if (m.maxTokens) e.max_tokens = m.maxTokens;
    out.push(e);
  }
  for (const entry of provided) {
    const label = entry.provider?.label || barePrefix(entry.provider?.prefix);
    const e: any = { id: entry.id, owned_by: label };
    if (entry.model?.name) e.name = entry.model.name;
    if (entry.model?.contextWindow) e.context_window = entry.model.contextWindow;
    if (entry.model?.maxTokens) e.max_tokens = entry.model.maxTokens;
    out.push(e);
  }
  return out;
}

/** `isBuiltIn(id)` — membership in the registry, with the TABLE passed in, because the registry's data is the
 *  console's own (`channels.ts`) and the rule is this module's. */
export function isBuiltIn(registryIds: string[], id: string): boolean {
  return registryIds.includes(id);
}

/** `putCustomModel`'s upsert: the record with this id is dropped and the new one APPENDED — so an update MOVES
 *  it to the end of the list rather than replacing it in place. */
export function upsertById<T extends { id: string }>(list: T[], spec: T): T[] {
  return [...list.filter((m) => m.id !== spec.id), spec];
}

/** `deleteCustomModel`: the list without that id, and whether anything left. */
export function removeById<T extends { id: string }>(
  list: T[],
  id: string,
): { next: T[]; removed: boolean } {
  const next = list.filter((m) => m.id !== id);
  return { next, removed: next.length !== list.length };
}

/** **AN OVERRIDE IS A DECORATION, SO CLEARING ALL OF IT MUST LEAVE THE MODEL EXACTLY AS THE REGISTRY DESCRIBES.**
 *  The alternative — keeping an empty record — is a stale row that looks like a decision somebody made. */
export function overrideEmptied(o: {
  name?: unknown;
  contextWindow?: unknown;
  maxTokens?: unknown;
  reasoningEffort?: unknown;
}): boolean {
  return !o.name && !o.contextWindow && !o.maxTokens && !o.reasoningEffort;
}

/** `putFacetOverride`: the emptied rule decides between a REMOVAL and the same upsert every other store uses. */
export function putOverride<T extends { id: string }>(list: T[], o: T): T[] {
  return overrideEmptied(o as any) ? removeById(list, o.id).next : upsertById(list, o);
}

/** `declaredReasoningEffort`'s FIRST source: a console-added record for this id. The ORDER between the three
 *  sources is the caller's — the source short-circuits, and passing all three results in would read KV three
 *  times where it reads once. */
export function declaredFromCustom(custom: any[], id: string): string | null {
  return custom.find((m) => m?.id === id)?.reasoningEffort || null;
}

/** The SECOND source: a model inside a CUSTOM PROVIDER's record, found by the id's part after the first `/`.
 *  **`String(m?.id ?? "")` IS THE COMPARISON**: an absent id compares as the EMPTY string, so an id of `"acme/"`
 *  matches it, while a `0` id compares as `"0"` and does not. */
export function declaredFromProvider(provider: any, id: string): string | null {
  const slash = id.indexOf("/");
  if (slash < 0) return null;
  const bare = id.slice(slash + 1);
  const models = Array.isArray(provider?.models) ? provider.models : [];
  const model = models.find((m: any) => String(m?.id ?? "") === bare);
  return model?.reasoningEffort || null;
}

/** The THIRD source: the facet override that DECORATES a built-in — last, because it may set a default the
 *  registry never had and cannot contradict it about anything else. */
export function declaredFromOverride(overrides: any[], id: string): string | null {
  return overrides.find((o) => o?.id === id)?.reasoningEffort || null;
}

/** The catalogue's ROUTE PROJECTION — the static cards with their per-channel lists filtered, plus one synthetic
 *  card per prefix that only custom models or a custom provider uses.
 *
 *  `routeInfo` is the console's own display table, `provided` is the advertised provider models FLATTENED to
 *  `{wire, provider}` (the provider's bare prefix), and `providerCards` carries the `host` the CALLER parsed —
 *  `new URL(baseURL).host`, or the raw string when that throws, because a hand-edited record must still show a
 *  card rather than dropping out of the console. */
export function catalogueRoutes(
  routeInfo: any[],
  advertised: string[],
  custom: any[],
  provided: { wire: string; provider: string }[],
  providerCards: { prefix: string; label: string; baseURL: string; api: string; host: string }[],
): any[] {
  const fullId = (prefix: string, bare: string) => (prefix === "none" ? bare : prefix + bare);
  const routes: any[] = routeInfo.map((r) => ({
    ...r,
    models: (r.models as string[]).filter((bare) => {
      const id = fullId(r.prefix, bare);
      // Gone from the advertised set for ANY reason — disabled, or replaced by a custom record of the same id —
      // means gone from this channel's list too.
      return advertised.includes(id) && !custom.some((c) => c.id === id);
    }),
  }));
  for (const c of custom) {
    const cut = c.id.indexOf("/");
    const prefix = cut > 0 ? c.id.slice(0, cut + 1) : "none";
    const bare = cut > 0 ? c.id.slice(cut + 1) : c.id;
    const existing = routes.find((r) => r.prefix === prefix);
    if (existing) {
      if (!existing.models.includes(bare)) existing.models.push(bare);
    } else {
      routes.push({
        prefix,
        backend: prefix === "none" ? "Command Code (default)" : prefix,
        desc: "Added from the console.",
        models: [bare],
      });
    }
  }
  for (const p of providerCards) {
    const prefix = `${p.prefix}/`;
    const models = provided.filter((m) => m.provider === p.prefix).map((m) => m.wire);
    const join = Object.prototype.hasOwnProperty.call(SUPPORTED_PROVIDER_APIS, p.api)
      ? SUPPORTED_PROVIDER_APIS[p.api]
      : "";
    const card = {
      prefix,
      backend: p.label || p.prefix,
      desc: `Custom provider — ${p.host} via ${p.api || "?"}; requests go to ${p.baseURL}${join}`,
      models,
    };
    const existing = routes.find((r) => r.prefix === card.prefix);
    if (!existing) routes.push(card);
    else for (const m of models) if (!existing.models.includes(m)) existing.models.push(m);
  }
  return routes;
}
